using System.IO.Pipes;
using System.Net.Sockets;
using System.Runtime.Versioning;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>What a client finds at the endpoint of the daemon, before any exchange.</summary>
public enum EndpointState
{
    /// <summary>No endpoint: no daemon runs for this user.</summary>
    Absent,

    /// <summary>An endpoint of the current user exists; a daemon may answer.</summary>
    Present,

    /// <summary>The endpoint or its folder belongs to another account (FR-008).</summary>
    Foreign,
}

/// <summary>
/// Local endpoint of the daemon (research R3): a named pipe restricted to the current user on
/// Windows, a Unix domain socket <c>0600</c> in a folder <c>0700</c> elsewhere. No network port.
/// </summary>
public static class IpcEndpoint
{
    private const UnixFileMode PrivateFolder =
        UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;

    private const UnixFileMode PrivateFile = UnixFileMode.UserRead | UnixFileMode.UserWrite;

    private const UnixFileMode GroupOrOthers =
        UnixFileMode.GroupRead
        | UnixFileMode.GroupWrite
        | UnixFileMode.GroupExecute
        | UnixFileMode.OtherRead
        | UnixFileMode.OtherWrite
        | UnixFileMode.OtherExecute;

    /// <summary>
    /// State of the endpoint, from file system metadata only: cheap enough to run before every
    /// command (SC-006). It never connects.
    /// </summary>
    public static EndpointState Inspect(DaemonPaths paths, IFileOwnership owners)
    {
        if (OperatingSystem.IsWindows())
        {
            // A directory listing of the pipe file system: opening the pipe to read its attributes
            // (File.Exists) would consume a server instance. The account is checked by
            // CurrentUserOnly when connecting.
            return Directory.EnumerateFiles(PipeFolder, paths.Endpoint).Any()
                ? EndpointState.Present
                : EndpointState.Absent;
        }
        string folder = paths.RuntimeDirectory!;
        EntryStatus? folderStatus = owners.Inspect(folder);
        if (folderStatus is null)
        {
            return EndpointState.Absent;
        }
        if (!folderStatus.IsDirectory || !folderStatus.IsOwnedByCurrentUser || !IsPrivate(folder))
        {
            return EndpointState.Foreign;
        }
        EntryStatus? socket = owners.Inspect(paths.Endpoint);
        if (socket is null)
        {
            return EndpointState.Absent;
        }
        return socket.IsOwnedByCurrentUser && !socket.IsDirectory && !socket.IsRegularFile
            ? EndpointState.Present
            : EndpointState.Foreign;
    }

    /// <summary>
    /// Connects to the daemon of the current user, or returns null when nothing answers within
    /// <paramref name="timeout"/>. An endpoint of another account is <c>foreign_endpoint</c>.
    /// </summary>
    public static async Task<Stream?> ConnectAsync(
        DaemonPaths paths,
        IFileOwnership owners,
        TimeSpan timeout,
        CancellationToken cancellation
    )
    {
        EndpointState state = Inspect(paths, owners);
        if (state == EndpointState.Foreign)
        {
            throw ForeignEndpoint(paths);
        }
        if (state == EndpointState.Absent)
        {
            return null;
        }
        return OperatingSystem.IsWindows()
            ? await ConnectPipeAsync(paths, timeout, cancellation)
            : await ConnectSocketAsync(paths, timeout, cancellation);
    }

    /// <summary>
    /// Opens the server side of the endpoint. Fails with <c>daemon_already_running</c> when a
    /// daemon already answers, and <c>foreign_endpoint</c> when the endpoint is not ours. A socket
    /// left by a crash is removed when it is ours and nothing answers on it.
    /// </summary>
    public static async Task<IpcListener> ListenAsync(
        DaemonPaths paths,
        IFileOwnership owners,
        CancellationToken cancellation
    )
    {
        if (OperatingSystem.IsWindows())
        {
            return PipeListener.Open(paths);
        }
        string folder = paths.RuntimeDirectory!;
        if (owners.Inspect(folder) is null)
        {
            Directory.CreateDirectory(folder, PrivateFolder);
        }
        EndpointState state = Inspect(paths, owners);
        if (state == EndpointState.Foreign)
        {
            throw ForeignEndpoint(paths);
        }
        if (state == EndpointState.Present)
        {
            await using Stream? running = await ConnectSocketAsync(
                paths,
                TimeSpan.FromMilliseconds(500),
                cancellation
            );
            if (running is not null)
            {
                throw AlreadyRunning();
            }
            File.Delete(paths.Endpoint);
        }
        return SocketListener.Open(paths);
    }

    public static SermofurException ForeignEndpoint(DaemonPaths paths) =>
        new SermofurException(
            "foreign_endpoint",
            $"The daemon endpoint {paths.Endpoint} or its folder belongs to another account or is not private; it is never used.",
            4
        );

    public static SermofurException AlreadyRunning() =>
        new SermofurException(
            "daemon_already_running",
            "A Sermofur daemon already serves this user.",
            3
        );

    private const string PipeFolder = @"\\.\pipe\";

