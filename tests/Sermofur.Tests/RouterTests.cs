using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Xunit.Abstractions;

namespace Sermofur.Tests;

/// <summary>
/// Router, load and timing tests run alone: their measures and their connection timeouts must not
/// compete with the rest of the suite for the machine.
/// </summary>
[CollectionDefinition("Daemon timing", DisableParallelization = true)]
public sealed class DaemonTimingCollection { }

/// <summary>The CLI through the daemon, in real processes (US3).</summary>
[Collection("Daemon timing")]
public class RouterTests(ITestOutputHelper log)
{
    [Fact]
    [Trait("Category", "DaemonParity")]
    public async Task RoutedCommandsWriteWhatTheDirectCliWrites()
    {
        using TestInstance fixture = new TestInstance();
        await using RealDaemon daemon = await RealDaemon.Start();
        string claim = Id(
            await daemon.Cli(
                fixture.Root,
                "claim",
                "add",
                "parity ée",
                "--origin",
                "user",
                "--json"
            )
        );
        await daemon.Cli(
            fixture.Root,
            "evidence",
            "add",
            claim,
            "observation",
            "seen",
            "--lineage",
            "test",
            "--origin",
            "user"
        );
        await daemon.Cli(
            fixture.Root,
            "retex",
            "add",
            "--event",
            "e",
            "--impact",
            "i",
            "--next",
            "n",
            "--origin",
            "llm"
        );
        Assert.Equal(0, (await daemon.Cli(fixture.Root, "daemon", "register")).ExitCode);
        string[][] commands =
        [
            ["claim", "list", "--json"],
            ["claim", "show", claim],
            ["evidence", "list"],
            ["retex", "list", "--json"],
            ["scope", "tree"],
            ["scope", "current", "--json"],
            ["recall", "parity"],
            ["challenge", claim, "--json"],
            ["root"],
            ["claim", "show", Guid.NewGuid().ToString()],
            ["claim", "nope"],
            ["claim", "add", "x", "--origin", "nobody", "--json"],
            ["export", "--json"],
        ];
        foreach (string[] command in commands)
        {
            RawCliResult direct = await daemon.Cli(fixture.Root, noDaemon: true, command);
            RawCliResult routed = await daemon.Cli(fixture.Root, noDaemon: false, command);
            Assert.True(
                direct.ExitCode == routed.ExitCode
                    && direct.Output.SequenceEqual(routed.Output)
                    && direct.Error.SequenceEqual(routed.Error),
                $"smf {string.Join(' ', command)} differs:\n{Text(direct)}\n---\n{Text(routed)}"
            );
        }
        Assert.Equal(
            "direct",
            Mode(await daemon.Cli(fixture.Root, noDaemon: true, "status", "--json"))
        );
        Assert.Equal(
            "daemon",
            Mode(await daemon.Cli(fixture.Root, noDaemon: false, "status", "--json"))
        );
    }

    [Fact]
    public async Task WritesThroughTheDaemonAreSeenDirectly()
    {
        using TestInstance fixture = new TestInstance();
        await using RealDaemon daemon = await RealDaemon.Start();
        await daemon.Cli(fixture.Root, "daemon", "register");
        RawCliResult added = await daemon.Cli(
            fixture.Root,
            "claim",
            "add",
            "via the daemon",
            "--origin",
            "user",
            "--json"
        );
        Assert.Equal(0, added.ExitCode);
        string listed = Encoding.UTF8.GetString(
            (await daemon.Cli(fixture.Root, noDaemon: true, "claim", "list", "--json")).Output
        );
        Assert.Contains("via the daemon", listed);
    }

    [Fact]
    public async Task UnregisteredInstanceAndStoppedDaemonRunDirectly()
    {
        using TestInstance fixture = new TestInstance();
        string home;
        await using (RealDaemon daemon = await RealDaemon.Start())
        {
            home = daemon.Home;
            Assert.Equal("direct", Mode(await daemon.Cli(fixture.Root, "status", "--json")));
            await daemon.Cli(fixture.Root, "daemon", "register");
            Assert.Equal("daemon", Mode(await daemon.Cli(fixture.Root, "status", "--json")));
            await daemon.StopAsync();
            Stopwatch watch = Stopwatch.StartNew();
            Assert.Equal("direct", Mode(await daemon.Cli(fixture.Root, "status", "--json")));
            log.WriteLine(
                $"status with a stopped daemon: {watch.ElapsedMilliseconds} ms (process included)"
            );
        }
    }

