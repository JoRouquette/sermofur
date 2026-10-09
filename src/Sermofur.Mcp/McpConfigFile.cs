using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;

namespace Sermofur.Mcp;

/// <summary>
/// The <c>sermofur</c> entry of a Claude Code project configuration (<c>.mcp.json</c>), changed in
/// place (research R7): only the bytes of that entry are replaced, inserted or removed, so every
/// other entry, comment-free JSON as Claude Code writes it, stays identical byte for byte.
/// </summary>
public static class McpConfigFile
{
    public const string FileName = ".mcp.json";
    public const string ServersProperty = "mcpServers";
    public const string EntryName = "sermofur";
    public const int MaxBytes = 1024 * 1024;

    private static readonly byte[] Bom = [0xEF, 0xBB, 0xBF];

    /// <summary>The entry that launches the bridge.</summary>
    public static JsonObject Entry(string command, IReadOnlyList<string> arguments) =>
        new JsonObject
        {
            ["type"] = "stdio",
            ["command"] = command,
            ["args"] = new JsonArray([.. arguments.Select(argument => JsonValue.Create(argument))]),
        };

    /// <summary>
    /// Content with the entry added or updated, or null when it is already the same. A null input
    /// is an absent file.
    /// </summary>
    public static byte[]? WithEntry(byte[]? content, JsonObject entry)
    {
        string entryText = entry.ToJsonString();
        if (content is null)
        {
            return Encoding.UTF8.GetBytes(
                $"{{\n  \"{ServersProperty}\": {{\n    \"{EntryName}\": {entryText}\n  }}\n}}\n"
            );
        }
        Layout layout = Read(content);
        if (layout.EntryValue is (int start, int end))
        {
            JsonNode? existing = JsonNode.Parse(content.AsSpan(start, end - start));
            return JsonNode.DeepEquals(existing, entry)
                ? null
                : Splice(content, start, end, entryText);
        }
        if (layout.Servers is (int serversStart, int serversEnd))
        {
            int closing = serversEnd - 1;
            int last = LastNonSpace(content, closing);
            string insert =
                content[last] == (byte)'{'
                    ? $"\n    \"{EntryName}\": {entryText}\n  "
                    : $",\n    \"{EntryName}\": {entryText}";
            return Splice(
                content,
                last + 1,
                content[last] == (byte)'{' ? closing : last + 1,
                insert
            );
        }
        int rootClosing = layout.RootEnd - 1;
        int rootLast = LastNonSpace(content, rootClosing);
        string servers = $"\"{ServersProperty}\": {{\n    \"{EntryName}\": {entryText}\n  }}";
        return content[rootLast] == (byte)'{'
            ? Splice(content, rootLast + 1, rootClosing, $"\n  {servers}\n")
            : Splice(content, rootLast + 1, rootLast + 1, $",\n  {servers}");
    }

    /// <summary>Content without the entry, or null when there is no entry.</summary>
    public static byte[]? WithoutEntry(byte[] content)
    {
        Layout layout = Read(content);
        if (layout.EntryName is not int nameStart || layout.EntryValue is not (int _, int valueEnd))
        {
            return null;
        }
        int next = NextNonSpace(content, valueEnd);
        if (content[next] == (byte)',')
        {
            // Not the last entry: remove it with its comma, up to the next entry.
            return Splice(content, nameStart, NextNonSpace(content, next + 1), "");
        }
        int previous = LastNonSpace(content, nameStart);
        return content[previous] == (byte)','
            ? Splice(content, previous, valueEnd, "")
            // The only entry: the object becomes empty, "{}".
            : Splice(content, previous + 1, next, "");
    }

    /// <summary>The entry of a configuration, or null.</summary>
    public static JsonObject? CurrentEntry(byte[] content)
    {
        Layout layout = Read(content);
        return layout.EntryValue is (int start, int end)
            ? JsonNode.Parse(content.AsSpan(start, end - start)) as JsonObject
            : null;
    }

    private sealed record Layout(
        int RootEnd,
        (int, int)? Servers,
        int? EntryName,
        (int, int)? EntryValue
    );

    private static Layout Read(byte[] content)
    {
        int offset = content.AsSpan().StartsWith(Bom) ? Bom.Length : 0;
        if (content.Length > MaxBytes)
        {
            throw Invalid("larger than 1 MiB");
        }
        try
        {
            Utf8JsonReader reader = new Utf8JsonReader(
                content.AsSpan(offset),
                new JsonReaderOptions { CommentHandling = JsonCommentHandling.Disallow }
            );
            if (!reader.Read() || reader.TokenType != JsonTokenType.StartObject)
            {
                throw Invalid("not a JSON object");
            }
            (int, int)? servers = null;
            int? entryName = null;
            (int, int)? entryValue = null;
            while (reader.Read() && reader.TokenType == JsonTokenType.PropertyName)
            {
                bool isServers = reader.ValueTextEquals(ServersProperty);
                reader.Read();
                if (!isServers)
                {
                    reader.Skip();
                    continue;
                }
                if (reader.TokenType != JsonTokenType.StartObject)
                {
                    throw Invalid($"\"{ServersProperty}\" is not an object");
                }
                int serversStart = offset + (int)reader.TokenStartIndex;
                while (reader.Read() && reader.TokenType == JsonTokenType.PropertyName)
                {
                    bool isEntry = reader.ValueTextEquals(EntryName);
                    int nameStart = offset + (int)reader.TokenStartIndex;
                    reader.Read();
                    int valueStart = offset + (int)reader.TokenStartIndex;
                    reader.Skip();
                    if (isEntry)
                    {
                        entryName = nameStart;
                        entryValue = (valueStart, offset + (int)reader.BytesConsumed);
                    }
                }
                servers = (serversStart, offset + (int)reader.BytesConsumed);
            }
            if (reader.TokenType != JsonTokenType.EndObject)
            {
                throw Invalid("malformed JSON");
            }
            int rootEnd = offset + (int)reader.BytesConsumed;
            if (reader.Read())
            {
                throw Invalid("content after the JSON object");
            }
            return new Layout(rootEnd, servers, entryName, entryValue);
        }
        catch (JsonException)
        {
            throw Invalid("malformed JSON");
        }
    }

    private static byte[] Splice(byte[] content, int start, int end, string insert)
    {
        byte[] inserted = Encoding.UTF8.GetBytes(insert);
        byte[] result = new byte[content.Length - (end - start) + inserted.Length];
        content.AsSpan(0, start).CopyTo(result);
        inserted.CopyTo(result, start);
        content.AsSpan(end).CopyTo(result.AsSpan(start + inserted.Length));
        return result;
    }

    private static int LastNonSpace(byte[] content, int before)
    {
        int index = before - 1;
        while (index >= 0 && IsSpace(content[index]))
        {
            index--;
        }
        return index;
    }

    private static int NextNonSpace(byte[] content, int from)
    {
        int index = from;
        while (index < content.Length && IsSpace(content[index]))
        {
            index++;
        }
        return Math.Min(index, content.Length - 1);
    }

    private static bool IsSpace(byte value) =>
        value is (byte)' ' or (byte)'\t' or (byte)'\r' or (byte)'\n';

    private static SermofurException Invalid(string reason) =>
        new SermofurException(
            "invalid_mcp_config",
            $"The MCP configuration cannot be changed ({reason}); it is left untouched.",
            3
        );
}
