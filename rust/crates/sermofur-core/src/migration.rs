//! `smf migrate`: an instance from format 1 to format 2, as `InstanceMigration.Migrate` of the
//! .NET oracle (ADR 0012; `rust/tests/golden/migration.json` is the truth).
//!
//! Consistency check, online backup through the SQLite backup API, one `IMMEDIATE` transaction
//! for the database, then `instance.json` replaced atomically. A database already in format 2
//! under a format 1 configuration is an unfinished migration that only needs its last step.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::backup::{Backup, StepResult};
use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags, TransactionBehavior};
use serde::Serialize;

use crate::atomic::{self, WriteOptions};
use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::init::{configuration_json, database_path};
use crate::instance::{
    CONFIGURATION_FILE, InstanceConfiguration, LEGACY_SCHEMA_VERSION, MARKER, SCHEMA_VERSION,
    invalid_storage, read_configuration, reject_links,
};
use crate::schema::{self, storage};
use crate::{guid, local_paths, search};

/// Name of the folder of the backups inside `.sermofur`.
pub const BACKUPS_DIRECTORY: &str = "backups";

/// `SqliteSchema.MigrateVersion1`: the records table is recreated (SQLite cannot change a CHECK
/// constraint), rows copied unchanged, then the indexes and the search index are created. The
/// text is the .NET one, word for word: `sqlite_master` of a migrated database keeps it.
const MIGRATE_VERSION_1: &str = "\
CREATE TABLE records_v2(id TEXT PRIMARY KEY, scope_id TEXT NOT NULL REFERENCES scopes(id),
    kind TEXT NOT NULL CHECK(kind IN ('Claim','Evidence','Retex','Source')),
    claim_id TEXT REFERENCES records(id),
    payload TEXT NOT NULL CHECK(json_valid(payload)), revision INTEGER NOT NULL CHECK(revision>0),
    source_path TEXT CHECK(source_path IS NULL OR kind='Source'));
INSERT INTO records_v2(id,scope_id,kind,claim_id,payload,revision,source_path)
    SELECT id,scope_id,kind,claim_id,payload,revision,NULL FROM records;
DROP TABLE records;
ALTER TABLE records_v2 RENAME TO records;
CREATE INDEX records_scope_kind ON records(scope_id,kind);
CREATE UNIQUE INDEX records_source_path ON records(source_path) WHERE source_path IS NOT NULL;
CREATE VIRTUAL TABLE search USING fts5(object_id UNINDEXED, scope_id UNINDEXED, kind UNINDEXED,
    passage UNINDEXED, length UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
CREATE VIRTUAL TABLE search_terms USING fts5vocab(search, instance);
";

/// The answer of `smf migrate` (`MigrationResult`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MigrationResult {
    pub migrated: bool,
    pub from: i32,
    pub to: i32,
    /// The backup taken, relative to the root with `/`; `None` when none was kept.
    pub backup: Option<String>,
}

/// Migrates the instance of `root`.
pub fn migrate(root: &Path) -> Result<MigrationResult> {
    migrate_with(root, &mut |_| Ok(()))
}