    [Fact]
    public async Task DaemonOfAnotherVersionStopsTheCommandWithoutWriting()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start(version: "0.0.0-other");
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        Dictionary<string, string> before = fixture.Snapshot();
        RawCliResult refused = await RealDaemon.Cli(
            daemon.Home,
            fixture.Root,
            false,
            "claim",
            "add",
            "x",
            "--origin",
            "user",
            "--json"
        );
        Assert.Equal(3, refused.ExitCode);
        JsonElement error = JsonDocument.Parse(refused.Error).RootElement;
        Assert.Equal("daemon_version_mismatch", error.GetProperty("code").GetString());
        Assert.Contains("smf daemon restart", error.GetProperty("message").GetString());
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
        RawCliResult doctor = await RealDaemon.Cli(
            daemon.Home,
            fixture.Root,
            false,
            "doctor",
            "--json"
        );
        Assert.Equal("error", Check(doctor, "daemon").GetProperty("status").GetString());
    }

    [Fact]
    public async Task DoctorReportsTheDaemon()
    {
        using TestInstance fixture = new TestInstance();
        await using RealDaemon daemon = await RealDaemon.Start();
        JsonElement running = Check(await daemon.Cli(fixture.Root, "doctor", "--json"), "daemon");
        Assert.Equal("ok", running.GetProperty("status").GetString());
        await daemon.StopAsync();
        JsonElement absent = Check(await daemon.Cli(fixture.Root, "doctor", "--json"), "daemon");
        Assert.Equal("warning", absent.GetProperty("status").GetString());
    }

    [Fact]
    public void AdministrationAndHelpAlwaysRunHere()
    {
        Assert.False(CliRouter.Routable([]));
        Assert.False(CliRouter.Routable(["daemon", "status"]));
        Assert.False(CliRouter.Routable(["init"]));
        Assert.False(CliRouter.Routable(["doctor", "--json"]));
        Assert.False(CliRouter.Routable(["claim", "add", "-h"]));
        Assert.False(CliRouter.Routable(["--version"]));
        Assert.True(CliRouter.Routable(["claim", "list"]));
        Assert.True(CliRouter.Routable(["--path", "x", "status"]));
    }

    /// <summary>
    /// SC-004 and SC-005: ten writers in parallel never meet storage_busy, nothing is lost or
    /// doubled, and the cost of the daemon stays within 50 ms at the 95th percentile.
    /// Ten seconds by default, one minute with SERMOFUR_PERFORMANCE=1.
    /// </summary>
    [Fact]
    [Trait("Category", "DaemonLoad")]
    public async Task TenWritersNeverMeetStorageBusy()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        TimeSpan duration =
            Environment.GetEnvironmentVariable("SERMOFUR_PERFORMANCE") == "1"
                ? TimeSpan.FromMinutes(1)
                : TimeSpan.FromSeconds(10);
        Stopwatch clock = Stopwatch.StartNew();
        int[] written = await Task.WhenAll(
            Enumerable
                .Range(0, 10)
                .Select(async writer =>
                {
                    await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
                    int count = 0;
                    while (clock.Elapsed < duration)
                    {
                        IpcMessage result = await client.RunAsync(
                            [
                                "claim",
                                "add",
                                $"w{writer}-{count}",
                                "--origin",
                                "user",
                                "--key",
                                $"k{writer}-{count}",
                                "--json",
                            ],
                            CancellationToken.None
                        );
                        Assert.True(result.ExitCode == 0, result.Stderr);
                        count++;
                    }
                    return count;
                })
        );
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        int stored = JsonDocument.Parse(listed.Output).RootElement.GetArrayLength();
        Assert.Equal(written.Sum(), stored);
        log.WriteLine($"{stored} claims written by 10 clients in {duration.TotalSeconds:0} s");

        await using DaemonClient reader = (await daemon.Connect(fixture.Root))!;
        List<double> direct = [];
        List<double> routed = [];
        for (int round = 0; round < 60; round++)
        {
            Stopwatch one = Stopwatch.StartNew();
            DaemonTests.Direct(fixture.Root, ["claim", "show", Guid.NewGuid().ToString()]);
            direct.Add(one.Elapsed.TotalMilliseconds);
            one.Restart();
            await reader.RunAsync(
                ["claim", "show", Guid.NewGuid().ToString()],
                CancellationToken.None
            );
            routed.Add(one.Elapsed.TotalMilliseconds);
        }
        double overhead = P95(routed) - P95(direct);
        log.WriteLine(
            $"p95 direct {P95(direct):0.0} ms, through the daemon {P95(routed):0.0} ms, overhead {overhead:0.0} ms"
        );
        Assert.True(overhead <= 50, $"overhead {overhead:0.0} ms");
    }

    private static double P95(List<double> values)
    {
        List<double> sorted = [.. values.Order()];
        return sorted[(int)Math.Ceiling(sorted.Count * 0.95) - 1];
    }

    private static string Id(RawCliResult result) =>
        JsonDocument.Parse(result.Output).RootElement.GetProperty("id").GetString()!;

    private static string Mode(RawCliResult result) =>
        JsonDocument.Parse(result.Output).RootElement.GetProperty("mode").GetString()!;

    private static JsonElement Check(RawCliResult doctor, string name) =>
        JsonDocument
            .Parse(doctor.Output)
            .RootElement.GetProperty("checks")
            .EnumerateArray()
            .Single(check => check.GetProperty("name").GetString() == name);

    private static string Text(RawCliResult result) =>
        $"exit {result.ExitCode}\n{Encoding.UTF8.GetString(result.Output)}{Encoding.UTF8.GetString(result.Error)}";
}

