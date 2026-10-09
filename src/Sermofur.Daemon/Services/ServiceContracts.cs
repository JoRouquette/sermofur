using System.ComponentModel;
using System.Diagnostics;
using System.Text.Json;
using System.Text.Json.Serialization;
using Sermofur.Domain;

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
    /// <summary>Variables copied from the installing process when they are set.</summary>
    public static readonly string[] CarriedVariables =
    [
        "DOTNET_ROOT",
        "PATH",
        DaemonPaths.HomeVariable,
    ];

    /// <summary>
    /// Definition for the running smf. Launched as <c>dotnet Sermofur.Cli.dll</c> (development),
    /// the assembly path goes first in the arguments; launched through the tool shim, the shim
    /// itself is the executable.
    /// </summary>
    public static ServiceDefinition ForCurrentProcess(
        string version,
        string assemblyPath,
        bool supervise
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
            string? value = System.Environment.GetEnvironmentVariable(name);
            if (!string.IsNullOrEmpty(value))
            {
                environment[name] = value;
            }
        }
        return new ServiceDefinition(process, arguments, version, environment);
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

    /// <summary>Writes the definition, registers it and starts it; reverts on failure.</summary>
    void Install(ServiceDefinition definition);

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

    public static ServiceDefinition? Read(DaemonPaths paths)
    {
        try
        {
            return System.IO.File.Exists(File(paths))
                ? JsonSerializer.Deserialize<ServiceDefinition>(
                    System.IO.File.ReadAllBytes(File(paths))
                )
                : null;
        }
        catch (JsonException)
        {
            return null;
        }
    }

    public static void Write(DaemonPaths paths, ServiceDefinition definition) =>
        // The Windows supervisor reads it at start: never half written.
        AtomicFile.Write(File(paths), JsonSerializer.SerializeToUtf8Bytes(definition, Options));

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
