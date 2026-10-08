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
        catch (SermofurException exception) when (exception.Code == "daemon_version_mismatch")
        {
            // Never write directly beside a daemon of another version (FR-013).
            WriteError(exception.Code, exception.Message, json);
            return exception.ExitCode;
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
            // Foreign endpoint, broken daemon, unencodable argument: the CLI still works.
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
        WriteError(code, answer.Message ?? code, json);
        return code == "request_too_large" ? 1 : 3;
    }

    /// <summary>
    /// Commands that always run in this process: help, version, empty, <c>init</c>,
    /// <c>doctor</c> and <c>daemon …</c>, or when <c>SERMOFUR_NO_DAEMON=1</c>.
    /// </summary>
    public static bool Routable(string[] arguments)
    {
        if (
            Environment.GetEnvironmentVariable(NoDaemonVariable) == "1"
            || arguments.Length == 0
            || CommandArguments.Asks(
                arguments,
                CommandArguments.Help,
                CommandArguments.HelpShortcut
            )
            || CommandArguments.Asks(
                arguments,
                CommandArguments.Version,
                CommandArguments.VersionShortcut
            )
        )
        {
            return false;
        }
        try
        {
            CommandArguments parsed = new CommandArguments(arguments);
            return parsed.Positionals.Count > 0
                && parsed.Positionals[0] is not ("init" or "doctor" or "daemon");
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
        using CancellationTokenSource limit = new CancellationTokenSource();
        using DaemonClient? client = DaemonClient
            .ConnectAsync(
                paths,
                owners,
                ProductVersion.Current,
                workingDirectory,
                ConnectTimeout,
                limit.Token
            )
            .GetAwaiter()
            .GetResult();
        return client?.RunAsync(arguments, limit.Token).GetAwaiter().GetResult();
    }

    private int Direct(string[] arguments, string workingDirectory) =>
        new CommandRunner(output, error).Run(arguments, workingDirectory);

    private void WriteError(string code, string message, bool json) =>
        error.WriteLine(json ? RecordJson.Write(new { code, message }) : $"smf: {code}: {message}");
}
