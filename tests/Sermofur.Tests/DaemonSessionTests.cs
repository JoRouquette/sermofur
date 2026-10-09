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
        EchoExecutor executor = new EchoExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> write = client.RunAsync(["block", "write"], CancellationToken.None);
        await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
        Task stopped = daemon.StopAsync();
        await Task.Delay(200);
        Assert.False(stopped.IsCompleted);
        // The endpoint is closed during the drain: a new client finds no daemon.
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
        EchoExecutor executor = new EchoExecutor(release);
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
        EchoExecutor executor = new EchoExecutor(null);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient client = (await daemon.Connect(fixture.Root))!;
        IpcMessage answer = await client.RunAsync(["big"], CancellationToken.None);
        Assert.Equal(MessageKind.Result, answer.Kind);
        Assert.Equal(EchoExecutor.Big, answer.Stdout);
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
        await using FakeDaemon fake = FakeDaemon.Start(
            DaemonPaths.ForCurrentUser(),
            FakeDaemon.Close
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
        await using FakeDaemon fake = FakeDaemon.Start(
            DaemonPaths.ForCurrentUser(),
            FakeDaemon.Close
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
        await using FakeDaemon fake = FakeDaemon.Start(
            DaemonPaths.ForCurrentUser(),
            FakeDaemon.Close,
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
        await using FakeDaemon fake = FakeDaemon.StartAlone(FakeDaemon.WrongId);
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
        EchoExecutor executor = new EchoExecutor(release);
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
        await using FakeDaemon fake = FakeDaemon.StartAlone(FakeDaemon.Close);
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
        await using FakeDaemon fake = FakeDaemon.Start(
            DaemonPaths.ForCurrentUser(),
            FakeDaemon.TooLarge
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

    /// <summary>
    /// Writes its first argument on stdout; "block" waits for the test; "big" is 300 KiB of
    /// accented text; "huge" is over the answer limit; "write" anywhere plans a write.
    /// </summary>
    private sealed class EchoExecutor(ManualResetEventSlim? release) : ICommandExecutor
    {
        public static readonly string Big = string.Concat(Enumerable.Repeat("é€ ", 100_000));

        private readonly TaskCompletionSource started = new(
            TaskCreationOptions.RunContinuationsAsynchronously
        );
        private readonly Dictionary<string, TaskCompletionSource> planned = [];

        public Task Started => started.Task;

        public List<string> Executed { get; } = [];

        public Task Planned(string command)
        {
            lock (planned)
            {
                if (!planned.TryGetValue(command, out TaskCompletionSource? source))
                {
                    source = new TaskCompletionSource(
                        TaskCreationOptions.RunContinuationsAsynchronously
                    );
                    planned[command] = source;
                }
                return source.Task;
            }
        }

        public CommandPlan Plan(IReadOnlyList<string> argv, string workingDirectory)
        {
            // Signalled just before the daemon waits for the write lock.
            _ = Planned(argv[0]);
            lock (planned)
            {
                planned[argv[0]].TrySetResult();
            }
            return new CommandPlan(workingDirectory, argv.Contains("write"));
        }

        public CommandOutcome Execute(IReadOnlyList<string> argv, string workingDirectory)
        {
            lock (Executed)
            {
                Executed.Add(argv[0]);
            }
            if (argv[0] == "block")
            {
                started.TrySetResult();
                release?.Wait(TimeSpan.FromSeconds(30));
            }
            string output = argv[0] switch
            {
                "big" => Big,
                "huge" => new string('x', Framing.MaxResponseBytes + 1),
                _ => argv[0],
            };
            return new CommandOutcome(0, output, "");
        }
    }

    /// <summary>
    /// A daemon that welcomes any client, then handles each run with <c>behavior</c>: close
    /// the session without answering, or answer another request.
    /// </summary>
    private sealed class FakeDaemon : IAsyncDisposable
    {
        public const string Close = "close";
        public const string WrongId = "wrong_id";
        public const string TooLarge = "too_large";

        private readonly CancellationTokenSource stop = new();
        private int runs;

        /// <summary>Run frames received so far.</summary>
        public int Runs => Volatile.Read(ref runs);
        private readonly Task serving;
        private readonly string? ownHome;

        private FakeDaemon(DaemonPaths paths, string behavior, bool refuseHello, string? ownHome)
        {
            Paths = paths;
            this.ownHome = ownHome;
            IpcListener listener = IpcEndpoint
                .ListenAsync(paths, new FileOwnership(), stop.Token)
                .GetAwaiter()
                .GetResult();
            serving = Task.Run(() => ServeAsync(listener, behavior, refuseHello));
        }

        public DaemonPaths Paths { get; }

        public static FakeDaemon Start(
            DaemonPaths paths,
            string behavior,
            bool refuseHello = false,
            string? ownHome = null
        ) => new FakeDaemon(paths, behavior, refuseHello, ownHome);

        /// <summary>A fake daemon on a home of its own, deleted when it stops.</summary>
        public static FakeDaemon StartAlone(string behavior)
        {
            string home = TestDaemon.NewHome();
            return new FakeDaemon(TestDaemon.PathsOf(home), behavior, false, home);
        }

        private async Task ServeAsync(IpcListener listener, string behavior, bool refuseHello)
        {
            await using (listener)
            {
                while (!stop.IsCancellationRequested)
                {
                    Stream stream;
                    try
                    {
                        stream = await listener.AcceptAsync(stop.Token);
                    }
                    catch (Exception exception)
                        when (exception is OperationCanceledException or IOException)
                    {
                        return;
                    }
                    await using (stream)
                    {
                        try
                        {
                            await Framing.ReadAsync(stream, stop.Token);
                            if (refuseHello)
                            {
                                await Framing.WriteAsync(
                                    stream,
                                    IpcMessage.Failure("protocol_error", "Refused by the test."),
                                    stop.Token
                                );
                                continue;
                            }
                            await Framing.WriteAsync(
                                stream,
                                new IpcMessage
                                {
                                    Kind = MessageKind.Welcome,
                                    Protocol = IpcMessage.CurrentProtocol,
                                    ToolVersion = ProductVersion.Current,
                                },
                                stop.Token
                            );
                            IpcMessage? run = await Framing.ReadAsync(stream, stop.Token);
                            if (run?.Kind == MessageKind.Run)
                            {
                                Interlocked.Increment(ref runs);
                            }
                            if (behavior == TooLarge && run?.Id is long large)
                            {
                                await Framing.WriteAsync(
                                    stream,
                                    IpcMessage.Failure(
                                        "response_too_large",
                                        "Too large.",
                                        large
                                    ) with
                                    {
                                        ExitCode = 1,
                                    },
                                    stop.Token
                                );
                            }
                            if (behavior == WrongId && run?.Id is long id)
                            {
                                await Framing.WriteAsync(
                                    stream,
                                    new IpcMessage
                                    {
                                        Kind = MessageKind.Result,
                                        Id = id + 1,
                                        ExitCode = 0,
                                        Stdout = "",
                                        Stderr = "",
                                    },
                                    stop.Token
                                );
                            }
                        }
                        catch (Exception exception)
                            when (exception
                                    is IOException
                                        or OperationCanceledException
                                        or SermofurException
                            )
                        {
                            // The client went away.
                        }
                    }
                }
            }
        }

        public async ValueTask DisposeAsync()
        {
            await stop.CancelAsync();
            try
            {
                await serving.WaitAsync(TimeSpan.FromSeconds(10));
            }
            catch (OperationCanceledException) { }
            stop.Dispose();
            if (ownHome is not null)
            {
                TestDaemon.DeleteHome(ownHome);
            }
        }
    }
}
