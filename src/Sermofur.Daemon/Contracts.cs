namespace Sermofur.Daemon;

/// <summary>What the daemon needs to know about a command before running it.</summary>
/// <param name="Path">Absolute directory the command targets (<c>--path</c> or the client folder).</param>
/// <param name="Writes">True when the command may write; an unparsable command counts as a write.</param>
public sealed record CommandPlan(string Path, bool Writes);

/// <summary>Output of a command, as the CLI would have written it.</summary>
public sealed record CommandOutcome(int ExitCode, string Stdout, string Stderr);

/// <summary>
/// Runs smf commands for the daemon. Implemented by the CLI with its own CommandRunner, so the
/// daemon applies exactly the rules of the engine (constitution IV) without depending on it.
/// </summary>
public interface ICommandExecutor
{
    CommandPlan Plan(IReadOnlyList<string> argv, string workingDirectory);

    CommandOutcome Execute(IReadOnlyList<string> argv, string workingDirectory);
}

/// <summary>Decides which instance, if any, the daemon serves for a target path (FR-005).</summary>
public interface IServingGate
{
    /// <summary>Root of the registered instance serving <paramref name="path"/>, or null.</summary>
    string? Resolve(string path);

    /// <summary>Instances opened since the start and not closed since.</summary>
    int OpenCount { get; }
}
