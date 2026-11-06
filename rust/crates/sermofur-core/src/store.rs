//! The memory store of an instance, as `SqliteStore` of the .NET oracle
//! (`rust/tests/golden/store.json` is the truth): scopes, records, evidence links, history and
//! invalidation, each write in one `BEGIN IMMEDIATE` transaction.
//!
//! After each write, the Markdown projection of the record is rewritten; a failure there leaves
//! the record saved and reports `projection_pending`. Not yet ported here: the move of sources
//! into a new scope (lot 5.2).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ToSql, TransactionBehavior};
use uuid::Uuid;

use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::init::open_store;
use crate::instance::invalid_storage;
use crate::projection::{is_current_safe, project};
use crate::records::{
    ClaimContent, DotNetEnum, EvidenceContent, HistoryEntry, KnowledgeStatus, MemoryRecord,
    RecordKind, RetexContent, Scope, ScopeKind, read, write,
};
use crate::schema::{SEARCH_TOKENIZER, storage};
use crate::scope_policy::{ScopeLike, kind_value, validate_tree};

/// A row of the `scopes` table. The kind keeps the text stored and the value `Enum.Parse`
/// gives it, which may be undefined (a number).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredScope {
    pub id: String,
    pub kind: i64,
    pub parent_id: Option<String>,
    pub relative_path: Option<String>,
}

impl StoredScope {
    /// The scope as the domain writes it; an undefined kind cannot be written (the
    /// `JsonException` of .NET, `invalid_storage_or_path`).
    pub fn to_scope(&self) -> Result<Scope> {
        let kind = usize::try_from(self.kind)
            .ok()
            .and_then(|index| ScopeKind::NAMES.get(index))
            .map(|(_, _, member)| *member)
            .ok_or_else(invalid_storage)?;
        Ok(Scope {
            id: self.id.clone(),
            kind,
            parent_id: self.parent_id.clone(),
            relative_path: self.relative_path.clone(),
        })
    }

    fn from_scope(scope: &Scope) -> Self {
        StoredScope {
            id: scope.id.clone(),
            kind: kind_value(scope.kind),
            parent_id: scope.parent_id.clone(),
            relative_path: scope.relative_path.clone(),
        }
    }
}

impl ScopeLike for StoredScope {
    fn id(&self) -> &str {
        &self.id
    }
    fn kind_value(&self) -> i64 {
        self.kind
    }
    fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }
    fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }
}

/// `Enum.Parse<T>(text)`: the .NET names, case-sensitive, spaces around ignored, a list
/// separated by commas combined by OR, or a number in `int` range. Anything else is the
/// `ArgumentException` of .NET (`invalid_storage_or_path`).
fn parse_dotnet_enum<E: DotNetEnum>(text: &str) -> Result<i64> {
    let trimmed = text.trim();
    let first = trimmed.chars().next().ok_or_else(invalid_storage)?;
    if first.is_ascii_digit() || first == '-' || first == '+' {
        return trimmed
            .parse::<i32>()
            .map(i64::from)
            .map_err(|_| invalid_storage());
    }
    let mut value = 0i64;
    for part in trimmed.split(',') {
        let part = part.trim();
        let index = E::NAMES
            .iter()
            .position(|(name, _, _)| *name == part)
            .ok_or_else(invalid_storage)?;
        value |= i64::try_from(index).map_err(|_| invalid_storage())?;
    }
    Ok(value)
}

/// The .NET name of a member (`ToString()`), as the tables store kinds.
fn dotnet_name<E: DotNetEnum + PartialEq>(member: E) -> &'static str {
    E::NAMES
        .iter()
        .find(|(_, _, m)| *m == member)
        .map_or("", |(name, _, _)| name)
}

/// The kind of a record in the `search` table.
fn search_kind(kind: RecordKind) -> Result<&'static str> {
    match kind {
        RecordKind::Claim => Ok("claim"),
        RecordKind::Retex => Ok("retex"),
        RecordKind::Source => Ok("source"),
        // `ArgumentOutOfRangeException` in .NET: never reached, evidence is not indexed.
        RecordKind::Evidence => Err(invalid_storage()),
    }
}

