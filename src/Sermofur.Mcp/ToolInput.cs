using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;

namespace Sermofur.Mcp;

/// <summary>Kinds of value a tool accepts.</summary>
public enum FieldKind
{
    Text,
    Guid,
    Choice,
    Flag,
    Limit,
}

/// <summary>
/// One field of a tool input. The JSON Schema given to the host and the validation of the bridge
/// both come from this single description, so they cannot drift apart.
/// </summary>
/// <param name="AsOption">True when the value goes to the CLI as an option value, which cannot
/// start with <c>--</c> (research R5).</param>
public sealed record ToolField(
    string Name,
    FieldKind Kind,
    bool Required,
    string Description,
    IReadOnlyList<string>? Choices = null,
    bool AsOption = true
)
{
    /// <summary>Longest text, as InputLimits of the CLI.</summary>
    public const int MaxTextLength = 16_384;

    public JsonObject Schema()
    {
        JsonObject schema = new JsonObject { ["description"] = Description };
        switch (Kind)
        {
            case FieldKind.Text:
                schema["type"] = "string";
                schema["minLength"] = 1;
                schema["maxLength"] = MaxTextLength;
                break;
            case FieldKind.Guid:
                schema["type"] = "string";
                schema["format"] = "uuid";
                break;
            case FieldKind.Choice:
                schema["type"] = "string";
                schema["enum"] = new JsonArray([
                    .. Choices!.Select(choice => JsonValue.Create(choice)),
                ]);
                break;
            case FieldKind.Flag:
                schema["type"] = "boolean";
                break;
            case FieldKind.Limit:
                schema["type"] = "integer";
                schema["minimum"] = 1;
                schema["maximum"] = 3;
                break;
        }
        return schema;
    }
}

/// <summary>Validated values of a call, by field name.</summary>
public sealed class ToolArguments(IReadOnlyDictionary<string, string> values)
{
    public string? Text(string name) => values.GetValueOrDefault(name);

    public bool Flag(string name) => values.GetValueOrDefault(name) == "true";

    public bool Has(string name) => values.ContainsKey(name);

    /// <summary>
    /// Checks the arguments of a call against the fields of a tool: no unknown field, required
    /// fields present, types, lengths, choices, identifiers. Refusal: <c>invalid_input</c>.
    /// </summary>
    public static ToolArguments Validate(
        IReadOnlyList<ToolField> fields,
        IReadOnlyDictionary<string, JsonElement>? arguments
    )
    {
        Dictionary<string, string> values = [];
        IReadOnlyDictionary<string, JsonElement> given =
            arguments ?? new Dictionary<string, JsonElement>();
        foreach (string name in given.Keys)
        {
            if (!fields.Any(field => field.Name == name))
            {
                throw Invalid($"Unknown field '{name}'.");
            }
        }
        foreach (ToolField field in fields)
        {
            if (
                !given.TryGetValue(field.Name, out JsonElement value)
                || value.ValueKind == JsonValueKind.Null
            )
            {
                if (field.Required)
                {
                    throw Invalid($"Field '{field.Name}' is required.");
                }
                continue;
            }
            values[field.Name] = Read(field, value);
        }
        return new ToolArguments(values);
    }

    private static string Read(ToolField field, JsonElement value)
    {
        switch (field.Kind)
        {
            case FieldKind.Flag:
                if (value.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
                {
                    throw Invalid($"Field '{field.Name}' is a boolean.");
                }
                return value.GetBoolean() ? "true" : "false";
            case FieldKind.Limit:
                if (
                    value.ValueKind != JsonValueKind.Number
                    || !value.TryGetInt32(out int limit)
                    || limit is < 1 or > 3
                )
                {
                    throw Invalid($"Field '{field.Name}' is an integer from 1 to 3.");
                }
                return limit.ToString(System.Globalization.CultureInfo.InvariantCulture);
        }
        if (value.ValueKind != JsonValueKind.String)
        {
            throw Invalid($"Field '{field.Name}' is a string.");
        }
        string text = value.GetString()!;
        if (text.Length == 0 || text.Length > ToolField.MaxTextLength || text.Contains('\0'))
        {
            throw Invalid(
                $"Field '{field.Name}' takes 1 to {ToolField.MaxTextLength} characters, without NUL."
            );
        }
        if (field.AsOption && text.StartsWith("--", StringComparison.Ordinal))
        {
            throw Invalid($"Field '{field.Name}' cannot start with --.");
        }
        if (field.Kind == FieldKind.Guid && !System.Guid.TryParseExact(text, "D", out _))
        {
            throw Invalid($"Field '{field.Name}' is an identifier (GUID).");
        }
        if (field.Kind == FieldKind.Choice && !field.Choices!.Contains(text))
        {
            throw Invalid($"Field '{field.Name}' is one of: {string.Join(", ", field.Choices!)}.");
        }
        return text;
    }

    private static SermofurException Invalid(string message) =>
        new SermofurException("invalid_input", message);
}
