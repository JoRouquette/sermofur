using System.Globalization;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Serialization;
using System.Text.Unicode;
using Sermofur.Domain;

namespace Sermofur.Application;

public static class RecordJson
{
    public static JsonSerializerOptions Options { get; } = CreateOptions();

    private static JsonSerializerOptions CreateOptions()
    {
        // Accented letters stay readable; < > & and the backtick stay escaped, so that a text
        // cannot close a Markdown block or inject HTML.
        JsonSerializerOptions options = new()
        {
            PropertyNamingPolicy = JsonNamingPolicy.CamelCase,
            WriteIndented = true,
            Encoder = JavaScriptEncoder.Create(UnicodeRanges.All),
        };
        options.Converters.Add(
            new JsonStringEnumConverter(JsonNamingPolicy.SnakeCaseLower, allowIntegerValues: false)
        );
        return options;
    }

    public static string Write<T>(T value) => JsonSerializer.Serialize(value, Options);

    public static T Read<T>(string json) =>
        JsonSerializer.Deserialize<T>(json, Options)
        ?? throw new SermofurException("invalid_record", "Invalid persisted object.", 3);
}

/// <summary>Bounds of text inputs, shared by the CLI and the services.</summary>
public static class InputLimits
{
    /// <summary>Maximum length of a text, an argument, an identifier or a mapping.</summary>
    public const int MaxTextLength = 16_384;

    /// <summary>Maximum number of arguments on a command line.</summary>
    public const int MaxArguments = 128;

    /// <summary>Message for an empty or out-of-bounds memory text.</summary>
    public static readonly string TextMessage =
        $"Text required, limited to {Format(MaxTextLength)} characters without NUL.";

    /// <summary>Message for an out-of-bounds scope identifier, parent or mapping.</summary>
    public static readonly string ScopeMessage =
        $"Identifier or mapping limited to {Format(MaxTextLength)} characters without NUL.";

    /// <summary>Message for a command line that exceeds the number of arguments.</summary>
    public static readonly string ArgumentCountMessage =
        $"Too many arguments: {Format(MaxArguments)} at most.";

    /// <summary>Message for an out-of-bounds command-line argument.</summary>
    public static readonly string ArgumentMessage =
        $"Argument limited to {Format(MaxTextLength)} characters without NUL.";

    /// <summary>True if the value exceeds the maximum length or contains a NUL character.</summary>
    public static bool IsOutOfBounds(string value) =>
        value.Length > MaxTextLength || value.Contains('\0');

    // Comma thousands separator, as in the documentation, whatever the culture of the machine.
    private static string Format(int value) => value.ToString("#,0", CultureInfo.InvariantCulture);
}

public interface IMemoryStore
{
    IReadOnlyList<Scope> ReadScopes();

    /// <summary>
    /// Inserts a scope under an exclusive write transaction: the scopes are read again there and
    /// passed to <paramref name="precondition"/>, which throws a <see cref="SermofurException"/> to
    /// refuse the insertion; nothing is written in that case.
    /// </summary>
    void AddScope(Scope scope, Action<IReadOnlyList<Scope>> precondition);

    IReadOnlyList<MemoryRecord> ReadRecords(IReadOnlySet<string> visible, RecordKind? kind = null);
    MemoryRecord? FindRecord(Guid id, IReadOnlySet<string> visible);
    IReadOnlyList<HistoryEntry> ReadHistory(Guid id, IReadOnlySet<string> visible);
    MemoryRecord CreateRecord(MemoryRecord candidate, string? key);
    MemoryRecord InvalidateRecord(Guid id, string scopeId, string reason, string actor);
    IReadOnlyList<string> Export(IReadOnlySet<string> visible);
}

/// <summary>Resolution of scope mappings relative to the instance root.</summary>
public interface IPathResolver
{
    /// <summary>
    /// Normalized absolute path of a mapping, contained in the root, existing and without links.
    /// </summary>
    string Resolve(string root, string relative);

    /// <summary>
    /// Normalized absolute path of a mapping by lexical checks only: relative, contained in the
    /// root, without requiring the directory to exist.
    /// </summary>
    string Normalize(string root, string relative);

    /// <summary>
    /// Mapping of <paramref name="absolute"/> relative to <paramref name="root"/>, as stored:
    /// forward slashes as separators whatever the operating system.
    /// </summary>
    string Relativize(string root, string absolute);

    /// <summary>True if <paramref name="child"/> equals <paramref name="parent"/> or lies below it.</summary>
    bool Contains(string parent, string child);
}

public sealed record MemoryContext(string Root, string ScopeId, IReadOnlySet<string> VisibleScopes);

/// <summary>Current scope and visible scopes, sorted by identifier.</summary>
public sealed record ScopeContextView(string Scope, IReadOnlyList<string> Visible);
