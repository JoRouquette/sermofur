//! Reading of the visible scopes and registration of a child scope of the current context, as
//! `ScopeService` of the .NET oracle (`rust/tests/golden/cli-3-4` is the truth). Every
//! registration rule lives here; the store only replays the last ones under its transaction.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::{Result, SermofurError};
use crate::limits::{SCOPE_MESSAGE, is_out_of_bounds};
use crate::local_paths::{
    canonical_case, contains, normalize_mapping, relativize_mapping, resolve_mapping,
};
use crate::memory::MemoryContext;
use crate::records::{Scope, ScopeKind};
use crate::scope_policy::{ScopeLike, expected_parent, kind_value, visible_ancestors};
use crate::store::{Store, StoredScope};

const DUPLICATE_MAPPING: &str = "duplicate_mapping";
const SCOPE_BOUNDARY: &str = "scope_boundary";

fn invalid_scope(message: &str) -> SermofurError {
    SermofurError::new("invalid_scope", message, 1)
}

fn boundary(message: &str) -> SermofurError {
    SermofurError::new(SCOPE_BOUNDARY, message, 4)
}

/// The current scope and the scopes it sees, sorted by identifier (`ScopeContextView`).
#[derive(Debug, Serialize)]
pub struct ScopeContextView {
    pub scope: String,
    pub visible: Vec<String>,
}

/// `Current`.
pub fn current(context: &MemoryContext) -> ScopeContextView {
    let mut visible: Vec<String> = context.visible.iter().cloned().collect();
    visible.sort();
    ScopeContextView {
        scope: context.scope_id.clone(),
        visible,
    }
}

/// `List`: the visible scopes, without siblings or descendants.
pub fn list(store: &Store, context: &MemoryContext) -> Result<Vec<Scope>> {
    store
        .read_scopes()?
        .iter()
        .filter(|scope| context.visible.contains(&scope.id))
        .map(StoredScope::to_scope)
        .collect()
}

