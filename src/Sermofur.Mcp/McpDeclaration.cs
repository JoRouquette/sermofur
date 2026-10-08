using System.Reflection;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Infrastructure;

namespace Sermofur.Mcp;

/// <summary>Result of <c>smf mcp install</c> or <c>uninstall</c>.</summary>
public sealed record DeclarationReport(
    string File,
    bool Changed,
    bool Declared,
    string? Command,
    bool Portable,
    IReadOnlyList<string> Next,
    string Host = McpDeclaration.ClaudeCode
);

/// <summary>Declaration of the bridge to an MCP host (spec 005 FR-013, spec 006).</summary>
public static class McpDeclaration
{
    public const string ClaudeCode = "claude-code";
    public const string Codex = "codex";
    public const string ProjectScope = "project";
    public const string UserScope = "user";

    /// <summary>Declares the bridge to a host, in the project folder or for the user (Codex).</summary>
    public static DeclarationReport Install(string folder, string host, string scope)
    {
        Check(host, scope);
        if (host == ClaudeCode)
        {
            return Install(folder);
        }
        string file = CodexFile(folder, scope);
        (string command, IReadOnlyList<string> arguments, bool portable) = Launcher();
        (string? current, bool bom) = ReadText(file);
        string? updated = CodexConfigFile.WithBlock(current, command, arguments);
        if (updated is not null)
        {
            WriteText(file, updated, bom);
        }
        List<string> next = [.. Pending(folder)];
        if (!portable)
        {
            next.Add(
                $"The table names {command}, a path of this machine: put the folder of smf on the PATH."
            );
        }
        next.Add(
            scope == ProjectScope
                ? "Codex loads .codex/config.toml only in a trusted project: start codex in the project and trust it."
                : "Start codex in a project served by the daemon."
        );
        return new DeclarationReport(
            file,
            updated is not null,
            true,
            command,
            portable,
            next,
            Codex
        );
    }

    public static DeclarationReport Uninstall(string folder, string host, string scope)
    {
        Check(host, scope);
        if (host == ClaudeCode)
        {
            return Uninstall(folder);
        }
        string file = CodexFile(folder, scope);
        (string? current, bool bom) = ReadText(file);
        string? updated = current is null ? null : CodexConfigFile.WithoutBlock(current);
        if (updated is not null)
        {
            WriteText(file, updated, bom);
        }
        return new DeclarationReport(file, updated is not null, false, null, true, [], Codex);
    }

    /// <summary>Hosts that declare the bridge for an instance root, with where.</summary>
    public static IReadOnlyList<string> Declarations(string root)
    {
        List<string> found = [];
        if (IsDeclared(root))
        {
            found.Add($"{ClaudeCode} ({McpConfigFile.FileName})");
        }
        if (CodexDeclares(CodexConfigFile.ProjectPath(root)))
        {
            found.Add($"{Codex} (.codex/config.toml)");
        }
        if (CodexDeclares(CodexConfigFile.UserPath(Environment.GetEnvironmentVariable)))
        {
            found.Add($"{Codex} (user configuration)");
        }
        return found;
    }

    private static void Check(string host, string scope)
    {
        if (host is not (ClaudeCode or Codex))
        {
            throw new Sermofur.Domain.SermofurException(
                "invalid_arguments",
                $"Unknown host '{host}'; expected {ClaudeCode} or {Codex}."
            );
        }
        if (scope is not (ProjectScope or UserScope))
        {
            throw new Sermofur.Domain.SermofurException(
                "invalid_arguments",
                $"Unknown scope '{scope}'; expected {ProjectScope} or {UserScope}."
            );
        }
        if (host == ClaudeCode && scope == UserScope)
        {
            throw new Sermofur.Domain.SermofurException(
                "invalid_arguments",
                "The user scope of Claude Code is not written by smf (~/.claude.json is rewritten by Claude Code): use claude mcp add --scope user sermofur -- smf mcp serve."
            );
        }
    }

    private static string CodexFile(string folder, string scope) =>
        scope == UserScope
            ? CodexConfigFile.UserPath(Environment.GetEnvironmentVariable)
            : CodexConfigFile.ProjectPath(folder);

    private static bool CodexDeclares(string file)
    {
        try
        {
            return File.Exists(file) && CodexConfigFile.Declares(ReadText(file).Content!);
        }
        catch (Sermofur.Domain.SermofurException)
        {
            return false;
        }
    }

