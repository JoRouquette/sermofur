using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Cli;

public sealed class CommandArguments
{
    /// <summary>Separator after which every argument is positional.</summary>
    public const string EndOfOptions = "--";

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
            if (name == "json")
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

    /// <summary>
    /// Presence of a flag before the <c>--</c> separator, without parsing the line: used for
    /// help and for the error format before any validation.
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
