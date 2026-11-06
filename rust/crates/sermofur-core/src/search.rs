//! The full-text index, as the .NET oracle (`rust/tests/golden/search.json` is the truth): the
//! terms of a question (`SearchTerms`), what an object is searched by (`SearchExpectations`,
//! `SearchDocuments`), the writes of the `search` table (`SqliteSearchIndex`) and its read side
//! (`SqliteStore` as `ISearchIndex`, plus `ReadSummaries`).
//!
//! A question is always plain text: it is split by the tokenizer of the index
//! ([`crate::store::tokenize`]), never parsed as FTS5 query syntax, and its terms are bound as
//! parameters. Every read is restricted to the visible scopes before any statistic is computed
//! (ADR 0013).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rusqlite::{Connection, ToSql};
use uuid::Uuid;

use crate::error::Result;
use crate::format::{DateTimeOffset, parse_json_text};
use crate::instance::invalid_storage;
use crate::records::{
    ClaimContent, KnowledgeStatus, MemoryRecord, RecordKind, RetexContent, SourceContent,
    SourceStatus, read,
};
use crate::schema::storage;
use crate::store::{Store, parse_dotnet_enum, tokenize};

/// Maximum number of distinct terms kept from a question (`SearchTerms.MaxQueryTerms`).
pub const MAX_QUERY_TERMS: usize = 32;

/// A searchable unit of an object: its passage number and its text (`SearchDocument`).
pub type SearchDocument = (i32, String);

/// Frequency of a question term in an indexed passage of a visible scope (`TermHit`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TermHit {
    pub document: i64,
    pub object_id: Uuid,
    pub scope_id: String,
    pub kind: String,
    pub passage: i32,
    pub length: i32,
    pub term: String,
    pub frequency: i32,
}

/// Size of the visible part of the index, what BM25 statistics are computed on (`VisibleCorpus`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisibleCorpus {
    pub documents: usize,
    pub average_length: f64,
}

/// What ranking needs to know of an object before describing it (`RecordSummary`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordSummary {
    pub id: Uuid,
    pub kind: RecordKind,
    pub status: KnowledgeStatus,
    pub created_at: DateTimeOffset,
}

/// An indexed passage of a visible scope (`SqliteStore.VisiblePassage`).
#[derive(Debug)]
pub(crate) struct VisiblePassage {
    object_id: Uuid,
    scope_id: String,
    kind: String,
    passage: i32,
    length: i32,
}

/// The visible passages by rowid, with the key they were read under.
pub(crate) type PassageCache = Option<(String, Arc<HashMap<i64, VisiblePassage>>)>;

/// The distinct terms of a question, in their order of appearance, at most 32
/// (`SearchTerms.Query`).
pub fn query_terms(store: &Store, query: &str) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    Ok(store
        .tokenize(query)?
        .into_iter()
        .filter(|term| seen.insert(term.clone()))
        .take(MAX_QUERY_TERMS)
        .collect())
}

/// Whether the object must have entries in the index (`SearchExpectations.IsIndexed`): claims
/// and RETEX always, sources when indexed.
pub fn is_indexed(record: &MemoryRecord) -> Result<bool> {
    Ok(match record.kind {
        RecordKind::Claim | RecordKind::Retex => true,
        RecordKind::Source => {
            let json = record.content_json.as_deref().ok_or_else(invalid_storage)?;
            read::<SourceContent>(json, &|_| 0)?.status == SourceStatus::Indexed
        }
        RecordKind::Evidence => false,
    })
}

