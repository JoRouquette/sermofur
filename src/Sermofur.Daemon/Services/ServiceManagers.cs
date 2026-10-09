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

    /// <summary>
    /// Same executable, arguments, version and environment. PATH is left out: it differs from
    /// one shell to the next, and reinstalling for it would stop a daemon serving requests.
    /// </summary>
    public static bool Same(ServiceDefinition? left, ServiceDefinition right)
    {
        if (
            left is null
            || left.Executable != right.Executable
            || left.Version != right.Version
            || !left.Arguments.SequenceEqual(right.Arguments)
        )
        {
            return false;
        }
        Dictionary<string, string> a = WithoutPath(left.Environment);
        Dictionary<string, string> b = WithoutPath(right.Environment);
        return a.Count == b.Count
            && a.All(pair => b.TryGetValue(pair.Key, out string? value) && value == pair.Value);
    }

    private static Dictionary<string, string> WithoutPath(
        IReadOnlyDictionary<string, string> environment
    ) =>
        environment
            .Where(pair => !pair.Key.Equals("PATH", StringComparison.OrdinalIgnoreCase))
            .ToDictionary(pair => pair.Key, pair => pair.Value);
}
