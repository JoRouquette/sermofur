namespace Sermofur.Daemon.Services;

public static class ServiceManagers
{
    /// <summary>Service manager of the user session on this system.</summary>
    public static IServiceManager ForCurrentSystem(IProcessRunner runner, DaemonPaths paths)
    {
        if (OperatingSystem.IsWindows())
        {
            return WindowsScheduledTask.ForCurrentUser(runner, paths);
        }
        if (OperatingSystem.IsMacOS())
        {
            return LaunchAgentService.ForCurrentUser(runner, paths);
        }
        return SystemdUserService.ForCurrentUser(runner);
    }

    /// <summary>Same executable, arguments, version and environment.</summary>
    public static bool Same(ServiceDefinition? left, ServiceDefinition right) =>
        left is not null
        && left.Executable == right.Executable
        && left.Version == right.Version
        && left.Arguments.SequenceEqual(right.Arguments)
        && left.Environment.Count == right.Environment.Count
        && left.Environment.All(pair =>
            right.Environment.TryGetValue(pair.Key, out string? value) && value == pair.Value
        );
}
