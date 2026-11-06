//! The memory of the current context, as `InstanceManager.ResolveContext` and `MemoryService`
//! of the .NET oracle (`rust/tests/golden/cli-3-4` is the truth): the scope of the working
//! folder, then claims, evidence and RETEX read, created and invalidated within the scopes it
//! sees, each input checked before the store is touched.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::confidence::evaluate_confidence;
use crate::discovery::validate_directory;
use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::limits::{TEXT_MESSAGE, is_out_of_bounds};
use crate::local_paths::{contains, normalize_mapping};
use crate::records::{
    ActorKind, ClaimContent, Confidence, EvidenceContent, HistoryEntry, KnowledgeStatus,
    MemoryCategory, MemoryRecord, Provenance, RecordKind, RetexContent, SourceContent,
    SourceStatus, read, write,
};
use crate::scope_policy::{WORKSPACE_SCOPE_ID, validate_tree, visible_ancestors};
use crate::store::{Store, StoredScope};

/// The offset of local time, which reads a stored date written without offset.
// ponytail: UTC until lot 7.2 ports the local offset (DateTimeOffset.Now); only a date stored
// without offset is read differently.
fn local_offset(_utc_ticks: i64) -> i16 {
    0
}

/// The instance root, the scope of the working folder and the scopes it sees
/// (`MemoryContext`).
#[derive(Clone, Debug)]
pub struct MemoryContext {
    pub root: PathBuf,
    pub scope_id: String,
    pub visible: HashSet<String>,
}

/// `ResolveContext`: the scope whose mapping is the longest one holding `path`, the workspace
/// otherwise. Mappings are compared lexically; only the one selected must exist on disk.
pub fn resolve_context(path: &Path, root: &Path, scopes: &[StoredScope]) -> Result<MemoryContext> {
    validate_tree(scopes)?;
    let current = validate_directory(path)?;
    let mut selected: Option<(&StoredScope, PathBuf, usize)> = None;
    for scope in scopes {
        let Some(relative) = scope.relative_path.as_deref() else {
            continue;
        };
        let mapped = normalize_mapping(root, relative)?;
        if !contains(&mapped, &current) {
            continue;
        }
        // `OrderByDescending(Length).ThenByDescending(Kind).First()`: the first of the best.
        let length = mapped.to_string_lossy().encode_utf16().count();
        let better = selected
            .as_ref()
            .is_none_or(|(best, _, best_length)| (length, scope.kind) > (*best_length, best.kind));
        if better {
            selected = Some((scope, mapped, length));
        }
    }
    let scope_id = match selected {
        Some((scope, mapped, _)) => {
            validate_directory(&mapped)?;
            scope.id.clone()
        }
        None => WORKSPACE_SCOPE_ID.to_owned(),
    };
    let visible = visible_ancestors(&scope_id, scopes)?;
    Ok(MemoryContext {
        root: root.to_path_buf(),
        scope_id,
        visible,
    })
}

/// `MemoryService.ValidateText`: a text that is not blank, within bounds, without NUL.
pub fn validate_text(value: &str) -> Result<()> {
    if value.chars().all(char::is_whitespace) || is_out_of_bounds(value) {
        return Err(SermofurError::new("invalid_input", TEXT_MESSAGE, 1));
    }
    Ok(())
}

fn not_found() -> SermofurError {
    SermofurError::new("not_found", "Object missing or inaccessible.", 1)
}

fn content<T: crate::records::Record>(record: &MemoryRecord) -> Result<T> {
    let json = record
        .content_json
        .as_deref()
        .ok_or_else(crate::instance::invalid_storage)?;
    read(json, &local_offset)
}

/// A claim with its confidence, its evidence and its history (`ClaimExplanation`).
#[derive(Debug, Serialize)]
pub struct ClaimExplanation {
    pub claim: MemoryRecord,
    pub confidence: Confidence,
    pub evidence: Vec<MemoryRecord>,
    pub history: Vec<HistoryEntry>,
}

/// `MemoryService`: the records of the scopes the context sees.
pub struct MemoryService<'a> {
    store: &'a mut Store,
    context: &'a MemoryContext,
}

impl<'a> MemoryService<'a> {
    pub fn new(store: &'a mut Store, context: &'a MemoryContext) -> Self {
        Self { store, context }
    }

    /// The visible records, of one kind or all.
    pub fn list(&self, kind: Option<RecordKind>) -> Result<Vec<MemoryRecord>> {
        self.store.read_records(&self.context.visible, kind)
    }

    /// A visible record, or `not_found`.
    pub fn get(&self, id: Uuid) -> Result<MemoryRecord> {
        self.store
            .find_record(id, &self.context.visible)?
            .ok_or_else(not_found)
    }

