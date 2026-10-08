using Sermofur.Cli;
using Sermofur.Daemon;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>
/// A daemon served in this process, under its own <c>SERMOFUR_DAEMON_HOME</c>-like folder: it
/// never reaches the real daemon of the user. The folder name stays short so that the Unix
/// socket path fits the 104 bytes of macOS.
/// </summary>
public sealed class TestDaemon : IAsyncDisposable
{
    private readonly CancellationTokenSource stop = new();
    private readonly Task serving;

    private TestDaemon(
        string home,
        ICommandExecutor? executor,
        DaemonLimits? limits,
        string? version
    )
    {
        Home = home;
        Paths = PathsOf(home);
        Registry = new InstanceRegistry(Paths.RegistryFile);
        Log = new DaemonLog(Paths.LogFile);
        TaskCompletionSource listening = new(TaskCreationOptions.RunContinuationsAsynchronously);
        DaemonServer server = new DaemonServer(
            Paths,
            version ?? ProductVersion.Current,
            executor ?? new CliCommandExecutor(),
            new ServingGate(Registry),
            Log,
            new FileOwnership(),
            limits
        );
        server.Listening += () => listening.TrySetResult();
        serving = Task.Run(() => server.RunAsync(stop.Token));
        Task.WhenAny(listening.Task, serving).GetAwaiter().GetResult();
        if (serving.IsFaulted)
        {
            serving.GetAwaiter().GetResult();
        }
    }

    public string Home { get; }

    public DaemonPaths Paths { get; }

    public InstanceRegistry Registry { get; }

    public DaemonLog Log { get; }

    public static string NewHome() =>
        Path.Combine(TestInstance.TempRoot, "smfd-" + Guid.NewGuid().ToString("N")[..8]);

    public static DaemonPaths PathsOf(string home) =>
        DaemonPaths.Resolve(name => name == DaemonPaths.HomeVariable ? home : null, _ => "");

    public static TestDaemon Start(
        ICommandExecutor? executor = null,
        DaemonLimits? limits = null,
        string? version = null
    ) => new TestDaemon(NewHome(), executor, limits, version);

    public Task<DaemonClient?> Connect(string workingDirectory, string? version = null) =>
        DaemonClient.ConnectAsync(
            Paths,
            new FileOwnership(),
            version ?? ProductVersion.Current,
            workingDirectory,
            TimeSpan.FromSeconds(5),
            CancellationToken.None
        );

    public async ValueTask DisposeAsync()
    {
        await stop.CancelAsync();
        try
        {
            await serving.WaitAsync(TimeSpan.FromSeconds(10));
        }
        catch (OperationCanceledException) { }
        stop.Dispose();
        DeleteHome(Home);
    }

    public static void DeleteHome(string home)
    {
        if (
            Path.GetFileName(home).StartsWith("smfd-", StringComparison.Ordinal)
            && Directory.Exists(home)
        )
        {
            // A killed daemon can hold its lock file for a moment on Windows.
            for (int attempt = 1; ; attempt++)
            {
                try
                {
                    Directory.Delete(home, true);
                    return;
                }
                catch (IOException) when (attempt < 25)
                {
                    Thread.Sleep(200);
                }
            }
        }
    }
}
