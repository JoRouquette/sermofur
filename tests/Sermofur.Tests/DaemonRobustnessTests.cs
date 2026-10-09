using System.Buffers.Binary;
using System.Text;
using System.Text.Json;
using Sermofur.Daemon;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>Limits, malformed requests, departures and replays (US4).</summary>
[Collection("Daemon timing")]
public class DaemonRobustnessTests
{
    [Fact]
    public async Task OversizedFrameIsRefusedAndOthersAreStillServed()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        await using Stream raw = await Hello(daemon, fixture.Root);
        byte[] header = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(header, Framing.MaxBodyBytes + 1);
        await raw.WriteAsync(header);
        IpcMessage refusal = (await Framing.ReadAsync(raw, CancellationToken.None))!;
        Assert.Equal(("error", "request_too_large"), (refusal.Kind, refusal.Code));
        await AssertServed(daemon, fixture.Root);
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("{\"kind\":\"teleport\"}")]
    [InlineData("{\"kind\":\"run\",\"argv\":[\"status\"]}")]
    public async Task MalformedRequestIsAProtocolErrorAndTheDaemonGoesOn(string body)
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        await using Stream raw = await Hello(daemon, fixture.Root);
        byte[] bytes = Encoding.UTF8.GetBytes(body);
        byte[] header = new byte[4];
        BinaryPrimitives.WriteUInt32LittleEndian(header, (uint)bytes.Length);
        await raw.WriteAsync(header);
        await raw.WriteAsync(bytes);
        IpcMessage refusal = (await Framing.ReadAsync(raw, CancellationToken.None))!;
        Assert.Equal(("error", "protocol_error"), (refusal.Kind, refusal.Code));
        await AssertServed(daemon, fixture.Root);
    }

    [Theory]
    [InlineData(2, "C:\\x")]
    [InlineData(1, "relative")]
    public async Task HelloOfAnotherProtocolOrWithoutAbsoluteFolderIsRefused(
        int protocol,
        string cwd
    )
    {
        await using TestDaemon daemon = TestDaemon.Start();
        await using Stream raw = (
            await IpcEndpoint.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        )!;
        IpcMessage hello = DaemonTests.Hello(cwd) with { Protocol = protocol };
        await Framing.WriteAsync(raw, hello, CancellationToken.None);
        IpcMessage refusal = (await Framing.ReadAsync(raw, CancellationToken.None))!;
        Assert.Equal(("error", "protocol_error"), (refusal.Kind, refusal.Code));
    }

    [Fact]
    public async Task SlowCommandTimesOutWithoutBlockingOtherClients()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(
            executor,
            new DaemonLimits(TimeSpan.FromSeconds(10), TimeSpan.FromMilliseconds(300))
        );
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient slow = (await daemon.Connect(fixture.Root))!;
        IpcMessage timedOut = await slow.RunAsync(["block", "read"], CancellationToken.None);
        Assert.Equal(("error", "request_timeout"), (timedOut.Kind, timedOut.Code));
        await using DaemonClient other = (await daemon.Connect(fixture.Root))!;
        Assert.Equal(0, (await other.RunAsync(["quick"], CancellationToken.None)).ExitCode);
        release.Set();
    }

    [Fact]
    public async Task ClientThatLeavesAbandonsItsQueuedWrite()
    {
        using TestInstance fixture = new TestInstance();
        using ManualResetEventSlim release = new ManualResetEventSlim();
        GatedExecutor executor = new GatedExecutor(release);
        await using TestDaemon daemon = TestDaemon.Start(executor);
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        await using DaemonClient holder = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> held = holder.RunAsync(["block", "write"], CancellationToken.None);
        await executor.Started.WaitAsync(TimeSpan.FromSeconds(10));
        DaemonClient leaver = (await daemon.Connect(fixture.Root))!;
        Task<IpcMessage> queued = leaver.RunAsync(["queued", "write"], CancellationToken.None);
        await Task.Delay(200);
        await leaver.DisposeAsync();
        await Task.Delay(200);
        release.Set();
        Assert.Equal(0, (await held).ExitCode);
        await Assert.ThrowsAnyAsync<Exception>(() => queued);
        await using DaemonClient after = (await daemon.Connect(fixture.Root))!;
        Assert.Equal(
            0,
            (await after.RunAsync(["quick", "write"], CancellationToken.None)).ExitCode
        );
        Assert.DoesNotContain("queued", executor.Executed);
    }

    [Fact]
    public async Task ClientThatLeavesDuringAWriteLeavesACompleteWrite()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        for (int round = 0; round < 20; round++)
        {
            DaemonClient client = (await daemon.Connect(fixture.Root))!;
            Task<IpcMessage> write = client.RunAsync(
                ["claim", "add", $"cut {round}", "--origin", "user", "--json"],
                CancellationToken.None
            );
            await Task.Delay(round % 5);
            await client.DisposeAsync();
            await Task.WhenAny(write);
        }
        // A last write queues behind any write still running for a client that left.
        await using (DaemonClient last = (await daemon.Connect(fixture.Root))!)
        {
            IpcMessage settled = await last.RunAsync(
                ["claim", "add", "after the cuts", "--origin", "user"],
                CancellationToken.None
            );
            Assert.Equal(0, settled.ExitCode);
        }
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.DoesNotContain(report.Checks, check => check.Status == "error");
        await AssertServed(daemon, fixture.Root);
    }

    [Fact]
    public async Task ReplayedKeyGivesTheFirstObject()
    {
        using TestInstance fixture = new TestInstance();
        await using TestDaemon daemon = TestDaemon.Start();
        daemon.Registry.Register(fixture.Root, DateTimeOffset.Now);
        string[] add = ["claim", "add", "once", "--origin", "user", "--key", "replay-1", "--json"];
        await using DaemonClient first = (await daemon.Connect(fixture.Root))!;
        IpcMessage original = await first.RunAsync(add, CancellationToken.None);
        await using DaemonClient second = (await daemon.Connect(fixture.Root))!;
        IpcMessage replayed = await second.RunAsync(add, CancellationToken.None);
        Assert.Equal(Id(original), Id(replayed));
        CliResult listed = DaemonTests.Direct(fixture.Root, ["claim", "list", "--json"]);
        Assert.Equal(1, JsonDocument.Parse(listed.Output).RootElement.GetArrayLength());
    }

    private static string Id(IpcMessage result) =>
        JsonDocument.Parse(result.Stdout!).RootElement.GetProperty("id").GetString()!;

    private static async Task<Stream> Hello(TestDaemon daemon, string cwd)
    {
        Stream raw = (
            await IpcEndpoint.ConnectAsync(
                daemon.Paths,
                new FileOwnership(),
                TimeSpan.FromSeconds(5),
                CancellationToken.None
            )
        )!;
        await Framing.WriteAsync(raw, DaemonTests.Hello(cwd), CancellationToken.None);
        Assert.Equal(
            MessageKind.Welcome,
            (await Framing.ReadAsync(raw, CancellationToken.None))!.Kind
        );
        return raw;
    }

    private static async Task AssertServed(TestDaemon daemon, string cwd)
    {
        await using DaemonClient client = (await daemon.Connect(cwd))!;
        Assert.Equal(MessageKind.Status, (await client.StatusAsync(CancellationToken.None)).Kind);
    }
}
