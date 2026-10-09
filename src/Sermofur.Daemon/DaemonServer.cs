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

    /// <summary>
    /// Longest a shutdown lets started commands finish and answer: one request timeout, plus
    /// the time to write the answers.
    /// </summary>
    public TimeSpan DrainTimeout => RequestTimeout + TimeSpan.FromSeconds(5);

    /// <summary>
    /// How long whoever stops the daemon waits for it to exit before forcing it (the CLI, the
    /// Windows supervisor, systemd and launchd): the drain plus a margin.
    /// </summary>
    public static TimeSpan StopTimeout => Default.DrainTimeout + TimeSpan.FromSeconds(10);
}

/// <summary>
/// The daemon: accepts clients on the local endpoint, checks the version and the protocol, and
/// runs their commands through the executor of the CLI (contracts/ipc.md). Writes to one
/// instance run one at a time; reads run concurrently (FR-014). A shutdown lets started commands
/// finish and answer, within the request timeout.
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
    private static readonly TimeSpan AnswerTimeout = TimeSpan.FromSeconds(5);

    private readonly DaemonLimits limits = limits ?? DaemonLimits.Default;
    private readonly ConcurrentDictionary<string, SemaphoreSlim> writers = new(
        OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal
    );
    private readonly ConcurrentDictionary<Task, byte> inFlight = new();
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
        IpcListener listener = await IpcEndpoint.ListenAsync(paths, owners, stop);
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
            // The endpoint goes first: a new client finds no daemon at once and runs directly,
            // instead of waiting for a hello nobody reads.
            await listener.DisposeAsync();
            // Sessions end once their started command has answered; commands whose client got a
            // timeout still finish their transaction. One budget for both, from now on.
            Task drained = Task.WhenAll([.. sessions, .. inFlight.Keys]);
            if (await Task.WhenAny(drained, Task.Delay(limits.DrainTimeout)) != drained)
            {
                log.Write("drain_timeout");
            }
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
                IpcMessage? hello = await ReadHelloAsync(stream, stop);
                if (hello is null)
                {
                    return;
                }
                pid = hello.Pid;
                await SessionAsync(stream, hello, stop);
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
        catch (Exception exception)
        {
            // An unexpected fault ends this session only, never the daemon.
            log.Write("error", exception.GetType().Name, pid);
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
            await TryAnswerAsync(
                stream,
                Failure(exception.Code, exception.Message, null, exception.ExitCode)
            );
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
            await AcceptShutdownAsync(stream, null, cancellation);
            return null;
        }
        if (hello.Kind != MessageKind.Hello)
        {
            await Refuse(stream, "protocol_error", $"Expected a hello, not '{hello.Kind}'.");
        }
        // The version first: a client of another version, whatever protocol it speaks, always
        // learns that a daemon of another version serves this user, and never writes beside it.
        if (hello.ToolVersion != toolVersion)
        {
            await Refuse(
                stream,
                IpcMessage.Failure(
                    "daemon_version_mismatch",
                    $"The daemon runs Sermofur {toolVersion}; this client is {hello.ToolVersion}. Run: smf daemon restart"
                ) with
                {
                    ToolVersion = toolVersion,
                }
            );
        }
        if (
            hello.Protocol != IpcMessage.CurrentProtocol
            || string.IsNullOrEmpty(hello.Cwd)
            || !Path.IsPathFullyQualified(hello.Cwd)
        )
        {
            await Refuse(
                stream,
                "protocol_error",
                $"Expected a hello of protocol {IpcMessage.CurrentProtocol} with an absolute cwd."
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

    private async Task SessionAsync(Stream stream, IpcMessage hello, CancellationToken stop)
    {
        // Departure of this client: abandons a command still waiting for the write lock. The
        // shutdown of the daemon does not cancel a started command (contracts/ipc.md).
        using CancellationTokenSource clientGone = new CancellationTokenSource();
        Task<IpcMessage?> next = Framing.ReadAsync(stream, stop);
        while (true)
        {
            IpcMessage? message;
            try
            {
                message = await next;
            }
            catch (SermofurException exception)
            {
                await TryAnswerAsync(
                    stream,
                    Failure(exception.Code, exception.Message, null, exception.ExitCode)
                );
                throw;
            }
            if (message is null)
            {
                return;
            }
            if (message.Kind == MessageKind.Status)
            {
                await Framing.WriteAsync(stream, StatusMessage(), Framing.MaxResponseBytes, stop);
                next = Framing.ReadAsync(stream, stop);
                continue;
            }
            if (message.Kind == MessageKind.Shutdown)
            {
                // Only a client of the same account reaches this point (endpoint and peer checks).
                await AcceptShutdownAsync(stream, hello.Pid, stop);
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
            // Read ahead while the command runs: a closed stream abandons a queued command, a
            // second frame while one is in flight is a protocol error.
            next = Framing.ReadAsync(stream, stop);
            Task<(IpcMessage Answer, bool Writes)> run = RunAsync(
                message,
                hello.Pid,
                clientGone.Token,
                stop
            );
            Task finished = await Task.WhenAny((Task)run, next);
            if (finished == next && next.IsCompletedSuccessfully && next.Result is null)
            {
                await clientGone.CancelAsync();
                log.Write("client_gone", null, hello.Pid);
                return;
            }
            if (finished == next && next.IsCompletedSuccessfully)
            {
                await Refuse(stream, "protocol_error", "One run at a time per connection.");
            }
            // Here the command has answered, or the daemon stops: the answer is still written.
            (IpcMessage answer, bool writes) = await run;
            await WriteAnswerAsync(stream, answer, writes);
        }
    }

    private async Task AcceptShutdownAsync(Stream stream, int? pid, CancellationToken cancellation)
    {
        await Framing.WriteAsync(
            stream,
            new IpcMessage { Kind = MessageKind.Stopping },
            cancellation
        );
        log.Write("shutdown_requested", null, pid);
        lifetime?.Cancel();
    }

    private static async Task WriteAnswerAsync(Stream stream, IpcMessage answer, bool writes)
    {
        using CancellationTokenSource limit = new CancellationTokenSource(AnswerTimeout);
        byte[] frame;
        // A character is at least one byte: over the limit in characters, the frame is too.
        int characters = (answer.Stdout?.Length ?? 0) + (answer.Stderr?.Length ?? 0);
        try
        {
            frame =
                characters > Framing.MaxResponseBytes
                    ? throw new SermofurException("request_too_large", "", 1)
                    : Framing.Encode(answer, Framing.MaxResponseBytes);
        }
        catch (SermofurException)
        {
            frame = Framing.Encode(TooLarge(answer.Id, writes), Framing.MaxResponseBytes);
        }
        await stream.WriteAsync(frame, limit.Token);
        await stream.FlushAsync(limit.Token);
    }

    /// <summary>
    /// The command has run, but its output does not fit an answer. A read can run again; a
    /// write already applied must not.
    /// </summary>
    private static IpcMessage TooLarge(long? id, bool writes) =>
        Failure(
            "response_too_large",
            writes
                ? $"The command ran, but its output is over {Framing.MaxResponseBytes / (1024 * 1024)} MiB and was not returned. Check its effect before running it again."
                : $"The output of the command is over {Framing.MaxResponseBytes / (1024 * 1024)} MiB; narrow it (from the CLI, it then runs directly).",
            id,
            1
        );

    private async Task<(IpcMessage Answer, bool Writes)> RunAsync(
        IpcMessage request,
        int? pid,
        CancellationToken clientGone,
        CancellationToken stop
    )
    {
        long id = request.Id!.Value;
        IReadOnlyList<string> argv = request.Argv!;
        string cwd = request.Cwd!;
        Stopwatch watch = Stopwatch.StartNew();
        CommandPlan plan;
        try
        {
            plan = executor.Plan(argv, cwd);
        }
        catch (SermofurException exception)
        {
            log.Write("refused", exception.Code, pid);
            return (Failure(exception.Code, exception.Message, id, exception.ExitCode), false);
        }
        string? root = gate.Resolve(plan.Path);
        if (root is null)
        {
            return (new IpcMessage { Kind = MessageKind.NotServing, Id = id }, plan.Writes);
        }
        SemaphoreSlim? writer = plan.Writes
            ? writers.GetOrAdd(root, _ => new SemaphoreSlim(1, 1))
            : null;
        if (writer is not null)
        {
            using CancellationTokenSource waiting = CancellationTokenSource.CreateLinkedTokenSource(
                clientGone,
                stop
            );
            try
            {
                await writer.WaitAsync(waiting.Token);
            }
            catch (OperationCanceledException) when (!clientGone.IsCancellationRequested)
            {
                // Nothing has started: the client may run the command again later.
                log.Write("stopping", "daemon_stopping", pid, watch.ElapsedMilliseconds);
                return (
                    Failure(
                        "daemon_stopping",
                        "The daemon is stopping; the command did not start.",
                        id,
                        3
                    ),
                    true
                );
            }
        }
        try
        {
            Task<CommandOutcome> work = Task.Run(
                () => executor.Execute(argv, cwd),
                CancellationToken.None
            );
            Track(work);
            Task done = await Task.WhenAny(work, Task.Delay(limits.RequestTimeout, clientGone));
            if (done != work)
            {
                // The transaction finishes on its own; the client gets a timeout now.
                ReleaseWhenDone(work, writer);
                writer = null;
                if (clientGone.IsCancellationRequested)
                {
                    log.Write("abandoned", null, pid, watch.ElapsedMilliseconds);
                    throw new OperationCanceledException(clientGone);
                }
                log.Write("timeout", "request_timeout", pid, watch.ElapsedMilliseconds);
                return (
                    Failure(
                        "request_timeout",
                        $"The command took longer than {limits.RequestTimeout.TotalSeconds:0} s; it keeps running and may still apply. Check before running it again.",
                        id,
                        3
                    ),
                    plan.Writes
                );
            }
            CommandOutcome outcome = await work;
            log.Write(
                "run",
                outcome.ExitCode == 0 ? null : $"exit_{outcome.ExitCode}",
                pid,
                watch.ElapsedMilliseconds
            );
            IpcMessage result = new IpcMessage
            {
                Kind = MessageKind.Result,
                Id = id,
                ExitCode = outcome.ExitCode,
                Stdout = outcome.Stdout,
                Stderr = outcome.Stderr,
            };
            return (result, plan.Writes);
        }
        finally
        {
            writer?.Release();
        }
    }

    private void Track(Task work)
    {
        inFlight.TryAdd(work, 0);
        _ = work.ContinueWith(
            finished => inFlight.TryRemove(finished, out _),
            TaskScheduler.Default
        );
    }

    private static void ReleaseWhenDone(Task work, SemaphoreSlim? writer)
    {
        if (writer is null)
        {
            return;
        }
        _ = work.ContinueWith(_ => writer.Release(), TaskScheduler.Default);
    }

    private static IpcMessage Failure(string code, string message, long? id, int exitCode) =>
        IpcMessage.Failure(code, message, id) with
        {
            ExitCode = exitCode,
        };

    private IpcMessage StatusMessage() =>
        new IpcMessage
        {
            Kind = MessageKind.Status,
            ToolVersion = toolVersion,
            Pid = Environment.ProcessId,
            Registry = paths.RegistryFile,
            StartedAt = startedAt.ToString("yyyy-MM-dd'T'HH:mm:sszzz"),
            InstancesOpen = gate.OpenCount,
            Clients = Volatile.Read(ref clients),
        };

    private static Task Refuse(Stream stream, string code, string message) =>
        Refuse(stream, IpcMessage.Failure(code, message));

    private static async Task Refuse(Stream stream, IpcMessage refusal)
    {
        await TryAnswerAsync(stream, refusal);
        throw new SermofurException(refusal.Code!, refusal.Message!, 3);
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
