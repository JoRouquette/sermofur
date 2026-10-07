namespace Sermofur.Domain;

public static class ScopePolicy
{
    /// <summary>Reserved identifier of the single root of the tree.</summary>
    public const string WorkspaceScopeId = "workspace";

    /// <summary>
    /// Kind of immediate parent required by a scope kind; null for the workspace root.
    /// </summary>
    public static ScopeKind? ExpectedParent(ScopeKind kind) =>
        kind switch
        {
            ScopeKind.Workspace => null,
            ScopeKind.Client => ScopeKind.Workspace,
            ScopeKind.Project => ScopeKind.Client,
            ScopeKind.Repository => ScopeKind.Project,
            ScopeKind.Task => ScopeKind.Repository,
            _ => throw new SermofurException("invalid_scope_tree", "Invalid scope kind.", 3),
        };

    public static IReadOnlySet<string> VisibleAncestors(
        string currentId,
        IReadOnlyList<Scope> scopes
    )
    {
        Dictionary<string, Scope> byId = scopes.ToDictionary(
            scope => scope.Id,
            StringComparer.Ordinal
        );
        HashSet<string> visible = new(StringComparer.Ordinal);
        string? next = currentId;
        while (next is not null)
        {
            if (!visible.Add(next) || !byId.TryGetValue(next, out Scope? scope))
            {
                throw new SermofurException("invalid_scope_tree", "Invalid scope tree.", 3);
            }
            next = scope.ParentId;
        }
        return visible;
    }

    public static void ValidateTree(IReadOnlyList<Scope> scopes)
    {
        if (scopes.Select(s => s.Id).Distinct(StringComparer.Ordinal).Count() != scopes.Count)
        {
            throw new SermofurException("invalid_scope_tree", "Duplicate scope identifier.", 3);
        }
        Scope[] roots = scopes.Where(s => s.ParentId is null).ToArray();
        if (
            roots.Length != 1
            || roots[0].Id != WorkspaceScopeId
            || roots[0].Kind != ScopeKind.Workspace
            || roots[0].RelativePath is not null
        )
        {
            throw new SermofurException("invalid_scope_tree", "Single workspace root required.", 3);
        }
        foreach (Scope scope in scopes)
        {
            if (!Enum.IsDefined(scope.Kind))
            {
                throw new SermofurException("invalid_scope_tree", "Invalid scope kind.", 3);
            }
            VisibleAncestors(scope.Id, scopes);
            if (scope.ParentId is null)
            {
                continue;
            }
            Scope parent = scopes.Single(s => s.Id == scope.ParentId);
            if (ExpectedParent(scope.Kind) != parent.Kind)
            {
                throw new SermofurException("invalid_scope_tree", "Invalid scope hierarchy.", 3);
            }
        }
    }
}
