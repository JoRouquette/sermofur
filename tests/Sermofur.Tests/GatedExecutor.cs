using Sermofur.Daemon;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>
/// Executor of the daemon for tests: writes its first argument on stdout and records what ran.
/// "write" anywhere plans a write; "block" waits until the test releases it; "big" answers about
/// 600 KiB of accented text; "huge" answers more than the limit of an answer. Signals let a test
/// wait for a command to be planned or for blocked commands to run.
/// </summary>
internal sealed class GatedExecutor(ManualResetEventSlim? release) : ICommandExecutor
{
    public static readonly string Big = string.Concat(Enumerable.Repeat("é€ ", 100_000));

    private readonly Dictionary<string, TaskCompletionSource> planned = [];
    private readonly List<TaskCompletionSource> blocked = [];
    private int blocking;

    public List<string> Executed { get; } = [];

    /// <summary>Completes once a first "block" command runs.</summary>
    public Task Started => Blocked(1);

    /// <summary>Completes once <paramref name="count"/> "block" commands are running.</summary>
    public Task Blocked(int count)
    {
        lock (blocked)
        {
            while (blocked.Count < count)
            {
                blocked.Add(
                    new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously)
                );
            }
            return blocked[count - 1].Task;
        }
    }

    /// <summary>Completes once the daemon has planned <paramref name="command"/>, just before it waits for a turn or a place.</summary>
    public Task Planned(string command)
    {
        lock (planned)
        {
            if (!planned.TryGetValue(command, out TaskCompletionSource? source))
            {
                source = new TaskCompletionSource(
                    TaskCreationOptions.RunContinuationsAsynchronously
                );
                planned[command] = source;
            }
            return source.Task;
        }
    }

    public bool Ran(string command)
    {
        lock (Executed)
        {
            return Executed.Contains(command);
        }
    }

    public CommandPlan Plan(IReadOnlyList<string> argv, string workingDirectory)
    {
        _ = Planned(argv[0]);
        lock (planned)
        {
            planned[argv[0]].TrySetResult();
        }
        return new CommandPlan(workingDirectory, argv.Contains("write"));
    }

    public CommandOutcome Execute(IReadOnlyList<string> argv, string workingDirectory)
    {
        lock (Executed)
        {
            Executed.Add(argv[0]);
        }
        if (argv[0] == "block")
        {
            int count = Interlocked.Increment(ref blocking);
            _ = Blocked(count);
            lock (blocked)
            {
                blocked[count - 1].TrySetResult();
            }
            release?.Wait(TimeSpan.FromSeconds(30));
        }
        string output = argv[0] switch
        {
            "big" => Big,
            "huge" => new string('x', Framing.MaxResponseBytes + 1),
            _ => argv[0],
        };
        return new CommandOutcome(0, output, "");
    }
}