/// The documents of a claim or a RETEX (`SearchDocuments.For`); sources are indexed from their
/// file, and evidence is not indexed.
pub fn documents(record: &MemoryRecord) -> Result<Vec<SearchDocument>> {
    let offset = |_| 0;
    let json = || record.content_json.as_deref().ok_or_else(invalid_storage);
    Ok(match record.kind {
        RecordKind::Claim => {
            let content: ClaimContent = read(json()?, &offset)?;
            vec![(0, content.text.ok_or_else(invalid_storage)?)]
        }
        RecordKind::Retex => {
            let content: RetexContent = read(json()?, &offset)?;
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

/// The kind of an object in the `search` table (`SqliteSearchIndex.KindOf`).
fn kind_of(kind: RecordKind) -> Result<&'static str> {
    match kind {
        RecordKind::Claim => Ok("claim"),
        RecordKind::Retex => Ok("retex"),
        RecordKind::Source => Ok("source"),
        // `ArgumentOutOfRangeException` in .NET: evidence is never indexed.
        RecordKind::Evidence => Err(invalid_storage()),
    }
}

/// Adds the documents of an object to the index, each with its length in terms
/// (`SqliteSearchIndex.Insert`). Called inside the transaction that writes the object.
pub fn insert(
    connection: &Connection,
    object_id: Uuid,
    scope_id: &str,
    kind: RecordKind,
    documents: &[SearchDocument],
) -> Result<()> {
    for (passage, text) in documents {
        let length = tokenize(connection, text)?.len();
        connection
            .execute(
                "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES(?1,?2,?3,?4,?5,?6)",
                rusqlite::params![
                    object_id.hyphenated().to_string(),
                    scope_id,
                    kind_of(kind)?,
                    passage,
                    i64::try_from(length).unwrap_or(i64::MAX),
                    text
                ],
            )
            .map_err(|e| storage(&e))?;
    }
    Ok(())
}

/// Removes every entry of an object (`SqliteSearchIndex.Remove`).
pub fn remove(connection: &Connection, object_id: Uuid) -> Result<()> {
    connection
        .execute(
            "DELETE FROM search WHERE object_id=?1",
            [object_id.hyphenated().to_string()],
        )
        .map_err(|e| storage(&e))?;
    Ok(())
}

/// Removes every entry of the index (`SqliteSearchIndex.Clear`).
pub fn clear(connection: &Connection) -> Result<()> {
    connection
        .execute("DELETE FROM search", [])
        .map_err(|e| storage(&e))?;
    Ok(())
}

/// Indexes every claim and RETEX of the registry, in identifier order, from their payload
/// (`SqliteSearchIndex.IndexRecords`: migration and rebuild). Returns how many were indexed.
pub fn index_records(connection: &Connection) -> Result<usize> {
    let mut statement = connection
        .prepare("SELECT payload FROM records WHERE kind IN ('Claim','Retex') ORDER BY id")
        .map_err(|e| storage(&e))?;
    let mut rows = statement.query([]).map_err(|e| storage(&e))?;
    let mut count = 0;
    // One record in memory at a time, as the .NET reader streams the registry.
    while let Some(row) = rows.next().map_err(|e| storage(&e))? {
        let payload: String = row.get(0).map_err(|e| storage(&e))?;
        let record: MemoryRecord = read(&payload, &|_| 0)?;
        let scope_id = record.scope_id.as_deref().ok_or_else(invalid_storage)?;
        insert(
            connection,
            record.id,
            scope_id,
            record.kind,
            &documents(&record)?,
        )?;
        count += 1;
    }
    Ok(count)
}

/// `$prefix0,$prefix1,...` bound to `values` (`SqliteStore.Parameters`).
fn parameters<'a>(
    prefix: &str,
    values: impl Iterator<Item = &'a String>,
) -> (String, Vec<(String, &'a String)>) {
    let bound: Vec<(String, &String)> = values
        .enumerate()
        .map(|(index, value)| (format!("{prefix}{index}"), value))
        .collect();
    let names: Vec<&str> = bound.iter().map(|(name, _)| name.as_str()).collect();
    (names.join(","), bound)
}

fn as_named<'a>(bound: &'a [(String, &'a String)]) -> Vec<(&'a str, &'a dyn ToSql)> {
    bound
        .iter()
        .map(|(name, value)| (name.as_str(), *value as &dyn ToSql))
        .collect()
}

impl Store {
    /// The terms of a text, by the tokenizer of the index (`SqliteStore.Tokenizer`).
    pub fn tokenize(&self, text: &str) -> Result<Vec<String>> {
        tokenize(&self.connection, text)
    }

    /// Frequencies of `terms` per passage, visible passages only, ordered by passage then term
    /// (`SqliteStore.Hits`).
    pub fn hits(&self, terms: &[String], visible: &HashSet<String>) -> Result<Vec<TermHit>> {
        let passages = self.visible_passages(visible)?;
        let (names, bound) = parameters("$term", terms.iter());
        // Grouping before joining is several times faster (ADR 0013).
        let sql = format!(
            "SELECT doc, term, count(*) FROM search_terms
            WHERE col = 'text' AND term IN ({names})
            GROUP BY doc, term ORDER BY doc, term"
        );
        let mut statement = self.connection.prepare(&sql).map_err(|e| storage(&e))?;
        let rows: Vec<(i64, String, i32)> = statement
            .query_map(as_named(&bound).as_slice(), |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .and_then(Iterator::collect)
            .map_err(|e| storage(&e))?;
        Ok(rows
            .into_iter()
            .filter_map(|(document, term, frequency)| {
                passages.get(&document).map(|p| TermHit {
                    document,
                    object_id: p.object_id,
                    scope_id: p.scope_id.clone(),
                    kind: p.kind.clone(),
                    passage: p.passage,
                    length: p.length,
                    term,
                    frequency,
                })
            })
            .collect())
    }

    /// The number of visible passages and their average length in terms (`SqliteStore.Corpus`).
    pub fn corpus(&self, visible: &HashSet<String>) -> Result<VisibleCorpus> {
        let passages = self.visible_passages(visible)?;
        let total: i64 = passages.values().map(|p| i64::from(p.length)).sum();
        // `Enumerable.Average` over ints: a long sum divided as a double.
        let average_length = if passages.is_empty() {
            0.0
        } else {
            total as f64 / passages.len() as f64
        };
        Ok(VisibleCorpus {
            documents: passages.len(),
            average_length,
        })
    }

    /// The indexed passages of the visible scopes, read once per visibility set and state of the
    /// database: any commit, by this connection or another one, changes the key
    /// (`SqliteStore.VisiblePassages`).
    fn visible_passages(
        &self,
        visible: &HashSet<String>,
    ) -> Result<Arc<HashMap<i64, VisiblePassage>>> {
        let data_version: i64 = self
            .connection
            .query_row("PRAGMA data_version", [], |row| row.get(0))
            .map_err(|e| storage(&e))?;
        let mut scopes: Vec<&String> = visible.iter().collect();
        scopes.sort();
        let key = format!(
            "{data_version}|{}|{}",
            self.connection.total_changes(),
            scopes
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        );
        if let Some((cached, passages)) = self.passages.borrow().as_ref()
            && *cached == key
        {
            return Ok(Arc::clone(passages));
        }
        let (names, bound) = parameters("$scope", visible.iter());
        let sql = format!(
            "SELECT rowid, object_id, scope_id, kind, passage, length FROM search WHERE scope_id IN ({names})"
        );
        let mut statement = self.connection.prepare(&sql).map_err(|e| storage(&e))?;
        type Row = (i64, String, String, String, i32, i32);
        let rows: Vec<Row> = statement
            .query_map(as_named(&bound).as_slice(), |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            })
            .and_then(Iterator::collect)
            .map_err(|e| storage(&e))?;
        let mut passages = HashMap::with_capacity(rows.len());
        for (rowid, object_id, scope_id, kind, passage, length) in rows {
            // A malformed identifier is the `FormatException` of `Guid.Parse`.
            let object_id = crate::guid::try_parse(&object_id).ok_or_else(invalid_storage)?;
            passages.insert(
                rowid,
                VisiblePassage {
                    object_id,
                    scope_id,
                    kind,
                    passage,
                    length,
                },
            );
        }
        let passages = Arc::new(passages);
        *self.passages.borrow_mut() = Some((key, Arc::clone(&passages)));
        Ok(passages)
    }

    /// Kind, status and creation time of the visible objects, without their content, in the
    /// order SQLite returns them (`SqliteStore.ReadSummaries`, no `ORDER BY`).
    pub fn read_summaries(&self, visible: &HashSet<String>) -> Result<Vec<RecordSummary>> {
        let (names, bound) = parameters("$scope", visible.iter());
        let sql = format!(
            "SELECT id, kind, json_extract(payload,'$.status'), json_extract(payload,'$.provenance.timestamp')
            FROM records WHERE scope_id IN ({names})"
        );
        let mut statement = self.connection.prepare(&sql).map_err(|e| storage(&e))?;
        let rows: Vec<(String, String, String, String)> = statement
            .query_map(as_named(&bound).as_slice(), |row| {
                Ok((row.get(0)?, row.get(1)?, text(row, 2)?, text(row, 3)?))
            })
            .and_then(Iterator::collect)
            .map_err(|e| storage(&e))?;
        rows.into_iter()
            .map(|(id, kind, status, created_at)| {
                Ok(RecordSummary {
                    id: crate::guid::try_parse(&id).ok_or_else(invalid_storage)?,
                    kind: member(parse_dotnet_enum::<RecordKind>(&kind, false)?)?,
                    status: member(parse_dotnet_enum::<KnowledgeStatus>(&status, true)?)?,
                    created_at: parse_json_text(&created_at, |_| 0).ok_or_else(invalid_storage)?,
                })
            })
            .collect()
    }

    /// The text of a passage, empty when there is none (`SqliteStore.PassageText`).
    pub fn passage_text(&self, document: i64) -> Result<String> {
        let text: Option<rusqlite::types::Value> = self
            .connection
            .query_row(
                "SELECT text FROM search WHERE rowid=?1",
                [document],
                |row| row.get(0),
            )
            .map(Some)
            .or_else(|error| match error {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(storage(&other)),
            })?;
        // `ExecuteScalar() as string`: anything but text is the empty string.
        Ok(match text {
            Some(rusqlite::types::Value::Text(text)) => text,
            _ => String::new(),
        })
    }
}

/// The member of an enumeration value; an undefined value (a number `Enum.Parse` accepts) is
/// refused as corrupt storage.
fn member<E: crate::records::DotNetEnum>(value: i64) -> Result<E> {
    usize::try_from(value)
        .ok()
        .and_then(|index| E::NAMES.get(index))
        .map(|(_, _, m)| *m)
        .ok_or_else(invalid_storage)
}

/// A column read as `GetString` reads it: an integer gives its decimal text (a status stored as
/// a JSON number); any other non-text value is a storage error.
fn text(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<String> {
    match row.get_ref(index)? {
        rusqlite::types::ValueRef::Integer(value) => Ok(value.to_string()),
        _ => row.get(index),
    }
}
