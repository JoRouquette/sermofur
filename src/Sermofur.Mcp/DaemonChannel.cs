using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Mcp;

/// <summary>
/// Channel of the bridge to the daemon (research R8): one session for the launch folder, opened on
/// the first call, opened again after the daemon went away, one command in flight at a time. The
/// bridge never opens an instance: every failure becomes an error that names the remedy.
/// </summary>
public sealed class DaemonChannel(
    DaemonPaths paths,
    string toolVersion,
    string launchFolder,
    IFileOwnership? owners = null
) : ICommandChannel, IAsyncDisposable
{
    private readonly IFileOwnership files = owners ?? new FileOwnership();
    private static readonly TimeSpan ProbeTimeout = TimeSpan.FromSeconds(2);

    private readonly SemaphoreSlim gate = new(1, 1);
    private DaemonClient? client;

    public async Task<CommandOutcome> RunAsync(
        IReadOnlyList<string> argv,
        CancellationToken cancellation
    )
    {
        await gate.WaitAsync(cancellation);
        try
        {
            DaemonClient session = await SessionAsync(cancellation);
            IpcMessage answer;
            try
            {
                answer = await session.RunAsync(argv, cancellation);
            }
            catch (IOException)
            {
                // Not sent: the daemon never saw the command.
                await DropAsync();
                throw Unavailable();
            }
            catch
            {
                // Cancelled, interrupted or out of step: this session can no longer pair a
                // request with its answer. The daemon learns of the departure from the closed
                // connection; an interrupted write is never sent again.
                await DropAsync();
                throw;
            }
            return Outcome(answer);
        }
        finally
        {
            gate.Release();
        }
    }

    public async ValueTask DisposeAsync()
    {
        await DropAsync();
        gate.Dispose();
    }

    /// <summary>
    /// The open session if the daemon still answers on it (a status round trip, which changes
    /// nothing and can be repeated), else a new one: a daemon restarted since the last call is
    /// found before any command is sent.
    /// </summary>
    private async Task<DaemonClient> SessionAsync(CancellationToken cancellation)
    {
        if (client is not null)
        {
            // Bounded on its own: a daemon alive but stuck must not hold the bridge.
            using CancellationTokenSource probe = CancellationTokenSource.CreateLinkedTokenSource(
                cancellation
            );
            probe.CancelAfter(ProbeTimeout);
            try
            {
                await client.StatusAsync(probe.Token);
                return client;
            }
            catch (Exception exception)
                when (exception is IOException or SermofurException
                    || (
                        exception is OperationCanceledException
                        && !cancellation.IsCancellationRequested
                    )
                )
            {
                // Nothing was sent: dropping the session and opening a new one is safe.
                await DropAsync();
            }
            catch
            {
                await DropAsync();
                throw;
            }
        }
        client = await ConnectAsync(cancellation);
        return client;
    }

    private async Task<DaemonClient> ConnectAsync(CancellationToken cancellation) =>
        await DaemonClient.ConnectAsync(
            paths,
            files,
            toolVersion,
            launchFolder,
            TimeSpan.FromMilliseconds(500),
            cancellation
        ) ?? throw Unavailable();

    private CommandOutcome Outcome(IpcMessage answer)
    {
        if (answer.Kind == MessageKind.Result && answer.ExitCode is int exitCode)
        {
            return new CommandOutcome(exitCode, answer.Stdout ?? "", answer.Stderr ?? "");
        }
        if (answer.Kind == MessageKind.NotServing)
        {
            // Discovery only reads the folders between the launch folder and the instance, to tell
            // "no instance" from "instance not registered".
            try
            {
                new InstanceManager(files).Discover(launchFolder);
            }
            catch (SermofurException exception) when (exception.Code == "no_instance")
            {
                throw new SermofurException(
                    exception.Code,
                    $"{exception.Message} Run, in the project: smf init, then smf daemon register",
                    exception.ExitCode
                );
            }
            throw new SermofurException(
                "not_served",
                $"The instance of {launchFolder} is not registered with the daemon. Run, in the project: smf daemon register",
                3
            );
        }
        throw new SermofurException(
            answer.Code ?? "protocol_error",
            answer.Message ?? "Unexpected answer of the daemon.",
            answer.ExitCode ?? 3
        );
    }

    private async Task DropAsync()
    {
        if (client is not null)
        {
            await client.DisposeAsync();
            client = null;
        }
    }

    private static SermofurException Unavailable() =>
        new SermofurException(
            "daemon_unavailable",
            "No Sermofur daemon answers. Run: smf daemon install (or smf daemon start), then smf daemon register in the project.",
            3
        );
}
