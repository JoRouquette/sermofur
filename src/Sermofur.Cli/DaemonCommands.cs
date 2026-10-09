using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Text.Json;
using Sermofur.Daemon;
using Sermofur.Daemon.Services;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Cli;

/// <summary>
/// <c>smf daemon …</c>: always run by the CLI itself, never through the daemon (FR-021).
/// </summary>
/// <param name="managers">Service manager for a set of paths; the one of this system by default.</param>
/// <param name="error">Where progress and warnings go (stderr); never the JSON output.</param>
public sealed class DaemonCommands(
    TextWriter output,
    Action<object, bool> write,
    Func<DaemonPaths, IServiceManager>? managers = null,
    TextWriter? error = null
)
{
    private static readonly string[] Subcommands =
    [
        "install",
        "uninstall",
        "start",
        "stop",
        "restart",
        "status",
        "register",
        "unregister",
        "instances",
        "run",
    ];

    /// <summary>How long install, start and restart wait for the daemon to answer.</summary>
    public TimeSpan StartTimeout { get; init; } = TimeSpan.FromSeconds(15);

    public int Run(CommandArguments args, bool json, string workingDirectory)
    {
        if (args.Positionals.Count < 2)
        {
            throw new SermofurException(
                "invalid_arguments",
                $"Subcommand required: {string.Join(", ", Subcommands)}."
            );
        }
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        string subcommand = args.Positionals[1];
        if (subcommand == "run")
        {
            bool supervise = args.Flag("supervise");
            args.RequireCount(2);
            args.ValidateUsed();
            return supervise ? Supervise(paths) : Serve(paths, json);
        }
        string target = Path.GetFullPath(args.Option("path", workingDirectory)!, workingDirectory);
        args.RequireCount(2);
        args.ValidateUsed();
        InstanceRegistry registry = new InstanceRegistry(paths.RegistryFile);
        switch (subcommand)
        {
            case "register":
                write(registry.Register(target, DateTimeOffset.Now), json);
                return 0;
            case "unregister":
                write(registry.Unregister(target), json);
                return 0;
            case "instances":
                write(registry.List(), json);
                return 0;
            case "status":
                write(Status(paths, Manager(paths)), json);
                return 0;
            case "install":
                write(Install(paths), json);
                return 0;
            case "uninstall":
                write(Uninstall(paths), json);
                return 0;
            case "start":
                write(Start(paths), json);
                return 0;
            case "stop":
                write(Stop(paths), json);
                return 0;
            case "restart":
                Stop(paths);
                write(Start(paths), json);
                return 0;
            default:
                throw new SermofurException(
                    "invalid_arguments",
                    $"Unknown daemon subcommand '{subcommand}'; expected one of: {string.Join(", ", Subcommands)}."
                );
        }
    }

    private IServiceManager Manager(DaemonPaths paths) =>
        managers?.Invoke(paths)
        ?? ServiceManagers.ForCurrentSystem(new SystemProcessRunner(), paths);

    private object Install(DaemonPaths paths)
    {
        IServiceManager manager = Usable(paths);
        ServiceDefinition definition = ServiceDefinition.ForCurrentProcess(
            ProductVersion.Current,
            typeof(DaemonCommands).Assembly.Location,
            supervise: OperatingSystem.IsWindows()
        );
        bool current =
            manager.Query().Installed
            && ServiceManagers.Same(InstalledDefinition.Read(paths), definition)
            && Answer(paths) is { Version: string version }
            && version == ProductVersion.Current;
        if (!current)
        {
            // A definition the manager cannot run is refused before the running daemon stops.
            manager.Validate(definition);
            // Whatever its version: only a daemon that ran is started again after a failure.
            bool wasRunning = DaemonRuns(paths);
            // Another version or executable: the running daemon makes way for the new one.
            RequestShutdown(paths);
            // Written first: the Windows supervisor reads it as soon as the task starts.
            ServiceDefinition? previous = InstalledDefinition.Read(paths);
            InstalledDefinition.Write(paths, definition);
            try
            {
                manager.Install(definition, previous);
            }
            catch
            {
                if (previous is null)
                {
                    InstalledDefinition.Delete(paths);
                }
                else
                {
                    // Restored before the previous service starts: its supervisor reads it.
                    InstalledDefinition.Write(paths, previous);
                    if (wasRunning)
                    {
                        RestartPrevious(paths, manager);
                    }
                }
                throw;
            }
            WaitForDaemon(paths);
        }
        return Status(paths, manager);
    }

    /// <summary>
    /// After a failed reinstall, brings the previous service back, as far as it can: the error of
    /// the install is the one reported, whatever happens here.
    /// </summary>
    private void RestartPrevious(DaemonPaths paths, IServiceManager manager)
    {
        try
        {
            if (!manager.Query().Installed)
            {
                // Nothing could be put back: no service to start.
                error?.WriteLine("smf: no service is installed any more; run: smf daemon install");
                return;
            }
            if (!DaemonRuns(paths))
            {
                manager.Start();
            }
            // The previous daemon usually runs an older version: any answer counts.
            Stopwatch waited = Stopwatch.StartNew();
            while (!DaemonRuns(paths))
            {
                if (waited.Elapsed >= StartTimeout)
                {
                    throw new SermofurException(
                        "daemon_unavailable",
                        $"no daemon answered within {StartTimeout.TotalSeconds:0} s",
                        3
                    );
                }
                Thread.Sleep(200);
            }
        }
        catch (Exception exception) when (exception is SermofurException or IOException)
        {
            error?.WriteLine(
                $"smf: the previous service could not be started again ({exception.Message}); run: smf daemon start"
            );
        }
    }

    /// <summary>True when a daemon of this account answers, whatever its version.</summary>
    private static bool DaemonRuns(DaemonPaths paths)
    {
        try
        {
            return Answer(paths) is not null;
        }
        catch (SermofurException exception) when (exception.Code == "daemon_version_mismatch")
        {
            return true;
        }
        catch (Exception exception)
            when (exception is SermofurException or IOException or OperationCanceledException)
        {
            // Closed between the welcome and the status, or not ours: not a running daemon.
            return false;
        }
    }

    private object Uninstall(DaemonPaths paths)
    {
        IServiceManager manager = Manager(paths);
        RequestShutdown(paths);
        manager.Uninstall();
        InstalledDefinition.Delete(paths);
        return Status(paths, manager);
    }

    private object Start(DaemonPaths paths)
    {
        IServiceManager manager = Installed(paths);
        if (Answer(paths) is null)
        {
            manager.Start();
            WaitForDaemon(paths);
        }
        return Status(paths, manager);
    }

    private object Stop(DaemonPaths paths)
    {
        IServiceManager manager = Installed(paths);
        RequestShutdown(paths);
        manager.Stop();
        return Status(paths, manager);
    }

    private IServiceManager Usable(DaemonPaths paths)
    {
        IServiceManager manager = Manager(paths);
        string? reason = manager.Unavailable();
        if (reason is not null)
        {
            throw new SermofurException(
                "service_manager_unavailable",
                $"Cannot install the daemon as a service: {reason} The CLI keeps working directly; smf daemon run serves in the foreground.",
                3
            );
        }
        return manager;
    }

    private IServiceManager Installed(DaemonPaths paths)
    {
        IServiceManager manager = Usable(paths);
        if (!manager.Query().Installed)
        {
            throw new SermofurException(
                "daemon_unavailable",
                "The daemon is not installed; run: smf daemon install",
                3
            );
        }
        return manager;
    }

    /// <summary>State, as described in contracts/cli.md.</summary>
    private static object Status(DaemonPaths paths, IServiceManager manager)
    {
        string? unavailable = manager.Unavailable();
        ServiceStatus service = unavailable is null
            ? manager.Query()
            : new ServiceStatus(false, false);
        ServiceDefinition? definition = InstalledDefinition.Read(paths);
        DaemonAnswer? answer = null;
        string state;
        try
        {
            answer = Answer(paths);
            state =
                answer is not null ? "running"
                : unavailable is not null ? "service_manager_unavailable"
                : service.Installed ? "installed_stopped"
                : "absent";
        }
        catch (SermofurException exception)
            when (exception.Code is "daemon_version_mismatch" or "foreign_endpoint")
        {
            state = exception.Code == "foreign_endpoint" ? "foreign_endpoint" : "version_mismatch";
        }
        return new
        {
            state,
            manager = manager.Name,
            installed = service.Installed,
            running = answer is not null,
            version = answer?.Version,
            executable = definition?.Executable,
            pid = answer?.Pid,
            registry = answer?.Registry,
            startedAt = answer?.StartedAt,
            instancesOpen = answer?.InstancesOpen,
            clients = answer?.Clients,
        };
    }

    private sealed record DaemonAnswer(
        string Version,
        int? Pid,
        string? StartedAt,
        int? InstancesOpen,
        int? Clients,
        string? Registry
    );

    /// <summary>What a running daemon of this version says about itself, or null when none answers.</summary>
    private static DaemonAnswer? Answer(DaemonPaths paths)
    {
        using DaemonClient? client = Connect(paths, TimeSpan.FromSeconds(1));
        if (client is null)
        {
            return null;
        }
        IpcMessage status = client.StatusAsync(CancellationToken.None).GetAwaiter().GetResult();
        return new DaemonAnswer(
            client.DaemonVersion,
            status.Pid,
            status.StartedAt,
            status.InstancesOpen,
            status.Clients,
            status.Registry
        );
    }

    /// <summary>
    /// Asks a daemon, of any version, to stop, and waits until it has drained its commands and
    /// exited; nothing happens when none answers. The service manager is called only after, so it
    /// never cuts a command short.
    /// </summary>
    private void RequestShutdown(DaemonPaths paths)
    {
        try
        {
            ShutdownAsync(paths, error)
                .Wait(DaemonLimits.Default.StopTimeout + TimeSpan.FromSeconds(5));
        }
        catch (AggregateException)
        {
            // Nothing answers or the daemon is already leaving.
        }
    }

    /// <summary>
    /// Asks the daemon to stop, then waits for its lock, which it holds until it has drained its
    /// commands and exited. While the lock is held and no stop was delivered (a daemon that has
    /// just started and does not listen yet), the request is sent again. Returns false when the
    /// daemon is still there at the end of the delay.
    /// </summary>
    internal static async Task<bool> ShutdownAsync(DaemonPaths paths, TextWriter? progress)
    {
        TimeSpan limit = DaemonLimits.Default.StopTimeout;
        Stopwatch waited = Stopwatch.StartNew();
        bool delivered = await AskToStopAsync(paths, TimeSpan.FromSeconds(1));
        bool told = false;
        while (DaemonLock.IsHeld(paths.LockFile))
        {
            if (waited.Elapsed >= limit)
            {
                progress?.WriteLine(
                    $"smf: the daemon did not exit within {limit.TotalSeconds:0} s; it may be stopped by force."
                );
                return false;
            }
            if (!told && waited.Elapsed >= TimeSpan.FromSeconds(1))
            {
                progress?.WriteLine(
                    $"smf: waiting for the daemon to finish its running commands ({limit.TotalSeconds:0} s at most)..."
                );
                told = true;
            }
            if (!delivered)
            {
                delivered = await AskToStopAsync(paths, TimeSpan.FromMilliseconds(200));
            }
            await Task.Delay(delivered ? 100 : 500);
        }
        return true;
    }

    /// <summary>
    /// Sends a shutdown, accepted before any hello and from any version (contracts/ipc.md);
    /// true when the daemon answered that it stops.
    /// </summary>
    private static async Task<bool> AskToStopAsync(DaemonPaths paths, TimeSpan timeout)
    {
        Stream? stream;
        try
        {
            stream = await IpcEndpoint.ConnectAsync(
                paths,
                new FileOwnership(),
                timeout,
                CancellationToken.None
            );
        }
        catch (SermofurException)
        {
            // Not an endpoint of ours: nothing to ask.
            return false;
        }
        if (stream is null)
        {
            return false;
        }
        await using (stream)
        {
            try
            {
                await Framing.WriteAsync(
                    stream,
                    new IpcMessage { Kind = MessageKind.Shutdown },
                    CancellationToken.None
                );
                using CancellationTokenSource answer = new CancellationTokenSource(
                    TimeSpan.FromSeconds(2)
                );
                IpcMessage? reply = await Framing.ReadAsync(stream, answer.Token);
                return reply?.Kind == MessageKind.Stopping;
            }
            catch (Exception exception)
                when (exception is IOException or OperationCanceledException or SermofurException)
            {
                // A daemon already stopping closes the connection without a word.
                return false;
            }
        }
    }

    private void WaitForDaemon(DaemonPaths paths)
    {
        Stopwatch waited = Stopwatch.StartNew();
        while (waited.Elapsed < StartTimeout)
        {
            using DaemonClient? client = Connect(paths, TimeSpan.FromMilliseconds(500));
            if (client is not null)
            {
                return;
            }
            Thread.Sleep(200);
        }
        throw new SermofurException(
            "daemon_unavailable",
            $"The service is registered but the daemon did not answer within {StartTimeout.TotalSeconds:0} s; see the journal {paths.LogFile}.",
            3
        );
    }

    private static DaemonClient? Connect(DaemonPaths paths, TimeSpan timeout) =>
        DaemonClient
            .ConnectAsync(
                paths,
                new FileOwnership(),
                ProductVersion.Current,
                Path.GetTempPath(),
                timeout,
                CancellationToken.None
            )
            .GetAwaiter()
            .GetResult();

    /// <summary>The supervisor of the Windows task: runs <c>daemon run</c> and restarts it.</summary>
    private static int Supervise(DaemonPaths paths)
    {
        ServiceDefinition child = ServiceDefinition.ForCurrentProcess(
            ProductVersion.Current,
            typeof(DaemonCommands).Assembly.Location,
            supervise: false
        );
        ProcessStartInfo start = Supervisor.ChildStart(child, InstalledDefinition.Read(paths));
        using CancellationTokenSource stop = new CancellationTokenSource();
        // A second signal ends the supervisor at once; its job object takes the daemon with it.
        using IDisposable signals = SignalStop.Register(stop, null);
        Supervisor supervisor = new Supervisor(
            start,
            new DaemonLog(paths.LogFile),
            () => ShutdownAsync(paths, null)
        );
        return supervisor.Run(stop.Token);
    }

    /// <summary>Runs the daemon in the foreground until Ctrl+C, SIGTERM, a stop request or the end of the session.</summary>
    private int Serve(DaemonPaths paths, bool json)
    {
        using FileStream lockFile = DaemonLock.Acquire(paths);
        FileOwnership owners = new FileOwnership();
        DaemonLog log = new DaemonLog(paths.LogFile);
        DaemonServer server = new DaemonServer(
            paths,
            ProductVersion.Current,
            new CliCommandExecutor(),
            new ServingGate(
                new InstanceRegistry(paths.RegistryFile),
                refused: code => log.Write("not_served", code)
            ),
            log,
            owners
        );
        using CancellationTokenSource stop = new CancellationTokenSource();
        // The first signal drains the started commands; a second one ends the process at once.
        using IDisposable signals = SignalStop.Register(
            stop,
            () =>
            {
                log.Write("draining");
                error?.WriteLine(
                    "smf: finishing the running commands; a second Ctrl+C or SIGTERM stops at once."
                );
            }
        );
        // One line, even in JSON: a supervisor or a test reads it as the signal of readiness.
        server.Listening += () =>
            output.WriteLine(
                json
                    ? JsonSerializer.Serialize(
                        new { listening = paths.Endpoint, version = ProductVersion.Current }
                    )
                    : $"Sermofur daemon {ProductVersion.Current} listening on {paths.Endpoint}"
            );
        server.RunAsync(stop.Token).GetAwaiter().GetResult();
        output.Flush();
        return 0;
    }
}