/// The texts a claim or a RETEX is searched by (`SearchDocuments.For`): one passage each.
fn search_documents(record: &MemoryRecord) -> Result<Vec<(i64, String)>> {
    let offset = |_| 0;
    let json = record.content_json.as_deref().ok_or_else(invalid_storage)?;
    Ok(match record.kind {
        RecordKind::Claim => {
            let content: ClaimContent = read(json, &offset)?;
            vec![(0, content.text.ok_or_else(invalid_storage)?)]
        }
        RecordKind::Retex => {
            let content: RetexContent = read(json, &offset)?;
            let part = |p: Option<String>| p.unwrap_or_default();
            vec![(
                0,
                format!(
                    "{}\n{}\n{}",
                    part(content.event),
                    part(content.impact),
                    part(content.next_action)
                ),
            )]
        }
        _ => Vec::new(),
    })
}

/// The terms of a text, by the exact tokenizer of the `search` table: a temporary FTS5 table
/// of the same configuration read back through `fts5vocab` (`SqliteTokenizer`).
pub fn tokenize(connection: &Connection, text: &str) -> Result<Vec<String>> {
    connection
        .execute_batch(&format!(
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.tokenizer USING fts5(text, tokenize='{SEARCH_TOKENIZER}');
             CREATE VIRTUAL TABLE IF NOT EXISTS temp.tokenizer_terms USING fts5vocab(temp, tokenizer, instance);
             DELETE FROM temp.tokenizer;"
        ))
        .map_err(|e| storage(&e))?;
    connection
        .execute("INSERT INTO temp.tokenizer(text) VALUES(?1)", [text])
        .map_err(|e| storage(&e))?;
    let terms = {
        let mut statement = connection
            .prepare("SELECT term FROM temp.tokenizer_terms ORDER BY offset")
            .map_err(|e| storage(&e))?;
        statement
            .query_map([], |row| row.get::<_, String>(0))
            .and_then(Iterator::collect::<rusqlite::Result<Vec<String>>>)
            .map_err(|e| storage(&e))?
    };
    connection
        .execute_batch("DELETE FROM temp.tokenizer;")
        .map_err(|e| storage(&e))?;
    Ok(terms)
}

fn not_found() -> SermofurError {
    SermofurError::new("not_found", "Object missing or inaccessible.", 1)
}

/// The store of one instance.
pub struct Store {
    connection: Connection,
    root: PathBuf,
}

impl Store {
    /// Opens the database of the instance at `root` (`SqliteStore` then `ValidateSchema`).
    pub fn open(root: &Path, instance_id: Uuid, read_only: bool) -> Result<Self> {
        Ok(Store {
            connection: open_store(root, instance_id, read_only)?,
            root: root.to_path_buf(),
        })
    }

    /// The scopes, ordered by identifier.
    pub fn read_scopes(&self) -> Result<Vec<StoredScope>> {
        read_scopes(&self.connection)
    }