    pub fn create_claim(
        &mut self,
        claim: &ClaimContent,
        provenance: Provenance,
        key: Option<&str>,
    ) -> Result<MemoryRecord> {
        validate_text(claim.text.as_deref().unwrap_or(""))?;
        if matches!(
            claim.category,
            MemoryCategory::Preferences | MemoryCategory::Decisions
        ) && provenance.origin != ActorKind::User
        {
            return Err(SermofurError::new(
                "user_choice_required",
                "A personal choice requires a user origin.",
                1,
            ));
        }
        self.create(RecordKind::Claim, claim, provenance, key)
    }

    pub fn create_evidence(
        &mut self,
        evidence: &EvidenceContent,
        provenance: Provenance,
        key: Option<&str>,
    ) -> Result<MemoryRecord> {
        validate_text(evidence.reference.as_deref().unwrap_or(""))?;
        validate_text(evidence.lineage_id.as_deref().unwrap_or(""))?;
        let claim = self.get(evidence.claim_id)?;
        if claim.kind != RecordKind::Claim
            || claim.scope_id.as_deref() != Some(self.context.scope_id.as_str())
        {
            return Err(SermofurError::new(
                "scope_boundary",
                "Evidence and claim must share the current scope.",
                4,
            ));
        }
        let mut stored = evidence.clone();
        stored.source_hash = None;
        if let Some(source_id) = evidence.source_id {
            // The source must be visible; its hash at this moment is frozen with the evidence.
            let source = self.get(source_id)?;
            if source.kind != RecordKind::Source {
                return Err(not_found());
            }
            let cited: SourceContent = content(&source)?;
            if cited.status != SourceStatus::Indexed {
                return Err(SermofurError::new(
                    "source_unavailable",
                    "The cited source is missing, unreadable or rejected; reindex it first.",
                    1,
                ));
            }
            stored.source_hash = cited.hash;
        }
        self.create(RecordKind::Evidence, &stored, provenance, key)
    }

    pub fn create_retex(
        &mut self,
        retex: &RetexContent,
        provenance: Provenance,
        key: Option<&str>,
    ) -> Result<MemoryRecord> {
        for text in [&retex.event, &retex.impact, &retex.next_action] {
            validate_text(text.as_deref().unwrap_or(""))?;
        }
        self.create(RecordKind::Retex, retex, provenance, key)
    }

    fn create<T: Serialize>(
        &mut self,
        kind: RecordKind,
        body: &T,
        provenance: Provenance,
        key: Option<&str>,
    ) -> Result<MemoryRecord> {
        validate_text(provenance.actor.as_deref().unwrap_or(""))?;
        if let Some(key) = key {
            validate_text(key)?;
        }
        let status = if kind == RecordKind::Retex {
            KnowledgeStatus::Draft
        } else {
            KnowledgeStatus::Proposed
        };
        let candidate = MemoryRecord {
            id: Uuid::new_v4(),
            scope_id: Some(self.context.scope_id.clone()),
            kind,
            status,
            revision: 1,
            content_json: Some(write(body)?),
            provenance: Some(provenance),
        };
        self.store.create_record(&candidate, key)
    }

    /// A claim with its confidence now, its visible evidence and its history (`Explain`).
    pub fn explain(&self, id: Uuid) -> Result<ClaimExplanation> {
        let claim = self.get(id)?;
        if claim.kind != RecordKind::Claim {
            return Err(SermofurError::new("wrong_kind", "A claim is required.", 1));
        }
        let mut evidence = Vec::new();
        for record in self.list(Some(RecordKind::Evidence))? {
            if content::<EvidenceContent>(&record)?.claim_id == id {
                evidence.push(record);
            }
        }
        let confidence =
            evaluate_confidence(&claim, &evidence, DateTimeOffset::now_utc(), &local_offset)?;
        let history = self.store.read_history(id, &self.context.visible)?;
        Ok(ClaimExplanation {
            claim,
            confidence,
            evidence,
            history,
        })
    }

    /// Invalidates a claim of the current scope.
    pub fn invalidate(&mut self, id: Uuid, reason: &str, actor: &str) -> Result<MemoryRecord> {
        validate_text(reason)?;
        validate_text(actor)?;
        let record = self.get(id)?;
        if record.kind != RecordKind::Claim
            || record.scope_id.as_deref() != Some(self.context.scope_id.as_str())
        {
            return Err(SermofurError::new(
                "scope_boundary",
                "Only a claim of the current scope can be invalidated.",
                4,
            ));
        }
        self.store
            .invalidate_record(id, &self.context.scope_id, reason, actor)
    }

    /// Rewrites the projection of every visible record; their paths.
    pub fn export(&self) -> Result<Vec<PathBuf>> {
        self.store.export(&self.context.visible)
    }
}
