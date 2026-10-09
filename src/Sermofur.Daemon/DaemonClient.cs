using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>
/// One session with the daemon of the current user: hello, then commands in sequence
/// (contracts/ipc.md). Used by the CLI router, doctor, the daemon commands and the MCP bridge.
/// </summary>
public sealed class DaemonClient : IAsyncDisposable, IDisposable
{
    /// <summary>The daemon stopped after receiving a command: its result is unknown.</summary>
    public const string InterruptedCode = "daemon_interrupted";

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
    /// answers within <paramref name="timeout"/>. Throws <c>foreign_endpoint</c> before any
    /// exchange; any refusal of the hello by a daemon is thrown with the daemon's code
    /// (<c>daemon_version_mismatch</c>, <c>protocol_error</c>), and a client must then not run
    /// the command beside that daemon (FR-013).
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
        IpcMessage? answer;
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
            answer = await Framing.ReadAsync(stream, limit.Token);
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
        catch
        {
            await stream.DisposeAsync();
            throw;
        }
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
            throw new SermofurException(
                answer.Code,
                VersionMismatchMessage(answer.ToolVersion, toolVersion, answer.Message),
                3
            );
        }
        throw new SermofurException(
            answer.Kind == MessageKind.Error ? answer.Code ?? "protocol_error" : "protocol_error",
            answer.Message ?? $"Unexpected answer '{answer.Kind}' to the hello.",
            3
        );
    }

    /// <summary>
    /// Runs one command: a <c>result</c>, a <c>not_serving</c> or an <c>error</c> message.
    /// A failure while sending is an <see cref="IOException"/> or <c>request_too_large</c>: the
    /// daemon never ran the command. A failure once the command is sent is
    /// <see cref="InterruptedCode"/>: the daemon may have run it.
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
        IpcMessage? answer;
        try
        {
            answer = await Framing.ReadAsync(stream, Framing.MaxResponseBytes, cancellation);
        }
        catch (Exception exception) when (exception is IOException or SermofurException)
        {
            answer = null;
        }
        if (answer is null)
        {
            throw new SermofurException(
                InterruptedCode,
                "The daemon stopped during the command; its result is unknown. Check before running it again.",
                3
            );
        }
        bool carriesId = answer.Kind is MessageKind.Result or MessageKind.NotServing;
        if (answer.Id is long other ? other != id : carriesId)
        {
            throw new SermofurException(
                "protocol_error",
                $"The daemon answered request {answer.Id?.ToString() ?? "?"} instead of {id}.",
                3
            );
        }
        return answer;
    }

    /// <summary>Uptime, open instances and clients of the daemon.</summary>
    public async Task<IpcMessage> StatusAsync(CancellationToken cancellation)
    {
        await Framing.WriteAsync(
            stream,
            new IpcMessage { Kind = MessageKind.Status },
            cancellation
        );
        return await Framing.ReadAsync(stream, Framing.MaxResponseBytes, cancellation)
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

    /// <summary>
    /// Message of a version refusal, with the remedy of the side that is behind: the daemon
    /// restarts on the new tool; an older client (an MCP server started before an update) is
    /// started again by its host.
    /// </summary>
    public static string VersionMismatchMessage(
        string? daemonVersion,
        string clientVersion,
        string? fallback
    )
    {
        if (daemonVersion is null)
        {
            return fallback ?? $"The daemon runs another version than {clientVersion}.";
        }
        int order = CompareVersions(clientVersion, daemonVersion);
        if (order < 0)
        {
            return $"The daemon runs Sermofur {daemonVersion}; this client is the older {clientVersion}. Restart the program that started it (for the MCP server: restart the sermofur server in Claude Code or Codex), or update smf.";
        }
        return $"The daemon runs Sermofur {daemonVersion}; this client is {clientVersion}. Run: smf daemon restart";
    }

    private static int CompareVersions(string left, string right)
    {
        static Version? Core(string value)
        {
            int dash = value.IndexOf('-');
            return Version.TryParse(dash < 0 ? value : value[..dash], out Version? parsed)
                ? parsed
                : null;
        }
        Version? a = Core(left);
        Version? b = Core(right);
        return a is null || b is null ? 0 : a.CompareTo(b);
    }
}
