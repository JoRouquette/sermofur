using System.Collections.Concurrent;
using System.Diagnostics;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>Limits of the server, replaceable in tests.</summary>
public sealed record DaemonLimits(TimeSpan HelloTimeout, TimeSpan RequestTimeout)
{
    public static DaemonLimits Default { get; } =
        new DaemonLimits(TimeSpan.FromSeconds(10), TimeSpan.FromSeconds(60));
}

/// <summary>
/// The daemon: accepts clients on the local endpoint, checks the protocol and the version, and
/// runs their commands through the executor of the CLI (contracts/ipc.md). Writes to one
/// instance run one at a time; reads run concurrently (FR-014).
/// </summary>
public sealed class DaemonServer(
    DaemonPaths paths,
    string toolVersion,
    ICommandExecutor executor,
    IServingGate gate,
    DaemonLog log,
    IFileOwnership owners,
    DaemonLimits? limits = null
)
{
    private readonly DaemonLimits limits = limits ?? DaemonLimits.Default;
    private readonly ConcurrentDictionary<string, SemaphoreSlim> writers = new(
        OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal
    );
    private readonly DateTimeOffset startedAt = DateTimeOffset.Now;
    private int clients;
    private CancellationTokenSource? lifetime;

    /// <summary>Raised once the endpoint listens; tests wait for it.</summary>
    public event Action? Listening;

    /// <summary>
    /// Serves until <paramref name="stop"/> is cancelled or a client of the user asks for a
    /// shutdown (<c>smf daemon stop</c>).
    /// </summary>
    public async Task RunAsync(CancellationToken stop)
    {
        using CancellationTokenSource source = CancellationTokenSource.CreateLinkedTokenSource(
            stop
        );
        lifetime = source;
        await ServeAllAsync(source.Token);
    }

    private async Task ServeAllAsync(CancellationToken stop)
    {
        await using IpcListener listener = await IpcEndpoint.ListenAsync(paths, owners, stop);
        log.Write("started");
        Listening?.Invoke();
        List<Task> sessions = [];
        try
        {
            while (!stop.IsCancellationRequested)
            {
                Stream stream;
                try
                {
                    stream = await listener.AcceptAsync(stop);
                }
                catch (OperationCanceledException) when (stop.IsCancellationRequested)
                {
                    break;
                }
                catch (Exception exception)
                    when (exception is IOException or System.Net.Sockets.SocketException)
                {
                    // A client that connects and leaves at once breaks only its own connection.
                    continue;
                }
                sessions.RemoveAll(task => task.IsCompleted);
                sessions.Add(ServeAsync(stream, stop));
            }
        }
        finally
        {
            await Task.WhenAll(sessions);
            log.Write("stopped");
        }
    }

    private async Task ServeAsync(Stream stream, CancellationToken stop)
    {
        Interlocked.Increment(ref clients);
        int? pid = null;
        try
        {
            await using (stream)
            {
                using CancellationTokenSource session =
                    CancellationTokenSource.CreateLinkedTokenSource(stop);
                IpcMessage? hello = await ReadHelloAsync(stream, session.Token);
                if (hello is null)
                {
                    return;
                }
                pid = hello.Pid;
                await SessionAsync(stream, hello, session);
            }
        }
        catch (SermofurException exception)
        {
            log.Write("refused", exception.Code, pid);
        }
        catch (Exception exception)
            when (exception is IOException or OperationCanceledException or ObjectDisposedException)
        {
            // The client went away or the daemon stops: nothing to answer.
        }
        finally
        {
            Interlocked.Decrement(ref clients);
        }
    }

    private async Task<IpcMessage?> ReadHelloAsync(Stream stream, CancellationToken cancellation)
    {
        using CancellationTokenSource limit = CancellationTokenSource.CreateLinkedTokenSource(
            cancellation
        );
        limit.CancelAfter(limits.HelloTimeout);
        IpcMessage? hello;
        try
        {
            hello = await Framing.ReadAsync(stream, limit.Token);
        }
        catch (SermofurException exception)
        {
            await TryAnswerAsync(stream, IpcMessage.Failure(exception.Code, exception.Message));
            throw;
        }
        if (hello is null)
        {
            return null;
        }
        if (hello.Kind == MessageKind.Shutdown)
        {
            // Accepted before any hello and from any version, so that a newer smf can stop an
            // older daemon. Only a client of the same account reaches this point.
            await Framing.WriteAsync(
                stream,
                new IpcMessage { Kind = MessageKind.Stopping },
                cancellation
            );
            log.Write("shutdown_requested");
            lifetime?.Cancel();
            return null;
        }
        if (
            hello.Kind != MessageKind.Hello
            || hello.Protocol != IpcMessage.CurrentProtocol
            || string.IsNullOrEmpty(hello.Cwd)
            || !Path.IsPathFullyQualified(hello.Cwd)
        )
        {
            await Refuse(
                stream,
                "protocol_error",
                "Expected a hello of protocol 1 with an absolute cwd."
            );
        }
        if (hello.ToolVersion != toolVersion)
        {
            await Refuse(
                stream,
                "daemon_version_mismatch",
                $"The daemon runs Sermofur {toolVersion}; this client is {hello.ToolVersion}. Run: smf daemon restart"
            );
        }
        await Framing.WriteAsync(
            stream,
            new IpcMessage
            {
                Kind = MessageKind.Welcome,
                Protocol = IpcMessage.CurrentProtocol,
                ToolVersion = toolVersion,
            },
            cancellation
        );
        return hello;
    }

    private async Task SessionAsync(
        Stream stream,
        IpcMessage hello,
        CancellationTokenSource session
    )
    {
        Task<IpcMessage?> next = Framing.ReadAsync(stream, session.Token);
        while (true)
        {
            IpcMessage? message;
            try
            {
                message = await next;
            }
            catch (SermofurException exception)
            {
                await TryAnswerAsync(stream, IpcMessage.Failure(exception.Code, exception.Message));
                throw;
            }
            if (message is null)
            {
                return;
            }
            if (message.Kind == MessageKind.Status)
            {
                await Framing.WriteAsync(stream, StatusMessage(), session.Token);
                next = Framing.ReadAsync(stream, session.Token);
                continue;
            }
            if (message.Kind == MessageKind.Shutdown)
            {
                // Only a client of the same account reaches this point (endpoint and peer checks).
                await Framing.WriteAsync(
                    stream,
                    new IpcMessage { Kind = MessageKind.Stopping },
                    session.Token
                );
                log.Write("shutdown_requested", null, hello.Pid);
                lifetime?.Cancel();
                return;
            }
            if (message.Kind != MessageKind.Run)
            {
                await Refuse(stream, "protocol_error", $"Unknown message kind '{message.Kind}'.");
            }
            if (message.Cwd != hello.Cwd || message.Argv is null || message.Id is null)
            {
                await Refuse(
                    stream,
                    "protocol_error",
                    "A run carries an id, argv and the cwd of its hello."
                );
            }
            // Read ahead while the command runs: a closed stream cancels a queued command, a
            // second frame while one is in flight is a protocol error.
            next = Framing.ReadAsync(stream, session.Token);
            Task<IpcMessage> run = RunAsync(message, hello.Pid, session.Token);
            Task finished = await Task.WhenAny((Task)run, next);
            if (finished == next && next.IsCompletedSuccessfully && next.Result is null)
            {
                await session.CancelAsync();
                log.Write("client_gone", null, hello.Pid);
                return;
            }
            if (finished == next && next.IsCompletedSuccessfully)
            {
                await Refuse(stream, "protocol_error", "One run at a time per connection.");
            }
            await Framing.WriteAsync(stream, await run, session.Token);
        }
    }

    private async Task<IpcMessage> RunAsync(
        IpcMessage request,
        int? pid,
        CancellationToken cancellation
    )
    {
        long id = request.Id!.Value;
        IReadOnlyList<string> argv = request.Argv!;
        string cwd = request.Cwd!;
        Stopwatch watch = Stopwatch.StartNew();
        CommandPlan plan = executor.Plan(argv, cwd);
        string? root = gate.Resolve(plan.Path);
        if (root is null)
        {
            return new IpcMessage { Kind = MessageKind.NotServing, Id = id };
        }
        SemaphoreSlim? writer = plan.Writes
            ? writers.GetOrAdd(root, _ => new SemaphoreSlim(1, 1))
            : null;
        if (writer is not null)
        {
            // Waiting is cancelled when the client goes; a started command always completes.
            await writer.WaitAsync(cancellation);
        }
        try
        {
            Task<CommandOutcome> work = Task.Run(
                () => executor.Execute(argv, cwd),
                CancellationToken.None
            );
            Task done = await Task.WhenAny(work, Task.Delay(limits.RequestTimeout, cancellation));
            if (done != work)
            {
                // The transaction finishes on its own; the client gets a timeout now.
                ReleaseWhenDone(work, writer);
                writer = null;
                if (cancellation.IsCancellationRequested)
                {
                    log.Write("abandoned", null, pid, watch.ElapsedMilliseconds);
                    throw new OperationCanceledException(cancellation);
                }
                log.Write("timeout", "request_timeout", pid, watch.ElapsedMilliseconds);
                return IpcMessage.Failure(
                    "request_timeout",
                    $"The command took longer than {limits.RequestTimeout.TotalSeconds:0} s.",
                    id
                );
            }
            CommandOutcome outcome = await work;
            log.Write(
                "run",
                outcome.ExitCode == 0 ? null : $"exit_{outcome.ExitCode}",
                pid,
                watch.ElapsedMilliseconds
            );
            return new IpcMessage
            {
                Kind = MessageKind.Result,
                Id = id,
                ExitCode = outcome.ExitCode,
                Stdout = outcome.Stdout,
                Stderr = outcome.Stderr,
            };
        }
        finally
        {
            writer?.Release();
        }
    }

    private static void ReleaseWhenDone(Task work, SemaphoreSlim? writer)
    {
        if (writer is null)
        {
            return;
        }
        _ = work.ContinueWith(_ => writer.Release(), TaskScheduler.Default);
    }

    private IpcMessage StatusMessage() =>
        new IpcMessage
        {
            Kind = MessageKind.Status,
            ToolVersion = toolVersion,
            Pid = Environment.ProcessId,
            StartedAt = startedAt.ToString("yyyy-MM-dd'T'HH:mm:sszzz"),
            InstancesOpen = gate.OpenCount,
            Clients = Volatile.Read(ref clients),
        };

    private static async Task Refuse(Stream stream, string code, string message)
    {
        await TryAnswerAsync(stream, IpcMessage.Failure(code, message));
        throw new SermofurException(code, message, 3);
    }

    private static async Task TryAnswerAsync(Stream stream, IpcMessage message)
    {
        try
        {
            using CancellationTokenSource limit = new CancellationTokenSource(
                TimeSpan.FromSeconds(2)
            );
            await Framing.WriteAsync(stream, message, limit.Token);
        }
        catch (Exception exception)
            when (exception is IOException or OperationCanceledException or ObjectDisposedException)
        {
            // The client is already gone.
        }
    }
}
