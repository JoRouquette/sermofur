using System.Text;
using Sermofur.Domain;

namespace Sermofur.Mcp;

/// <summary>
/// The <c>[mcp_servers.sermofur]</c> table of a Codex configuration (TOML), changed in place
/// (spec 006): only the lines of that table and of its sub-tables are replaced, added or removed,
/// so the rest of the file, comments included, stays byte for byte. There is no full TOML parser:
/// table headers are found outside strings, and anything the edit cannot handle safely is refused.
/// </summary>
public static class CodexConfigFile
{
    public const string ProjectFile = ".codex/config.toml";
    public const string HomeVariable = "CODEX_HOME";

    private static readonly string[] Table = ["mcp_servers", "sermofur"];

    /// <summary>Locations of the configuration: project folder, or the Codex home of the user.</summary>
    public static string ProjectPath(string folder) =>
        Path.Combine(folder, ".codex", "config.toml");

    public static string UserPath(Func<string, string?> variable) =>
        Path.Combine(
            variable(HomeVariable) is { Length: > 0 } home
                ? home
                : Path.Combine(
                    Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                    ".codex"
                ),
            "config.toml"
        );

    /// <summary>The block that declares the bridge, with the given line ending.</summary>
    public static string Block(string command, IReadOnlyList<string> arguments, string newline) =>
        string.Join(
            newline,
            "[mcp_servers.sermofur]",
            $"command = {Literal(command)}",
            $"args = [{string.Join(", ", arguments.Select(Literal))}]"
        ) + newline;

    /// <summary>Content with the block added or updated, or null when it is already the same.</summary>
    public static string? WithBlock(
        string? content,
        string command,
        IReadOnlyList<string> arguments
    )
    {
        if (string.IsNullOrEmpty(content))
        {
            return Block(command, arguments, "\n");
        }
        string newline = content.Contains("\r\n", StringComparison.Ordinal) ? "\r\n" : "\n";
        string block = Block(command, arguments, newline);
        Layout layout = Scan(content);
        if (layout.Block is (int start, int end))
        {
            // Only the command and args lines are ours: keys and sub-tables the user added (an
            // approval mode, environment variables) stay.
            string updated = WithLaunchLines(content[start..end], command, arguments, newline);
            return updated == content[start..end]
                ? null
                : content[..start] + updated + content[end..];
        }
        string ending = content.EndsWith('\n') ? "" : newline;
        return content + ending + newline + block;
    }

    /// <summary>
    /// The block with its <c>command</c> and <c>args</c> lines set, in its main table (before any
    /// sub-table); missing lines are added right after the header, in that order.
    /// </summary>
    private static string WithLaunchLines(
        string block,
        string command,
        IReadOnlyList<string> arguments,
        string newline
    )
    {
        List<string> lines = [.. block.Split('\n')];
        string commandLine = $"command = {Literal(command)}";
        string argsLine = $"args = [{string.Join(", ", arguments.Select(Literal))}]";
        int mainEnd = lines.Count;
        for (int index = 1; index < lines.Count; index++)
        {
            if (HeaderPattern.IsMatch(lines[index].TrimStart().TrimEnd('\r')))
            {
                mainEnd = index;
                break;
            }
        }
        string carriage = newline == "\r\n" ? "\r" : "";
        int commandIndex = -1;
        bool argsSet = false;
        for (int index = 1; index < mainEnd; index++)
        {
            string key = KeyOf(lines[index].Trim()) ?? "";
            if (key == "command" && commandIndex < 0)
            {
                lines[index] = commandLine + carriage;
                commandIndex = index;
            }
            else if (key == "args" && !argsSet)
            {
                if (lines[index].Count(c => c == '[') > lines[index].Count(c => c == ']'))
                {
                    throw Invalid("args of [mcp_servers.sermofur] spans several lines");
                }
                lines[index] = argsLine + carriage;
                argsSet = true;
            }
        }
        if (commandIndex < 0)
        {
            lines.Insert(1, commandLine + carriage);
            commandIndex = 1;
        }
        if (!argsSet)
        {
            lines.Insert(commandIndex + 1, argsLine + carriage);
        }
        return string.Join('\n', lines);
    }

    /// <summary>Content without the block and its sub-tables, or null when there is none.</summary>
    public static string? WithoutBlock(string content)
    {
        Layout layout = Scan(content);
        if (layout.Block is not (int start, int end))
        {
            return null;
        }
        string before = content[..start];
        string after = content[end..];
        // In the middle, the span already holds the blank lines that follow the block. At the end,
        // the blank line written before the block goes with it.
        if (after.Length > 0)
        {
            return before + after;
        }
        if (before.EndsWith("\r\n\r\n", StringComparison.Ordinal))
        {
            before = before[..^2];
        }
        else if (before.EndsWith("\n\n", StringComparison.Ordinal))
        {
            before = before[..^1];
        }
        else if (before is "\n" or "\r\n")
        {
            before = "";
        }
        return before + after;
    }

    /// <summary>True when the content declares the bridge.</summary>
    public static bool Declares(string content) => Scan(content).Block is not null;

    private sealed record Layout((int Start, int End)? Block);

