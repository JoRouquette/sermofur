using System.Runtime.InteropServices;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Mcp;

namespace Sermofur.Cli;

/// <summary><c>smf mcp …</c>: always run by the CLI itself, never through the daemon.</summary>
public sealed class McpCommands(Action<object, bool> write)
{
    public int Run(CommandArguments args, bool json, string workingDirectory)
    {
        if (args.Positionals.Count < 2)
        {
            throw new SermofurException(
                "invalid_arguments",
                "Subcommand required: serve, install or uninstall."
            );
        }
        string subcommand = args.Positionals[1];
        if (subcommand == "serve")
        {
            args.RequireCount(2);
            args.ValidateUsed();
            return Serve(workingDirectory);
        }
        string target = Path.GetFullPath(args.Option("path", workingDirectory)!, workingDirectory);
        args.RequireCount(2);
        args.ValidateUsed();
        switch (subcommand)
        {
            case "install":
                write(McpDeclaration.Install(target), json);
                return 0;
            case "uninstall":
                write(McpDeclaration.Uninstall(target), json);
                return 0;
            default:
                throw new SermofurException(
                    "invalid_arguments",
                    $"Unknown mcp subcommand '{subcommand}'; expected serve, install or uninstall."
                );
        }
    }

    /// <summary>The bridge on stdio, until the host closes stdin or stops the process.</summary>
    private static int Serve(string workingDirectory)
    {
        DaemonPaths paths = DaemonPaths.ForCurrentUser();
        string folder = Session.LaunchFolder(Environment.GetEnvironmentVariable, workingDirectory);
        using CancellationTokenSource stop = new CancellationTokenSource();
        using PosixSignalRegistration terminate = PosixSignalRegistration.Create(
            PosixSignal.SIGTERM,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
            }
        );
        using PosixSignalRegistration interrupt = PosixSignalRegistration.Create(
            PosixSignal.SIGINT,
            context =>
            {
                context.Cancel = true;
                stop.Cancel();
            }
        );
        DaemonChannel channel = new DaemonChannel(paths, ProductVersion.Current, folder);
        try
        {
            new McpBridge(channel, ProductVersion.Current, new DaemonLog(paths.LogFile))
                .RunAsync(stop.Token)
                .GetAwaiter()
                .GetResult();
        }
        catch (OperationCanceledException) when (stop.IsCancellationRequested) { }
        finally
        {
            channel.DisposeAsync().AsTask().GetAwaiter().GetResult();
        }
        return 0;
    }
}
