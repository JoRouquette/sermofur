using System.Security;
using System.Text;

namespace Sermofur.Daemon.Services;

/// <summary>
/// macOS: a launchd agent of the user session, started at login and kept alive after an
/// abnormal exit (research R2).
/// </summary>
/// <param name="sleep">Waits between two checks of launchd; replaceable in tests.</param>
public sealed class LaunchAgentService(
    IProcessRunner runner,
    string agentDirectory,
    uint uid,
    string logDirectory,
    Action<TimeSpan>? sleep = null
) : IServiceManager
{
    private readonly Action<TimeSpan> wait = sleep ?? Thread.Sleep;

    public const string Label = "io.github.jorouquette.sermofur";

    public string Name => "launchd agent";

    public string PlistFile => Path.Combine(agentDirectory, Label + ".plist");

    private string Domain => $"gui/{uid}";

    private string Target => $"{Domain}/{Label}";

    public static LaunchAgentService ForCurrentUser(IProcessRunner runner, DaemonPaths paths) =>
        new LaunchAgentService(
            runner,
            Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                "Library",
                "LaunchAgents"
            ),
            UnixNative.EffectiveUserId(),
            paths.StateDirectory
        );

    public string? Unavailable() =>
        runner.Run("launchctl", "print", Domain).Succeeded
            ? null
            : $"no launchd session domain {Domain} (no user logged in to the graphical session).";

    public ServiceStatus Query()
    {
        ProcessOutcome printed = runner.Run("launchctl", "print", Target);
        return new ServiceStatus(
            File.Exists(PlistFile),
            printed.Succeeded
                && printed.Output.Contains("state = running", StringComparison.Ordinal)
        );
    }

    // The previous unit or agent file is kept on disk and restored as is.
    public void Install(ServiceDefinition definition, ServiceDefinition? previousDefinition)
    {
        string? previous = File.Exists(PlistFile) ? File.ReadAllText(PlistFile) : null;
        Directory.CreateDirectory(agentDirectory);
        Directory.CreateDirectory(logDirectory);
        // Loaded agents are reloaded, never stacked.
        BootOut();
        File.WriteAllText(PlistFile, Plist(definition, logDirectory));
        ProcessOutcome loaded = Bootstrap();
        if (!loaded.Succeeded)
        {
            if (previous is null)
            {
                File.Delete(PlistFile);
            }
            else
            {
                File.WriteAllText(PlistFile, previous);
                Bootstrap();
            }
            throw ServiceErrors.Failed("launchctl bootstrap", loaded);
        }
    }

    /// <summary>
    /// bootout returns before launchd has removed the service, and a bootstrap right after fails
    /// with error 5: wait until the service is gone (5 s at most).
    /// </summary>
    private void BootOut()
    {
        runner.Run("launchctl", "bootout", Target);
        for (int attempt = 0; attempt < 50; attempt++)
        {
            if (!runner.Run("launchctl", "print", Target).Succeeded)
            {
                return;
            }
            wait(TimeSpan.FromMilliseconds(100));
        }
    }

    /// <summary>Loads the agent; error 5 (still being removed) is retried once, a second later.</summary>
    private ProcessOutcome Bootstrap()
    {
        ProcessOutcome loaded = runner.Run("launchctl", "bootstrap", Domain, PlistFile);
        bool stillRemoving =
            loaded.ExitCode == 5 || loaded.Error.Contains("failed: 5:", StringComparison.Ordinal);
        if (loaded.Succeeded || !stillRemoving)
        {
            return loaded;
        }
        wait(TimeSpan.FromSeconds(1));
        return runner.Run("launchctl", "bootstrap", Domain, PlistFile);
    }

    public void Uninstall()
    {
        runner.Run("launchctl", "bootout", Target);
        if (File.Exists(PlistFile))
        {
            File.Delete(PlistFile);
        }
    }

    public void Start()
    {
        ProcessOutcome started = runner.Run("launchctl", "kickstart", Target);
        if (!started.Succeeded)
        {
            throw ServiceErrors.Failed("launchctl kickstart", started);
        }
    }

    /// <summary>SIGTERM: the daemon exits cleanly and KeepAlive does not restart a clean exit.</summary>
    public void Stop() => runner.Run("launchctl", "kill", "SIGTERM", Target);

    public static string Plist(ServiceDefinition definition, string logDirectory)
    {
        StringBuilder plist = new StringBuilder();
        plist.Append("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        plist.Append(
            "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n"
        );
        plist.Append("<plist version=\"1.0\">\n<dict>\n");
        plist.Append($"  <key>Label</key><string>{Label}</string>\n");
        plist.Append("  <key>ProgramArguments</key>\n  <array>\n");
        foreach (string argument in new[] { definition.Executable }.Concat(definition.Arguments))
        {
            plist.Append($"    <string>{Escape(argument)}</string>\n");
        }
        plist.Append("  </array>\n");
        plist.Append("  <key>RunAtLoad</key><true/>\n");
        plist.Append("  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n");
        plist.Append("  <key>ThrottleInterval</key><integer>1</integer>\n");
        // Time for the daemon to drain its started commands before launchd forces it.
        plist.Append(
            $"  <key>ExitTimeOut</key><integer>{(int)DaemonLimits.Default.StopTimeout.TotalSeconds}</integer>\n"
        );
        plist.Append("  <key>EnvironmentVariables</key>\n  <dict>\n");
        foreach (
            KeyValuePair<string, string> variable in definition.Environment.OrderBy(pair =>
                pair.Key
            )
        )
        {
            plist.Append(
                $"    <key>{Escape(variable.Key)}</key><string>{Escape(variable.Value)}</string>\n"
            );
        }
        plist.Append("  </dict>\n");
        plist.Append(
            $"  <key>StandardErrorPath</key><string>{Escape(Path.Combine(logDirectory, "launchd.err"))}</string>\n"
        );
        plist.Append("</dict>\n</plist>\n");
        return plist.ToString();
    }

    private static string Escape(string value) => SecurityElement.Escape(value);
}
