using System.Diagnostics;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Daemon.Services;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>Service definitions, service commands with a simulated manager, the supervisor (US1).</summary>
[Collection("Daemon timing")]
public class ServiceTests
{
    private static readonly ServiceDefinition Definition = new ServiceDefinition(
        "/opt/tools/smf",
        ["daemon", "run", "a b", "100%", "q\"uote"],
        "0.4.0",
        new Dictionary<string, string>
        {
            ["PATH"] = "/usr/bin:/bin",
            ["DOTNET_ROOT"] = "/opt/dotnet & co",
        }
    );

    [Fact]
    public void SystemdUnitQuotesEveryArgument()
    {
        string unit = SystemdUserService.Unit(Definition);
        Assert.Contains(
            "ExecStart=\"/opt/tools/smf\" \"daemon\" \"run\" \"a b\" \"100%%\" \"q\\\"uote\"\n",
            unit
        );
        Assert.Contains("Environment=\"DOTNET_ROOT=/opt/dotnet & co\"\n", unit);
        Assert.Contains("Restart=on-failure\nRestartSec=1\n", unit);
        Assert.Contains("WantedBy=default.target", unit);
    }

    [Fact]
    public void LaunchAgentEscapesItsValues()
    {
        string plist = LaunchAgentService.Plist(Definition, "/logs");
        Assert.Contains("<string>q\"uote</string>", plist.Replace("&quot;", "\""));
        Assert.Contains("<key>DOTNET_ROOT</key><string>/opt/dotnet &amp; co</string>", plist);
        Assert.Contains(
            "<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>",
            plist
        );
        Assert.Contains($"<key>Label</key><string>{LaunchAgentService.Label}</string>", plist);
    }

    [Fact]
    public void WindowsTaskRunsInteractivelyWithQuotedArguments()
    {
        string xml = WindowsScheduledTask.TaskXml(Definition, @"PC\john");
        Assert.Contains("<LogonType>InteractiveToken</LogonType>", xml);
        Assert.DoesNotContain("S4U", xml);
        Assert.Contains("<UserId>PC\\john</UserId>", xml);
        Assert.Contains("<RestartOnFailure>", xml);
        Assert.Contains("daemon run &quot;a b&quot; 100% &quot;q\\&quot;uote&quot;", xml);
        Assert.Equal("\"C:\\a b\\\\\"", WindowsScheduledTask.QuoteArgument("C:\\a b\\"));
        Assert.Equal("\"\"", WindowsScheduledTask.QuoteArgument(""));
    }

    [Fact]
    public void UnavailableManagerRefusesInstallAndWritesNothing()
    {
        FakeManager manager = new FakeManager
        {
            Reason = "no systemd user manager in this session.",
        };
        CliResult result = Daemon(manager, "install", "--json");
        Assert.Equal(3, result.ExitCode);
        Assert.Equal("service_manager_unavailable", TestInstance.ErrorCode(result));
        Assert.Equal(0, manager.Installs);
        Assert.Null(InstalledDefinition.Read(DaemonPaths.ForCurrentUser()));
    }

    [Theory]
    [InlineData("start")]
    [InlineData("stop")]
    [InlineData("restart")]
    public void PilotingANotInstalledServiceIsDaemonUnavailable(string subcommand)
    {
        CliResult result = Daemon(new FakeManager(), subcommand, "--json");
        Assert.Equal(("daemon_unavailable", 3), (TestInstance.ErrorCode(result), result.ExitCode));
    }

