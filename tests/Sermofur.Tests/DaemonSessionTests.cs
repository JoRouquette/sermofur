using System.Diagnostics;
using System.Text;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;
using Sermofur.Mcp;

namespace Sermofur.Tests;

/// <summary>
/// What happens to a command when the session ends around it: shutdown, interruption,
/// cancellation, large answers, commands reserved to the CLI, versions (review of lot 003).
/// </summary>
[Collection("Daemon timing")]
public class DaemonSessionTests
{
    [Fact]
    public async Task ShutdownLetsAStartedWriteAnswer()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> write = client.RunAsync(["block", "write"], CancellationToken.None);
        await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
        Task stopped = daemon.StopAsync();
        await Task.Delay(200);
        Assert.False(stopped.IsCompleted);
        // The endpoint is closed during the drain: a new client finds no daemon, at once (well
        // under its one-second connect timeout, on Windows too).
        Stopwatch refused = Stopwatch.StartNew();
        Assert.Null(
            await DaemonClient.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                ProductVersion.Current,
                fixture.Root,
                TimeSpan.FromSeconds(1),
                CancellationToken.None
            )
        );
        Assert.True(refused.ElapsedMilliseconds < 500, $"{refused.ElapsedMilliseconds} ms");
        release.Set();
        IpcMessage answer = await write.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Equal(
            (MessageKind.Result, 0, "block"),
            (answer.Kind, answer.ExitCode, answer.Stdout)
        );
        await stopped.WaitAsync(TimeSpan.FromSeconds(10));
    }

    [Fact]
    public async Task ShutdownRefusesAWriteThatHasNotStarted()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient holder = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> held = holder.RunAsync(["block", "write"], CancellationToken.None);
        await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
        await using DaemonClient waiting = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> queued = waiting.RunAsync(["queued", "write"], CancellationToken.None);
        await executor.Planned("queued").WaitAsync(TimeSpan.FromSeconds(10));
        Task stopped = daemon.StopAsync();
        IpcMessage refused = await queued.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.Equal(("error", "daemon_stopping"), (refused.Kind, refused.Code));
        release.Set();
        Assert.Equal(0, (await held.WaitAsync(TimeSpan.FromSeconds(10))).ExitCode);
        await stopped.WaitAsync(TimeSpan.FromSeconds(10));
        Assert.DoesNotContain("queued", executor.Executed);
    }

    [Fact]
    public async Task LargeNonAsciiOutputIsAnsweredOnce()
    {
        using TestInstance fixture = new TestInstance();
        GatedExecutor executor = new GatedExecutor(null);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        IpcMessage answer = await client.RunAsync(["big"], CancellationToken.None);
        Assert.Equal(MessageKind.Result, answer.Kind);
        Assert.Equal(GatedExecutor.Big, answer.Stdout);
        IpcMessage huge = await client.RunAsync(["huge"], CancellationToken.None);
        Assert.Equal(("error", "response_too_large", 1), (huge.Kind, huge.Code, huge.ExitCode));
        // The session is still usable, and each command ran exactly once.
        Assert.Equal("quick", (await client.RunAsync(["quick"], CancellationToken.None)).Stdout);
        Assert.Equal(["big", "huge", "quick"], executor.Executed);
    }

    [Theory]
    [InlineData("daemon", "stop")]
    [InlineData("mcp", "serve")]
    [InlineData("init")]
    [InlineData("doctor")]
    [InlineData("status", "--help")]
    public async Task CommandsOfTheCliAloneAreRefusedByTheDaemon(params string[] argv)
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        IpcMessage refused = await client.RunAsync(argv, CancellationToken.None);
        Assert.Equal(("error", "cli_only", 3), (refused.Kind, refused.Code, refused.ExitCode));
        Assert.Equal(MessageKind.Status, (await client.StatusAsync(CancellationToken.None)).Kind);
    }

    [Fact]
    public async Task OtherVersionIsReportedBeforeAnyProtocolCheck()
    {
        await using TestDaemon daemon = TestDaemon.Start(version: "9.0.0");
        await using Stream raw = (
            await IpcEndpoint.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        )!;
        IpcMessage hello = DaemonTests.Hello(TestInstance.TempRoot, "0.4.0") with { Protocol = 2 };
        await Framing.WriteAsync(raw, hello, CancellationToken.None);
        IpcMessage refusal = (await Framing.ReadAsync(raw, CancellationToken.None))!;
        Assert.Equal(
            ("error", "daemon_version_mismatch", "9.0.0"),
            (refusal.Kind, refusal.Code, refusal.ToolVersion)
        );
    }

    [Theory]
    [InlineData("9.0.0", "0.4.0", "restart the sermofur server")]
    [InlineData("0.3.0", "0.4.0", "smf daemon restart")]
    public void VersionRemedyNamesTheSideThatIsBehind(
        string daemonVersion,
        string clientVersion,
        string remedy
    ) =>
        Assert.Contains(
            remedy,
            DaemonClient.VersionMismatchMessage(daemonVersion, clientVersion, null)
        );

    [Fact]
    public async Task InterruptedWriteIsNeverReplayedDirectly()
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon fake = new StubDaemon(
            DaemonPaths.ForCurrentUser(),
            StubDaemon.Close
        );
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        int code = new CliRouter(output, error).Run(
            ["claim", "add", "only once", "--origin", "user"],
            fixture.Root
        );
        Assert.Equal(3, code);
        Assert.Contains(DaemonClient.InterruptedCode, error.ToString());
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(0, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    [Fact]
    public async Task InterruptedReadRunsDirectly()
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon fake = new StubDaemon(
            DaemonPaths.ForCurrentUser(),
            StubDaemon.Close
        );
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        int code = new CliRouter(output, error).Run(["claim", "list", "--json"], fixture.Root);
        Assert.Equal(0, code);
        Assert.Equal("[]", output.ToString().Trim());
    }

    [Fact]
    public async Task RefusedHelloIsNeverFollowedByADirectRun()
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon fake = new StubDaemon(
            DaemonPaths.ForCurrentUser(),
            StubDaemon.Close,
            refuseHello: true
        );
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        int code = new CliRouter(output, error).Run(["claim", "list", "--json"], fixture.Root);
        Assert.Equal(3, code);
        Assert.Contains("protocol_error", error.ToString());
        Assert.Equal("", output.ToString());
    }

    [Fact]
    public async Task AnswerToAnotherRequestIsAProtocolError()
    {
        await using StubDaemon fake = StubDaemon.StartAlone(StubDaemon.WrongId);
        await using DaemonClient client = (
            await DaemonClient.ConnectAsync(
                fake.Paths,
                new FileOwnership(),
                ProductVersion.Current,
                TestInstance.TempRoot,
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        )!;
        SermofurException error = await Assert.ThrowsAsync<SermofurException>(() =>
            client.RunAsync(["status"], CancellationToken.None)
        );
        Assert.Equal("protocol_error", error.Code);
    }

    [Fact]
    public async Task CancelledMcpCallLeavesTheNextOneItsOwnAnswer()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonChannel channel = new DaemonChannel(
            daemon.Paths,
            ProductVersion.Current,
            fixture.Root
        );
        using CancellationTokenSource cancel = new CancellationTokenSource();
        Task<CommandOutcome> slow = channel.RunAsync(["block"], cancel.Token);
        await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
        await cancel.CancelAsync();
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => slow);
        release.Set();
        CommandOutcome next = await channel.RunAsync(["quick"], CancellationToken.None);
        Assert.Equal("quick", next.Stdout);
        CommandOutcome after = await channel.RunAsync(["other"], CancellationToken.None);
        Assert.Equal("other", after.Stdout);
    }

    [Fact]
    public async Task InterruptedMcpWriteIsReportedAndNeverSentAgain()
    {
        await using StubDaemon fake = StubDaemon.StartAlone(StubDaemon.Close);
        await using DaemonChannel channel = new DaemonChannel(
            fake.Paths,
            ProductVersion.Current,
            TestInstance.TempRoot
        );
        SermofurException error = await Assert.ThrowsAsync<SermofurException>(() =>
            channel.RunAsync(
                ["claim", "add", "once", "--origin", "llm", "--actor", "test"],
                CancellationToken.None
            )
        );
        Assert.Equal(DaemonClient.InterruptedCode, error.Code);
        Assert.Equal(1, fake.Runs);
    }

    [Fact]
    public async Task OversizedReadRunsDirectlyButAnOversizedWriteIsNotReplayed()
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon fake = new StubDaemon(
            DaemonPaths.ForCurrentUser(),
            StubDaemon.TooLarge
        );
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        CliRouter router = new CliRouter(output, error);
        Assert.Equal(0, router.Run(["claim", "list", "--json"], fixture.Root));
        Assert.Equal("[]", output.ToString().Trim());
        Assert.Equal(1, router.Run(["claim", "add", "big", "--origin", "user"], fixture.Root));
        Assert.Contains("response_too_large", error.ToString());
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(0, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    [Fact]
    public async Task StopperWaitsForTheLockOfTheDaemon()
    {
        string folder = TestDaemon.NewHome();
        Directory.CreateDirectory(folder);
        string lockFile = Path.Combine(folder, "daemon.lock");
        try
        {
            FileStream held = new FileStream(
                lockFile,
                FileMode.OpenOrCreate,
                FileAccess.ReadWrite,
                FileShare.None
            );
            Assert.True(DaemonLock.IsHeld(lockFile));
            Task<bool> waiting = DaemonLock.WaitForReleaseAsync(lockFile, TimeSpan.FromSeconds(10));
            await Task.Delay(300);
            Assert.False(waiting.IsCompleted);
            await held.DisposeAsync();
            Assert.True(await waiting.WaitAsync(TimeSpan.FromSeconds(5)));
            Assert.True(
                await DaemonLock.WaitForReleaseAsync(
                    Path.Combine(folder, "absent.lock"),
                    TimeSpan.Zero
                )
            );
        }
        finally
        {
            TestDaemon.DeleteHome(folder);
        }
    }

    [Fact]
    public void FramesKeepAccentsAsUtf8()
    {
        byte[] frame = Framing.Encode(
            new IpcMessage { Kind = MessageKind.Result, Stdout = new string('é', 1000) },
            Framing.MaxResponseBytes
        );
        // Two bytes per "é", not the six of "é".
        Assert.True(frame.Length < 2100, $"{frame.Length} bytes");
    }
}
