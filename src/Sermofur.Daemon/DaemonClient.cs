using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>
/// One session with the daemon of the current user: hello, then commands in sequence
/// (contracts/ipc.md). Used by the CLI router, and later by the MCP bridge.
/// </summary>
public sealed class DaemonClient : IAsyncDisposable, IDisposable
{
    private readonly Stream stream;
    private readonly string workingDirectory;
    private long nextId;

    private DaemonClient(Stream stream, string workingDirectory, string daemonVersion)
    {
        this.stream = stream;
        this.workingDirectory = workingDirectory;
        DaemonVersion = daemonVersion;
    }

    /// <summary>Version reported by the daemon in its welcome.</summary>
    public string DaemonVersion { get; }

    /// <summary>
    /// Opens a session for <paramref name="workingDirectory"/>, or returns null when no daemon
    /// answers within <paramref name="timeout"/>. Throws <c>foreign_endpoint</c> and
    /// <c>daemon_version_mismatch</c>; a daemon that answers garbage is <c>protocol_error</c>.
    /// </summary>
    public static async Task<DaemonClient?> ConnectAsync(
        DaemonPaths paths,
        IFileOwnership owners,
        string toolVersion,
        string workingDirectory,
        TimeSpan timeout,
        CancellationToken cancellation
    )
    {
        Stream? stream = await IpcEndpoint.ConnectAsync(paths, owners, timeout, cancellation);
        if (stream is null)
        {
            return null;
        }
        try
        {
            using CancellationTokenSource limit = CancellationTokenSource.CreateLinkedTokenSource(
                cancellation
            );
            limit.CancelAfter(timeout);
            await Framing.WriteAsync(
                stream,
                new IpcMessage
                {
                    Kind = MessageKind.Hello,
                    Protocol = IpcMessage.CurrentProtocol,
                    ToolVersion = toolVersion,
                    Cwd = workingDirectory,
                    Pid = Environment.ProcessId,
                },
                limit.Token
            );
            IpcMessage? answer = await Framing.ReadAsync(stream, limit.Token);
            if (answer?.Kind == MessageKind.Welcome && answer.ToolVersion is not null)
            {
                return new DaemonClient(stream, workingDirectory, answer.ToolVersion);
            }
            await stream.DisposeAsync();
            if (answer is null)
            {
                // Closed without a word: a daemon that is dying or stopping counts as none.
                return null;
            }
            if (answer.Kind == MessageKind.Error && answer.Code == "daemon_version_mismatch")
            {
                throw new SermofurException(answer.Code, answer.Message ?? answer.Code, 3);
            }
            throw new SermofurException(
                "protocol_error",
                answer.Message ?? $"Unexpected answer '{answer.Kind}' to the hello.",
                3
            );
        }
        catch (Exception exception)
            when (exception is IOException or OperationCanceledException
                && !cancellation.IsCancellationRequested
            )
        {
            // A daemon that stops answering mid-hello counts as no daemon.
            await stream.DisposeAsync();
            return null;
        }
    }

    /// <summary>
    /// Runs one command: a <c>result</c>, a <c>not_serving</c> or an <c>error</c> message.
    /// A closed session is an <see cref="IOException"/>.
    /// </summary>
    public async Task<IpcMessage> RunAsync(
        IReadOnlyList<string> argv,
        CancellationToken cancellation
    )
    {
        long id = ++nextId;
        await Framing.WriteAsync(
            stream,
            new IpcMessage
            {
                Kind = MessageKind.Run,
                Id = id,
                Argv = argv,
                Cwd = workingDirectory,
            },
            cancellation
        );
        return await Framing.ReadAsync(stream, cancellation)
            ?? throw new IOException("The daemon closed the session.");
    }

    /// <summary>Uptime, open instances and clients of the daemon.</summary>
    public async Task<IpcMessage> StatusAsync(CancellationToken cancellation)
    {
        await Framing.WriteAsync(
            stream,
            new IpcMessage { Kind = MessageKind.Status },
            cancellation
        );
        return await Framing.ReadAsync(stream, cancellation)
            ?? throw new IOException("The daemon closed the session.");
    }

    /// <summary>Asks the daemon to stop; it ends its sessions and exits cleanly.</summary>
    public async Task ShutdownAsync(CancellationToken cancellation)
    {
        await Framing.WriteAsync(
            stream,
            new IpcMessage { Kind = MessageKind.Shutdown },
            cancellation
        );
        await Framing.ReadAsync(stream, cancellation);
    }

    public ValueTask DisposeAsync() => stream.DisposeAsync();

    public void Dispose() => stream.Dispose();
}
