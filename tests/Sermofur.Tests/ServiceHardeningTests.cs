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
                Assert.Throws<SermofurException>(() => task.Install(definition)).Code
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