/// <summary>A real <c>smf daemon run</c> process under its own home.</summary>
public sealed class RealDaemon : IAsyncDisposable
{
    private Process? process;

    private RealDaemon(string home) => Home = home;

    public string Home { get; }

    public static async Task<RealDaemon> Start()
    {
        RealDaemon daemon = new RealDaemon(TestDaemon.NewHome());
        System.Diagnostics.ProcessStartInfo start = TestInstance.CliStart([
            "daemon",
            "run",
            "--json",
        ]);
        start.Environment[DaemonPaths.HomeVariable] = daemon.Home;
        daemon.process = Process.Start(start)!;
        Task<string?> line = daemon.process.StandardOutput.ReadLineAsync();
        if (
            await Task.WhenAny(line, Task.Delay(TimeSpan.FromSeconds(20))) != line
            || await line is null
        )
        {
            string error = await daemon.process.StandardError.ReadToEndAsync();
            await daemon.DisposeAsync();
            throw new InvalidOperationException($"The daemon did not start: {error}");
        }
        return daemon;
    }

    public Task<RawCliResult> Cli(string workingDirectory, params string[] arguments) =>
        Cli(Home, workingDirectory, false, arguments);

    public Task<RawCliResult> Cli(
        string workingDirectory,
        bool noDaemon,
        params string[] arguments
    ) => Cli(Home, workingDirectory, noDaemon, arguments);

    public static async Task<RawCliResult> Cli(
        string home,
        string workingDirectory,
        bool noDaemon,
        params string[] arguments
    )
    {
        System.Diagnostics.ProcessStartInfo start = TestInstance.CliStart(arguments);
        start.WorkingDirectory = workingDirectory;
        start.Environment[DaemonPaths.HomeVariable] = home;
        if (noDaemon)
        {
            start.Environment[CliRouter.NoDaemonVariable] = "1";
        }
        else
        {
            start.Environment.Remove(CliRouter.NoDaemonVariable);
        }
        using Process cli = Process.Start(start)!;
        using MemoryStream output = new MemoryStream();
        using MemoryStream error = new MemoryStream();
        Task copyOut = cli.StandardOutput.BaseStream.CopyToAsync(output);
        Task copyErr = cli.StandardError.BaseStream.CopyToAsync(error);
        await cli.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(30));
        await Task.WhenAll(copyOut, copyErr);
        return new RawCliResult(cli.ExitCode, output.ToArray(), error.ToArray());
    }

    public async Task StopAsync()
    {
        if (process is null)
        {
            return;
        }
        process.Kill(true);
        await process.WaitForExitAsync();
        process.Dispose();
        process = null;
    }

    public async ValueTask DisposeAsync()
    {
        await StopAsync();
        TestDaemon.DeleteHome(Home);
    }
}
