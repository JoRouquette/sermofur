using Sermofur.Application;

namespace Sermofur.Infrastructure;

/// <summary>Adapter of the <see cref="IPathResolver"/> port onto the rules of <see cref="LocalPaths"/>.</summary>
public sealed class LocalPathResolver : IPathResolver
{
    public string Resolve(string root, string relative) =>
        LocalPaths.ResolveMapping(root, relative);

    public string Normalize(string root, string relative) =>
        LocalPaths.NormalizeMapping(root, relative);

    public string Relativize(string root, string absolute) =>
        LocalPaths.RelativizeMapping(root, absolute);

    public bool Contains(string parent, string child) => LocalPaths.Contains(parent, child);

    public StringComparison Comparison => LocalPaths.Comparison;

    public string CanonicalCase(string root, string relative) =>
        LocalPaths.CanonicalCase(root, relative);
}
