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

impl MemoryContext {
    /// The context with its visible scopes recomputed from the store, as the constructors of
    /// `MemoryService` and `ScopeService` do: the list of the caller is never trusted.
    pub fn checked(&self, store: &Store) -> Result<MemoryContext> {
        let scopes = store.read_scopes()?;
        validate_tree(&scopes)?;
        Ok(MemoryContext {
            visible: visible_ancestors(&self.scope_id, &scopes)?,
            ..self.clone()
        })
    }
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
    context: MemoryContext,
}

impl<'a> MemoryService<'a> {
    /// The service of `context`, whose visible scopes are recomputed from the store.
    pub fn new(store: &'a mut Store, context: &MemoryContext) -> Result<Self> {
        let context = context.checked(store)?;
        Ok(Self { store, context })
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::PathBuf;

    use uuid::Uuid;

    use super::{MemoryContext, MemoryService, resolve_context};
    use crate::format::DateTimeOffset;
    use crate::init::initialize;
    use crate::records::{
        ActorKind, ClaimContent, MemoryCategory, Provenance, Scope, ScopeKind, Volatility,
    };
    use crate::store::Store;

    /// An instance with the clients `a` and `b` under the temporary directory, removed on drop.
    /// Fields drop in order: the store closes its database before `_cleanup` removes the folder.
    struct Fixture {
        root: PathBuf,
        store: Store,
        _cleanup: Cleanup,
    }

    struct Cleanup(PathBuf);

    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> Fixture {
        let temp = std::env::temp_dir();
        // The discovery compares long names: the 8.3 names of the folder are expanded.
        let temp = if cfg!(windows) {
            crate::discovery::expand_short_names(&temp)
        } else {
            std::fs::canonicalize(temp).expect("temp")
        };
        let root = temp.join(format!("sermofur-memory-unit-{}", Uuid::new_v4().simple()));
        std::fs::create_dir_all(&root).expect("root");
        let initialized = initialize(&root).expect("init");
        let mut store =
            Store::open(&root, initialized.configuration.instance_id, false).expect("store");
        for id in ["a", "b"] {
            std::fs::create_dir(root.join(id)).expect("client folder");
            let scope = Scope {
                id: id.to_owned(),
                kind: ScopeKind::Client,
                parent_id: Some("workspace".to_owned()),
                relative_path: Some(id.to_owned()),
            };
            store.add_scope(&scope, |_| Ok(())).expect("scope");
        }
        Fixture {
            _cleanup: Cleanup(root.clone()),
            root,
            store,
        }
    }

    fn claim(text: String) -> ClaimContent {
        ClaimContent {
            text: Some(text),
            category: MemoryCategory::Semantic,
            volatility: Volatility::Evolving,
            last_verified: None,
            review_after: None,
        }
    }

    fn user() -> Provenance {
        Provenance {
            origin: ActorKind::User,
            actor: Some("local-user".to_owned()),
            timestamp: DateTimeOffset::now_utc(),
        }
    }

    /// Port of `ServiceRecomputesVisibilityInsteadOfTrustingCallerList`.
    #[test]
    fn the_service_recomputes_visibility_instead_of_trusting_the_caller() {
        let mut f = fixture();
        let scopes = f.store.read_scopes().expect("scopes");
        let a = resolve_context(&f.root.join("a"), &f.root, &scopes).expect("context a");
        let id = MemoryService::new(&mut f.store, &a)
            .expect("service")
            .create_claim(&claim("private-a".to_owned()), user(), None)
            .expect("claim")
            .id;
        let forged = MemoryContext {
            root: f.root.clone(),
            scope_id: "b".to_owned(),
            visible: HashSet::from(["workspace", "a", "b"].map(str::to_owned)),
        };
        let memory = MemoryService::new(&mut f.store, &forged).expect("service");
        assert!(memory.list(None).expect("list").is_empty());
        assert_eq!(memory.get(id).expect_err("hidden").code, "not_found");
    }

    /// Port of the text part of `OversizedAndMalformedInputsCannotCreateObjects`: the service
    /// bound holds on its own, behind the bound of the command line. An out-of-range category
    /// cannot be built in Rust.
    #[test]
    fn an_oversized_text_creates_nothing() {
        let mut f = fixture();
        let scopes = f.store.read_scopes().expect("scopes");
        let workspace = resolve_context(&f.root, &f.root, &scopes).expect("context");
        let mut memory = MemoryService::new(&mut f.store, &workspace).expect("service");
        let error = memory
            .create_claim(&claim("x".repeat(16_385)), user(), None)
            .expect_err("too long");
        assert_eq!(error.code, "invalid_input");
        assert!(memory.list(None).expect("list").is_empty());
    }
}
