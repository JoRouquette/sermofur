using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Daemon;

/// <summary>The <c>daemon</c> check of doctor (contracts/cli.md): never fatal to the CLI.</summary>
public static class DaemonProbe
{
    public static DiagnosticCheck Check(
        Func<DaemonPaths> paths,
        IFileOwnership owners,
        string toolVersion
    )
    {
        try
        {
            DaemonPaths resolved = paths();
            EndpointState state = IpcEndpoint.Inspect(resolved, owners);
            if (state == EndpointState.Foreign)
            {
                return new DiagnosticCheck(
                    "daemon",
                    "error",
                    "foreign_endpoint: the daemon endpoint belongs to another account; commands run directly."
                );
            }
            if (state == EndpointState.Absent)
            {
                return new DiagnosticCheck("daemon", "warning", "absent: commands run directly.");
            }
            using DaemonClient? client = Connect(resolved, owners, toolVersion);
            return client is null
                ? new DiagnosticCheck("daemon", "warning", "not_answering: commands run directly.")
                : new DiagnosticCheck("daemon", "ok", $"running: version {client.DaemonVersion}.");
        }
        catch (SermofurException exception)
        {
            return new DiagnosticCheck("daemon", "error", $"{exception.Code}: {exception.Message}");
        }
    }

    private static DaemonClient? Connect(
        DaemonPaths paths,
        IFileOwnership owners,
        string toolVersion
    ) =>
        DaemonClient
            .ConnectAsync(
                paths,
                owners,
                toolVersion,
                Path.GetTempPath(),
                TimeSpan.FromSeconds(1),
                CancellationToken.None
            )
            .GetAwaiter()
            .GetResult();
}
