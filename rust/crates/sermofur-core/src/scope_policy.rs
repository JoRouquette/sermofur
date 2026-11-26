//! The tree of scopes, as `ScopePolicy` of the .NET oracle (`rust/tests/golden/
//! scope-policy.json` is the truth): one workspace root, each kind under the kind above it, no
//! cycle, and a scope sees only itself and its ancestors.

use std::collections::{HashMap, HashSet};

use crate::error::{Result, SermofurError};
use crate::instance::invalid_storage;
use crate::records::{DotNetEnum, Scope, ScopeKind};

/// Reserved identifier of the single root of the tree.
pub const WORKSPACE_SCOPE_ID: &str = "workspace";

/// A scope as the policy reads it. The kind is the value of the .NET enumeration, so that a
/// row read from the store with an undefined kind (`Enum.Parse` accepts a number) is refused
/// where .NET refuses it.
pub trait ScopeLike {
    fn id(&self) -> &str;
    fn kind_value(&self) -> i64;
    fn parent_id(&self) -> Option<&str>;
    fn relative_path(&self) -> Option<&str>;
}

impl ScopeLike for Scope {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind_value(&self) -> i64 {
        kind_value(self.kind)
    }

    fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    fn relative_path(&self) -> Option<&str> {
        self.relative_path.as_deref()
    }
}

/// The .NET value of a kind: its index in the declaration.
pub fn kind_value(kind: ScopeKind) -> i64 {
    ScopeKind::NAMES
        .iter()
        .position(|(_, _, member)| *member == kind)
        .map_or(-1, |index| i64::try_from(index).unwrap_or(-1))
}

fn tree_error(message: &str) -> SermofurError {
    SermofurError::new("invalid_scope_tree", message, 3)
}

/// Kind of immediate parent required by a kind; `None` for the workspace root.
pub fn expected_parent(kind: i64) -> Result<Option<i64>> {
    match kind {
        0 => Ok(None),
        1..=4 => Ok(Some(kind - 1)),
        _ => Err(tree_error("Invalid scope kind.")),
    }
}

/// The identifiers `current` sees: itself and its ancestors. A cycle or a missing scope is
/// `invalid_scope_tree`; two scopes with one identifier make `ToDictionary` throw, which the CLI
/// answers with `invalid_storage_or_path`.
pub fn visible_ancestors<S: ScopeLike>(current: &str, scopes: &[S]) -> Result<HashSet<String>> {
    let mut by_id: HashMap<&str, &S> = HashMap::with_capacity(scopes.len());
    for scope in scopes {
        if by_id.insert(scope.id(), scope).is_some() {
            return Err(invalid_storage());
        }
    }
    let mut visible = HashSet::new();
    let mut next = Some(current);
    while let Some(id) = next {
        let scope = by_id.get(id);
        if !visible.insert(id.to_owned()) || scope.is_none() {
            return Err(tree_error("Invalid scope tree."));
        }
        next = scope.and_then(|s| s.parent_id());
    }
    Ok(visible)
}

/// Checks the whole tree, in the order of `ValidateTree`: unique identifiers, one workspace
/// root without mapping, then for each scope in order a defined kind, a path to the root, and
/// the expected kind of parent.
pub fn validate_tree<S: ScopeLike>(scopes: &[S]) -> Result<()> {
    let ids: HashSet<&str> = scopes.iter().map(ScopeLike::id).collect();
    if ids.len() != scopes.len() {
        return Err(tree_error("Duplicate scope identifier."));
    }
    let roots: Vec<&S> = scopes.iter().filter(|s| s.parent_id().is_none()).collect();
    let workspace = kind_value(ScopeKind::Workspace);
    if roots.len() != 1
        || roots[0].id() != WORKSPACE_SCOPE_ID
        || roots[0].kind_value() != workspace
        || roots[0].relative_path().is_some()
    {
        return Err(tree_error("Single workspace root required."));
    }
    for scope in scopes {
        if !(0..=4).contains(&scope.kind_value()) {
            return Err(tree_error("Invalid scope kind."));
        }
        visible_ancestors(scope.id(), scopes)?;
        let Some(parent_id) = scope.parent_id() else {
            continue;
        };
        // The walk above found the parent, and identifiers are unique.
        let parent = scopes
            .iter()
            .find(|s| s.id() == parent_id)
            .ok_or_else(|| tree_error("Invalid scope tree."))?;
        if expected_parent(scope.kind_value())? != Some(parent.kind_value()) {
            return Err(tree_error("Invalid scope hierarchy."));
        }
    }
    Ok(())
}