    /// Adds a scope after `precondition` accepted the current ones and the tree stays valid,
    /// in one transaction.
    pub fn add_scope(
        &mut self,
        scope: &Scope,
        precondition: impl FnOnce(&[StoredScope]) -> Result<()>,
    ) -> Result<()> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| storage(&e))?;
        let mut scopes = read_scopes(&transaction)?;
        precondition(&scopes)?;
        scopes.push(StoredScope::from_scope(scope));
        validate_tree(&scopes)?;
        transaction
            .execute(
                "INSERT INTO scopes VALUES(?1,?2,?3,?4)",
                rusqlite::params![
                    scope.id,
                    dotnet_name(scope.kind),
                    scope.parent_id,
                    scope.relative_path
                ],
            )
            .map_err(|e| storage(&e))?;
        // ponytail: sources of ancestors are moved into the new scope by lot 5.2.
        transaction.commit().map_err(|e| storage(&e))
    }

    /// The records of the visible scopes, of one kind or all, ordered by identifier.
    pub fn read_records(
        &self,
        visible: &HashSet<String>,
        kind: Option<RecordKind>,
    ) -> Result<Vec<MemoryRecord>> {
        let suffix = if kind.is_some() {
            "AND kind=$kind ORDER BY id"
        } else {
            "ORDER BY id"
        };
        let kind_text = kind.map(dotnet_name);
        let mut extra: Vec<(&str, &dyn ToSql)> = Vec::new();
        if let Some(text) = &kind_text {
            extra.push(("$kind", text));
        }
        visible_payloads(&self.connection, visible, suffix, &extra)?
            .iter()
            .map(|payload| read(payload, &|_| 0))
            .collect()
    }

    /// One record if its scope is visible.
    pub fn find_record(&self, id: Uuid, visible: &HashSet<String>) -> Result<Option<MemoryRecord>> {
        find_record(&self.connection, id, visible)
    }

    /// The history of a visible record, in order; empty when the record is not visible.
    pub fn read_history(&self, id: Uuid, visible: &HashSet<String>) -> Result<Vec<HistoryEntry>> {
        if self.find_record(id, visible)?.is_none() {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT sequence,payload FROM history WHERE object_id=?1 ORDER BY sequence")
            .map_err(|e| storage(&e))?;
        let rows: Vec<(i64, String)> = statement
            .query_map([id.hyphenated().to_string()], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .and_then(Iterator::collect)
            .map_err(|e| storage(&e))?;
        rows.into_iter()
            .map(|(sequence, payload)| {
                let mut entry: HistoryEntry = read(&payload, &|_| 0)?;
                entry.sequence = sequence;
                Ok(entry)
            })
            .collect()
    }

    /// Creates a record with its search passages and its first history entry, in one
    /// transaction. Evidence must cite a claim of its own scope. With an idempotency `key`, a
    /// second creation of the same content in the same scope and operation returns the first
    /// record; other content under that key is `idempotency_conflict`.
    pub fn create_record(
        &mut self,
        candidate: &MemoryRecord,
        key: Option<&str>,
    ) -> Result<MemoryRecord> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| storage(&e))?;
        let scope_id = candidate.scope_id.clone().ok_or_else(invalid_storage)?;
        let fingerprint = fingerprint(candidate)?;
        if let Some(key) = key
            && let Some(existing) =
                find_idempotent(&transaction, candidate, &scope_id, key, &fingerprint)?
        {
            transaction.commit().map_err(|e| storage(&e))?;
            if !is_current_safe(&self.root, &existing)? {
                project(&self.root, &existing)?;
            }
            return Ok(existing);
        }
        let claim_id = if candidate.kind == RecordKind::Evidence {
            let json = candidate
                .content_json
                .as_deref()
                .ok_or_else(invalid_storage)?;
            let content: EvidenceContent = read(json, &|_| 0)?;
            let own = HashSet::from([scope_id.clone()]);
            let claim = find_record(&transaction, content.claim_id, &own)?;
            if claim.map(|c| c.kind) != Some(RecordKind::Claim) {
                return Err(SermofurError::new(
                    "scope_boundary",
                    "Claim of the evidence missing from the scope.",
                    4,
                ));
            }
            Some(content.claim_id.hyphenated().to_string())
        } else {
            None
        };
        transaction
            .execute(
                "INSERT INTO records(id,scope_id,kind,claim_id,payload,revision) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    candidate.id.hyphenated().to_string(),
                    scope_id,
                    dotnet_name(candidate.kind),
                    claim_id,
                    write(candidate)?,
                    candidate.revision
                ],
            )
            .map_err(|e| storage(&e))?;
        if matches!(candidate.kind, RecordKind::Claim | RecordKind::Retex) {
            for (passage, text) in search_documents(candidate)? {
                let length = tokenize(&transaction, &text)?.len();
                transaction
                    .execute(
                        "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES(?1,?2,?3,?4,?5,?6)",
                        rusqlite::params![
                            candidate.id.hyphenated().to_string(),
                            scope_id,
                            search_kind(candidate.kind)?,
                            passage,
                            i64::try_from(length).unwrap_or(i64::MAX),
                            text
                        ],
                    )
                    .map_err(|e| storage(&e))?;
            }
        }
        let actor = candidate
            .provenance
            .as_ref()
            .ok_or_else(invalid_storage)?
            .actor
            .clone();
        append_history(&transaction, None, candidate, "create", actor)?;
        if let Some(key) = key {
            transaction
                .execute(
                    "INSERT INTO idempotency VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![
                        scope_id,
                        dotnet_name(candidate.kind),
                        key,
                        fingerprint,
                        candidate.id.hyphenated().to_string()
                    ],
                )
                .map_err(|e| storage(&e))?;
        }
        transaction.commit().map_err(|e| storage(&e))?;
        project(&self.root, candidate)?;
        Ok(candidate.clone())
    }

    /// Rewrites the projection of every visible record and returns their paths (`Export`); the
    /// first failure stops with `projection_pending`.
    pub fn export(&self, visible: &HashSet<String>) -> Result<Vec<PathBuf>> {
        let records = self.read_records(visible, None)?;
        for record in &records {
            project(&self.root, record)?;
        }
        Ok(records
            .iter()
            .map(|record| crate::projection::path_for(&self.root, record))
            .collect())
    }

    /// Invalidates a claim of `scope_id`, once: a second call returns it unchanged.
    pub fn invalidate_record(
        &mut self,
        id: Uuid,
        scope_id: &str,
        reason: &str,
        actor: &str,
    ) -> Result<MemoryRecord> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| storage(&e))?;
        let own = HashSet::from([scope_id.to_owned()]);
        let previous = find_record(&transaction, id, &own)?.ok_or_else(not_found)?;
        if previous.kind != RecordKind::Claim {
            return Err(SermofurError::new("wrong_kind", "A claim is required.", 1));
        }
        if previous.status == KnowledgeStatus::Invalidated {
            transaction.commit().map_err(|e| storage(&e))?;
            return Ok(previous);
        }
        let mut updated = previous.clone();
        updated.status = KnowledgeStatus::Invalidated;
        updated.revision = previous.revision + 1;
        transaction
            .execute(
                "UPDATE records SET payload=?1,revision=?2 WHERE id=?3",
                rusqlite::params![
                    write(&updated)?,
                    updated.revision,
                    id.hyphenated().to_string()
                ],
            )
            .map_err(|e| storage(&e))?;
        append_history(
            &transaction,
            Some(&previous),
            &updated,
            reason,
            Some(actor.to_owned()),
        )?;
        transaction.commit().map_err(|e| storage(&e))?;
        project(&self.root, &updated)?;
        Ok(updated)
    }
}

