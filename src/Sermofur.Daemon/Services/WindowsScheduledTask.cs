using System.Security;
using System.Text;

namespace Sermofur.Daemon.Services;

/// <summary>
/// Windows: a scheduled task of the user, started at logon with an interactive token, no
/// administrator rights (research R2). S4U is avoided: outside a domain it never starts the
/// task. The task runs the supervisor, which restarts the daemon within a second; the task
/// scheduler itself only restarts the supervisor, after a minute.
/// </summary>
public sealed class WindowsScheduledTask(IProcessRunner runner, string userId, string workDirectory)
    : IServiceManager
{
    public const string TaskName = @"Sermofur\Daemon";

    public string Name => "scheduled task";

    public static WindowsScheduledTask ForCurrentUser(IProcessRunner runner, DaemonPaths paths) =>
        new WindowsScheduledTask(
            runner,
            $"{Environment.UserDomainName}\\{Environment.UserName}",
            paths.StateDirectory
        );

    public string? Unavailable() => null;

    public ServiceStatus Query()
    {
        ProcessOutcome query = runner.Run(
            "schtasks",
            "/Query",
            "/TN",
            TaskName,
            "/FO",
            "CSV",
            "/NH"
        );
        // The status column is localized; status cross-checks it with the endpoint anyway.
        bool running =
            query.Succeeded
            && (
                query.Output.Contains("\"Running\"", StringComparison.OrdinalIgnoreCase)
                // French: "En cours" or "En cours d’exécution", apostrophe varies.
                || query.Output.Contains("\"En cours", StringComparison.OrdinalIgnoreCase)
            );
        return new ServiceStatus(query.Succeeded, running);
    }

    public void Install(ServiceDefinition definition)
    {
        Directory.CreateDirectory(workDirectory);
        string file = Path.Combine(workDirectory, "task.xml");
        // schtasks reads the task XML as UTF-16.
        File.WriteAllText(file, TaskXml(definition, userId), Encoding.Unicode);
        try
        {
            ProcessOutcome created = runner.Run(
                "schtasks",
                "/Create",
                "/TN",
                TaskName,
                "/XML",
                file,
                "/F"
            );
            if (!created.Succeeded)
            {
                throw ServiceErrors.Failed("schtasks /Create", created);
            }
            ProcessOutcome started = runner.Run("schtasks", "/Run", "/TN", TaskName);
            if (!started.Succeeded)
            {
                runner.Run("schtasks", "/Delete", "/TN", TaskName, "/F");
                throw ServiceErrors.Failed("schtasks /Run", started);
            }
        }
        finally
        {
            File.Delete(file);
        }
    }

    public void Uninstall()
    {
        runner.Run("schtasks", "/End", "/TN", TaskName);
        runner.Run("schtasks", "/Delete", "/TN", TaskName, "/F");
    }

    public void Start()
    {
        ProcessOutcome started = runner.Run("schtasks", "/Run", "/TN", TaskName);
        if (!started.Succeeded)
        {
            throw ServiceErrors.Failed("schtasks /Run", started);
        }
    }

    public void Stop() => runner.Run("schtasks", "/End", "/TN", TaskName);

    public static string TaskXml(ServiceDefinition definition, string userId)
    {
        string arguments = string.Join(' ', definition.Arguments.Select(QuoteArgument));
        return $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <RegistrationInfo>
                <Description>Sermofur daemon {SecurityElement.Escape(
                definition.Version
            )}</Description>
              </RegistrationInfo>
              <Triggers>
                <LogonTrigger>
                  <Enabled>true</Enabled>
                  <UserId>{SecurityElement.Escape(userId)}</UserId>
                </LogonTrigger>
              </Triggers>
              <Principals>
                <Principal id="Author">
                  <UserId>{SecurityElement.Escape(userId)}</UserId>
                  <LogonType>InteractiveToken</LogonType>
                  <RunLevel>LeastPrivilege</RunLevel>
                </Principal>
              </Principals>
              <Settings>
                <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
                <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
                <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
                <AllowHardTerminate>true</AllowHardTerminate>
                <StartWhenAvailable>true</StartWhenAvailable>
                <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
                <IdleSettings>
                  <StopOnIdleEnd>false</StopOnIdleEnd>
                  <RestartOnIdle>false</RestartOnIdle>
                </IdleSettings>
                <AllowStartOnDemand>true</AllowStartOnDemand>
                <Enabled>true</Enabled>
                <Hidden>true</Hidden>
                <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
                <Priority>7</Priority>
                <RestartOnFailure>
                  <Interval>PT1M</Interval>
                  <Count>999</Count>
                </RestartOnFailure>
              </Settings>
              <Actions Context="Author">
                <Exec>
                  <Command>{SecurityElement.Escape(definition.Executable)}</Command>
                  <Arguments>{SecurityElement.Escape(arguments)}</Arguments>
                </Exec>
              </Actions>
            </Task>
            """;
    }

    /// <summary>Quoting of the Windows command line (CommandLineToArgvW rules).</summary>
    internal static string QuoteArgument(string argument)
    {
        if (argument.Length > 0 && !argument.Any(c => c is ' ' or '\t' or '"'))
        {
            return argument;
        }
        StringBuilder quoted = new StringBuilder("\"");
        int backslashes = 0;
        foreach (char character in argument)
        {
            if (character == '\\')
            {
                backslashes++;
                continue;
            }
            quoted.Append('\\', character == '"' ? backslashes * 2 + 1 : backslashes);
            backslashes = 0;
            quoted.Append(character);
        }
        quoted.Append('\\', backslashes * 2).Append('"');
        return quoted.ToString();
    }
}