    private static Layout Scan(string content)
    {
        List<(int Start, string[] Name)> headers = [];
        bool rootKeysOfServers = false;
        string[]? current = null;
        int index = 0;
        string? multiline = null;
        while (index < content.Length)
        {
            int lineStart = index;
            int lineEnd = content.IndexOf('\n', index);
            int next = lineEnd < 0 ? content.Length : lineEnd + 1;
            string line = content[lineStart..(lineEnd < 0 ? content.Length : lineEnd)]
                .TrimEnd('\r');
            if (multiline is null)
            {
                string trimmed = line.TrimStart();
                System.Text.RegularExpressions.Match header = HeaderPattern.Match(trimmed);
                if (header.Success)
                {
                    string[] name = Names(header.Groups["name"].Value);
                    headers.Add((lineStart, name));
                    current = name;
                }
                else if (KeyOf(trimmed) is string key)
                {
                    bool aboutUs =
                        (current is null && Names(key).Take(2).SequenceEqual(Table))
                        || (
                            current is null
                            && key == "mcp_servers"
                            && trimmed.Contains("sermofur", StringComparison.Ordinal)
                        )
                        || (
                            current is ["mcp_servers"] && Names(key).FirstOrDefault() == "sermofur"
                        );
                    rootKeysOfServers |= aboutUs;
                }
            }
            multiline = Strings(line, multiline);
            index = next;
        }
        if (multiline is not null)
        {
            throw Invalid("a multi-line string is not closed");
        }
        if (rootKeysOfServers)
        {
            throw Invalid("mcp_servers.sermofur is written as a key, not as a table");
        }
        int[] mains =
        [
            .. headers
                .Select((header, position) => (header, position))
                .Where(item => item.header.Name.SequenceEqual(Table))
                .Select(item => item.position),
        ];
        if (mains.Length == 0)
        {
            return new Layout(null);
        }
        if (mains.Length > 1)
        {
            throw Invalid("[mcp_servers.sermofur] appears more than once");
        }
        int first = mains[0];
        int last = first;
        while (
            last + 1 < headers.Count
            && headers[last + 1].Name.Take(2).SequenceEqual(Table)
            && headers[last + 1].Name.Length > 2
        )
        {
            last++;
        }
        int end = last + 1 < headers.Count ? headers[last + 1].Start : content.Length;
        return new Layout((headers[first].Start, end));
    }

    /// <summary>Multi-line string still open at the end of the line, given the one open at its start.</summary>
    private static string? Strings(string line, string? open)
    {
        int index = 0;
        while (index < line.Length)
        {
            if (open is not null)
            {
                int close = line.IndexOf(open, index, StringComparison.Ordinal);
                if (close < 0)
                {
                    return open;
                }
                index = close + 3;
                open = null;
                continue;
            }
            char character = line[index];
            if (character == '#')
            {
                return null;
            }
            if (line.AsSpan(index).StartsWith("\"\"\"") || line.AsSpan(index).StartsWith("'''"))
            {
                open = line.Substring(index, 3);
                index += 3;
                continue;
            }
            if (character is '"' or '\'')
            {
                index = SkipString(line, index);
                continue;
            }
            index++;
        }
        return open;
    }

    private static int SkipString(string line, int start)
    {
        char quote = line[start];
        int index = start + 1;
        while (index < line.Length)
        {
            if (quote == '"' && line[index] == '\\')
            {
                index += 2;
                continue;
            }
            if (line[index] == quote)
            {
                return index + 1;
            }
            index++;
        }
        return index;
    }

    /// <summary>
    /// A table header alone on its line, comment allowed: <c>[a.b]</c>, <c>[[a]]</c>,
    /// <c>[a."b c"]</c>. An array value such as <c>["x", "y"],</c> does not match.
    /// </summary>
    private static readonly System.Text.RegularExpressions.Regex HeaderPattern = new(
        """^\[\[?(?<name>\s*(?:[A-Za-z0-9_-]+|"[^"\r\n]*"|'[^'\r\n]*')(?:\s*\.\s*(?:[A-Za-z0-9_-]+|"[^"\r\n]*"|'[^'\r\n]*'))*\s*)\]\]?\s*(?:#.*)?$""",
        System.Text.RegularExpressions.RegexOptions.CultureInvariant
    );

    private static string? KeyOf(string trimmed)
    {
        if (trimmed.Length == 0 || trimmed[0] == '#')
        {
            return null;
        }
        int equals = trimmed.IndexOf('=');
        return equals <= 0 ? null : trimmed[..equals].Trim();
    }

    private static string[] Names(string dotted) =>
        [.. dotted.Split('.').Select(part => part.Trim().Trim('"', '\''))];

    private static string Normalize(string text) => text.Replace("\r\n", "\n");

    /// <summary>A TOML literal string when possible, so that Windows backslashes stay as they are.</summary>
    private static string Literal(string value) =>
        value.Contains('\'') || value.Contains('\n')
            ? "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("\n", "\\n") + "\""
            : $"'{value}'";

    private static SermofurException Invalid(string reason) =>
        new SermofurException(
            "invalid_mcp_config",
            $"The Codex configuration cannot be changed safely ({reason}); it is left untouched.",
            3
        );
}
