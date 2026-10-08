using Sermofur.Daemon;
using Sermofur.Domain;

namespace Sermofur.Tests;

public class DaemonPathsTests
{
    [Fact]
    public void HomeVariableMovesEveryLocation()
    {
        string root = Path.Combine(Path.GetTempPath(), "smf-home");
        DaemonPaths paths = DaemonPaths.Resolve(
            name => name == DaemonPaths.HomeVariable ? root : null,
            _ => throw new InvalidOperationException("No user folder is read with a home.")
        );
        Assert.Equal(Path.Combine(root, "config"), paths.ConfigDirectory);
        Assert.Equal(Path.Combine(root, "state"), paths.StateDirectory);
        Assert.Equal(Path.Combine(root, "config", "instances.json"), paths.RegistryFile);
        if (OperatingSystem.IsWindows())
        {
            Assert.StartsWith("sermofur-home-", paths.Endpoint);
            Assert.Null(paths.RuntimeDirectory);
        }
        else
        {
            Assert.Equal(Path.Combine(root, "run", "daemon.sock"), paths.Endpoint);
            Assert.Equal(Path.Combine(root, "run"), paths.RuntimeDirectory);
        }
    }

    [Fact]
    public void TwoHomesNeverShareAnEndpoint()
    {
        DaemonPaths first = DaemonPaths.Resolve(
            name => name == DaemonPaths.HomeVariable ? Path.Combine(Path.GetTempPath(), "a") : null,
            _ => ""
        );
        DaemonPaths second = DaemonPaths.Resolve(
            name => name == DaemonPaths.HomeVariable ? Path.Combine(Path.GetTempPath(), "b") : null,
            _ => ""
        );
        Assert.NotEqual(first.Endpoint, second.Endpoint);
    }

    [WindowsFact("the endpoint is a named pipe per account.")]
    public void WindowsUsesProfileFoldersAndAPipePerAccount()
    {
        DaemonPaths paths = DaemonPaths.Resolve(
            _ => null,
            folder =>
                folder == Environment.SpecialFolder.ApplicationData ? @"C:\Roaming" : @"C:\Local"
        );
        Assert.Equal(@"C:\Roaming\Sermofur", paths.ConfigDirectory);
        Assert.Equal(@"C:\Local\Sermofur", paths.StateDirectory);
        Assert.Matches("^sermofur-[0-9a-f]{16}$", paths.Endpoint);
        Assert.Equal(paths.Endpoint, DaemonPaths.Resolve(_ => null, _ => "").Endpoint);
    }

    [UnixFact]
    public void UnixFollowsTheBaseDirectoriesOfTheSystem()
    {
        Dictionary<string, string> environment = new Dictionary<string, string>
        {
            ["XDG_CONFIG_HOME"] = "/x/config",
            ["XDG_STATE_HOME"] = "/x/state",
            ["XDG_RUNTIME_DIR"] = "/run/user/1000",
            ["TMPDIR"] = "/tmp/t",
        };
        DaemonPaths paths = DaemonPaths.Resolve(
            name => environment.GetValueOrDefault(name),
            _ => "/home/u"
        );
        if (OperatingSystem.IsMacOS())
        {
            Assert.Equal("/home/u/Library/Application Support/Sermofur", paths.ConfigDirectory);
            Assert.Equal("/home/u/Library/Logs/Sermofur", paths.StateDirectory);
            Assert.StartsWith("/tmp/t/sermofur-", paths.Endpoint);
        }
        else
        {
            Assert.Equal("/x/config/sermofur", paths.ConfigDirectory);
            Assert.Equal("/x/state/sermofur", paths.StateDirectory);
            Assert.Equal("/run/user/1000/sermofur/daemon.sock", paths.Endpoint);
        }
    }

    [UnixFact]
    public void LinuxFallsBackToTheHomeAndTheTemporaryFolder()
    {
        if (!OperatingSystem.IsLinux())
        {
            return;
        }
        DaemonPaths paths = DaemonPaths.Resolve(_ => null, _ => "/home/u");
        Assert.Equal("/home/u/.config/sermofur", paths.ConfigDirectory);
        Assert.Equal("/home/u/.local/state/sermofur", paths.StateDirectory);
        Assert.Matches("^/tmp/sermofur-[0-9]+/daemon.sock$", paths.Endpoint);
    }

    [UnixFact]
    public void TooLongSocketPathIsRefusedWithItsRemedy()
    {
        string root = "/tmp/" + new string('a', 100);
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            DaemonPaths.Resolve(name => name == DaemonPaths.HomeVariable ? root : null, _ => "")
        );
        Assert.Equal(("invalid_path", 3), (refusal.Code, refusal.ExitCode));
        Assert.Contains(DaemonPaths.HomeVariable, refusal.Message);
    }
}
