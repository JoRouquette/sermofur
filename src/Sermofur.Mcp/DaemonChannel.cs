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
            for (int attempt = 1; ; attempt++)
            {
                DaemonClient session = client ??= await ConnectAsync(cancellation);
                IpcMessage answer;
                try
                {
                    answer = await session.RunAsync(argv, cancellation);
                }
                catch (IOException) when (attempt == 1)
                {
                    // The daemon restarted since the last call: open a new session once.
                    await DropAsync();
                    continue;
                }
                catch (IOException)
                {
                    await DropAsync();
                    throw Unavailable();
                }
                return Outcome(answer);
            }
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
            new InstanceManager(files).Discover(launchFolder);
            throw new SermofurException(
                "not_served",
                $"The instance of {launchFolder} is not registered with the daemon. Run, in the project: smf daemon register",
                3
            );
        }
        throw new SermofurException(
            answer.Code ?? "protocol_error",
            answer.Message ?? "Unexpected answer of the daemon.",
            3
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