    private static bool IsPrivate(string path) =>
        OperatingSystem.IsWindows() || (File.GetUnixFileMode(path) & GroupOrOthers) == 0;

    [SupportedOSPlatform("windows")]
    private static async Task<Stream?> ConnectPipeAsync(
        DaemonPaths paths,
        TimeSpan timeout,
        CancellationToken cancellation
    )
    {
        NamedPipeClientStream pipe = new NamedPipeClientStream(
            ".",
            paths.Endpoint,
            PipeDirection.InOut,
            PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly
        );
        try
        {
            await pipe.ConnectAsync(timeout, cancellation);
            return pipe;
        }
        catch (UnauthorizedAccessException)
        {
            // CurrentUserOnly: the server of this pipe runs under another account.
            await pipe.DisposeAsync();
            throw ForeignEndpoint(paths);
        }
        catch (Exception exception) when (exception is TimeoutException or IOException)
        {
            await pipe.DisposeAsync();
            return null;
        }
    }

    private static async Task<Stream?> ConnectSocketAsync(
        DaemonPaths paths,
        TimeSpan timeout,
        CancellationToken cancellation
    )
    {
        Socket socket = new Socket(AddressFamily.Unix, SocketType.Stream, ProtocolType.Unspecified);
        using CancellationTokenSource limit = CancellationTokenSource.CreateLinkedTokenSource(
            cancellation
        );
        limit.CancelAfter(timeout);
        try
        {
            await socket.ConnectAsync(new UnixDomainSocketEndPoint(paths.Endpoint), limit.Token);
            return new NetworkStream(socket, ownsSocket: true);
        }
        catch (Exception exception)
            when (exception is SocketException or OperationCanceledException
                && !cancellation.IsCancellationRequested
            )
        {
            socket.Dispose();
            return null;
        }
    }

    /// <summary>Named pipe server restricted to the current user.</summary>
    [SupportedOSPlatform("windows")]
    private sealed class PipeListener(DaemonPaths paths, NamedPipeServerStream first) : IpcListener
    {
        private NamedPipeServerStream? next = first;

        public static PipeListener Open(DaemonPaths paths)
        {
            try
            {
                // FirstPipeInstance: fails when any process already owns a pipe of this name.
                return new PipeListener(paths, Create(paths, PipeOptions.FirstPipeInstance));
            }
            catch (Exception exception)
                when (exception is IOException or UnauthorizedAccessException)
            {
                throw AlreadyRunning();
            }
        }

        public override async Task<Stream> AcceptAsync(CancellationToken cancellation)
        {
            NamedPipeServerStream pipe = next ?? Create(paths, PipeOptions.None);
            next = null;
            try
            {
                await pipe.WaitForConnectionAsync(cancellation);
                return pipe;
            }
            catch
            {
                await pipe.DisposeAsync();
                throw;
            }
        }

        public override async ValueTask DisposeAsync()
        {
            if (next is not null)
            {
                await next.DisposeAsync();
                next = null;
            }
        }

        private static NamedPipeServerStream Create(DaemonPaths paths, PipeOptions extra) =>
            new NamedPipeServerStream(
                paths.Endpoint,
                PipeDirection.InOut,
                NamedPipeServerStream.MaxAllowedServerInstances,
                PipeTransmissionMode.Byte,
                PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly | extra
            );
    }

    /// <summary>Unix domain socket server that only keeps peers of the current user.</summary>
    [UnsupportedOSPlatform("windows")]
    private sealed class SocketListener(DaemonPaths paths, Socket socket) : IpcListener
    {
        private readonly uint owner = UnixNative.EffectiveUserId();

        public static SocketListener Open(DaemonPaths paths)
        {
            Socket socket = new Socket(
                AddressFamily.Unix,
                SocketType.Stream,
                ProtocolType.Unspecified
            );
            try
            {
                socket.Bind(new UnixDomainSocketEndPoint(paths.Endpoint));
                File.SetUnixFileMode(paths.Endpoint, PrivateFile);
                socket.Listen(64);
                return new SocketListener(paths, socket);
            }
            catch
            {
                socket.Dispose();
                throw;
            }
        }

        public override async Task<Stream> AcceptAsync(CancellationToken cancellation)
        {
            while (true)
            {
                Socket client = await socket.AcceptAsync(cancellation);
                if (UnixNative.PeerUserId(client) == owner)
                {
                    return new NetworkStream(client, ownsSocket: true);
                }
                // A peer of another account (or of unknown identity) is closed unanswered.
                client.Dispose();
            }
        }

        public override ValueTask DisposeAsync()
        {
            socket.Dispose();
            try
            {
                File.Delete(paths.Endpoint);
            }
            catch (IOException)
            {
                // Removed on the next start when it is ours and nothing answers on it.
            }
            return ValueTask.CompletedTask;
        }
    }
}

/// <summary>Server side of the endpoint: hands out one stream per connected client.</summary>
public abstract class IpcListener : IAsyncDisposable
{
    public abstract Task<Stream> AcceptAsync(CancellationToken cancellation);

    public abstract ValueTask DisposeAsync();
}
