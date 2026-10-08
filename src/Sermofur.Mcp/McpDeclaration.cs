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
    IReadOnlyList<string> Next
);

/// <summary>Declaration of the bridge to Claude Code for a project (FR-013, FR-014).</summary>
public static class McpDeclaration
{
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