    [Fact]
    public void InstallIsIdempotentAndUninstallKeepsTheData()
    {
        using TestInstance fixture = new TestInstance();
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        new InstanceRegistry(paths.RegistryFile).Register(fixture.Root, DateTimeOffset.Now);
        string registry = File.ReadAllText(paths.RegistryFile);
        Dictionary<string, string> data = fixture.Snapshot();
        using FakeManager manager = new FakeManager();
        try
        {
            JsonElement installed = Json(Daemon(manager, "install", "--json"));
            Assert.Equal(("running", true, ProductVersion.Current), State(installed));
            Assert.Equal(1, manager.Installs);
            Assert.Equal(
                ("running", true, ProductVersion.Current),
                State(Json(Daemon(manager, "install", "--json")))
            );
            Assert.Equal(1, manager.Installs);
            Assert.Equal("running", State(Json(Daemon(manager, "status", "--json"))).Item1);
            Assert.Equal("installed_stopped", State(Json(Daemon(manager, "stop", "--json"))).Item1);
            Assert.Equal("running", State(Json(Daemon(manager, "start", "--json"))).Item1);
            Assert.Equal("running", State(Json(Daemon(manager, "restart", "--json"))).Item1);
            Assert.Equal("absent", State(Json(Daemon(manager, "uninstall", "--json"))).Item1);
            Assert.Equal("absent", State(Json(Daemon(manager, "uninstall", "--json"))).Item1);
        }
        finally
        {
            manager.Uninstall();
        }
        Assert.Equal(registry, File.ReadAllText(paths.RegistryFile));
        Assert.Equal(data.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
        Assert.Null(InstalledDefinition.Read(paths));
        new InstanceRegistry(paths.RegistryFile).Unregister(fixture.Root);
    }

    [Fact]
    public void FailedRegistrationLeavesNoDefinition()
    {
        using FakeManager manager = new FakeManager { FailInstall = true };
        CliResult result = Daemon(manager, "install", "--json");
        Assert.Equal(
            ("service_install_failed", 3),
            (TestInstance.ErrorCode(result), result.ExitCode)
        );
        Assert.Null(InstalledDefinition.Read(DaemonPaths.ForCurrentUser()));
    }

    [Fact]
    public void FailedReinstallRestoresThePreviousDefinition()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        ServiceDefinition previous = new ServiceDefinition(
            "/opt/old/smf",
            ["daemon", "run"],
            "0.3.9",
            new Dictionary<string, string> { ["DOTNET_ROOT"] = "/opt/dotnet" }
        );
        InstalledDefinition.Write(paths, previous);
        try
        {
            using FakeManager manager = new FakeManager { FailInstall = true };
            CliResult result = Daemon(manager, "install", "--json");
            Assert.Equal("service_install_failed", TestInstance.ErrorCode(result));
            ServiceDefinition restored = InstalledDefinition.Read(paths)!;
            Assert.Equal(
                (previous.Executable, previous.Version, "/opt/dotnet"),
                (restored.Executable, restored.Version, restored.Environment["DOTNET_ROOT"])
            );
        }
        finally
        {
            InstalledDefinition.Delete(paths);
        }
    }

    [Fact]
    public async Task RefusedDefinitionLeavesTheRunningDaemonAlone()
    {
        using FakeManager running = new FakeManager();
        running.Start();
        using FakeManager refusing = new FakeManager { Rejects = true };
        CliResult result = Daemon(refusing, "install", "--json");
        Assert.Equal(
            ("service_install_failed", 3),
            (TestInstance.ErrorCode(result), result.ExitCode)
        );
        await using DaemonClient? still = await Connect(DaemonPaths.ForCurrentUser());
        Assert.NotNull(still);
        Assert.Null(InstalledDefinition.Read(DaemonPaths.ForCurrentUser()));
    }

