using System.Diagnostics;
using System.Text.Json;
using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Infrastructure;
using Xunit.Abstractions;

namespace Sermofur.Tests;

/// <summary>
/// The daemon under load (issue #9): one deadline per request, a cap on the commands that run at
/// once, a deadline on the client side, and what the CLI does with each refusal.
/// </summary>
[Collection("Daemon timing")]
public class DaemonLoadTests(ITestOutputHelper report)
{
    /// <summary>
    /// Generous bound for a hello and a status in tests: the product gives up after 500 ms, but
    /// a loaded CI runner must not turn a slow machine into a red test. The measure is reported.
    /// </summary>
    private static readonly TimeSpan TestHelloBound = TimeSpan.FromSeconds(5);

    /// <summary>Bound of a call that must end on its own: a regression fails, never hangs.</summary>
    private static readonly TimeSpan CallBound = TimeSpan.FromSeconds(30);

    [Fact]
    public async Task WriteBehindALongWriteIsRefusedAtTheDeadlineAndNeverRuns()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            await using TestDaemon daemon = TestDaemon.Start(
                executor,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(1))
            );
            daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
            await using DaemonClient holder = (await daemon.Connect(fixture.Root))!;
            Task<IpcMessage> held = holder.RunAsync(["block", "write"], CancellationToken.None);
            await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
            await using DaemonClient waiting = (await daemon.Connect(fixture.Root))!;
            Stopwatch watch = Stopwatch.StartNew();
            IpcMessage refused = await waiting
                .RunAsync(["queued", "write"], CancellationToken.None)
                .WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(
                ("error", "daemon_busy", 3),
                (refused.Kind, refused.Code, refused.ExitCode)
            );
            Assert.True(watch.Elapsed < TimeSpan.FromSeconds(4), $"{watch.ElapsedMilliseconds} ms");
            // The long write still answers with its own timeout, and finishes once released.
            Assert.Equal("request_timeout", (await held.WaitAsync(TimeSpan.FromSeconds(10))).Code);
            release.Set();
            await daemon.StopAsync().WaitAsync(TimeSpan.FromSeconds(10));
            Assert.False(executor.Ran("queued"));
        }
        finally
        {
            release.Set();
        }
    }

    [Fact]
    public async Task ReadWaitingForAPlaceGivesUpLongBeforeTheDeadline()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            await using PlacesTaken taken = await PlacesTaken.StartAsync(
                executor,
                fixture.Root,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(20))
                {
                    MaxExecutions = 2,
                    ReadPlaceTimeout = TimeSpan.FromSeconds(1),
                }
            );
            await using DaemonClient third = (await taken.Daemon.Connect(fixture.Root))!;
            Stopwatch watch = Stopwatch.StartNew();
            IpcMessage refused = await third
                .RunAsync(["extra"], CancellationToken.None)
                .WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(("error", "daemon_busy"), (refused.Kind, refused.Code));
            // The read limit, not the 20 s deadline.
            Assert.True(watch.Elapsed < TimeSpan.FromSeconds(5), $"{watch.ElapsedMilliseconds} ms");
            release.Set();
            await taken.FinishAsync();
            Assert.False(executor.Ran("extra"));
        }
        finally
        {
            release.Set();
        }
    }

    [Fact]
    public async Task PlaceThatComesTooLateIsGivenBackWithoutStarting()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            // One place; a command must have 28 s of its 30 s left to start: a place that comes
            // within 2 s is in time, one that comes after 3 s is too late.
            await using TestDaemon daemon = TestDaemon.Start(
                executor,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(30))
                {
                    MaxExecutions = 1,
                    MinimumRunTime = TimeSpan.FromSeconds(28),
                }
            );
            daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
            await using DaemonClient holder = (await daemon.Connect(fixture.Root))!;
            Task<IpcMessage> held = holder.RunAsync(["block"], CancellationToken.None);
            await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
            await using DaemonClient late = (await daemon.Connect(fixture.Root))!;
            Task<IpcMessage> lateWrite = late.RunAsync(["late", "write"], CancellationToken.None);
            await executor.Planned("late").WaitAsync(TimeSpan.FromSeconds(10));
            await Task.Delay(3000);
            release.Set();
            IpcMessage refused = await lateWrite.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(("error", "daemon_busy"), (refused.Kind, refused.Code));
            Assert.Equal(0, (await held.WaitAsync(TimeSpan.FromSeconds(10))).ExitCode);
            // The place and the turn both came back: the next write starts at once.
            await using DaemonClient next = (await daemon.Connect(fixture.Root))!;
            IpcMessage written = await next.RunAsync(["next", "write"], CancellationToken.None)
                .WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal((MessageKind.Result, 0), (written.Kind, written.ExitCode));
            Assert.False(executor.Ran("late"));
        }
        finally
        {
            release.Set();
        }
    }

    [Fact]
    public void NoPlaceAtAllIsRefused() =>
        Assert.Throws<ArgumentOutOfRangeException>(() =>
            DaemonLimits.Default with
            {
                MaxExecutions = 0,
            }
        );

    [Fact]
    public async Task FullPlacesLeaveHelloAndStatusFreeAndAnExtraReadWaitsItsTurn()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            await using PlacesTaken taken = await PlacesTaken.StartAsync(
                executor,
                fixture.Root,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(20))
                {
                    MaxExecutions = 2,
                    // The extra read must still wait when the hello of a slow runner takes long.
                    ReadPlaceTimeout = TimeSpan.FromSeconds(20),
                }
            );
            await using DaemonClient extraClient = (await taken.Daemon.Connect(fixture.Root))!;
            Task<IpcMessage> extra = extraClient.RunAsync(["extra"], CancellationToken.None);
            await executor.Planned("extra").WaitAsync(TimeSpan.FromSeconds(10));
            // Both places are taken: the hello and the status do not wait for them.
            Stopwatch watch = Stopwatch.StartNew();
            await using DaemonClient probe = (
                await DaemonClient.ConnectAsync(
                    taken.Daemon.Paths,
                    new FileOwnership(),
                    ProductVersion.Current,
                    fixture.Root,
                    TestHelloBound,
                    CancellationToken.None
                )
            )!;
            Assert.Equal(
                MessageKind.Status,
                (await probe.StatusAsync(CancellationToken.None)).Kind
            );
            report.WriteLine(
                $"hello and status with every place taken: {watch.ElapsedMilliseconds} ms"
            );
            Assert.True(watch.Elapsed < TestHelloBound, $"{watch.ElapsedMilliseconds} ms");
            await Task.Delay(200);
            Assert.False(extra.IsCompleted);
            Assert.False(executor.Ran("extra"));
            release.Set();
            Assert.Equal("extra", (await extra.WaitAsync(TimeSpan.FromSeconds(10))).Stdout);
            await taken.FinishAsync();
        }
        finally
        {
            release.Set();
        }
    }

    [Fact]
    public async Task WriterThatLeavesWhileWaitingForAPlaceGivesItsTurnBack()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            await using PlacesTaken taken = await PlacesTaken.StartAsync(
                executor,
                fixture.Root,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(20))
                {
                    MaxExecutions = 2,
                }
            );
            // This write takes the turn of the instance, then waits for a place, and leaves.
            DaemonClient leaver = (await taken.Daemon.Connect(fixture.Root))!;
            Task<IpcMessage> left = leaver.RunAsync(["left", "write"], CancellationToken.None);
            await executor.Planned("left").WaitAsync(TimeSpan.FromSeconds(10));
            await Task.Delay(200);
            await leaver.DisposeAsync();
            await Task.Delay(200);
            release.Set();
            await taken.FinishAsync();
            // A turn kept by the leaver would make this write wait for the whole 20 s deadline.
            await using DaemonClient after = (await taken.Daemon.Connect(fixture.Root))!;
            IpcMessage written = await after
                .RunAsync(["after", "write"], CancellationToken.None)
                .WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal((MessageKind.Result, 0), (written.Kind, written.ExitCode));
            await Assert.ThrowsAnyAsync<Exception>(() => left);
            Assert.False(executor.Ran("left"));
        }
        finally
        {
            release.Set();
        }
    }

    [Fact]
    public async Task ShutdownRefusesAReadWaitingForAPlace()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        try
        {
            await using PlacesTaken taken = await PlacesTaken.StartAsync(
                executor,
                fixture.Root,
                new DaemonLimits(TimeSpan.FromSeconds(5), TimeSpan.FromSeconds(20))
                {
                    MaxExecutions = 2,
                }
            );
            await using DaemonClient third = (await taken.Daemon.Connect(fixture.Root))!;
            Task<IpcMessage> waiting = third.RunAsync(["extra"], CancellationToken.None);
            await executor.Planned("extra").WaitAsync(TimeSpan.FromSeconds(10));
            Task stopped = taken.Daemon.StopAsync();
            IpcMessage refused = await waiting.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.Equal(("error", "daemon_stopping"), (refused.Kind, refused.Code));
            release.Set();
            await taken.FinishAsync();
            await stopped.WaitAsync(TimeSpan.FromSeconds(10));
            Assert.False(executor.Ran("extra"));
        }
        finally
        {
            release.Set();
        }
    }

    [Theory]
    [InlineData("daemon_stopping")]
    [InlineData("daemon_busy")]
    public async Task CliRunsARefusedReadDirectlyButNeverAWrite(string refusal)
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon stub = new StubDaemon(
            DaemonPaths.ForCurrentUser(),
            StubDaemon.Refuse(refusal)
        );
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        CliRouter router = new CliRouter(output, error);
        Assert.Equal(
            0,
            await Task.Run(() => router.Run(["claim", "list", "--json"], fixture.Root))
                .WaitAsync(CallBound)
        );
        Assert.Equal("[]", output.ToString().Trim());
        Assert.Equal(
            3,
            await Task.Run(() =>
                    router.Run(["claim", "add", "not sent twice", "--origin", "user"], fixture.Root)
                )
                .WaitAsync(CallBound)
        );
        Assert.Contains(refusal, error.ToString());
        // One run for the read, one for the write: the write was never sent again.
        Assert.Equal(2, stub.Runs);
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(0, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    [Fact]
    public async Task SilentDaemonMakesTheCliReplayAReadAndReportAWrite()
    {
        using TestInstance fixture = new TestInstance();
        await using StubDaemon silent = new StubDaemon(DaemonPaths.ForCurrentUser());
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        CliRouter router = new CliRouter(output, error, TimeSpan.FromSeconds(1));
        Stopwatch watch = Stopwatch.StartNew();
        // Bounded from outside: without the client deadline, these calls would never return.
        Assert.Equal(
            0,
            await Task.Run(() => router.Run(["claim", "list", "--json"], fixture.Root))
                .WaitAsync(CallBound)
        );
        Assert.Equal("[]", output.ToString().Trim());
        Assert.Equal(
            3,
            await Task.Run(() =>
                    router.Run(["claim", "add", "unknown fate", "--origin", "user"], fixture.Root)
                )
                .WaitAsync(CallBound)
        );
        Assert.Contains(DaemonClient.InterruptedCode, error.ToString());
        Assert.True(watch.Elapsed < TimeSpan.FromSeconds(8), $"{watch.ElapsedMilliseconds} ms");
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(0, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    /// <summary>
    /// Measure of the review: 24 recalls at once and a write, on a real instance. Every command
    /// answers and every client is welcomed; the slowest hello is reported, against the 500 ms
    /// after which the CLI runs directly.
    /// </summary>
    [Fact]
    public async Task TwentyFourRecallsAndAWriteAllAnswerAndHellosStayInTime()
    {
        using TestInstance fixture = new TestInstance();
        // Reads may wait the whole deadline for a place: this measures the hellos, and a CI
        // runner with three or four cores has only two or three places for 25 commands.
        await using TestDaemon daemon = TestDaemon.Start(
            limits: DaemonLimits.Default with
            {
                ReadPlaceTimeout = DaemonLimits.Default.RequestTimeout,
            }
        );
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using (DaemonClient seeder = (await daemon.Connect(fixture.Root))!)
        {
            for (int index = 0; index < 40; index++)
            {
                IpcMessage added = await seeder.RunAsync(
                    [
                        "claim",
                        "add",
                        $"billing paginates by cursor, case {index}",
                        "--origin",
                        "user",
                    ],
                    CancellationToken.None
                );
                Assert.Equal(0, added.ExitCode);
            }
        }
        Stopwatch watch = Stopwatch.StartNew();
        List<Task<(IpcMessage Answer, long Hello)>> commands = [];
        for (int index = 0; index < 24; index++)
        {
            commands.Add(
                RunAlone(daemon, fixture.Root, ["recall", "how does billing paginate", "--json"])
            );
        }
        commands.Add(
            RunAlone(
                daemon,
                fixture.Root,
                ["claim", "add", "written under load", "--origin", "user"]
            )
        );
        long slowestProbe = 0;
        for (int probe = 0; probe < 5; probe++)
        {
            Stopwatch hello = Stopwatch.StartNew();
            await using DaemonClient? client = await DaemonClient.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                ProductVersion.Current,
                fixture.Root,
                TestHelloBound,
                CancellationToken.None
            );
            Assert.NotNull(client);
            slowestProbe = Math.Max(slowestProbe, hello.ElapsedMilliseconds);
            await Task.Delay(20);
        }
        (IpcMessage Answer, long Hello)[] answers = await Task.WhenAll(commands)
            .WaitAsync(TimeSpan.FromSeconds(60));
        Assert.All(
            answers,
            pair => Assert.Equal((MessageKind.Result, 0), (pair.Answer.Kind, pair.Answer.ExitCode))
        );
        report.WriteLine(
            $"25 commands in {watch.ElapsedMilliseconds} ms, {DaemonLimits.Default.MaxExecutions} places; "
                + $"slowest hello of a command {answers.Max(pair => pair.Hello)} ms, of a probe {slowestProbe} ms"
        );
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(41, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    private static async Task<(IpcMessage Answer, long Hello)> RunAlone(
        TestDaemon daemon,
        string root,
        string[] argv
    )
    {
        await Task.Yield();
        Stopwatch hello = Stopwatch.StartNew();
        await using DaemonClient? client = await DaemonClient.ConnectAsync(
            daemon.Paths,
            new FileOwnership(),
            ProductVersion.Current,
            root,
            TestHelloBound,
            CancellationToken.None
        );
        Assert.NotNull(client);
        long welcomed = hello.ElapsedMilliseconds;
        return (await client.RunAsync(argv, CancellationToken.None), welcomed);
    }

    /// <summary>
    /// A daemon whose two places are both taken by "block" reads of two clients, until the
    /// executor is released.
    /// </summary>
    private sealed class PlacesTaken : IAsyncDisposable
    {
        private readonly List<DaemonClient> holders;
        private readonly List<Task<IpcMessage>> held;

        private PlacesTaken(
            TestDaemon daemon,
            List<DaemonClient> holders,
            List<Task<IpcMessage>> held
        )
        {
            Daemon = daemon;
            this.holders = holders;
            this.held = held;
        }

        public TestDaemon Daemon { get; }

        public static async Task<PlacesTaken> StartAsync(
            GatedExecutor executor,
            string root,
            DaemonLimits limits
        )
        {
            TestDaemon daemon = TestDaemon.Start(executor, limits);
            daemon.Registry.Register(root, DateTimeOffset.Now);
            List<DaemonClient> holders = [];
            List<Task<IpcMessage>> held = [];
            for (int index = 0; index < limits.MaxExecutions; index++)
            {
                DaemonClient client = (await daemon.Connect(root))!;
                holders.Add(client);
                held.Add(client.RunAsync(["block"], CancellationToken.None));
            }
            await executor.Blocked(limits.MaxExecutions).WaitAsync(TimeSpan.FromSeconds(10));
            return new PlacesTaken(daemon, holders, held);
        }

        /// <summary>Waits for the blocked reads to answer, once the executor is released.</summary>
        public async Task FinishAsync()
        {
            foreach (Task<IpcMessage> answer in held)
            {
                await answer.WaitAsync(TimeSpan.FromSeconds(10));
            }
        }

        public async ValueTask DisposeAsync()
        {
            foreach (DaemonClient client in holders)
            {
                await client.DisposeAsync();
            }
            await Daemon.DisposeAsync();
        }
    }
}