fn read_scopes(connection: &Connection) -> Result<Vec<StoredScope>> {
    let mut statement = connection
        .prepare("SELECT id,kind,parent_id,relative_path FROM scopes ORDER BY id")
        .map_err(|e| storage(&e))?;
    let rows: Vec<(String, String, Option<String>, Option<String>)> = statement
        .query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .and_then(Iterator::collect)
        .map_err(|e| storage(&e))?;
    rows.into_iter()
        .map(|(id, kind, parent_id, relative_path)| {
            Ok(StoredScope {
                id,
                kind: parse_dotnet_enum::<ScopeKind>(&kind)?,
                parent_id,
                relative_path,
            })
        })
        .collect()
}

/// `SELECT payload FROM records WHERE scope_id IN (<visible>) <suffix>` (`VisibleCommand`).
fn visible_payloads(
    connection: &Connection,
    visible: &HashSet<String>,
    suffix: &str,
    extra: &[(&str, &dyn ToSql)],
) -> Result<Vec<String>> {
    let names: Vec<String> = (0..visible.len()).map(|i| format!("$scope{i}")).collect();
    let sql = format!(
        "SELECT payload FROM records WHERE scope_id IN ({}) {suffix}",
        names.join(",")
    );
    let mut parameters: Vec<(&str, &dyn ToSql)> = names
        .iter()
        .map(String::as_str)
        .zip(visible.iter().map(|id| id as &dyn ToSql))
        .collect();
    parameters.extend_from_slice(extra);
    let mut statement = connection.prepare(&sql).map_err(|e| storage(&e))?;
    statement
        .query_map(parameters.as_slice(), |row| row.get::<_, String>(0))
        .and_then(Iterator::collect)
        .map_err(|e| storage(&e))
}

