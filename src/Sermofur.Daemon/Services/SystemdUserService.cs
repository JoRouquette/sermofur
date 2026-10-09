using System.Text;

namespace Sermofur.Daemon.Services;

/// <summary>
/// Linux: a <c>systemd --user</c> unit, restarted one second after an abnormal exit, started
/// with the session (research R2).
/// </summary>
public sealed class SystemdUserService(IProcessRunner runner, string unitDirectory)
    : IServiceManager
{
    public const string UnitName = "sermofur.service";

    public string Name => "systemd user service";

    public string UnitFile => Path.Combine(unitDirectory, UnitName);

    public static SystemdUserService ForCurrentUser(IProcessRunner runner)
    {
        string config = Environment.GetEnvironmentVariable("XDG_CONFIG_HOME")
            is { Length: > 0 } value
            ? value
            : Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.UserProfile),
                ".config"
            );
        return new SystemdUserService(runner, Path.Combine(config, "systemd", "user"));
    }

    public string? Unavailable()
    {
        ProcessOutcome outcome = runner.Run("systemctl", "--user", "show-environment");
        return outcome.Succeeded
            ? null
            : $"no systemd user manager in this session ({(outcome.ExitCode == -1 ? "systemctl not found" : "systemctl --user failed")}).";
    }

    public ServiceStatus Query() =>
        new ServiceStatus(
            File.Exists(UnitFile),
            runner.Run("systemctl", "--user", "is-active", UnitName).Output.Trim() == "active"
        );

    public void Install(ServiceDefinition definition)
    {
        string? previous = File.Exists(UnitFile) ? File.ReadAllText(UnitFile) : null;
        Directory.CreateDirectory(unitDirectory);
        File.WriteAllText(UnitFile, Unit(definition));
        ProcessOutcome reload = runner.Run("systemctl", "--user", "daemon-reload");
        ProcessOutcome enable = reload.Succeeded
            ? runner.Run("systemctl", "--user", "enable", UnitName)
            : reload;
        ProcessOutcome restart = enable.Succeeded
            ? runner.Run("systemctl", "--user", "restart", UnitName)
            : enable;
        if (!restart.Succeeded)
        {
            if (previous is null)
            {
                // A first install leaves nothing behind, not even the enable link.
                runner.Run("systemctl", "--user", "disable", UnitName);
                File.Delete(UnitFile);
            }
            else
            {
                File.WriteAllText(UnitFile, previous);
            }
            runner.Run("systemctl", "--user", "daemon-reload");
            throw ServiceErrors.Failed("systemctl --user", restart);
        }
    }

    /// <summary>A value the unit cannot hold is refused before the running daemon stops.</summary>
    public void Validate(ServiceDefinition definition) => _ = Unit(definition);

    public void Uninstall()
    {
        runner.Run("systemctl", "--user", "disable", "--now", UnitName);
        if (File.Exists(UnitFile))
        {
            File.Delete(UnitFile);
        }
        runner.Run("systemctl", "--user", "daemon-reload");
    }

    public void Start() => Check("start", runner.Run("systemctl", "--user", "start", UnitName));

    public void Stop() => Check("stop", runner.Run("systemctl", "--user", "stop", UnitName));

    /// <summary>Unit text; every argument quoted, as systemd splits ExecStart on spaces.</summary>
    public static string Unit(ServiceDefinition definition)
    {
        StringBuilder unit = new StringBuilder();
        unit.Append("[Unit]\n");
        unit.Append($"Description=Sermofur daemon {definition.Version}\n\n");
        unit.Append("[Service]\nType=simple\n");
        unit.Append("ExecStart=")
            .Append(
                string.Join(
                    ' ',
                    new[] { definition.Executable }
                        .Concat(definition.Arguments)
                        .Select(value => Quote(value).Replace("$", "$$"))
                )
            )
            .Append('\n');
        foreach (
            KeyValuePair<string, string> variable in definition.Environment.OrderBy(pair =>
                pair.Key
            )
        )
        {
            // Environment= expands specifiers (%) but not variables ($).
            unit.Append($"Environment={Quote($"{variable.Key}={variable.Value}")}\n");
        }
        unit.Append("Restart=on-failure\nRestartSec=1\n");
        // Time for the daemon to drain its started commands before systemd forces it.
        unit.Append($"TimeoutStopSec={(int)DaemonLimits.StopTimeout.TotalSeconds}\n\n");
        unit.Append("[Install]\nWantedBy=default.target\n");
        return unit.ToString();
    }

    private static string Quote(string value)
    {
        if (value.IndexOfAny(['\n', '\r', '\0']) >= 0)
        {
            // A line break would end the directive and start another one.
            throw new Sermofur.Domain.SermofurException(
                "service_install_failed",
                "A path or variable of the service holds a line break or NUL; it cannot go into a unit file.",
                3
            );
        }
        return "\"" + value.Replace("\\", "\\\\").Replace("\"", "\\\"").Replace("%", "%%") + "\"";
    }

    private static void Check(string action, ProcessOutcome outcome)
    {
        if (!outcome.Succeeded)
        {
            throw ServiceErrors.Failed($"systemctl --user {action}", outcome);
        }
    }
}
