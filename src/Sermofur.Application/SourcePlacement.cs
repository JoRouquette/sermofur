using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>
/// Single definition of FR-002: a source belongs to the narrowest scope whose directory holds its
/// file. Works on stored forms (paths relative to the instance root, <c>/</c> separators), so that
/// source add, scope add and doctor apply exactly the same rule.
/// </summary>
public static class SourcePlacement
{
    /// <summary>True if <paramref name="mapping"/> is the path or one of its parent directories.</summary>
    public static bool Holds(string mapping, string relativePath, StringComparison comparison)
    {
        string directory = mapping.Trim('/');
        return directory.Length == 0
            || string.Equals(relativePath, directory, comparison)
            || relativePath.StartsWith(directory + "/", comparison);
    }

    /// <summary>
    /// Narrowest scope below <paramref name="ownerScopeId"/> whose mapping holds the path, or
    /// null when the owner is already the narrowest.
    /// </summary>
    public static Scope? NarrowerScope(
        string relativePath,
        string ownerScopeId,
        IReadOnlyList<Scope> scopes,
        StringComparison comparison
    ) =>
        scopes
            .Where(scope =>
                scope.Id != ownerScopeId
                && scope.RelativePath is not null
                && ScopePolicy.VisibleAncestors(scope.Id, scopes).Contains(ownerScopeId)
                && Holds(scope.RelativePath, relativePath, comparison)
            )
            .OrderByDescending(scope => scope.RelativePath!.Trim('/').Length)
            .ThenByDescending(scope => scope.Kind)
            .FirstOrDefault();
}
