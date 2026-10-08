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

    private static Task<DaemonClient?> Connect(DaemonPaths paths) =>
        DaemonClient.ConnectAsync(
            paths,
            new FileOwnership(),
            ProductVersion.Current,
            TestInstance.TempRoot,
            TimeSpan.FromMilliseconds(500),
            CancellationToken.None
        );

    private static CliResult Daemon(FakeManager manager, params string[] arguments)
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
                _ => manager
            )
            {
                StartTimeout = TimeSpan.FromSeconds(10),
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

        public int Installs { get; private set; }

        private bool installed;

        public string Name => "simulated";

        public string? Unavailable() => Reason;

        public ServiceStatus Query() => new ServiceStatus(installed, daemon is not null);

        public void Install(ServiceDefinition definition)
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

        public void Start() => daemon ??= new TestDaemonOnPaths(DaemonPaths.ForCurrentUser());

        public void Stop()
        {
            daemon?.Dispose();
            daemon = null;
        }

        public void Dispose() => Stop();
    }

    /// <summary>A server of this process on given paths, stopped by Dispose or by a shutdown request.</summary>
    private sealed class TestDaemonOnPaths : IDisposable
    {
        private readonly CancellationTokenSource stop = new();
        private readonly Task serving;

        public TestDaemonOnPaths(DaemonPaths paths)
        {
            TaskCompletionSource listening = new(
                TaskCreationOptions.RunContinuationsAsynchronously
            );
            DaemonServer server = new DaemonServer(
                paths,
                ProductVersion.Current,
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
