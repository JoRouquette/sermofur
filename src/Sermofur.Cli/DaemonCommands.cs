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
public sealed class DaemonCommands(
    TextWriter output,
    Action<object, bool> write,
    Func<DaemonPaths, IServiceManager>? managers = null
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
            // Another version or executable: the running daemon makes way for the new one.
            RequestShutdown(paths);
            manager.Install(definition);
            InstalledDefinition.Write(paths, definition);
            WaitForDaemon(paths);
        }
        return Status(paths, manager);
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
        int? Clients
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
            status.Clients
        );
    }

    /// <summary>Asks a daemon, of any version, to stop; nothing happens when none answers.</summary>
    private static void RequestShutdown(DaemonPaths paths)
    {
        try
        {
            ShutdownAsync(paths).Wait(TimeSpan.FromSeconds(5));
        }
        catch (AggregateException)
        {
            // Nothing answers or the daemon is already leaving.
        }
    }

    private static async Task ShutdownAsync(DaemonPaths paths)
    {
        // A shutdown is accepted before any hello, from any version (contracts/ipc.md).
        Stream? stream = await IpcEndpoint.ConnectAsync(
            paths,
            new FileOwnership(),
            TimeSpan.FromSeconds(1),
            CancellationToken.None
        );
        if (stream is null)
        {
            return;
        }
        await using (stream)
        {
            await Framing.WriteAsync(
                stream,
                new IpcMessage { Kind = MessageKind.Shutdown },
                CancellationToken.None
            );
            await Framing.ReadAsync(stream, CancellationToken.None);
        }
        // Give the daemon the time to release its endpoint.
        Stopwatch waited = Stopwatch.StartNew();
        while (
            waited.Elapsed < TimeSpan.FromSeconds(5)
            && Connect(paths, TimeSpan.FromMilliseconds(200)) is DaemonClient still
        )
        {
            still.Dispose();
            await Task.Delay(100);
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
        ProcessStartInfo start = new ProcessStartInfo(child.Executable) { UseShellExecute = false };
        foreach (string argument in child.Arguments)
        {
            start.ArgumentList.Add(argument);
        }
        using CancellationTokenSource stop = new CancellationTokenSource();
        using PosixSignalRegistration terminate = PosixSignalRegistration.Create(
            PosixSignal.SIGTERM,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
            }
        );
        using PosixSignalRegistration interrupt = PosixSignalRegistration.Create(
            PosixSignal.SIGINT,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
            }
        );
        Supervisor supervisor = new Supervisor(
            start,
            new DaemonLog(paths.LogFile),
            () => ShutdownAsync(paths)
        );
        return supervisor.Run(stop.Token);
    }

    /// <summary>Runs the daemon in the foreground until Ctrl+C, SIGTERM, a stop request or the end of the session.</summary>
    private int Serve(DaemonPaths paths, bool json)
    {
        Directory.CreateDirectory(paths.StateDirectory);
        using FileStream lockFile = AcquireLock(paths);
        FileOwnership owners = new FileOwnership();
        DaemonServer server = new DaemonServer(
            paths,
            ProductVersion.Current,
            new CliCommandExecutor(),
            new ServingGate(new InstanceRegistry(paths.RegistryFile)),
            new DaemonLog(paths.LogFile),
            owners
        );
        using CancellationTokenSource stop = new CancellationTokenSource();
        using PosixSignalRegistration terminate = PosixSignalRegistration.Create(
            PosixSignal.SIGTERM,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
            }
        );
        using PosixSignalRegistration interrupt = PosixSignalRegistration.Create(
            PosixSignal.SIGINT,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
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

    /// <summary>One daemon per user: an exclusive lock held for the life of the process (FR-018).</summary>
    private static FileStream AcquireLock(DaemonPaths paths)
    {
        try
        {
            return new FileStream(
                paths.LockFile,
                FileMode.OpenOrCreate,
                FileAccess.ReadWrite,
                FileShare.None
            );
        }
        catch (IOException)
        {
            throw IpcEndpoint.AlreadyRunning();
        }
    }
}