    private static (string? Content, bool Bom) ReadText(string file)
    {
        if (!File.Exists(file))
        {
            return (null, false);
        }
        byte[] bytes = File.ReadAllBytes(file);
        bool bom = bytes.AsSpan().StartsWith((byte[])[0xEF, 0xBB, 0xBF]);
        try
        {
            return (
                new System.Text.UTF8Encoding(false, true).GetString(
                    bytes,
                    bom ? 3 : 0,
                    bytes.Length - (bom ? 3 : 0)
                ),
                bom
            );
        }
        catch (System.Text.DecoderFallbackException)
        {
            throw new Sermofur.Domain.SermofurException(
                "invalid_mcp_config",
                $"{file} is not UTF-8; it is left untouched.",
                3
            );
        }
    }

    private static void WriteText(string file, string content, bool bom)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        byte[] body = System.Text.Encoding.UTF8.GetBytes(content);
        Write(file, bom ? [0xEF, 0xBB, 0xBF, .. body] : body);
    }

    public static DeclarationReport Install(string folder)
    {
        string file = Path.Combine(folder, McpConfigFile.FileName);
        (string command, IReadOnlyList<string> arguments, bool portable) = Launcher();
        byte[]? current = File.Exists(file) ? File.ReadAllBytes(file) : null;
        byte[]? updated = McpConfigFile.WithEntry(current, McpConfigFile.Entry(command, arguments));
        if (updated is not null)
        {
            Write(file, updated);
        }
        List<string> next = [.. Pending(folder)];
        if (!portable)
        {
            next.Add(
                $"The entry names {command}, a path of this machine: put the folder of smf on the PATH before sharing {McpConfigFile.FileName}."
            );
        }
        next.Add("Start claude in the project and approve the sermofur server.");
        return new DeclarationReport(file, updated is not null, true, command, portable, next);
    }

    public static DeclarationReport Uninstall(string folder)
    {
        string file = Path.Combine(folder, McpConfigFile.FileName);
        byte[]? updated = File.Exists(file)
            ? McpConfigFile.WithoutEntry(File.ReadAllBytes(file))
            : null;
        if (updated is not null)
        {
            Write(file, updated);
        }
        return new DeclarationReport(file, updated is not null, false, null, true, []);
    }

    /// <summary>True when the root of an instance declares the bridge.</summary>
    public static bool IsDeclared(string folder)
    {
        string file = Path.Combine(folder, McpConfigFile.FileName);
        try
        {
            return File.Exists(file)
                && McpConfigFile.CurrentEntry(File.ReadAllBytes(file)) is not null;
        }
        catch (Sermofur.Domain.SermofurException)
        {
            return false;
        }
    }

    /// <summary>
    /// How the host launches the bridge: <c>smf mcp serve</c> when the smf running now is found on
    /// the PATH (a tool installed globally), its full path otherwise (development build).
    /// </summary>
    public static (string Command, IReadOnlyList<string> Arguments, bool Portable) Launcher()
    {
        string process = Environment.ProcessPath ?? "smf";
        string name = Path.GetFileNameWithoutExtension(process);
        if (name.Equals("dotnet", StringComparison.OrdinalIgnoreCase))
        {
            string assembly = Assembly.GetEntryAssembly()?.Location ?? "Sermofur.Cli.dll";
            return (process, [assembly, "mcp", "serve"], false);
        }
        string? folder = Path.GetDirectoryName(process);
        bool onPath = (Environment.GetEnvironmentVariable("PATH") ?? "")
            .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
            .Any(entry =>
                folder is not null
                && string.Equals(
                    Path.TrimEndingDirectorySeparator(entry),
                    Path.TrimEndingDirectorySeparator(folder),
                    LocalPaths.Comparison
                )
            );
        return onPath ? (name, ["mcp", "serve"], true) : (process, ["mcp", "serve"], false);
    }

    /// <summary>What remains to do for the bridge to answer in this project.</summary>
    private static IEnumerable<string> Pending(string folder)
    {
        DaemonPaths paths;
        try
        {
            paths = DaemonPaths.ForCurrentUser();
        }
        catch (Sermofur.Domain.SermofurException exception)
        {
            return [exception.Message];
        }
        List<string> pending = [];
        if (IpcEndpoint.Inspect(paths, new FileOwnership()) != EndpointState.Present)
        {
            pending.Add("No daemon runs: smf daemon install");
        }
        try
        {
            string root = new InstanceManager().Discover(folder);
            bool registered = new InstanceRegistry(paths.RegistryFile)
                .Read()
                .Any(entry => InstanceRegistry.Same(entry.Root, root));
            if (!registered)
            {
                pending.Add("The instance is not registered: smf daemon register");
            }
        }
        catch (Sermofur.Domain.SermofurException exception)
        {
            pending.Add($"{exception.Code}: {exception.Message}");
        }
        return pending;
    }

    private static void Write(string file, byte[] content)
    {
        string temporary = $"{file}.{Environment.ProcessId}.tmp";
        File.WriteAllBytes(temporary, content);
        File.Move(temporary, file, overwrite: true);
    }
}
