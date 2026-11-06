//! SQLite schema, format 2, as `SqliteSchema.cs` creates it.
//!
//! The statements are the .NET ones, word for word, so that `sqlite_master` of a database made
//! by either tool is the same: a Rust instance must stay readable by the .NET tool and the other
//! way round (step 1b, cross-read tests).

use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use uuid::Uuid;

use crate::error::{Result, SermofurError};

/// Format of the instance and of the database written by this version.
pub const SCHEMA_VERSION: i64 = 2;

/// Format of the 0.1 CLI, read only to tell the user to migrate.
pub const LEGACY_SCHEMA_VERSION: i64 = 1;

/// Tokenizer of the search index, shared with the query tokenizer (ADR 0013).
pub const SEARCH_TOKENIZER: &str = "unicode61 remove_diacritics 2";

/// Busy timeout of a connection, in seconds, as `DefaultTimeout = 5` in .NET.
const BUSY_TIMEOUT_SECONDS: u64 = 5;

const TABLES: &str = "\
CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE scopes(id TEXT PRIMARY KEY, kind TEXT NOT NULL, parent_id TEXT REFERENCES scopes(id),
    relative_path TEXT COLLATE NOCASE);
CREATE TABLE records(id TEXT PRIMARY KEY, scope_id TEXT NOT NULL REFERENCES scopes(id),
    kind TEXT NOT NULL CHECK(kind IN ('Claim','Evidence','Retex','Source')),
    claim_id TEXT REFERENCES records(id),
    payload TEXT NOT NULL CHECK(json_valid(payload)), revision INTEGER NOT NULL CHECK(revision>0),
    source_path TEXT CHECK(source_path IS NULL OR kind='Source'));
CREATE TABLE history(sequence INTEGER PRIMARY KEY AUTOINCREMENT, object_id TEXT NOT NULL REFERENCES records(id),
    scope_id TEXT NOT NULL REFERENCES scopes(id), payload TEXT NOT NULL CHECK(json_valid(payload)));
CREATE TABLE idempotency(scope_id TEXT NOT NULL REFERENCES scopes(id), operation TEXT NOT NULL,
    key TEXT NOT NULL, content_hash TEXT NOT NULL, object_id TEXT NOT NULL REFERENCES records(id),
    PRIMARY KEY(scope_id,operation,key));
CREATE INDEX records_scope_kind ON records(scope_id,kind);
CREATE UNIQUE INDEX records_source_path ON records(source_path) WHERE source_path IS NOT NULL;
CREATE VIRTUAL TABLE search USING fts5(object_id UNINDEXED, scope_id UNINDEXED, kind UNINDEXED,
    passage UNINDEXED, length UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE search_terms USING fts5vocab(search, instance);
";

/// Maps a SQLite failure like `CommandRunner.Run` does in .NET: `storage_busy` for a database
/// that stays busy or locked, `storage_error` otherwise, with a fixed message (the raw SQLite
/// text may hold paths) and exit code 3.
fn storage(error: &rusqlite::Error) -> SermofurError {
    let busy = matches!(
        error,
        rusqlite::Error::SqliteFailure(failure, _)
            if matches!(
                failure.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    );
    SermofurError::new(
        if busy {
            "storage_busy"
        } else {
            "storage_error"
        },
        "Storage unavailable or inconsistent; run doctor.",
        3,
    )
}

/// Absolute form of a database path; an empty or unresolvable path is `invalid_path` (the .NET
/// tool would open a temporary database for an empty one, which is never wanted: recorded in
/// `PARITY.md`, Écarts assumés).
fn resolve(path: &Path) -> Result<std::path::PathBuf> {
    std::path::absolute(path)
        .map_err(|_| SermofurError::new("invalid_path", "The database path cannot be resolved.", 1))
}

/// Opens a connection like the .NET store: foreign keys on, five seconds of busy timeout, no
/// pooling (there is none here).
///
/// The bundled SQLite is built with URI filenames on whatever the flags say, so a relative path
/// that starts with `file:` would be read as a URI with parameters (`mode=memory`, `nolock=1`,
/// `%2e%2e`). The path is made absolute first: an absolute path is never a URI.
pub fn open(path: &Path, flags: OpenFlags) -> Result<Connection> {
    let absolute = resolve(path)?;
    let connection = Connection::open_with_flags(absolute, flags).map_err(|e| storage(&e))?;
    connection
        .busy_timeout(std::time::Duration::from_secs(BUSY_TIMEOUT_SECONDS))
        .map_err(|e| storage(&e))?;
    connection
        .pragma_update(None, "foreign_keys", true)
        .map_err(|e| storage(&e))?;
    Ok(connection)
}

/// Creates the database file with the schema of format 2, the workspace scope and the identity
/// of the instance, in one transaction.
pub fn create_database(path: &Path, instance_id: Uuid) -> Result<()> {
    // No SQLITE_OPEN_URI: a path is a path, never a `file:` URI with parameters.
    let mut connection = open(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    let transaction = connection
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| storage(&e))?;
    transaction.execute_batch(TABLES).map_err(|e| storage(&e))?;
    transaction
        .execute(
            "INSERT INTO metadata VALUES('instance_id',?1)",
            [instance_id.hyphenated().to_string()],
        )
        .map_err(|e| storage(&e))?;
    transaction
        .execute(
            "INSERT INTO scopes VALUES('workspace','Workspace',NULL,NULL)",
            [],
        )
        .map_err(|e| storage(&e))?;
    // PRAGMA does not take parameters; the value is a constant.
    transaction
        .execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION};"))
        .map_err(|e| storage(&e))?;
    transaction.commit().map_err(|e| storage(&e))
}

/// Checks the version and the identity of an open database, as `ValidateSchema` does.
pub fn validate(connection: &Connection, instance_id: Uuid) -> Result<()> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| storage(&e))?;
    if version == LEGACY_SCHEMA_VERSION {
        return Err(migration_required());
    }
    if version != SCHEMA_VERSION {
        return Err(SermofurError::new(
            "unsupported_schema",
            "Unsupported database schema version.",
            3,
        ));
    }
    let stored: Option<String> = connection
        .query_row(
            "SELECT value FROM metadata WHERE key='instance_id'",
            [],
            |row| row.get(0),
        )
        .map_or_else(
            |error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(storage(&other)),
            },
            |value| Ok(Some(value)),
        )?;
    match stored.and_then(|text| Uuid::parse_str(&text).ok()) {
        Some(id) if id == instance_id => Ok(()),
        _ => Err(SermofurError::new(
            "instance_mismatch",
            "Inconsistent SQLite identity.",
            3,
        )),
    }
}