fn find_record(
    connection: &Connection,
    id: Uuid,
    visible: &HashSet<String>,
) -> Result<Option<MemoryRecord>> {
    let id_text = id.hyphenated().to_string();
    let payload = visible_payloads(connection, visible, "AND id=$id", &[("$id", &id_text)])?
        .into_iter()
        .next();
    payload.map(|p| read(&p, &|_| 0)).transpose()
}

/// Appends a history entry stamped with the present instant (`AppendHistory`).
fn append_history(
    connection: &Connection,
    previous: Option<&MemoryRecord>,
    current: &MemoryRecord,
    reason: &str,
    actor: Option<String>,
) -> Result<()> {
    let entry = HistoryEntry {
        sequence: 0,
        object_id: current.id,
        scope_id: current.scope_id.clone(),
        previous_state: previous.map(write).transpose()?,
        new_state: Some(write(current)?),
        reason: Some(reason.to_owned()),
        actor,
        timestamp: DateTimeOffset::now_utc(),
    };
    connection
        .execute(
            "INSERT INTO history(object_id,scope_id,payload) VALUES(?1,?2,?3)",
            rusqlite::params![
                current.id.hyphenated().to_string(),
                current.scope_id,
                write(&entry)?
            ],
        )
        .map_err(|e| storage(&e))?;
    Ok(())
}

/// The content a key stands for (`CreateRecord`): the upper-case SHA-256 of the JSON of
/// `{ ScopeId, Kind, ContentJson, Origin, Actor }`, as `RecordJson` writes it (indented, with
/// the line break of the platform), so the identifier and the instant do not count.
fn fingerprint(candidate: &MemoryRecord) -> Result<String> {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Fingerprint<'a> {
        scope_id: &'a Option<String>,
        kind: RecordKind,
        content_json: &'a Option<String>,
        origin: crate::records::ActorKind,
        actor: &'a Option<String>,
    }
    let provenance = candidate.provenance.as_ref().ok_or_else(invalid_storage)?;
    let json = write(&Fingerprint {
        scope_id: &candidate.scope_id,
        kind: candidate.kind,
        content_json: &candidate.content_json,
        origin: provenance.origin,
        actor: &provenance.actor,
    })?;
    Ok(sha256_upper(json.as_bytes()))
}

/// `Convert.ToHexString(SHA256.HashData(bytes))`: 64 upper-case hexadecimal digits.
pub fn sha256_upper(bytes: &[u8]) -> String {
    use sha2::Digest;
    use std::fmt::Write as _;
    sha2::Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02X}");
            out
        })
}

/// The record a key already stands for (`FindIdempotent`), or `None` for a new key.
fn find_idempotent(
    connection: &Connection,
    candidate: &MemoryRecord,
    scope_id: &str,
    key: &str,
    fingerprint: &str,
) -> Result<Option<MemoryRecord>> {
    let row: Option<(String, String)> = connection
        .query_row(
            "SELECT content_hash,object_id FROM idempotency WHERE scope_id=?1 AND operation=?2 AND key=?3",
            rusqlite::params![scope_id, dotnet_name(candidate.kind), key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(storage(&other)),
        })?;
    let Some((hash, object_id)) = row else {
        return Ok(None);
    };
    if hash != fingerprint {
        return Err(SermofurError::new(
            "idempotency_conflict",
            "Key reused with different content.",
            1,
        ));
    }
    // `Guid.Parse` reads more formats than the D the store writes; a bad value is its
    // `FormatException`, which the CLI does not catch either: refused as corrupt storage.
    let id = Uuid::parse_str(&object_id).map_err(|_| invalid_storage())?;
    let own = HashSet::from([scope_id.to_owned()]);
    find_record(connection, id, &own)?
        .map(Some)
        .ok_or_else(|| SermofurError::new("orphan_idempotency", "Orphan idempotency reference.", 3))
}
