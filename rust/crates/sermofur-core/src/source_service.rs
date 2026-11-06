//! The declared sources of the current scope, as `SourceService` and `SourcePlacement` of the
//! .NET oracle (`rust/tests/golden/cli-5-2` and `source-service*.json` are the truth): explicit
//! add, list, show and reindexing. A source belongs to the narrowest scope whose folder holds
//! its file, never to an ancestor that would make it visible to sibling scopes.

use std::collections::HashSet;
use std::path::Path;

use serde::Serialize;
use uuid::Uuid;

use crate::casing::ordinal_ignore_case_equal;
use crate::discovery::{full_path, get_full_path};
use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::instance::invalid_storage;
use crate::limits::{TEXT_MESSAGE, is_out_of_bounds};
use crate::local_paths::{canonical_case, contains, normalize_mapping, relativize_mapping};
use crate::memory::{MemoryContext, validate_text};
use crate::ownership::SystemOwnership;
use crate::records::{
    HistoryEntry, KnowledgeStatus, MemoryRecord, Provenance, RecordKind, SourceContent,
    SourceStatus, read, write,
};
use crate::scope_policy::{ScopeLike, validate_tree, visible_ancestors};
use crate::sources::{SourceSnapshot, refuse_unindexable};
use crate::store::{SourceChange, Store, StoredScope, holds};

/// Reads a declared source (`ISourceReader.Read`): the instance root and the stored path.
pub type SourceReader<'a> = &'a dyn Fn(&Path, &str) -> Result<SourceSnapshot>;

/// The reader of the command line (`FileSourceReader` on the real ownership rules).
pub fn file_reader(root: &Path, relative: &str) -> Result<SourceSnapshot> {
    crate::sources::read(root, relative, &SystemOwnership)
}

/// Outcome of a reindexing, per source (`ReindexOutcome`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReindexOutcome {
    Unchanged,
    Modified,
    Missing,
    Unreadable,
    Rejected,
    Restored,
    /// Changed by another command since it was listed: left as that command wrote it.
    Skipped,
}

impl ReindexOutcome {
    /// `outcome.ToString().ToLowerInvariant()`, the reason of the history entry.
    fn reason(self) -> &'static str {
        match self {
            Self::Unchanged => "unchanged",
            Self::Modified => "modified",
            Self::Missing => "missing",
            Self::Unreadable => "unreadable",
            Self::Rejected => "rejected",
            Self::Restored => "restored",
            Self::Skipped => "skipped",
        }
    }
}

/// One line of `source reindex` (`ReindexEntry`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReindexEntry {
    pub id: String,
    pub path: String,
    pub outcome: ReindexOutcome,
    pub previous_hash: Option<String>,
    pub hash: Option<String>,
}

/// Actor recorded for the changes made by an index rebuild, an instance operation
/// (`SourceService.SystemActor`).
pub const SYSTEM_ACTOR: &str = "sermofur";

/// Result of an index rebuild, counted on visible objects only (`IndexRebuild`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexRebuild {
    pub indexed: usize,
    pub changed_sources: usize,
}

/// A source with its history, as `source show` prints it (`SourceView`).
#[derive(Debug, Serialize)]
pub struct SourceView {
    pub source: MemoryRecord,
    pub history: Vec<HistoryEntry>,
}

/// `SourceService.Content`.
pub fn content(source: &MemoryRecord) -> Result<SourceContent> {
    let json = source.content_json.as_deref().ok_or_else(invalid_storage)?;
    read(json, &|_| 0)
}

fn not_found() -> SermofurError {
    SermofurError::new("not_found", "Object missing or inaccessible.", 1)
}

fn boundary(message: &str) -> SermofurError {
    SermofurError::new("scope_boundary", message, 4)
}

