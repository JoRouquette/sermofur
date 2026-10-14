using System.ComponentModel;
using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon.Services;

/// <summary>What the service runs: the smf that installed it, with <c>daemon run</c>.</summary>
/// <param name="Executable">Program started by the service manager.</param>
/// <param name="Arguments">Its arguments, <c>daemon run</c> last.</param>
/// <param name="Version">Version of the installing tool.</param>
/// <param name="Environment">Variables the daemon needs to start (DOTNET_ROOT, PATH, home).</param>
public sealed record ServiceDefinition(
    [property: JsonPropertyName("executable")] string Executable,
    [property: JsonPropertyName("arguments")] IReadOnlyList<string> Arguments,
    [property: JsonPropertyName("version")] string Version,
    [property: JsonPropertyName("environment")] IReadOnlyDictionary<string, string> Environment
)
{
    /// <summary>The one carried variable that holds a list of paths, not a single one.</summary>
    private const string PathVariable = "PATH";

    /// <summary>Variables copied from the installing process when they are set.</summary>
    public static readonly string[] CarriedVariables =
    [
        "DOTNET_ROOT",
        PathVariable,
        DaemonPaths.HomeVariable,
    ];

    /// <summary>
    /// Definition for the running smf. Launched as <c>dotnet Sermofur.Cli.dll</c> (development),
    /// the assembly path goes first in the arguments; launched through the tool shim, the shim
    /// itself is the executable.
    /// </summary>
    /// <param name="dropped">Called with the name of each variable left out, never its value.</param>
    /// <param name="narrowed">
    /// Called with the name of a variable kept with fewer entries than it had, never its value.
    /// </param>
    public static ServiceDefinition ForCurrentProcess(
        string version,
        string assemblyPath,
        bool supervise,
        Action<string>? dropped = null,
        Action<string>? narrowed = null
    )
    {
        string process =
            System.Environment.ProcessPath
            ?? throw new SermofurException(
                "service_install_failed",
                "The path of smf is unknown.",
                3
            );
        List<string> arguments = [];
        if (
            Path.GetFileNameWithoutExtension(process)
                .Equals("dotnet", StringComparison.OrdinalIgnoreCase)
        )
        {
            arguments.Add(assemblyPath);
        }
        arguments.AddRange(["daemon", "run"]);
        if (supervise)
        {
            arguments.Add("--supervise");
        }
        Dictionary<string, string> environment = [];
        foreach (string name in CarriedVariables)
        {
            string? set = System.Environment.GetEnvironmentVariable(name);
            string? value = Carried(name, set, narrowed);
            if (value is not null)
            {
                environment[name] = value;
                continue;
            }
            // A variable the installing shell did hold, dropped here: the daemon starts without
            // it, so the name is reported rather than lost.
            if (!string.IsNullOrEmpty(set))
            {
                dropped?.Invoke(name);
            }
        }
        return new ServiceDefinition(process, arguments, version, environment);
    }

    /// <summary>
    /// The variables of this definition that may reach a supervised daemon: the carried ones
    /// only, each holding absolute paths. A recorded <c>service.json</c> is a file the user may
    /// have edited, so what it holds is filtered again when it is read, not only when written.
    /// </summary>
    /// <param name="dropped">Called with the name of each variable left out, never its value.</param>
    /// <param name="narrowed">
    /// Called with the name of a variable kept with fewer entries than it had, never its value:
    /// a PATH shortened in silence is what a daemon that no longer finds a tool looks like.
    /// </param>
    public IReadOnlyDictionary<string, string> CarriedEnvironment(
        Action<string>? dropped = null,
        Action<string>? narrowed = null
    )
    {
        Dictionary<string, string> carried = [];
        foreach (KeyValuePair<string, string> variable in Environment)
        {
            string? value = CarriedVariables.Contains(variable.Key, StringComparer.Ordinal)
                ? Carried(variable.Key, variable.Value, narrowed)
                : null;
            if (value is null)
            {
                dropped?.Invoke(variable.Key);
                continue;
            }
            carried[variable.Key] = value;
        }
        return carried;
    }

    /// <summary>
    /// The value a variable may carry, or null when it carries nothing usable: a path variable
    /// must be absolute, and PATH keeps only its absolute entries (a relative one would be read
    /// from the working folder of the daemon, which is not the one of the installing shell).
    /// </summary>
    /// <param name="narrowed">Called with the name of a PATH that lost entries, never its value.</param>
    private static string? Carried(string name, string? value, Action<string>? narrowed = null)
    {
        if (string.IsNullOrEmpty(value))
        {
            return null;
        }
        if (!name.Equals(PathVariable, StringComparison.Ordinal))
        {
            return Path.IsPathFullyQualified(value) ? value : null;
        }
        string[] held = value.Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries);
        string[] entries = [.. held.Where(Path.IsPathFullyQualified)];
        if (entries.Length == 0)
        {
            return null;
        }
        if (entries.Length != held.Length)
        {
            narrowed?.Invoke(name);
        }
        return string.Join(Path.PathSeparator, entries);
    }
}