/// [`migrate`] with the test hook of the oracle (`StepHook`): called with `backup`,
/// `transaction` and `database`, it may fail to simulate a crash at that point.
pub fn migrate_with(
    root: &Path,
    hook: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<MigrationResult> {
    // A `createdAt` without offset is read in the local zone and written back with its offset.
    let configuration = read_configuration(root, true, crate::format::local_offset_of_clock)?;
    let database = database_path(root);
    reject_links(&database)?;
    // `File.Exists`: false for a folder.
    if !fs::metadata(&database).is_ok_and(|metadata| metadata.is_file()) {
        return Err(SermofurError::new(
            "missing_database",
            "Memory database missing.",
            3,
        ));
    }
    let mut version;
    let mut backup = None;
    {
        let mut connection = schema::open(&database, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        // Foreign keys off: the records table is rebuilt; they are checked before commit.
        connection
            .pragma_update(None, "foreign_keys", false)
            .map_err(|e| storage(&e))?;
        version = user_version(&connection)?;
        check_identity(&connection, &configuration)?;
        if version == i64::from(LEGACY_SCHEMA_VERSION) {
            require_consistent(&connection)?;
            let taken = back_up(&connection, root)?;
            backup = Some(taken.clone());
            hook("backup")?;
            if !migrate_database(&mut connection, hook)? {
                // Another migrate committed first: nothing left to do on the database, and this
                // backup is not the one taken before the migration that ran. A file held by a
                // scanner stays in backups/, which is harmless.
                version = i64::from(SCHEMA_VERSION);
                let _ = fs::remove_file(&taken);
                backup = None;
            }
        } else if version != i64::from(SCHEMA_VERSION) {
            return Err(SermofurError::new(
                "unsupported_schema",
                "Unsupported database schema version.",
                3,
            ));
        }
    }
    hook("database")?;
    let legacy_configuration = configuration.schema_version == LEGACY_SCHEMA_VERSION;
    // E1 of INVENTAIRE.md, reproduced: the configuration was read before the lock, so the loser
    // of a race also answers `migrated: true`.
    let migrated = version == i64::from(LEGACY_SCHEMA_VERSION) || legacy_configuration;
    if legacy_configuration {
        write_configuration(
            root,
            &InstanceConfiguration {
                schema_version: SCHEMA_VERSION,
                ..configuration
            },
        )?;
    }
    Ok(MigrationResult {
        migrated,
        from: if migrated {
            LEGACY_SCHEMA_VERSION
        } else {
            SCHEMA_VERSION
        },
        to: SCHEMA_VERSION,
        backup: backup
            .map(|path| local_paths::relativize_mapping(root, &path))
            .transpose()?,
    })
}

fn user_version(connection: &Connection) -> Result<i64> {
    connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(|e| storage(&e))
}

/// The first column of the first row as `Convert.ToString` of `ExecuteScalar` gives it for a
/// text; `None` without rows, for a null or for another type (none of them is ever compared
/// equal to a text here).
fn scalar_text(connection: &Connection, sql: &str) -> Result<Option<String>> {
    let mut statement = connection.prepare(sql).map_err(|e| storage(&e))?;
    let mut rows = statement.query([]).map_err(|e| storage(&e))?;
    let Some(row) = rows.next().map_err(|e| storage(&e))? else {
        return Ok(None);
    };
    Ok(match row.get_ref(0).map_err(|e| storage(&e))? {
        ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
        _ => None,
    })
}

fn check_identity(connection: &Connection, configuration: &InstanceConfiguration) -> Result<()> {
    let stored = scalar_text(
        connection,
        "SELECT value FROM metadata WHERE key='instance_id'",
    )?;
    match stored.as_deref().and_then(guid::try_parse) {
        Some(id) if id == configuration.instance_id => Ok(()),
        _ => Err(SermofurError::new(
            "instance_mismatch",
            "Inconsistent SQLite identity.",
            3,
        )),
    }
}

/// True when `PRAGMA foreign_key_check` reports a row (its first column, the table, is never
/// empty).
fn broken_reference(connection: &Connection) -> Result<bool> {
    let mut statement = connection
        .prepare("PRAGMA foreign_key_check")
        .map_err(|e| storage(&e))?;
    statement.exists([]).map_err(|e| storage(&e))
}

fn require_consistent(connection: &Connection) -> Result<()> {
    if scalar_text(connection, "PRAGMA integrity_check")?.as_deref() != Some("ok")
        || broken_reference(connection)?
    {
        return Err(SermofurError::new(
            "storage_error",
            "The instance is not consistent; run smf doctor before migrating.",
            3,
        ));
    }
    Ok(())
}

/// Copies the database to `.sermofur/backups/memory-v1-<stamp>-<guid32>.db` through the online
/// backup API, in one step, as `SqliteConnection.BackupDatabase`: a locked source is
/// `storage_busy` at once (measured on the oracle: under 1 s, scenario `locked` of
/// `migration.json`), so the busy timeout of `connection` is off during the step. The empty
/// target file stays, as in the oracle. Public for the golden test, which times it alone.
pub fn back_up(connection: &Connection, root: &Path) -> Result<PathBuf> {
    let directory = root.join(MARKER).join(BACKUPS_DIRECTORY);
    reject_links(&directory)?;
    fs::create_dir_all(&directory).map_err(|_| invalid_storage())?;
    let target = directory.join(format!(
        "memory-v1-{}-{}.db",
        DateTimeOffset::now_utc().to_backup(),
        guid::Uuid::new_v4().simple()
    ));
    let mut destination = schema::open(
        &target,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
    )?;
    let copy = Backup::new(connection, &mut destination).map_err(|e| storage(&e))?;
    let busy = |code| {
        storage(&rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(code),
            None,
        ))
    };
    // SQLite calls the busy handler of the source; `BackupDatabase` has none.
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(|e| storage(&e))?;
    let step = copy.step(-1);
    connection
        .busy_timeout(Duration::from_secs(schema::BUSY_TIMEOUT_SECONDS))
        .map_err(|e| storage(&e))?;
    match step.map_err(|e| storage(&e))? {
        StepResult::Done => Ok(target),
        StepResult::Busy => Err(busy(rusqlite::ffi::SQLITE_BUSY)),
        StepResult::Locked => Err(busy(rusqlite::ffi::SQLITE_LOCKED)),
        // `-1` copies every page at once; `More` would be a broken promise of SQLite.
        _ => Err(storage(&rusqlite::Error::InvalidQuery)),
    }
}

/// Migrates under an `IMMEDIATE` transaction; the version is read again under that lock, so
/// that two concurrent migrations never both rebuild the database. Returns false when it was
/// already migrated.
fn migrate_database(
    connection: &mut Connection,
    hook: &mut dyn FnMut(&str) -> Result<()>,
) -> Result<bool> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|e| storage(&e))?;
    if user_version(&transaction)? != i64::from(LEGACY_SCHEMA_VERSION) {
        return Ok(false);
    }
    transaction
        .execute_batch(MIGRATE_VERSION_1)
        .map_err(|e| storage(&e))?;
    search::index_records(&transaction)?;
    // PRAGMA does not take parameters; the value is a constant.
    transaction
        .execute_batch(&format!("PRAGMA user_version={SCHEMA_VERSION};"))
        .map_err(|e| storage(&e))?;
    if broken_reference(&transaction)? {
        return Err(SermofurError::new(
            "storage_error",
            "Migration broke a reference; nothing changed.",
            3,
        ));
    }
    hook("transaction")?;
    transaction.commit().map_err(|e| storage(&e))?;
    Ok(true)
}

fn write_configuration(root: &Path, configuration: &InstanceConfiguration) -> Result<()> {
    let file = root.join(MARKER).join(CONFIGURATION_FILE);
    // The marker and everything under it are refused as links (ADR 0010), and the atomic write
    // refuses one in place of the file itself.
    reject_links(&file)?;
    atomic::write(
        &file,
        configuration_json(configuration)?.as_bytes(),
        &WriteOptions::default(),
    )?;
    Ok(())
}