/// The narrowest scope below `owner` whose mapping holds the stored path `relative`, or `None`
/// when the owner is already the narrowest (`SourcePlacement.NarrowerScope`): the longest
/// mapping (UTF-16 units, `/` trimmed), then the greatest kind, then the first in `scopes`.
pub fn narrower_scope<'s>(
    relative: &str,
    owner: &str,
    scopes: &'s [StoredScope],
) -> Result<Option<&'s StoredScope>> {
    let mut best: Option<(&StoredScope, (usize, i64))> = None;
    for scope in scopes {
        let Some(mapping) = scope.relative_path.as_deref() else {
            continue;
        };
        if scope.id == owner
            || !visible_ancestors(scope.id(), scopes)?.contains(owner)
            || !holds(mapping, relative)
        {
            continue;
        }
        let rank = (mapping.trim_matches('/').encode_utf16().count(), scope.kind);
        if best.is_none_or(|(_, best_rank)| rank > best_rank) {
            best = Some((scope, rank));
        }
    }
    Ok(best.map(|(scope, _)| scope))
}

fn refuse_narrower(relative: &str, owner: &str, scopes: &[StoredScope]) -> Result<()> {
    if narrower_scope(relative, owner, scopes)?.is_some() {
        return Err(boundary(
            "The file belongs to a narrower scope; add it from that scope's directory.",
        ));
    }
    Ok(())
}

/// `SourceService.Indexed`: the content of a freshly indexed source, stamped now.
fn indexed(relative: Option<&str>, snapshot: &SourceSnapshot) -> SourceContent {
    SourceContent {
        relative_path: relative.map(str::to_owned),
        status: SourceStatus::Indexed,
        hash: Some(snapshot.hash.clone()),
        size: snapshot.size,
        indexed_at: Some(DateTimeOffset::now_utc()),
        passages: i32::try_from(snapshot.passages.len()).unwrap_or(i32::MAX),
        rejection: None,
    }
}

fn change(
    current: &MemoryRecord,
    content: &SourceContent,
    outcome: ReindexOutcome,
    actor: &str,
) -> Result<SourceChange> {
    let mut next = current.clone();
    next.revision = current.revision.wrapping_add(1);
    next.content_json = Some(write(content)?);
    Ok(SourceChange {
        next,
        reason: outcome.reason().to_owned(),
        actor: actor.to_owned(),
    })
}

/// The table of transitions of the data model (`SourceService.Transition`); no change when
/// nothing changed.
pub fn transition(
    current: &MemoryRecord,
    snapshot: &SourceSnapshot,
    actor: &str,
) -> Result<(Option<SourceChange>, ReindexOutcome)> {
    let previous = content(current)?;
    if snapshot.status == SourceStatus::Indexed {
        if previous.status == SourceStatus::Indexed
            && previous.hash.as_deref() == Some(snapshot.hash.as_str())
        {
            return Ok((None, ReindexOutcome::Unchanged));
        }
        let outcome = if previous.status == SourceStatus::Indexed {
            ReindexOutcome::Modified
        } else {
            ReindexOutcome::Restored
        };
        // A missing path is kept missing, as `Indexed(previous.RelativePath, ...)` of .NET
        // (altered base only; `reindex` refuses such a source earlier, before reading it).
        let next = change(
            current,
            &indexed(previous.relative_path.as_deref(), snapshot),
            outcome,
            actor,
        )?;
        return Ok((Some(next), outcome));
    }
    if previous.status == snapshot.status && previous.rejection == snapshot.rejection {
        return Ok((None, ReindexOutcome::Unchanged));
    }
    let lost = match snapshot.status {
        SourceStatus::Missing => ReindexOutcome::Missing,
        SourceStatus::Unreadable => ReindexOutcome::Unreadable,
        _ => ReindexOutcome::Rejected,
    };
    let kept = SourceContent {
        status: snapshot.status,
        rejection: snapshot.rejection,
        passages: 0,
        ..previous
    };
    Ok((Some(change(current, &kept, lost, actor)?), lost))
}

/// `SourceService`: the sources the context sees, and those of its own scope it declares.
pub struct SourceService<'a> {
    store: &'a mut Store,
    reader: SourceReader<'a>,
    context: MemoryContext,
    /// The scopes read when the service was built; `source add` checks them again under the
    /// write lock.
    scopes: Vec<StoredScope>,
}

