using FsCheck;
using FsCheck.Fluent;
using FsCheck.Xunit;
using Sermofur.Cli;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>Properties of mapping normalization and of the command-line grammar (FR-025).</summary>
public class PropertyTests
{
    private static readonly string Root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-property-root"
    );

    private static readonly string[] Segments =
    [
        "a",
        "b",
        "..",
        ".",
        "dir",
        "x y",
        "été",
        string.Empty,
        "C:",
        "~",
        "-",
    ];

    private static readonly string[] Words =
    [
        "--json",
        "--path",
        "--actor",
        "--text",
        "--contradicts",
        "--",
        "--x",
        "claim",
        "add",
        "value",
        "-v",
        string.Empty,
    ];

    private static Arbitrary<string> Mappings() =>
        (
            from parts in Gen.Elements(Segments).ListOf()
            from separator in Gen.Elements('/', '\\')
            select string.Join(separator, parts)
        ).ToArbitrary();

    private static Arbitrary<string[]> CommandLines() =>
        Gen.Elements(Words).ArrayOf().ToArbitrary();

    [Property(MaxTest = 500)]
    public Property MappingNormalizationStaysInsideTheRootAndIsStable() =>
        Prop.ForAll(
            Mappings(),
            mapping =>
            {
                string normalized;
                try
                {
                    normalized = LocalPaths.NormalizeMapping(Root, mapping);
                }
                catch (SermofurException exception)
                {
                    return exception.Code is "unsafe_path" or "scope_boundary";
                }
                string relative = LocalPaths.RelativizeMapping(Root, normalized);
                string swapped = mapping.Replace('/', '#').Replace('\\', '/').Replace('#', '\\');
                return LocalPaths.Contains(Root, normalized)
                    && LocalPaths.NormalizeMapping(Root, relative) == normalized
                    && !relative.Contains('\\')
                    && LocalPaths.NormalizeMapping(Root, swapped) == normalized;
            }
        );

    [Property(MaxTest = 1000)]
    public Property CommandLineGrammarHoldsForAnyArguments() =>
        Prop.ForAll(
            CommandLines(),
            arguments =>
            {
                CommandArguments parsed;
                try
                {
                    parsed = new CommandArguments(arguments);
                }
                catch (SermofurException exception)
                {
                    return exception.Code == "invalid_arguments";
                }
                int separator = Array.IndexOf(arguments, CommandArguments.EndOfOptions);
                string[] tail = separator < 0 ? [] : arguments[(separator + 1)..];
                bool tailKept =
                    parsed.Positionals.Count >= tail.Length
                    && parsed.Positionals.TakeLast(tail.Length).SequenceEqual(tail);
                bool headPlain = parsed
                    .Positionals.Take(parsed.Positionals.Count - tail.Length)
                    .All(value => !value.StartsWith("--", StringComparison.Ordinal));
                bool valuesPlain = new[] { "path", "actor", "text", "x" }
                    .Select(name => parsed.Option(name))
                    .All(value =>
                        value is null || !value.StartsWith("--", StringComparison.Ordinal)
                    );
                return tailKept && headPlain && valuesPlain;
            }
        );
}
