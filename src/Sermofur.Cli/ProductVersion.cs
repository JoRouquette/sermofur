using System.Reflection;

namespace Sermofur.Cli;

/// <summary>
/// Version of the tool as stamped at build time, and the documentation that matches it. The
/// release pipeline passes the released version to the build; a local build keeps the
/// development version from Directory.Build.props.
/// </summary>
public static class ProductVersion
{
    /// <summary>
    /// Same value as <c>Version</c> in Directory.Build.props; used only when an assembly carries
    /// no informational version.
    /// </summary>
    public const string DevelopmentVersion = "0.0.0-dev";

    private const string Repository = "https://github.com/JoRouquette/sermofur";

    public static string Current { get; } = Read(typeof(ProductVersion).Assembly);

    public static string Read(Assembly assembly) =>
        Normalize(
            assembly
                .GetCustomAttribute<AssemblyInformationalVersionAttribute>()
                ?.InformationalVersion
        );

    /// <summary>
    /// Informational version without its build metadata (the <c>+commit</c> suffix that the SDK
    /// appends from source control), or <see cref="DevelopmentVersion"/> when there is none.
    /// </summary>
    public static string Normalize(string? informational)
    {
        if (string.IsNullOrEmpty(informational))
        {
            return DevelopmentVersion;
        }
        int metadata = informational.IndexOf('+');
        return metadata < 0 ? informational : informational[..metadata];
    }

    /// <summary>
    /// CLI reference for <paramref name="version"/>: the documentation of its release tag, or
    /// that of <c>main</c> for a build that carries a pre-release label, such as a local build.
    /// </summary>
    public static string CliDocumentation(string version) =>
        version.Contains('-')
            ? $"{Repository}/blob/main/docs/cli.md"
            : $"{Repository}/blob/v{version}/docs/cli.md";
}
