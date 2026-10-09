using Sermofur.Application;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Cli;

/// <summary>
/// Sends a command to the daemon when it runs, has the version of this CLI and serves the
/// instance; runs it directly otherwise (research R7, FR-010 to FR-013, FR-021).
/// </summary>
public sealed class CliRouter(TextWriter output, TextWriter error)
{
    /// <summary>Set to 1 to never try the daemon (diagnosis).</summary>
    public const string NoDaemonVariable = "SERMOFUR_NO_DAEMON";

    private static readonly TimeSpan ConnectTimeout = TimeSpan.FromMilliseconds(500);

    public int Run(string[] arguments, string workingDirectory)
    {
        if (!Routable(arguments))
        {
            return Direct(arguments, workingDirectory);
        }
        bool json = CommandArguments.HasFlag(arguments, "json");
        IpcMessage? answer;
        try
        {
            answer = Send(arguments, workingDirectory);
        }
        catch (DaemonAnswer refusal)
        {
            // A daemon answered, refused or stopped mid-command: never run directly beside it
            // (FR-013), and never replay a write whose result is unknown.
            if (
                refusal.Error.Code == DaemonClient.InterruptedCode
                && !CommandRunner.Writes(new CommandArguments(arguments))
            )
            {
                return Direct(arguments, workingDirectory);
            }
            WriteError(refusal.Error.Code, refusal.Error.Message, json);
            return refusal.Error.ExitCode;
        }
        catch (Exception exception)
            when (exception
                    is SermofurException
                        or IOException
                        or UnauthorizedAccessException
                        or ArgumentException
                        or InvalidOperationException
                        or NotSupportedException
            )
        {
            // No daemon of ours, or the command never reached it (foreign endpoint, endpoint
            // gone, request too large to send): the CLI still works.
            return Direct(arguments, workingDirectory);
        }
        if (answer is null || answer.Kind == MessageKind.NotServing)
        {
            return Direct(arguments, workingDirectory);
        }
        if (answer.Kind == MessageKind.Result && answer.ExitCode is int exitCode)
        {
            output.Write(answer.Stdout);
            error.Write(answer.Stderr);
            return exitCode;
        }
        string code = answer.Code ?? "protocol_error";
        if (code == "response_too_large" && !CommandRunner.Writes(new CommandArguments(arguments)))
        {
            // A read can run again: directly, its output has no size limit.
            return Direct(arguments, workingDirectory);
        }
        WriteError(code, answer.Message ?? code, json);
        return answer.ExitCode ?? 3;
    }

    /// <summary>
    /// Commands that never run in the daemon, whoever sends them: help, version, empty,
    /// <c>init</c>, <c>doctor</c>, <c>daemon …</c> and <c>mcp …</c> (FR-021).
    /// </summary>
    public static bool LocalOnly(IReadOnlyList<string> arguments)
    {
        string[] values = [.. arguments];
        if (
            values.Length == 0
            || CommandArguments.Asks(values, CommandArguments.Help, CommandArguments.HelpShortcut)
            || CommandArguments.Asks(
                values,
                CommandArguments.Version,
                CommandArguments.VersionShortcut
            )
        )
        {
            return true;
        }
        try
        {
            CommandArguments parsed = new CommandArguments(values);
            return parsed.Positionals.Count == 0
                || parsed.Positionals[0] is "init" or "doctor" or "daemon" or "mcp";
        }
        catch (SermofurException)
        {
            // Not a command the daemon could run anyway; the runner reports the parsing error.
            return false;
        }
    }

    /// <summary>
    /// Commands that always run in this process: help, version, empty, <c>init</c>,
    /// <c>doctor</c> and <c>daemon …</c>, or when <c>SERMOFUR_NO_DAEMON=1</c>.
    /// </summary>
    public static bool Routable(string[] arguments)
    {
        if (Environment.GetEnvironmentVariable(NoDaemonVariable) == "1" || LocalOnly(arguments))
        {
            return false;
        }
        try
        {
            _ = new CommandArguments(arguments);
            return true;
        }
        catch (SermofurException)
        {
            // The runner reports the parsing error itself.
            return false;
        }
    }

    private static IpcMessage? Send(string[] arguments, string workingDirectory)
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        FileOwnership owners = new FileOwnership();
        // Metadata only: with no daemon, the cost is one stat or one pipe lookup (SC-006).
        if (IpcEndpoint.Inspect(paths, owners) != EndpointState.Present)
        {
            return null;
        }
        DaemonClient? client;
        try
        {
            client = DaemonClient
                .ConnectAsync(
                    paths,
                    owners,
                    ProductVersion.Current,
                    workingDirectory,
                    ConnectTimeout,
                    CancellationToken.None
                )
                .GetAwaiter()
                .GetResult();
        }
        catch (SermofurException exception) when (exception.Code != "foreign_endpoint")
        {
            // Our daemon answered the hello with a refusal.
            throw new DaemonAnswer(exception);
        }
        using (client)
        {
            if (client is null)
            {
                return null;
            }
            try
            {
                return client.RunAsync(arguments, CancellationToken.None).GetAwaiter().GetResult();
            }
            catch (SermofurException exception)
                when (exception.Code is DaemonClient.InterruptedCode or "protocol_error")
            {
                // The command was sent: the daemon may have run it.
                throw new DaemonAnswer(exception);
            }
        }
    }

    /// <summary>An error from our daemon after which the command must not simply run directly.</summary>
    private sealed class DaemonAnswer(SermofurException error) : Exception(error.Message, error)
    {
        public SermofurException Error { get; } = error;
    }

    private int Direct(string[] arguments, string workingDirectory) =>
        new CommandRunner(output, error).Run(arguments, workingDirectory);

    private void WriteError(string code, string message, bool json) =>
        error.WriteLine(json ? RecordJson.Write(new { code, message }) : $"smf: {code}: {message}");
}