    /// <summary>
    /// A daemon still draining (endpoint already closed, lock still held) is waited for before
    /// the service manager is called.
    /// </summary>
    [Fact]
    public async Task StopWaitsForADrainingDaemonBeforeTheServiceManager()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        Directory.CreateDirectory(paths.StateDirectory);
        FileStream draining = new FileStream(
            paths.LockFile,
            FileMode.OpenOrCreate,
            FileAccess.ReadWrite,
            FileShare.None
        );
        using FakeManager manager = new FakeManager { StartsInstalled = true };
        Task<CliResult> stop = Task.Run(() => Daemon(manager, "stop", "--json"));
        try
        {
            await Task.Delay(800);
            Assert.False(stop.IsCompleted);
            Assert.Equal(0, manager.Stops);
        }
        finally
        {
            await draining.DisposeAsync();
        }
        CliResult result = await stop.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Equal(0, result.ExitCode);
        Assert.Equal(1, manager.Stops);
    }

    [Fact]
    public async Task StopSaysItIsWaitingOnStderrOnly()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        Directory.CreateDirectory(paths.StateDirectory);
        FileStream draining = new FileStream(
            paths.LockFile,
            FileMode.OpenOrCreate,
            FileAccess.ReadWrite,
            FileShare.None
        );
        using StringWriter text = new StringWriter();
        TextWriter progress = TextWriter.Synchronized(text);
        using FakeManager manager = new FakeManager { StartsInstalled = true };
        Task<CliResult> stop = Task.Run(() => DaemonWith(manager, progress, "stop", "--json"));
        try
        {
            // The lock is held until the line shows: no race with the one-second threshold.
            Stopwatch waited = Stopwatch.StartNew();
            while (
                !text.ToString().Contains("waiting for the daemon", StringComparison.Ordinal)
                && waited.Elapsed < TimeSpan.FromSeconds(15)
            )
            {
                await Task.Delay(50);
            }
            Assert.Equal(0, manager.Stops);
        }
        finally
        {
            await draining.DisposeAsync();
        }
        CliResult result = await stop.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Contains("waiting for the daemon to finish its running commands", text.ToString());
        // The JSON on stdout stays one object.
        Assert.Equal(
            "installed_stopped",
            JsonDocument.Parse(result.Output).RootElement.GetProperty("state").GetString()
        );
    }

    [Fact]
    public async Task StopReachesADaemonThatStartsListeningLate()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        Directory.CreateDirectory(paths.StateDirectory);
        FileStream starting = new FileStream(
            paths.LockFile,
            FileMode.OpenOrCreate,
            FileAccess.ReadWrite,
            FileShare.None
        );
        using FakeManager manager = new FakeManager { StartsInstalled = true };
        Task<CliResult> stop = Task.Run(() => Daemon(manager, "stop", "--json"));
        TestDaemonOnPaths? late = null;
        try
        {
            await Task.Delay(300);
            // The daemon holds its lock but only now opens its endpoint.
            late = new TestDaemonOnPaths(paths);
            await late.Serving.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(0, manager.Stops);
        }
        finally
        {
            await starting.DisposeAsync();
            late?.Dispose();
        }
        Assert.Equal(0, (await stop.WaitAsync(TimeSpan.FromSeconds(10))).ExitCode);
        Assert.Equal(1, manager.Stops);
    }

    [Fact]
    public async Task FailedUpgradeStartsThePreviousOlderDaemonAgainWithoutAWarning()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        InstalledDefinition.Write(
            paths,
            new ServiceDefinition(
                "/opt/old/smf",
                ["daemon", "run"],
                "0.3.9",
                new Dictionary<string, string>()
            )
        );
        using StringWriter progress = new StringWriter();
        using FakeManager manager = new FakeManager
        {
            FailInstall = true,
            StartsInstalled = true,
            DaemonVersion = "0.3.9",
        };
        manager.Start();
        try
        {
            CliResult result = DaemonWith(manager, progress, "install", "--json");
            Assert.Equal("service_install_failed", TestInstance.ErrorCode(result));
            // Started once before the upgrade, once again after its failure.
            Assert.Equal(2, manager.Starts);
            Assert.DoesNotContain("could not be started again", progress.ToString());
            await using DaemonClient? back = await ConnectAs(paths, "0.3.9");
            Assert.NotNull(back);
        }
        finally
        {
            manager.Uninstall();
            InstalledDefinition.Delete(paths);
        }
    }

    [Fact]
    public async Task FailedReinstallLeavesAStoppedServiceStopped()
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        InstalledDefinition.Write(
            paths,
            new ServiceDefinition(
                "/opt/old/smf",
                ["daemon", "run"],
                "0.3.9",
                new Dictionary<string, string>()
            )
        );
        using FakeManager manager = new FakeManager { FailInstall = true, StartsInstalled = true };
        try
        {
            CliResult result = Daemon(manager, "install", "--json");
            Assert.Equal("service_install_failed", TestInstance.ErrorCode(result));
            Assert.Equal(0, manager.Starts);
            Assert.Null(await Connect(paths));
        }
        finally
        {
            manager.Uninstall();
            InstalledDefinition.Delete(paths);
        }
    }

    [Fact]
    public void SecondSignalEndsTheProcess()
    {
        using CancellationTokenSource stop = new CancellationTokenSource();
        int first = 0;
        Assert.True(SignalStop.Handle(stop, () => first++));
        Assert.True(stop.IsCancellationRequested);
        Assert.False(SignalStop.Handle(stop, () => first++));
        Assert.Equal(1, first);
    }

    [Fact]
    public void SystemdRefusesAnImpossibleUnitBeforeAnythingStops() =>
        Assert.Equal(
            "service_install_failed",
            Assert
                .Throws<SermofurException>(() =>
                    new SystemdUserService(new NoRunner(), TestInstance.TempRoot).Validate(
                        new ServiceDefinition(
                            "/opt/smf",
                            ["daemon", "run"],
                            "0.4.0",
                            new Dictionary<string, string> { ["PATH"] = "/bin\n/x" }
                        )
                    )
                )
                .Code
        );

    private sealed class NoRunner : IProcessRunner
    {
        public ProcessOutcome Run(string program, params string[] arguments) =>
            throw new InvalidOperationException("Nothing may run.");
    }

    /// <summary>SC-002 for the Windows path: a killed daemon is back within ten seconds.</summary>
    [Fact]
    public async Task SupervisorRestartsAKilledDaemonAndStopsCleanly()
    {
        string home = TestDaemon.NewHome();
        DaemonPaths paths = TestDaemon.PathsOf(home);
        ProcessStartInfo child = TestInstance.CliStart(["daemon", "run"]);
        child.RedirectStandardOutput = false;
        child.RedirectStandardError = false;
        child.Environment[DaemonPaths.HomeVariable] = home;
        DaemonLog log = new DaemonLog(Path.Combine(home, "supervisor.log"));
        using CancellationTokenSource stop = new CancellationTokenSource();
        Supervisor supervisor = new Supervisor(
            child,
            log,
            async () =>
            {
                await using DaemonClient? client = await Connect(paths);
                if (client is not null)
                {
                    await client.ShutdownAsync(CancellationToken.None);
                }
            }
        );
        Task<int> supervising = Task.Run(() => supervisor.Run(stop.Token));
        try
        {
            int first = await WaitForPid(paths, null);
            Process.GetProcessById(first).Kill();
            Stopwatch back = Stopwatch.StartNew();
            int second = await WaitForPid(paths, first);
            Assert.True(
                back.Elapsed < TimeSpan.FromSeconds(10),
                $"back after {back.Elapsed.TotalSeconds:0.0} s"
            );
            Assert.NotEqual(first, second);
            await stop.CancelAsync();
            Assert.Equal(0, await supervising.WaitAsync(TimeSpan.FromSeconds(20)));
            Assert.Null(await Connect(paths));
        }
        finally
        {
            await stop.CancelAsync();
            await supervising.WaitAsync(TimeSpan.FromSeconds(20));
            TestDaemon.DeleteHome(home);
        }
    }

    private static async Task<int> WaitForPid(DaemonPaths paths, int? other)
    {
        Stopwatch waited = Stopwatch.StartNew();
        while (waited.Elapsed < TimeSpan.FromSeconds(20))
        {
            await using DaemonClient? client = await Connect(paths);
            if (client is not null)
            {
                int pid = (await client.StatusAsync(CancellationToken.None)).Pid!.Value;
                if (pid != other)
                {
                    return pid;
                }
            }
            await Task.Delay(100);
        }
        throw new TimeoutException("The supervised daemon did not answer.");
    }

    private static Task<DaemonClient?> ConnectAs(DaemonPaths paths, string version) =>
        DaemonClient.ConnectAsync(
            paths,
            new FileOwnership(),
            version,
            TestInstance.TempRoot,
            TimeSpan.FromMilliseconds(500),
            CancellationToken.None
        );

    [Fact]
    public async Task StatusOfASilentDaemonSaysItRunsButDoesNotAnswer()
    {
        await using StubDaemon silent = new StubDaemon(DaemonPaths.ForCurrentUser());
        using FakeManager manager = new FakeManager();
        Stopwatch watch = Stopwatch.StartNew();
        // Bounded from outside: without the status deadline, the call would never return.
        CliResult result = await Task.Run(() => DaemonWith(manager, null, "status", "--json"))
            .WaitAsync(TimeSpan.FromSeconds(30));
        Assert.True(watch.Elapsed < TimeSpan.FromSeconds(6), $"{watch.ElapsedMilliseconds} ms");
        JsonElement status = Json(result);
        Assert.Equal("running", status.GetProperty("state").GetString());
        Assert.False(status.GetProperty("answering").GetBoolean());
    }

    [Fact]
    public async Task InstallReplacesADaemonThatDoesNotAnswer()
    {
        using FakeManager manager = new FakeManager();
        Json(Daemon(manager, "install", "--json"));
        // The installed daemon is replaced by one that welcomes clients and answers nothing.
        manager.Stop();
        StubDaemon silent = new StubDaemon(DaemonPaths.ForCurrentUser());
        manager.BeforeStart = () => Silence(silent);
        try
        {
            CliResult result = await Task.Run(() => DaemonWith(manager, null, "install", "--json"))
                .WaitAsync(TimeSpan.FromSeconds(60));
            JsonElement status = Json(result);
            Assert.Equal(2, manager.Installs);
            Assert.Equal("running", status.GetProperty("state").GetString());
            Assert.True(status.GetProperty("answering").GetBoolean());
        }
        finally
        {
            await silent.DisposeAsync();
            // The run shares one daemon home: leave no installed definition behind.
            Daemon(manager, "uninstall", "--json");
        }
    }

    /// <summary>Stops the silent daemon before the service starts its own, as a crash would.</summary>
    private static void Silence(StubDaemon silent) =>
        silent.DisposeAsync().AsTask().GetAwaiter().GetResult();

    private static Task<DaemonClient?> Connect(DaemonPaths paths) =>
        DaemonClient.ConnectAsync(
            paths,
            new FileOwnership(),
            ProductVersion.Current,
            TestInstance.TempRoot,
            TimeSpan.FromMilliseconds(500),
            CancellationToken.None
        );

    private static CliResult Daemon(FakeManager manager, params string[] arguments) =>
        DaemonWith(manager, null, arguments);

    /// <summary>The daemon commands, with progress lines sent to <paramref name="progress"/>.</summary>
    private static CliResult DaemonWith(
        FakeManager manager,
        TextWriter? progress,
        params string[] arguments
    )
    {
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        bool json = arguments.Contains("--json");
        int code;
        try
        {
            code = new DaemonCommands(
                output,
                (result, asJson) => output.WriteLine(RecordJsonOf(result)),
                _ => manager,
                progress
            )
            {
                StartTimeout = TimeSpan.FromSeconds(10),
                StatusTimeout = TimeSpan.FromSeconds(1),
            }.Run(new CommandArguments(["daemon", .. arguments]), json, TestInstance.TempRoot);
        }
        catch (SermofurException exception)
        {
            error.WriteLine(
                JsonSerializer.Serialize(new { code = exception.Code, message = exception.Message })
            );
            code = exception.ExitCode;
        }
        return new CliResult(code, output.ToString(), error.ToString());
    }

    private static string RecordJsonOf(object value) =>
        Sermofur.Application.RecordJson.Write(value);

    private static JsonElement Json(CliResult result)
    {
        Assert.True(result.ExitCode == 0, result.Error);
        return JsonDocument.Parse(result.Output).RootElement;
    }

    private static (string, bool, string?) State(JsonElement status) =>
        (
            status.GetProperty("state").GetString()!,
            status.GetProperty("running").GetBoolean(),
            status.TryGetProperty("version", out JsonElement version)
            && version.ValueKind == JsonValueKind.String
                ? version.GetString()
                : null
        );

    /// <summary>
    /// A service manager that "starts" the daemon as a server of this process on the paths of
    /// the test run.
    /// </summary>
    private sealed class FakeManager : IServiceManager, IDisposable
    {
        private TestDaemonOnPaths? daemon;

        public string? Reason { get; init; }

        public bool FailInstall { get; init; }

        public bool Rejects { get; init; }

        public int Stops { get; private set; }

        public int Starts { get; private set; }

        public int Installs { get; private set; }

        public void Validate(ServiceDefinition definition)
        {
            if (Rejects)
            {
                throw new SermofurException("service_install_failed", "simulated rejection", 3);
            }
        }

        private bool installed;

        public string Name => "simulated";

        public string? Unavailable() => Reason;

        public ServiceStatus Query() => new ServiceStatus(installed, daemon is not null);

        public void Install(ServiceDefinition definition, ServiceDefinition? previous)
        {
            if (FailInstall)
            {
                throw new SermofurException("service_install_failed", "simulated refusal", 3);
            }
            Installs++;
            installed = true;
            Start();
        }

        public void Uninstall()
        {
            Stop();
            installed = false;
        }

        /// <summary>Version the simulated daemon answers with; the current one by default.</summary>
        public string DaemonVersion { get; init; } = ProductVersion.Current;

        /// <summary>Runs before the simulated daemon starts (to clear the endpoint, for example).</summary>
        public Action? BeforeStart { get; set; }

        public void Start()
        {
            // A daemon stopped by a shutdown request is started anew, as a service manager would.
            if (daemon is null || daemon.Serving.IsCompleted)
            {
                BeforeStart?.Invoke();
                Starts++;
                daemon?.Dispose();
                daemon = new TestDaemonOnPaths(DaemonPaths.ForCurrentUser(), DaemonVersion);
            }
        }

        public void Stop()
        {
            Stops++;
            daemon?.Dispose();
            daemon = null;
        }

        /// <summary>Installed from the start, without any daemon running.</summary>
        public bool StartsInstalled
        {
            init => installed = value;
        }

        public void Dispose() => Stop();
    }

    /// <summary>A server of this process on given paths, stopped by Dispose or by a shutdown request.</summary>
    private sealed class TestDaemonOnPaths : IDisposable
    {
        private readonly CancellationTokenSource stop = new();
        private readonly Task serving;

        /// <summary>Completes when the server stops, on Dispose or on a shutdown request.</summary>
        public Task Serving => serving;

        public TestDaemonOnPaths(DaemonPaths paths, string? version = null)
        {
            TaskCompletionSource listening = new(
                TaskCreationOptions.RunContinuationsAsynchronously
            );
            DaemonServer server = new DaemonServer(
                paths,
                version ?? ProductVersion.Current,
                new CliCommandExecutor(),
                new ServingGate(new InstanceRegistry(paths.RegistryFile)),
                new DaemonLog(paths.LogFile),
                new FileOwnership()
            );
            server.Listening += () => listening.TrySetResult();
            serving = Task.Run(() => server.RunAsync(stop.Token));
            Task.WhenAny(listening.Task, serving).GetAwaiter().GetResult();
        }

        public void Dispose()
        {
            stop.Cancel();
            try
            {
                serving.Wait(TimeSpan.FromSeconds(10));
            }
            catch (AggregateException) { }
        }
    }
}
