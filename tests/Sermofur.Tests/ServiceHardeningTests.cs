using Sermofur.Daemon;
using Sermofur.Daemon.Services;
using Sermofur.Domain;

namespace Sermofur.Tests;

/// <summary>
/// System tools by absolute path, unit quoting, idempotent removal, the Windows task and its
/// environment, atomic configuration writes (review of lot 003).
/// </summary>
public class ServiceHardeningTests
{
    [UnixFact]
    public void SystemToolsNeverComeFromARelativeFolder()
    {
        string folder = NewFolder();
        try
        {
            string tool = Path.Combine(folder, "sermofur-fake-tool");
            File.WriteAllText(tool, "#!/bin/sh\n");
            string relative = Path.GetRelativePath(Directory.GetCurrentDirectory(), folder);
            Assert.Null(SystemTool.Resolve("sermofur-fake-tool", $".:{relative}"));
            Assert.Equal(tool, SystemTool.Resolve("sermofur-fake-tool", folder));
            Assert.True(Path.IsPathFullyQualified(SystemTool.Resolve("sh", "")!));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [WindowsFact("schtasks lives in the system folder.")]
    public void SchtasksComesFromTheSystemFolder()
    {
        Assert.Equal(
            Path.Combine(Environment.SystemDirectory, "schtasks.exe"),
            SystemTool.Resolve("schtasks", Directory.GetCurrentDirectory())
        );
        Assert.Null(SystemTool.Resolve("sermofur-fake-tool", Directory.GetCurrentDirectory()));
    }

    [Fact]
    public void SystemdUnitEscapesDollarsAndRefusesLineBreaks()
    {
        ServiceDefinition dollars = new ServiceDefinition(
            "/home/a$b/smf",
            ["daemon", "run"],
            "0.4.0",
            new Dictionary<string, string> { ["DOTNET_ROOT"] = "/opt/$dotnet" }
        );
        string unit = SystemdUserService.Unit(dollars);
        Assert.Contains("ExecStart=\"/home/a$$b/smf\"", unit);
        Assert.Contains("Environment=\"DOTNET_ROOT=/opt/$dotnet\"", unit);
        ServiceDefinition broken = dollars with
        {
            Environment = new Dictionary<string, string> { ["PATH"] = "/bin\nExecStartPre=/x" },
        };
        Assert.Equal(
            "service_install_failed",
            Assert.Throws<SermofurException>(() => SystemdUserService.Unit(broken)).Code
        );
    }

    [Fact]
    public void RemovingWhatWasNeverInstalledSucceeds()
    {
        string parent = NewFolder();
        try
        {
            string folder = Path.Combine(parent, "missing");
            RecordingRunner runner = new RecordingRunner();
            new SystemdUserService(runner, Path.Combine(folder, "units")).Uninstall();
            new LaunchAgentService(
                runner,
                Path.Combine(folder, "agents"),
                501,
                Path.Combine(folder, "logs")
            ).Uninstall();
            InstalledDefinition.Delete(new DaemonPaths(folder, folder, Path.Combine(folder, "s")));
            Assert.False(Directory.Exists(folder));
        }
        finally
        {
            Directory.Delete(parent, true);
        }
    }

    [UnixFact]
    [System.Runtime.Versioning.UnsupportedOSPlatform("windows")]
    public void CodexDeclarationKeepsOrCreatesAPrivateFile()
    {
        UnixFileMode secret = UnixFileMode.UserRead | UnixFileMode.UserWrite;
        string existing = NewFolder();
        string fresh = NewFolder();
        try
        {
            string file = Sermofur.Mcp.CodexConfigFile.ProjectPath(existing);
            Directory.CreateDirectory(Path.GetDirectoryName(file)!);
            File.WriteAllText(file, "model = \"x\"\n");
            File.SetUnixFileMode(file, secret);
            Sermofur.Mcp.McpDeclaration.Install(existing, "codex", "project");
            Assert.Equal(secret, File.GetUnixFileMode(file));
            Sermofur.Mcp.McpDeclaration.Uninstall(existing, "codex", "project");
            Assert.Equal(secret, File.GetUnixFileMode(file));
            Sermofur.Mcp.McpDeclaration.Install(fresh, "codex", "project");
            Assert.Equal(
                secret,
                File.GetUnixFileMode(Sermofur.Mcp.CodexConfigFile.ProjectPath(fresh))
            );
        }
        finally
        {
            Directory.Delete(existing, true);
            Directory.Delete(fresh, true);
        }
    }

    [Fact]
    public void ReinstallIgnoresAnotherPath()
    {
        ServiceDefinition installed = Definition(new() { ["PATH"] = "/a", ["DOTNET_ROOT"] = "/d" });
        Assert.True(
            ServiceManagers.Same(
                installed,
                Definition(new() { ["PATH"] = "/b", ["DOTNET_ROOT"] = "/d" })
            )
        );
        Assert.False(
            ServiceManagers.Same(
                installed,
                Definition(new() { ["PATH"] = "/a", ["DOTNET_ROOT"] = "/e" })
            )
        );
    }

    [Fact]
    public void WindowsTaskRefusesADaemonHomeItCannotCarry()
    {
        RecordingRunner runner = new RecordingRunner();
        string folder = NewFolder();
        try
        {
            WindowsScheduledTask task = new WindowsScheduledTask(runner, @"PC\john", folder);
            ServiceDefinition definition = Definition(
                new() { [DaemonPaths.HomeVariable] = "C:\\elsewhere" }
            );
            Assert.Equal(
                "service_install_failed",
                Assert.Throws<SermofurException>(() => task.Validate(definition)).Code
            );
            Assert.Equal(
                "service_install_failed",
                Assert.Throws<SermofurException>(() => task.Install(definition, null)).Code
            );
            Assert.Empty(runner.Calls);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void SupervisedDaemonGetsTheRecordedEnvironment()
    {
        ServiceDefinition child = Definition([]);
        System.Diagnostics.ProcessStartInfo start = Supervisor.ChildStart(
            child,
            Definition(new() { ["DOTNET_ROOT"] = "/opt/dotnet-recorded" })
        );
        Assert.Equal("/opt/dotnet-recorded", start.Environment["DOTNET_ROOT"]);
        Assert.Equal(["daemon", "run"], start.ArgumentList);
        Assert.False(start.UseShellExecute);
    }

    [UnixFact]
    [System.Runtime.Versioning.UnsupportedOSPlatform("windows")]
    public void AtomicWriteKeepsThePermissionsOfTheFile()
    {
        string folder = NewFolder();
        try
        {
            string file = Path.Combine(folder, "config.toml");
            File.WriteAllText(file, "a = 1\n");
            UnixFileMode secret = UnixFileMode.UserRead | UnixFileMode.UserWrite;
            File.SetUnixFileMode(file, secret);
            AtomicFile.Write(file, "a = 2\n"u8.ToArray());
            Assert.Equal(secret, File.GetUnixFileMode(file));
            string fresh = Path.Combine(folder, "new.toml");
            AtomicFile.Write(fresh, "b = 1\n"u8.ToArray(), secret);
            Assert.Equal(secret, File.GetUnixFileMode(fresh));
            Assert.Equal(
                ["config.toml", "new.toml"],
                Directory.GetFiles(folder).Select(Path.GetFileName).Order()
            );
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void AtomicWriteReplacesTheContentAndLeavesNoTemporaryFile()
    {
        string folder = NewFolder();
        try
        {
            string file = Path.Combine(folder, ".mcp.json");
            AtomicFile.Write(file, "{}"u8.ToArray());
            AtomicFile.Write(file, "{\"a\":1}"u8.ToArray());
            Assert.Equal("{\"a\":1}", File.ReadAllText(file));
            Assert.Single(Directory.GetFiles(folder));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void StoppersWaitLongerThanTheDrainWithOneValue()
    {
        DaemonLimits limits = DaemonLimits.Default;
        Assert.True(limits.StopTimeout > limits.DrainTimeout);
        int seconds = (int)limits.StopTimeout.TotalSeconds;
        ServiceDefinition definition = Definition([]);
        Assert.Contains($"TimeoutStopSec={seconds}\n", SystemdUserService.Unit(definition));
        Assert.Contains(
            $"<key>ExitTimeOut</key><integer>{seconds}</integer>",
            LaunchAgentService.Plist(definition, "/tmp/logs")
        );
        DaemonLimits shorter = new DaemonLimits(TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(2));
        Assert.Equal(TimeSpan.FromSeconds(17), shorter.StopTimeout);
    }

    [Fact]
    public void FailedTaskStartPutsThePreviousTaskBackWithoutStartingIt()
    {
        string folder = NewFolder();
        try
        {
            List<string> registered = [];
            ScriptedRunner runner = new ScriptedRunner(call =>
            {
                if (call.Contains("/Create"))
                {
                    // The task XML as schtasks reads it: UTF-16.
                    string file = call[(call.IndexOf("/XML ", StringComparison.Ordinal) + 5)..^3];
                    registered.Add(File.ReadAllText(file, System.Text.Encoding.Unicode));
                }
                return call.Contains("/Run")
                    ? new ProcessOutcome(1, "", "refused")
                    : new ProcessOutcome(0, "", "");
            });
            WindowsScheduledTask task = new WindowsScheduledTask(
                runner,
                @"PC\jérôme",
                folder,
                _ => "schtasks.exe"
            );
            ServiceDefinition previous = new ServiceDefinition(
                @"C:\Users\Jérôme\.dotnet\tools\smf.exe",
                ["daemon", "run", "--supervise"],
                "0.4.0",
                new Dictionary<string, string>()
            );
            Assert.Throws<SermofurException>(() => task.Install(Definition([]), previous));
            Assert.Equal(["/Create", "/Run", "/Create"], runner.Calls.Select(Verb));
            Assert.Contains(@"C:\Users\Jérôme\.dotnet\tools\smf.exe", registered[1]);
            Assert.Contains(@"PC\jérôme", registered[1]);
            Assert.Contains("/opt/smf", registered[0]);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Theory]
    [InlineData(false)]
    [InlineData(true)]
    public void FailedTaskStartRemovesTheNewTaskWhenNothingCanBePutBack(bool hadPrevious)
    {
        string folder = NewFolder();
        try
        {
            int creates = 0;
            ScriptedRunner runner = new ScriptedRunner(call =>
                call.Contains("/Run") || (call.Contains("/Create") && ++creates == 2)
                    ? new ProcessOutcome(1, "", "refused")
                    : new ProcessOutcome(0, "", "")
            );
            WindowsScheduledTask task = new WindowsScheduledTask(
                runner,
                @"PC\john",
                folder,
                _ => "schtasks.exe"
            );
            ServiceDefinition? previous = hadPrevious ? Definition([]) : null;
            Assert.Throws<SermofurException>(() => task.Install(Definition([]), previous));
            string[] expected = hadPrevious
                ? ["/Create", "/Run", "/Create", "/Delete"]
                : ["/Create", "/Run", "/Delete"];
            Assert.Equal(expected, runner.Calls.Select(Verb));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void MissingSchtasksMakesTheManagerUnavailable()
    {
        WindowsScheduledTask task = new WindowsScheduledTask(
            new ScriptedRunner(_ => new ProcessOutcome(0, "", "")),
            @"PC\john",
            TestInstance.TempRoot,
            _ => null
        );
        Assert.Contains("schtasks", task.Unavailable());
    }

    [Fact]
    public void LaunchdIsGivenTimeToRemoveTheAgentBeforeLoadingIt()
    {
        string folder = NewFolder();
        try
        {
            int prints = 0;
            int bootstraps = 0;
            List<TimeSpan> waits = [];
            ScriptedRunner runner = new ScriptedRunner(call =>
            {
                if (call.StartsWith("launchctl print gui/501/"))
                {
                    // Still loaded twice after bootout, then gone.
                    return new ProcessOutcome(++prints <= 2 ? 0 : 113, "", "");
                }
                if (call.StartsWith("launchctl bootstrap"))
                {
                    return ++bootstraps == 1
                        ? new ProcessOutcome(5, "", "Bootstrap failed: 5: Input/output error")
                        : new ProcessOutcome(0, "", "");
                }
                return new ProcessOutcome(0, "", "");
            });
            LaunchAgentService agent = new LaunchAgentService(
                runner,
                Path.Combine(folder, "agents"),
                501,
                Path.Combine(folder, "logs"),
                waits.Add
            );
            agent.Install(Definition([]), null);
            Assert.Equal(3, prints);
            Assert.Equal(2, bootstraps);
            Assert.Equal(
                [
                    TimeSpan.FromMilliseconds(100),
                    TimeSpan.FromMilliseconds(100),
                    TimeSpan.FromSeconds(1),
                ],
                waits
            );
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    private static string Verb(string call)
    {
        string[] parts = call.Split(' ');
        return parts[1];
    }

    private sealed class ScriptedRunner(Func<string, ProcessOutcome> answer) : IProcessRunner
    {
        public List<string> Calls { get; } = [];

        public ProcessOutcome Run(string program, params string[] arguments)
        {
            string call = $"{program} {string.Join(' ', arguments)}";
            Calls.Add(call);
            return answer(call);
        }
    }

    private static ServiceDefinition Definition(Dictionary<string, string> environment) =>
        new ServiceDefinition("/opt/smf", ["daemon", "run"], "0.4.0", environment);

    private static string NewFolder()
    {
        string folder = Path.Combine(
            TestInstance.TempRoot,
            "smf-hard-" + Guid.NewGuid().ToString("N")[..8]
        );
        Directory.CreateDirectory(folder);
        return folder;
    }

    private sealed class RecordingRunner : IProcessRunner
    {
        public List<string> Calls { get; } = [];

        public ProcessOutcome Run(string program, params string[] arguments)
        {
            Calls.Add($"{program} {string.Join(' ', arguments)}");
            return new ProcessOutcome(0, "", "");
        }
    }
}