impl<'a> SourceService<'a> {
    /// The service of `context`: the scope tree is checked and the visible scopes recomputed
    /// from the store.
    pub fn new(
        store: &'a mut Store,
        reader: SourceReader<'a>,
        context: &MemoryContext,
    ) -> Result<Self> {
        let scopes = store.read_scopes()?;
        validate_tree(&scopes)?;
        let context = MemoryContext {
            visible: visible_ancestors(&context.scope_id, &scopes)?,
            ..context.clone()
        };
        Ok(Self {
            store,
            reader,
            context,
            scopes,
        })
    }

    /// The visible sources.
    pub fn list(&self) -> Result<Vec<MemoryRecord>> {
        self.store
            .read_records(&self.context.visible, Some(RecordKind::Source))
    }

    /// A visible source with its history.
    pub fn show(&self, id: Uuid) -> Result<SourceView> {
        let source = self.get(id)?;
        let history = self.store.read_history(id, &self.context.visible)?;
        Ok(SourceView { source, history })
    }

    fn get(&self, id: Uuid) -> Result<MemoryRecord> {
        self.store
            .find_record(id, &self.context.visible)?
            .filter(|record| record.kind == RecordKind::Source)
            .ok_or_else(not_found)
    }

    /// Declares `file` (a full path) as a source of the current scope, or indexes it again when
    /// it already is one.
    pub fn add(&mut self, file: &Path, provenance: Provenance) -> Result<MemoryRecord> {
        validate_text(provenance.actor.as_deref().unwrap_or(""))?;
        let relative = self.locate(file)?;
        let snapshot = (self.reader)(&self.context.root, &relative)?;
        refuse_unindexable(&snapshot)?;
        let scope_id = self.context.scope_id.clone();
        let saved = self.store.save_source(
            &relative,
            |existing, live_scopes| {
                let Some(existing) = existing else {
                    // Checked again under the write lock: a scope created meanwhile on the
                    // file's folder would otherwise leave the source in a broader scope.
                    refuse_narrower(&relative, &scope_id, live_scopes)?;
                    let actor = provenance.actor.clone().unwrap_or_default();
                    let record = MemoryRecord {
                        id: Uuid::new_v4(),
                        scope_id: Some(scope_id.clone()),
                        kind: RecordKind::Source,
                        status: KnowledgeStatus::Active,
                        revision: 1,
                        content_json: Some(write(&indexed(Some(&relative), &snapshot))?),
                        provenance: Some(provenance.clone()),
                    };
                    return Ok(Some(SourceChange {
                        next: record,
                        reason: "create".to_owned(),
                        actor,
                    }));
                };
                if existing.scope_id.as_deref() != Some(scope_id.as_str()) {
                    return Err(boundary(
                        "This file is already declared as a source of another scope.",
                    ));
                }
                let actor = provenance.actor.as_deref().unwrap_or("");
                Ok(transition(existing, &snapshot, actor)?.0)
            },
            &snapshot.passages,
        )?;
        // A missing source is created or refused: something is saved.
        saved.ok_or_else(invalid_storage)
    }

    /// The stored path of a file of the current scope, or a refusal (`Locate`).
    fn locate(&self, file: &Path) -> Result<String> {
        let text = file.to_string_lossy();
        if text.chars().all(char::is_whitespace) || is_out_of_bounds(&text) {
            return Err(SermofurError::new("invalid_input", TEXT_MESSAGE, 1));
        }
        let root = self.context.root.as_path();
        // `Path.GetFullPath`: an NT path stays as typed, then lies outside the instance.
        let typed = get_full_path(file)?;
        if !contains(root, &typed) {
            return Err(boundary("Source outside the instance."));
        }
        // One file, one stored path: the names found on disk, whatever the case typed. Every
        // check below runs on that form, so a case variant or an alias cannot slip past them.
        let relative = canonical_case(root, &relativize_mapping(root, &typed)?)?;
        let full = full_path(&root.join(&relative))?;
        let first = relative.split('/').next().unwrap_or("");
        let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        if contains(&root.join(".sermofur"), &full)
            || ordinal_ignore_case_equal(&units(first), &units(".sermofur"))
        {
            return Err(SermofurError::new(
                "unsafe_path",
                "Sermofur's own files cannot be sources.",
                4,
            ));
        }
        let current = self
            .scopes
            .iter()
            .find(|scope| scope.id == self.context.scope_id)
            .ok_or_else(invalid_storage)?;
        let directory = match current.relative_path.as_deref() {
            None => root.to_path_buf(),
            Some(mapping) => normalize_mapping(root, mapping)?,
        };
        if !contains(&directory, &full) {
            return Err(boundary(
                "The file is outside the directory of the current scope.",
            ));
        }
        refuse_narrower(&relative, &self.context.scope_id, &self.scopes)?;
        Ok(relative)
    }

