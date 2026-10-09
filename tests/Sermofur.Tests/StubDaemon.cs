using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>
/// A false daemon for tests: welcomes every client (or refuses the hello), then handles each run
/// with its behaviour. <see cref="Silent"/> reads the requests and never answers (alive but
/// stuck); <see cref="Close"/> closes the session without answering; <see cref="WrongId"/>
/// answers another request; <see cref="TooLarge"/> answers <c>response_too_large</c>;
/// <see cref="Refuse"/> answers an error with the given code (exit 3).
/// </summary>
internal sealed class StubDaemon : IAsyncDisposable
{
    public const string Silent = "silent";
    public const string Close = "close";
    public const string WrongId = "wrong_id";
    public const string TooLarge = "too_large";
    private const string RefusePrefix = "refuse:";

    private readonly CancellationTokenSource stop = new();
    private readonly Task serving;
    private readonly string behavior;
    private readonly bool refuseHello;
    private readonly string? ownHome;
    private int runs;
    private int disposed;

    public StubDaemon(
        DaemonPaths paths,
        string behavior = Silent,
        bool refuseHello = false,
        string? ownHome = null
    )
    {
        Paths = paths;
        this.behavior = behavior;
        this.refuseHello = refuseHello;
        this.ownHome = ownHome;
        IpcListener listener = IpcEndpoint
            .ListenAsync(paths, new FileOwnership(), stop.Token)
            .GetAwaiter()
            .GetResult();
        serving = Task.Run(() => ServeAsync(listener));
    }

    public DaemonPaths Paths { get; }

    /// <summary>Run frames received so far.</summary>
    public int Runs => Volatile.Read(ref runs);

    /// <summary>The behaviour that answers each run with an error of this code.</summary>
    public static string Refuse(string code) => RefusePrefix + code;

    /// <summary>A false daemon on a home of its own, deleted when it stops.</summary>
    public static StubDaemon StartAlone(string behavior)
    {
        string home = TestDaemon.NewHome();
        return new StubDaemon(TestDaemon.PathsOf(home), behavior, false, home);
    }

    private async Task ServeAsync(IpcListener listener)
    {
        List<Task> sessions = [];
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
                    break;
                }
                sessions.Add(HoldAsync(stream));
            }
        }
        await Task.WhenAll(sessions);
    }

    private async Task HoldAsync(Stream stream)
    {
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
                    return;
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
                while (await Framing.ReadAsync(stream, stop.Token) is IpcMessage request)
                {
                    if (request.Kind != MessageKind.Run)
                    {
                        continue;
                    }
                    Interlocked.Increment(ref runs);
                    IpcMessage? answer = Answer(request);
                    if (behavior == Close)
                    {
                        return;
                    }
                    if (answer is not null)
                    {
                        await Framing.WriteAsync(stream, answer, stop.Token);
                    }
                }
            }
            catch (Exception exception)
                when (exception is IOException or OperationCanceledException or SermofurException)
            {
                // The client went away, or the test ends.
            }
        }
    }

    private IpcMessage? Answer(IpcMessage request)
    {
        if (behavior == WrongId)
        {
            return new IpcMessage
            {
                Kind = MessageKind.Result,
                Id = request.Id + 1,
                ExitCode = 0,
                Stdout = "",
                Stderr = "",
            };
        }
        if (behavior == TooLarge)
        {
            return IpcMessage.Failure("response_too_large", "Too large.", request.Id) with
            {
                ExitCode = 1,
            };
        }
        if (behavior.StartsWith(RefusePrefix, StringComparison.Ordinal))
        {
            string code = behavior[RefusePrefix.Length..];
            return IpcMessage.Failure(code, $"Refused by the test ({code}).", request.Id) with
            {
                ExitCode = 3,
            };
        }
        return null;
    }

    public async ValueTask DisposeAsync()
    {
        if (Interlocked.Exchange(ref disposed, 1) != 0)
        {
            return;
        }
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