/// The error for an instance still in format 1.
pub fn migration_required() -> SermofurError {
    SermofurError::new(
        "migration_required",
        "Instance in format 1; back it up if you wish, then run smf migrate.",
        3,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary_database(name: &str) -> std::path::PathBuf {
        let directory =
            std::env::temp_dir().join(format!("sermofur-schema-{name}-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("directory");
        directory.join("memory.db")
    }

    #[test]
    fn a_new_database_validates_with_its_identity() {
        let path = temporary_database("validate");
        let id = Uuid::new_v4();
        create_database(&path, id).expect("created");
        let connection = open(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).expect("opened");
        validate(&connection, id).expect("valid");
        let other = validate(&connection, Uuid::new_v4()).expect_err("another identity");
        assert_eq!(other.code, "instance_mismatch");
    }

    fn writable(path: &Path) -> Connection {
        open(path, OpenFlags::SQLITE_OPEN_READ_WRITE).expect("opened")
    }

    #[test]
    fn validate_refuses_other_formats_and_a_missing_identity() {
        let path = temporary_database("branches");
        let id = Uuid::new_v4();
        create_database(&path, id).expect("created");
        let connection = writable(&path);

        connection
            .execute_batch("PRAGMA user_version=1;")
            .expect("version");
        let legacy = validate(&connection, id).expect_err("format 1");
        assert_eq!(
            (legacy.code.as_str(), legacy.exit_code),
            ("migration_required", 3)
        );
        assert_eq!(
            legacy.message,
            "Instance in format 1; back it up if you wish, then run smf migrate."
        );

        connection
            .execute_batch("PRAGMA user_version=3;")
            .expect("version");
        let unknown = validate(&connection, id).expect_err("format 3");
        assert_eq!(
            (unknown.code.as_str(), unknown.exit_code),
            ("unsupported_schema", 3)
        );
        assert_eq!(unknown.message, "Unsupported database schema version.");

        connection
            .execute_batch("PRAGMA user_version=2; DELETE FROM metadata;")
            .expect("identity removed");
        let missing = validate(&connection, id).expect_err("no identity");
        assert_eq!(
            (missing.code.as_str(), missing.exit_code),
            ("instance_mismatch", 3)
        );
        assert_eq!(missing.message, "Inconsistent SQLite identity.");
    }

    #[test]
    fn creating_over_an_existing_database_fails_and_keeps_it() {
        let path = temporary_database("twice");
        let id = Uuid::new_v4();
        create_database(&path, id).expect("created");
        let error = create_database(&path, Uuid::new_v4()).expect_err("already there");
        assert_eq!(error.code, "storage_error");
        validate(&writable(&path), id).expect("the first identity is kept");
    }

    #[test]
    fn a_busy_database_is_reported_as_storage_busy() {
        let error = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
            None,
        );
        let mapped = storage(&error);
        assert_eq!(
            (mapped.code.as_str(), mapped.exit_code),
            ("storage_busy", 3)
        );
        assert_eq!(
            mapped.message,
            "Storage unavailable or inconsistent; run doctor."
        );
        let locked = rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_LOCKED),
            None,
        );
        assert_eq!(storage(&locked).code, "storage_busy");
        assert_eq!(
            storage(&rusqlite::Error::InvalidQuery).code,
            "storage_error"
        );
    }

    #[test]
    fn a_path_that_looks_like_a_uri_is_made_absolute_before_sqlite_sees_it() {
        for text in [
            "file:literal.db?mode=memory",
            "file:x.db?nolock=1",
            "file:..%2fx",
        ] {
            let resolved = resolve(Path::new(text)).expect("resolved");
            assert!(resolved.is_absolute(), "{text} is absolute once resolved");
            assert!(
                !resolved.to_string_lossy().starts_with("file:"),
                "{text} does not start with file:"
            );
        }
    }

    #[test]
    fn an_empty_path_is_refused() {
        let error = open(Path::new(""), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect_err("an empty path is not a database");
        assert_eq!((error.code.as_str(), error.exit_code), ("invalid_path", 1));
    }

    #[test]
    fn fts5_and_the_vocabulary_table_work() {
        let path = temporary_database("fts");
        create_database(&path, Uuid::new_v4()).expect("created");
        let connection = open(&path, OpenFlags::SQLITE_OPEN_READ_WRITE).expect("opened");
        connection
            .execute(
                "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES('a','workspace','Claim',0,2,'Déjà vu')",
                [],
            )
            .expect("indexed");
        let term: String = connection
            .query_row(
                "SELECT term FROM search_terms WHERE col='text' ORDER BY term LIMIT 1",
                [],
                |r| r.get(0),
            )
            .expect("term");
        assert_eq!(term, "deja");
    }
}