    /// Rebuilds the whole index in one write transaction: claims and RETEX from the registry,
    /// then every source of the instance read again, one at a time, with the transitions of
    /// [`Self::reindex`], recorded under [`SYSTEM_ACTOR`]. Counts cover visible objects only
    /// (`RebuildIndex`).
    pub fn rebuild_index(&mut self) -> Result<IndexRebuild> {
        let root = self.context.root.clone();
        let reader = self.reader;
        let changed = self.store.rebuild_index(|source| {
            // A missing path is the `NullReferenceException` of .NET (altered base only).
            let path = content(source)?.relative_path.ok_or_else(invalid_storage)?;
            let snapshot = reader(&root, &path)?;
            let (change, _) = transition(source, &snapshot, SYSTEM_ACTOR)?;
            Ok((change, snapshot.passages))
        })?;
        let mut indexed = 0;
        for record in self.store.read_records(&self.context.visible, None)? {
            indexed += usize::from(crate::search::is_indexed(&record)?);
        }
        let changed_sources = changed
            .iter()
            .filter(|source| {
                source
                    .scope_id
                    .as_ref()
                    .is_some_and(|scope| self.context.visible.contains(scope))
            })
            .count();
        Ok(IndexRebuild {
            indexed,
            changed_sources,
        })
    }

    /// Reads again the sources of the current scope, or one of them, and records what changed;
    /// never reads a file that is not a declared source. A source changed by another command
    /// since it was listed is left as that command wrote it (`Reindex`).
    pub fn reindex(&mut self, id: Option<Uuid>, actor: &str) -> Result<Vec<ReindexEntry>> {
        validate_text(actor)?;
        let own = HashSet::from([self.context.scope_id.clone()]);
        let mut sources = self.store.read_records(&own, Some(RecordKind::Source))?;
        if let Some(id) = id {
            sources.retain(|source| source.id == id);
            if sources.is_empty() {
                // Same answer for an unknown source and one of another scope.
                return Err(not_found());
            }
        }
        let mut entries = Vec::with_capacity(sources.len());
        for source in sources {
            let previous = content(&source)?;
            // A missing path is the `NullReferenceException` of .NET (altered base only).
            let path = previous.relative_path.clone().ok_or_else(invalid_storage)?;
            let snapshot = (self.reader)(&self.context.root, &path)?;
            let mut outcome = ReindexOutcome::Unchanged;
            let saved = self.store.save_source(
                &path,
                |current, _| match current {
                    Some(current)
                        if current.id == source.id && current.revision == source.revision =>
                    {
                        let (next, result) = transition(current, &snapshot, actor)?;
                        outcome = result;
                        Ok(next)
                    }
                    _ => {
                        outcome = ReindexOutcome::Skipped;
                        Ok(None)
                    }
                },
                &snapshot.passages,
            )?;
            // A missing hash is the `NullReferenceException` of .NET, after the save.
            let previous_hash = previous.hash.clone().ok_or_else(invalid_storage)?;
            let hash = match saved {
                Some(saved) if outcome != ReindexOutcome::Skipped => content(&saved)?.hash,
                _ => Some(previous_hash.clone()),
            };
            entries.push(ReindexEntry {
                id: source.id.hyphenated().to_string(),
                path,
                outcome,
                previous_hash: (!previous_hash.is_empty()).then_some(previous_hash),
                hash,
            });
        }
        Ok(entries)
    }
}
