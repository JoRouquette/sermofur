using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Cli;

public sealed class CommandArguments
{
    /// <summary>Separator after which every argument is positional.</summary>
    public const string EndOfOptions = "--";

    /// <summary>Options that take no value.</summary>
    private static readonly HashSet<string> Flags = new(StringComparer.Ordinal)
    {
        "json",
        "contradicts",
        "supervise",
    };

    private readonly Dictionary<string, string?> options = new(StringComparer.Ordinal);
    private readonly HashSet<string> used = new(StringComparer.Ordinal);
    public List<string> Positionals { get; } = [];

    public CommandArguments(string[] arguments)
    {
        ValidateBounds(arguments);
        bool optionsEnded = false;
        for (int index = 0; index < arguments.Length; index++)
        {
            string value = arguments[index];
            if (optionsEnded || !value.StartsWith("--", StringComparison.Ordinal))
            {
                Positionals.Add(value);
                continue;
            }
            if (value == EndOfOptions)
            {
                optionsEnded = true;
                continue;
            }
            string name = value[2..];
            if (options.ContainsKey(name))
            {
                throw new SermofurException("invalid_arguments", "Duplicate option.");
            }
            if (Flags.Contains(name))
            {
                options.Add(name, null);
                continue;
            }
            if (
                ++index >= arguments.Length
                || arguments[index].StartsWith("--", StringComparison.Ordinal)
            )
            {
                throw new SermofurException("invalid_arguments", "Option value required.");
            }
            options.Add(name, arguments[index]);
        }
        used.Add("json");
    }

    public const string Help = "help";
    public const string HelpShortcut = "-h";
    public const string Version = "version";
    public const string VersionShortcut = "-v";

    /// <summary>True for an option that takes no value, help and version included.</summary>
    public static bool IsFlag(string name) => Flags.Contains(name) || name is Help or Version;

    /// <summary>The single-dash shortcuts: <c>-h</c> for help, <c>-v</c> for version.</summary>
    public static bool IsShortcut(string argument) => argument is HelpShortcut or VersionShortcut;

    /// <summary>
    /// Help or version asked before <c>--</c>, as <c>--NAME</c> or as its shortcut, without
    /// parsing the line: answered before the bounds check and any instance lookup. The value of
    /// an option is never taken for a shortcut: <c>--actor -v</c> keeps the actor <c>-v</c>.
    /// </summary>
    public static bool Asks(IReadOnlyList<string> arguments, string name, string shortcut)
    {
        for (int index = 0; index < arguments.Count; index++)
        {
            string argument = arguments[index];
            if (argument == EndOfOptions)
            {
                return false;
            }
            if (argument == shortcut || argument == "--" + name)
            {
                return true;
            }
            if (TakesValue(arguments, index))
            {
                // The next argument is this option's value, not a shortcut.
                index++;
            }
        }
        return false;
    }

    /// <summary>
    /// True when the argument at <paramref name="index"/> is an option whose value is the next
    /// argument, with the rule of the parser: an option that is not a flag, followed by an
    /// argument that does not start with <c>--</c>.
    /// </summary>
    public static bool TakesValue(IReadOnlyList<string> arguments, int index) =>
        arguments[index].StartsWith("--", StringComparison.Ordinal)
        && arguments[index] != EndOfOptions
        && !IsFlag(arguments[index][2..])
        && index + 1 < arguments.Count
        && !arguments[index + 1].StartsWith("--", StringComparison.Ordinal);

    /// <summary>
    /// Presence of a flag before the <c>--</c> separator, without parsing the line: used for
    /// the error format before any validation.
    /// </summary>
    public static bool HasFlag(IReadOnlyList<string> arguments, string name)
    {
        string flag = "--" + name;
        foreach (string argument in arguments)
        {
            if (argument == EndOfOptions)
            {
                return false;
            }
            if (argument == flag)
            {
                return true;
            }
        }
        return false;
    }

    public string? Option(string name, string? fallback = null)
    {
        used.Add(name);
        return options.GetValueOrDefault(name, fallback);
    }

    /// <summary>Presence of a value-less option, marked as used.</summary>
    public bool Flag(string name)
    {
        used.Add(name);
        return options.ContainsKey(name);
    }

    public string Required(string name) =>
        Option(name)
        ?? throw new SermofurException("invalid_arguments", $"Option --{name} required.");

    public void RequireCount(int count)
    {
        if (Positionals.Count != count)
        {
            throw new SermofurException("invalid_arguments", "Wrong number of arguments.");
        }
    }

    public void ValidateUsed()
    {
        if (options.Keys.Any(name => !used.Contains(name)))
        {
            throw new SermofurException("invalid_arguments", "Unknown option for this command.");
        }
    }

    public static T ParseEnum<T>(string value, string errorCode = "invalid_arguments")
        where T : struct, Enum
    {
        string normalized = value
            .Replace("_", "", StringComparison.Ordinal)
            .Replace("-", "", StringComparison.Ordinal);
        if (
            !normalized.All(char.IsLetter)
            || !Enum.TryParse<T>(normalized, true, out T parsed)
            || !Enum.IsDefined(parsed)
        )
        {
            throw new SermofurException(errorCode, $"Invalid {typeof(T).Name} value.");
        }
        return parsed;
    }

    public static Guid ParseId(string value) =>
        Guid.TryParse(value, out Guid id) && id != Guid.Empty
            ? id
            : throw new SermofurException("invalid_arguments", "GUID identifier required.");

    private static void ValidateBounds(string[] arguments)
    {
        if (arguments.Length > InputLimits.MaxArguments)
        {
            throw new SermofurException("invalid_arguments", InputLimits.ArgumentCountMessage);
        }
        if (arguments.Any(InputLimits.IsOutOfBounds))
        {
            throw new SermofurException("invalid_arguments", InputLimits.ArgumentMessage);
        }
    }
}
