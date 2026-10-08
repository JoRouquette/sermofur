using System.Diagnostics;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class HardeningTests
{
    [Fact]
    public void EntryOfAnotherAccountStopsDiscoveryAndDoctorNamesIt()
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        InstanceManager manager = new InstanceManager(new ForeignOwnership());
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            manager.Discover(fixture.Root)
        );
        Assert.Equal(("foreign_owner", 4), (refusal.Code, refusal.ExitCode));
        Assert.Equal(fixture.Root, manager.DiscoverForDiagnosis(fixture.Root));
        DoctorReport report = new InstanceDoctor(new ForeignOwnership()).Inspect(fixture.Root);
        Assert.Equal(
            "invalid_instance: foreign_owner",
            report.Checks.Single(c => c.Name == "instance").Detail
        );
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void OwnInstanceAndFilesAreRecognized()
    {
        using TestInstance fixture = new TestInstance();
        FileOwnership ownership = new FileOwnership();
        EntryStatus marker = ownership.Inspect(Path.Combine(fixture.Root, ".sermofur"))!;
        Assert.True(marker.IsDirectory);
        Assert.True(marker.IsOwnedByCurrentUser);
        EntryStatus file = ownership.Inspect(InstanceManager.DatabasePath(fixture.Root))!;
        Assert.True(file.IsRegularFile);
        Assert.Null(ownership.Inspect(Path.Combine(fixture.Root, "absent")));
    }

    [Fact]
    public void OversizedConfigurationIsADamagedInstance()
    {
        using TestInstance fixture = new TestInstance();
        string file = Path.Combine(fixture.Root, ".sermofur", "instance.json");
        File.WriteAllText(
            file,
            File.ReadAllText(file) + new string(' ', (int)InstanceManager.MaxConfigurationBytes)
        );
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            fixture.Manager.ReadConfiguration(fixture.Root)
        );
        Assert.Equal(("invalid_instance", 3), (refusal.Code, refusal.ExitCode));
        Assert.Contains(
            new InstanceDoctor().Inspect(fixture.Root).Checks,
            check => check.Name == "instance" && check.Detail == "invalid_instance"
        );
    }

    /// <summary>
    /// Real entry of another account: root through passwordless sudo, as on the CI runners. Fails
    /// explicitly when sudo is not available instead of being skipped.
    /// </summary>
    [UnixFact]
    public async Task RealEntryOfAnotherAccountIsRefused()
    {
        using TestInstance fixture = new TestInstance();
        string marker = Path.Combine(fixture.Root, ".sermofur");
        Assert.True(
            await Sudo("-n", "true"),
            "This test needs passwordless sudo to create an entry owned by root."
        );
        try
        {
            Assert.True(await Sudo("-n", "chown", "-R", "0:0", marker));
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                new InstanceManager().Discover(fixture.Root)
            );
            Assert.Equal("foreign_owner", refusal.Code);
        }
        finally
        {
            await Sudo("-n", "chown", "-R", Environment.UserName, marker);
        }
    }

    [UnixFact]
    public async Task SpecialFileIsNotReadAsASource()
    {
        using TestInstance fixture = new TestInstance();
        string fifo = Path.Combine(fixture.Root, "pipe");
        using (Process mkfifo = Process.Start("mkfifo", [fifo])!)
        {
            await mkfifo.WaitForExitAsync();
            Assert.Equal(0, mkfifo.ExitCode);
        }
        using SqliteStore store = fixture.Open();
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            fixture.Sources(store).Add(fifo, TestInstance.User)
        );
        Assert.Equal("source_rejected", refusal.Code);
    }

    private static async Task<bool> Sudo(params string[] arguments)
    {
        try
        {
            using Process process = Process.Start(
                new ProcessStartInfo("sudo", arguments) { RedirectStandardError = true }
            )!;
            await process.WaitForExitAsync();
            return process.ExitCode == 0;
        }
        catch (System.ComponentModel.Win32Exception)
        {
            return false;
        }
    }

    private sealed class ForeignOwnership : IFileOwnership
    {
        public EntryStatus? Inspect(string path) =>
            Directory.Exists(path) ? new EntryStatus(false, true, false)
            : File.Exists(path) ? new EntryStatus(true, false, false)
            : null;
    }
}