/// <summary>State of the service as its manager sees it.</summary>
public sealed record ServiceStatus(bool Installed, bool Running);

/// <summary>A service manager of the user session (research R2).</summary>
public interface IServiceManager
{
    /// <summary>Name shown by status: scheduled task, systemd user, launchd agent.</summary>
    string Name { get; }

    /// <summary>Null when usable, or why not (container, WSL without systemd).</summary>
    string? Unavailable();

    ServiceStatus Query();

    /// <summary>
    /// Throws <c>service_install_failed</c> when this manager cannot run the definition; called
    /// before anything is stopped or written.
    /// </summary>
    void Validate(ServiceDefinition definition) { }

    /// <summary>
    /// Writes the definition, registers it and starts it. On failure, puts the previous
    /// registration back (<paramref name="previous"/>, null when there was none), without starting
    /// it where the manager allows (launchd starts an agent as it loads it): the caller restores
    /// its own state first, then starts it if it was running.
    /// </summary>
    void Install(ServiceDefinition definition, ServiceDefinition? previous);

    /// <summary>Stops and removes the service; nothing else.</summary>
    void Uninstall();

    void Start();

    void Stop();
}

/// <summary>Runs the command-line tools of the service managers; replaceable in tests.</summary>
public interface IProcessRunner
{
    ProcessOutcome Run(string program, params string[] arguments);
}

public sealed record ProcessOutcome(int ExitCode, string Output, string Error)
{
    public bool Succeeded => ExitCode == 0;
}

/// <summary>
/// Absolute path of a system tool. A bare name would be looked up in the current folder first
/// (.NET on Unix, CreateProcess on Windows), so a file planted in a project could run instead.
/// </summary>
public static class SystemTool
{
    private static readonly string[] UnixFolders = ["/usr/bin", "/bin", "/usr/sbin", "/sbin"];

    /// <summary>
    /// The tool in the system folder of Windows, or in the usual Unix folders then the absolute
    /// entries of <paramref name="searchPath"/>; null when it is nowhere.
    /// </summary>
    public static string? Resolve(string name, string? searchPath)
    {
        if (Path.IsPathFullyQualified(name))
        {
            return name;
        }
        if (OperatingSystem.IsWindows())
        {
            string file = Path.HasExtension(name) ? name : name + ".exe";
            string candidate = Path.Combine(System.Environment.SystemDirectory, file);
            return File.Exists(candidate) ? candidate : null;
        }
        IEnumerable<string> folders = UnixFolders.Concat(
            (searchPath ?? "")
                .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries)
                .Where(Path.IsPathFullyQualified)
        );
        return folders.Select(folder => Path.Combine(folder, name)).FirstOrDefault(File.Exists);
    }
}

public sealed class SystemProcessRunner : IProcessRunner
{
    public ProcessOutcome Run(string program, params string[] arguments)
    {
        string? path = SystemTool.Resolve(
            program,
            System.Environment.GetEnvironmentVariable("PATH")
        );
        if (path is null)
        {
            return new ProcessOutcome(-1, "", $"{program} was not found in the system folders.");
        }
        ProcessStartInfo start = new ProcessStartInfo(path)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        foreach (string argument in arguments)
        {
            start.ArgumentList.Add(argument);
        }
        try
        {
            using Process process = Process.Start(start)!;
            Task<string> output = process.StandardOutput.ReadToEndAsync();
            Task<string> error = process.StandardError.ReadToEndAsync();
            if (!process.WaitForExit(TimeSpan.FromSeconds(30)))
            {
                process.Kill(true);
                return new ProcessOutcome(-2, "", $"{program} did not answer within 30 s.");
            }
            return new ProcessOutcome(process.ExitCode, output.Result, error.Result);
        }
        catch (Win32Exception exception)
        {
            return new ProcessOutcome(-1, "", exception.Message);
        }
    }
}

