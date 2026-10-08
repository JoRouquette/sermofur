using System.Text.RegularExpressions;
using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>
/// Reading of visible scopes and registration of a child scope of the current context.
/// All registration rules live here; the store only inserts.
/// </summary>
public sealed partial class ScopeService
{
    /// <summary>Code of a mapping identical to that of another scope.</summary>
    public const string DuplicateMapping = "duplicate_mapping";

    /// <summary>Code of a mapping that contains, or is contained in, one of another branch.</summary>
    public const string ScopeBoundary = "scope_boundary";

    private const string UnavailableId = "Scope identifier unavailable.";

    private readonly IMemoryStore store;
    private readonly IPathResolver paths;
    private readonly MemoryContext context;

    public ScopeService(IMemoryStore store, IPathResolver paths, MemoryContext context)
    {
        this.store = store;
        this.paths = paths;
        IReadOnlyList<Scope> scopes = store.ReadScopes();
        ScopePolicy.ValidateTree(scopes);
        this.context = context with
        {
            VisibleScopes = ScopePolicy.VisibleAncestors(context.ScopeId, scopes),
        };
    }

    /// <summary>Current scope and its visible ancestors.</summary>
    public ScopeContextView Current() =>
        new(context.ScopeId, context.VisibleScopes.Order(StringComparer.Ordinal).ToArray());

    /// <summary>Scopes visible from the current context, without siblings or descendants.</summary>
    public IReadOnlyList<Scope> List() =>
        store.ReadScopes().Where(scope => context.VisibleScopes.Contains(scope.Id)).ToArray();

    /// <summary>
    /// Registers a direct child of the current scope after checking the identifier, the kind and
    /// the mapping; returns the scope as stored, with its mapping normalized.
    /// </summary>
    /// <remarks>
    /// The mapping and the parent are checked physically before the transaction; the free
    /// identifier, the duplicate and the overlap are replayed by the store on the scopes read
    /// again under its transaction, so that a concurrent registration cannot slip in between.
    /// </remarks>
    public Scope Register(Scope candidate)
    {
        ValidateBounds(candidate);
        ValidateSlugs(candidate);
        IReadOnlyList<Scope> scopes = store.ReadScopes();
        Scope parent = ValidateParent(candidate, scopes);
        string mapped = ValidateMapping(candidate, parent);
        // Stored as named on disk, like source paths: both sides of FR-002 compare exactly.
        Scope normalized = candidate with
        {
            RelativePath = paths.CanonicalCase(
                context.Root,
                paths.Relativize(context.Root, mapped)
            ),
        };
        store.AddScope(normalized, current => EnsureRegistrable(normalized, current));
        return normalized;
    }

    /// <summary>
    /// Pairs of scopes whose lexically normalized mappings break the registration rules:
    /// identical path outside the project/repository sharing, or nesting between two scopes
    /// neither of which is the ancestor of the other.
    /// </summary>
    public static IReadOnlyList<MappingConflict> MappingConflicts(
        IReadOnlyList<Scope> scopes,
        string root,
        IPathResolver paths
    )
    {
        IReadOnlyList<MappedScope> mappings = Mappings(scopes, root, paths);
        Dictionary<string, IReadOnlySet<string>> lineages = Lineages(scopes);
        List<MappingConflict> conflicts = [];
        for (int first = 0; first < mappings.Count; first++)
        {
            for (int second = first + 1; second < mappings.Count; second++)
            {
                string? code = ConflictBetween(mappings[first], mappings[second], lineages, paths);
                if (code is not null)
                {
                    conflicts.Add(
                        new MappingConflict(mappings[first].Scope, mappings[second].Scope, code)
                    );
                }
            }
        }
        return conflicts;
    }

    private static void ValidateBounds(Scope candidate)
    {
        string?[] values = [candidate.Id, candidate.ParentId, candidate.RelativePath];
        if (values.Any(value => value is not null && InputLimits.IsOutOfBounds(value)))
        {
            throw new SermofurException("invalid_scope", InputLimits.ScopeMessage);
        }
    }

    private static void ValidateSlugs(Scope candidate)
    {
        if (!Slug().IsMatch(candidate.Id))
        {
            throw new SermofurException(
                "invalid_scope",
                "Scope identifier: slug of 1 to 64 characters."
            );
        }
        if (candidate.ParentId is null || !Slug().IsMatch(candidate.ParentId))
        {
            throw new SermofurException("invalid_scope", "Invalid scope parent.");
        }
    }

    private Scope ValidateParent(Scope candidate, IReadOnlyList<Scope> scopes)
    {
        if (candidate.ParentId != context.ScopeId)
        {
            throw new SermofurException(
                ScopeBoundary,
                "The new scope must be a child of the current context.",
                4
            );
        }
        Scope parent = scopes.Single(scope => scope.Id == context.ScopeId);
        if (
            candidate.Kind == ScopeKind.Workspace
            || ScopePolicy.ExpectedParent(candidate.Kind) != parent.Kind
        )
        {
            throw new SermofurException(
                "invalid_scope",
                "Scope kind incompatible with its parent."
            );
        }
        return parent;
    }