/// `^[a-z][a-z0-9-]{0,63}$`.
fn is_slug(value: &str) -> bool {
    let bytes = value.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// `Register`: a direct child of the current scope, after checking its identifier, its kind
/// and its mapping; the scope as stored, its mapping as named on disk.
pub fn register(store: &mut Store, context: &MemoryContext, candidate: Scope) -> Result<Scope> {
    let values = [
        Some(candidate.id.as_str()),
        candidate.parent_id.as_deref(),
        candidate.relative_path.as_deref(),
    ];
    if values.into_iter().flatten().any(is_out_of_bounds) {
        return Err(invalid_scope(SCOPE_MESSAGE));
    }
    if !is_slug(&candidate.id) {
        return Err(invalid_scope(
            "Scope identifier: slug of 1 to 64 characters.",
        ));
    }
    if !candidate.parent_id.as_deref().is_some_and(is_slug) {
        return Err(invalid_scope("Invalid scope parent."));
    }
    let scopes = store.read_scopes()?;
    if candidate.parent_id.as_deref() != Some(context.scope_id.as_str()) {
        return Err(boundary(
            "The new scope must be a child of the current context.",
        ));
    }
    let parent = scopes
        .iter()
        .find(|scope| scope.id == context.scope_id)
        .ok_or_else(crate::instance::invalid_storage)?;
    if candidate.kind == ScopeKind::Workspace
        || expected_parent(kind_value(candidate.kind))? != Some(parent.kind)
    {
        return Err(invalid_scope("Scope kind incompatible with its parent."));
    }
    let root = context.root.as_path();
    let mapped = validate_mapping(root, &candidate, parent)?;
    let normalized = Scope {
        relative_path: Some(relativize_mapping(root, &mapped)?),
        ..candidate
    };
    store.add_scope(&normalized, |current| {
        ensure_registrable(root, &normalized, current)
    })?;
    Ok(normalized)
}

fn validate_mapping(root: &Path, candidate: &Scope, parent: &StoredScope) -> Result<PathBuf> {
    let relative = candidate.relative_path.as_deref().unwrap_or("");
    if relative.chars().all(char::is_whitespace) {
        return Err(invalid_scope("Relative mapping required."));
    }
    // Both sides as named on disk, before any check.
    let mapped = on_disk(root, &resolve_mapping(root, relative)?)?;
    let parent_path = match parent.relative_path.as_deref() {
        None => root.to_path_buf(),
        Some(path) => on_disk(root, &resolve_mapping(root, path)?)?,
    };
    let may_share =
        candidate.kind == ScopeKind::Repository && parent.kind == kind_value(ScopeKind::Project);
    if !contains(&parent_path, &mapped) || (same_path(&mapped, &parent_path) && !may_share) {
        return Err(boundary(
            "Child mapping must lie inside its parent; only a repository may share its project's path.",
        ));
    }
    Ok(mapped)
}

/// `EnsureRegistrable`, replayed on the scopes read under the transaction of the store: a free
/// identifier, then no mapping conflict with an existing scope.
fn ensure_registrable(root: &Path, candidate: &Scope, scopes: &[StoredScope]) -> Result<()> {
    if scopes.iter().any(|scope| scope.id == candidate.id) {
        // Same message whoever the owner is: a sibling scope cannot be guessed.
        return Err(invalid_scope("Scope identifier unavailable."));
    }
    let mut with_candidate = scopes.to_vec();
    with_candidate.push(StoredScope::from_scope(candidate));
    let mut mappings: Vec<(&StoredScope, PathBuf)> = Vec::new();
    for scope in &with_candidate {
        if let Some(mapping) = scope.relative_path.as_deref() {
            mappings.push((scope, stored(root, mapping)?));
        }
    }
    let mut lineages: HashMap<&str, HashSet<String>> = HashMap::new();
    for scope in &with_candidate {
        lineages.insert(&scope.id, visible_ancestors(&scope.id, &with_candidate)?);
    }
    let Some(mine) = mappings.iter().find(|(s, _)| s.id == candidate.id) else {
        return Err(crate::instance::invalid_storage());
    };
    let codes: Vec<&str> = mappings
        .iter()
        .filter(|(s, _)| s.id != candidate.id)
        .filter_map(|other| conflict_between(mine, other, &lineages))
        .collect();
    if codes.contains(&DUPLICATE_MAPPING) {
        return Err(SermofurError::new(
            DUPLICATE_MAPPING,
            "Mapping already assigned.",
            1,
        ));
    }
    if !codes.is_empty() {
        return Err(boundary("Mapping nested in another branch of scopes."));
    }
    Ok(())
}

fn conflict_between(
    first: &(&StoredScope, PathBuf),
    second: &(&StoredScope, PathBuf),
    lineages: &HashMap<&str, HashSet<String>>,
) -> Option<&'static str> {
    if same_path(&first.1, &second.1) {
        return (!(is_repository_of(first.0, second.0) || is_repository_of(second.0, first.0)))
            .then_some(DUPLICATE_MAPPING);
    }
    let sees = |a: &StoredScope, b: &StoredScope| {
        lineages
            .get(a.id.as_str())
            .is_some_and(|lineage| lineage.contains(&b.id))
    };
    let same_lineage = sees(first.0, second.0) || sees(second.0, first.0);
    let nested = contains(&first.1, &second.1) || contains(&second.1, &first.1);
    (nested && !same_lineage).then_some(SCOPE_BOUNDARY)
}

/// Only a repository may share the path of its parent project.
fn is_repository_of(repository: &StoredScope, project: &StoredScope) -> bool {
    repository.kind == kind_value(ScopeKind::Repository)
        && project.kind == kind_value(ScopeKind::Project)
        && repository.parent_id() == Some(project.id.as_str())
}

/// `absolute` with each existing entry named as on disk (`OnDisk`); `unsafe_path` for an
/// alias.
fn on_disk(root: &Path, absolute: &Path) -> Result<PathBuf> {
    normalize_mapping(
        root,
        &canonical_case(root, &relativize_mapping(root, absolute)?)?,
    )
}

/// A stored mapping in the form the rules compare (`Stored`): as named on disk, lexical when
/// the disk cannot tell. Only a `SermofurException` falls back in .NET; a failure of the
/// runtime (`invalid_storage_or_path`) still stops.
fn stored(root: &Path, mapping: &str) -> Result<PathBuf> {
    let lexical = normalize_mapping(root, mapping)?;
    match on_disk(root, &lexical) {
        Ok(path) => Ok(path),
        Err(error) if error.code == "invalid_storage_or_path" => Err(error),
        Err(_) => Ok(lexical),
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    contains(left, right) && contains(right, left)
}