/// <summary>
/// What was installed, kept beside the journal: the manager files hold no version, and status
/// must say which executable the service starts.
/// </summary>
public static class InstalledDefinition
{
    private static readonly JsonSerializerOptions Options = new JsonSerializerOptions
    {
        WriteIndented = true,
    };

    public static string File(DaemonPaths paths) =>
        Path.Combine(paths.StateDirectory, "service.json");

    /// <summary>
    /// What was installed, or null when nothing was: no file, a file that is not JSON, or one
    /// whose shape does not hold (truncated, edited by hand). An unusable definition reads as
    /// "nothing installed", so status says so and install writes a sound one again, instead of
    /// a failure outside the business errors.
    /// </summary>
    public static ServiceDefinition? Read(DaemonPaths paths)
    {
        try
        {
            return System.IO.File.Exists(File(paths))
                ? Sound(
                    JsonSerializer.Deserialize<ServiceDefinition>(
                        System.IO.File.ReadAllBytes(File(paths))
                    )
                )
                : null;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    /// <summary>
    /// The definition when every field it is read for holds, null otherwise. This establishes
    /// that something is installed, not that the recorded command is safe to run: nothing is
    /// launched from a read definition, the supervisor builds its own and takes the environment
    /// alone. JSON leaves the non-nullable members of a record null without a word, so they are
    /// checked here. The path of the executable is not required to be absolute: a service manager
    /// refuses a relative one itself (<see cref="IServiceManager.Validate"/>), and a definition
    /// written by an older version is still worth reading for its version and its environment.
    /// An environment left null is read as one with no variable.
    /// </summary>
    private static ServiceDefinition? Sound(ServiceDefinition? definition)
    {
        if (
            definition is null
            || string.IsNullOrWhiteSpace(definition.Executable)
            || string.IsNullOrWhiteSpace(definition.Version)
            || definition.Arguments is null
            || !RunsDaemon(definition.Arguments)
        )
        {
            return null;
        }
        return WithEnvironment(definition);
    }

    /// <summary>
    /// Whether the arguments launch <c>daemon run</c>: the two words side by side and in that
    /// order, as <see cref="ForCurrentProcess"/> writes them. A list holding a null, which JSON
    /// allows, simply matches nothing.
    /// </summary>
    private static bool RunsDaemon(IReadOnlyList<string> arguments)
    {
        for (int index = 0; index + 1 < arguments.Count; index++)
        {
            if (
                string.Equals(arguments[index], "daemon", StringComparison.Ordinal)
                && string.Equals(arguments[index + 1], "run", StringComparison.Ordinal)
            )
            {
                return true;
            }
        }
        return false;
    }

    /// <summary>The definition with an environment, empty when JSON left it null.</summary>
    private static ServiceDefinition WithEnvironment(ServiceDefinition definition) =>
        definition.Environment is null
            ? definition with
            {
                Environment = new Dictionary<string, string>(),
            }
            : definition;

    public static void Write(DaemonPaths paths, ServiceDefinition definition)
    {
        // Usually the first file of the state folder: the folder is born private, like the one
        // the daemon would create for its journal and its lock.
        FileTurn.CreatePrivateFolder(paths.StateDirectory);
        // The Windows supervisor reads it at start: never half written.
        AtomicFile.Write(File(paths), JsonSerializer.SerializeToUtf8Bytes(definition, Options));
    }

    public static void Delete(DaemonPaths paths)
    {
        // Nothing installed, not even the state folder: nothing to remove.
        if (System.IO.File.Exists(File(paths)))
        {
            System.IO.File.Delete(File(paths));
        }
    }
}

internal static class ServiceErrors
{
    public static SermofurException Failed(string action, ProcessOutcome outcome) =>
        new SermofurException(
            "service_install_failed",
            $"{action} failed (exit {outcome.ExitCode}): {FirstLine(outcome.Error, outcome.Output)}",
            3
        );

    private static string FirstLine(string error, string output)
    {
        string text = string.IsNullOrWhiteSpace(error) ? output : error;
        return text.Trim().Split('\n').FirstOrDefault()?.Trim() ?? "";
    }
}