    private string ValidateMapping(Scope candidate, Scope parent)
    {
        if (string.IsNullOrWhiteSpace(candidate.RelativePath))
        {
            throw new SermofurException("invalid_scope", "Relative mapping required.");
        }
        string mapped = paths.Resolve(context.Root, candidate.RelativePath);
        string parentPath = parent.RelativePath is null
            ? context.Root
            : paths.Resolve(context.Root, parent.RelativePath);
        bool mayShareParentPath =
            candidate.Kind == ScopeKind.Repository && parent.Kind == ScopeKind.Project;
        if (
            !paths.Contains(parentPath, mapped)
            || (SamePath(mapped, parentPath, paths) && !mayShareParentPath)
        )
        {
            throw new SermofurException(
                ScopeBoundary,
                "Child mapping must lie inside its parent; only a repository may share its project's path.",
                4
            );
        }
        return mapped;
    }

    /// <summary>
    /// Precondition replayed by the store on the scopes read under its transaction: free
    /// identifier, then no mapping conflict between the candidate and an existing scope.
    /// </summary>
    private void EnsureRegistrable(Scope candidate, IReadOnlyList<Scope> scopes)
    {
        if (scopes.Any(scope => scope.Id == candidate.Id))
        {
            // Same message whoever the owner is: a sibling scope cannot be guessed.
            throw new SermofurException("invalid_scope", UnavailableId);
        }
        Scope[] withCandidate = [.. scopes, candidate];
        IReadOnlyList<MappedScope> mappings = Mappings(withCandidate, context.Root, paths);
        Dictionary<string, IReadOnlySet<string>> lineages = Lineages(withCandidate);
        MappedScope mappedCandidate = mappings.Single(mapping => mapping.Scope.Id == candidate.Id);
        string[] codes = mappings
            .Where(mapping => mapping.Scope.Id != candidate.Id)
            .Select(mapping => ConflictBetween(mappedCandidate, mapping, lineages, paths))
            .OfType<string>()
            .ToArray();
        if (codes.Contains(DuplicateMapping))
        {
            throw new SermofurException(DuplicateMapping, "Mapping already assigned.");
        }
        if (codes.Length > 0)
        {
            throw new SermofurException(
                ScopeBoundary,
                "Mapping nested in another branch of scopes.",
                4
            );
        }
    }

    private static string? ConflictBetween(
        MappedScope first,
        MappedScope second,
        Dictionary<string, IReadOnlySet<string>> lineages,
        IPathResolver paths
    )
    {
        if (SamePath(first.Path, second.Path, paths))
        {
            return MayShareMapping(first.Scope, second.Scope) ? null : DuplicateMapping;
        }
        bool sameLineage =
            lineages[first.Scope.Id].Contains(second.Scope.Id)
            || lineages[second.Scope.Id].Contains(first.Scope.Id);
        bool nested =
            paths.Contains(first.Path, second.Path) || paths.Contains(second.Path, first.Path);
        return nested && !sameLineage ? ScopeBoundary : null;
    }

    /// <summary>Only a repository may share the path of its parent project.</summary>
    private static bool MayShareMapping(Scope first, Scope second) =>
        IsRepositoryOf(first, second) || IsRepositoryOf(second, first);

    private static bool IsRepositoryOf(Scope repository, Scope project) =>
        repository.Kind == ScopeKind.Repository
        && project.Kind == ScopeKind.Project
        && repository.ParentId == project.Id;

    private static Dictionary<string, IReadOnlySet<string>> Lineages(IReadOnlyList<Scope> scopes) =>
        scopes.ToDictionary(
            scope => scope.Id,
            scope => ScopePolicy.VisibleAncestors(scope.Id, scopes),
            StringComparer.Ordinal
        );

    private static IReadOnlyList<MappedScope> Mappings(
        IReadOnlyList<Scope> scopes,
        string root,
        IPathResolver paths
    ) =>
        scopes
            .Where(scope => scope.RelativePath is not null)
            .Select(scope => new MappedScope(scope, paths.Normalize(root, scope.RelativePath!)))
            .ToArray();

    private static bool SamePath(string left, string right, IPathResolver paths) =>
        paths.Contains(left, right) && paths.Contains(right, left);

    [GeneratedRegex(@"\A[a-z][a-z0-9-]{0,63}\z", RegexOptions.CultureInvariant)]
    private static partial Regex Slug();

    private sealed record MappedScope(Scope Scope, string Path);
}

/// <summary>Two scopes whose mappings cannot coexist, with the error code.</summary>
public sealed record MappingConflict(Scope First, Scope Second, string Code);
