using Sermofur.Daemon;
using Sermofur.Domain;

namespace Sermofur.Cli;

/// <summary>
/// Runs commands for the daemon with the very CommandRunner of the CLI, from the folder of the
/// client: the output is what the direct CLI would have written (SC-003).
/// </summary>
public sealed class CliCommandExecutor : ICommandExecutor
{
    public CommandPlan Plan(IReadOnlyList<string> argv, string workingDirectory)
    {
        try
        {
            CommandArguments args = new CommandArguments([.. argv]);
            string path = Path.GetFullPath(
                args.Option("path", workingDirectory)!,
                workingDirectory
            );
            bool writes = args.Positionals.Count == 0 || CommandRunner.Writes(args);
            return new CommandPlan(path, writes);
        }
        catch (Exception exception)
            when (exception is SermofurException or ArgumentException or NotSupportedException)
        {
            // The runner reports the error itself; meanwhile, count it as a write on the folder
            // of the client, the most cautious plan.
            return new CommandPlan(workingDirectory, true);
        }
    }

    public CommandOutcome Execute(IReadOnlyList<string> argv, string workingDirectory)
    {
        using StringWriter output = new StringWriter();
        using StringWriter error = new StringWriter();
        int exitCode = new CommandRunner(output, error, "daemon").Run([.. argv], workingDirectory);
        return new CommandOutcome(exitCode, output.ToString(), error.ToString());
    }
}
