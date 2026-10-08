using System.Diagnostics;
using System.Runtime.Versioning;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text.Json;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class InstanceTests
{
    [Fact]
    public void InitIsIdempotentAndDiscoversNearestParent()
    {
        using TestInstance fixture = new TestInstance();
        InstanceConfiguration config = fixture.Manager.ReadConfiguration(fixture.Root);
        Dictionary<string, string> snapshot = fixture.Snapshot();
        fixture.Manager.Initialize(fixture.Root);
        Assert.Equal(config, fixture.Manager.ReadConfiguration(fixture.Root));
        Assert.Equal(snapshot.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
        string child = Path.Combine(fixture.Root, "a", "b", "c");
        Directory.CreateDirectory(child);
        Assert.Equal(fixture.Root, fixture.Manager.Discover(child));
        Assert.Equal(
            "nested_instance",
            Assert.Throws<SermofurException>(() => fixture.Manager.Initialize(child)).Code
        );
        Assert.False(Directory.Exists(Path.Combine(child, ".sermofur")));
    }

    [Fact]
    public void InstancesRemainPhysicallyIndependent()
    {
        using TestInstance a = new TestInstance();
        using TestInstance b = new TestInstance();
        Assert.NotEqual(
            a.Manager.ReadConfiguration(a.Root).InstanceId,
            b.Manager.ReadConfiguration(b.Root).InstanceId
        );
        using SqliteStore storeA = a.Open();
        using SqliteStore storeB = b.Open();
        MemoryRecord record = a.Memory(storeA)
            .CreateClaim(TestInstance.Fact(), TestInstance.User, null);
        Assert.Equal(
            "not_found",
            Assert.Throws<SermofurException>(() => b.Memory(storeB).Get(record.Id)).Code
        );
    }

    [Fact]
    public void ForeignMarkerInsideAnInstanceStopsDiscoveryAndIsNeverOverwritten()
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        string child = Path.Combine(fixture.Root, "child");
        Directory.CreateDirectory(child);
        string marker = Path.Combine(child, ".sermofur");
        Directory.CreateDirectory(marker);
        AssertInvalidInstance(() => fixture.Manager.Discover(child), "rename or move it");
        AssertInvalidInstance(() => fixture.Manager.Initialize(child), "rename or move it");
        Assert.Empty(Directory.GetFileSystemEntries(marker));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void ForeignMarkerInAParentBlocksDiscoveryAndInitBelowIt()
    {
        string parent = BareDirectory();
        try
        {
            string marker = Path.Combine(parent, ".sermofur");
            Directory.CreateDirectory(marker);
            string child = Path.Combine(parent, "child");
            string below = Path.Combine(child, "below");
            Directory.CreateDirectory(below);
            InstanceManager manager = new InstanceManager();
            AssertInvalidInstance(() => manager.Discover(parent), "rename or move it");
            AssertInvalidInstance(() => manager.Discover(below), "rename or move it");
            AssertInvalidInstance(() => manager.Initialize(child), "rename or move it");
            Assert.False(Path.Exists(Path.Combine(child, ".sermofur")));
            Assert.Empty(Directory.GetFileSystemEntries(marker));
        }
        finally
        {
            Directory.Delete(parent, true);
        }
    }

    [Theory]
    [InlineData(InstanceManager.DatabaseFile)]
    [InlineData(InstanceManager.RecordsDirectory)]
    public void SingleSermofurArtifactIsADamagedInstance(string artifact)
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        string child = Path.Combine(fixture.Root, "child");
        string marker = Path.Combine(child, ".sermofur");
        Directory.CreateDirectory(marker);
        if (artifact == InstanceManager.RecordsDirectory)
        {
            Directory.CreateDirectory(Path.Combine(marker, artifact));
        }
        else
        {
            File.WriteAllText(Path.Combine(marker, artifact), string.Empty);
        }
        AssertInvalidInstance(() => fixture.Manager.Discover(child), "restore instance.json");
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    /// <summary>
    /// The marker holds a valid configuration that can still be opened by name: only listing is
    /// denied. Discovery and doctor must both report the entry as unreadable instead of trusting
    /// the configuration file.
    /// </summary>
    [WindowsFact("deny ACEs; mode 000 does not stop root on Unix.")]
    [SupportedOSPlatform("windows")]
    public void UnreadableMarkerStopsDiscoveryAndDoctorReportsIt()
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        string child = Path.Combine(fixture.Root, "child");
        string marker = Path.Combine(child, ".sermofur");
        Directory.CreateDirectory(marker);
        File.WriteAllText(
            Path.Combine(marker, InstanceManager.ConfigurationFile),
            RecordJson.Write(
                new InstanceConfiguration(
                    InstanceManager.SchemaVersion,
                    Guid.NewGuid(),
                    DateTimeOffset.UtcNow
                )
            )
        );
        using (DenyListingOnWindows(marker))
        {
            AssertInvalidInstance(() => fixture.Manager.Discover(child), "permissions");
            Assert.Equal(child, fixture.Manager.DiscoverForDiagnosis(child));
            AssertDoctorReports(child, child, "invalid_instance: unreadable");
        }
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    /// <summary>
    /// Doctor runs from a subfolder of the folder that holds the invalid entry: it must report the
    /// entry's folder, name the case, and write nothing in the entry, the start folder nor the
    /// ancestor instance.
    /// </summary>
    [Theory]
    [InlineData("empty folder", "foreign")]
    [InlineData("file", "foreign")]
    [InlineData(InstanceManager.RecordsDirectory, "damaged")]
    [InlineData(InstanceManager.DatabaseFile, "damaged")]
    public void DoctorReportsTheCaseOfAnInvalidMarkerAbove(string layout, string kind)
    {
        using TestInstance fixture = new TestInstance();
        Dictionary<string, string> before = fixture.Snapshot();
        string child = Path.Combine(fixture.Root, "child");
        string below = Path.Combine(child, "below");
        string marker = Path.Combine(child, ".sermofur");
        Directory.CreateDirectory(below);
        switch (layout)
        {
            case "file":
                File.WriteAllText(marker, "another tool");
                break;
            case "empty folder":
                Directory.CreateDirectory(marker);
                break;
            case InstanceManager.RecordsDirectory:
                Directory.CreateDirectory(Path.Combine(marker, layout));
                break;
            case InstanceManager.DatabaseFile:
                Directory.CreateDirectory(marker);
                File.WriteAllText(Path.Combine(marker, layout), string.Empty);
                break;
            default:
                throw new ArgumentOutOfRangeException(nameof(layout), layout, null);
        }
        string entryBefore = DescribeEntry(marker);
        AssertDoctorReports(below, child, $"invalid_instance: {kind}");
        Assert.Equal(entryBefore, DescribeEntry(marker));
        Assert.Empty(Directory.GetFileSystemEntries(below));
        Assert.Equal(
            new[] { ".sermofur", "below" },
            Directory
                .GetFileSystemEntries(child)
                .Select(Path.GetFileName)
                .Order(StringComparer.Ordinal)
        );
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void ForeignMarkerInTheTargetDirectoryIsReportedAndLeftUntouched()
    {
        string root = BareDirectory();
        try
        {
            string marker = Path.Combine(root, ".sermofur");
            File.WriteAllText(marker, "another tool");
            SermofurException error = Assert.Throws<SermofurException>(() =>
                new InstanceManager().Initialize(root)
            );
            Assert.Equal("invalid_instance", error.Code);
            Assert.Equal(3, error.ExitCode);
            Assert.Equal("another tool", File.ReadAllText(marker));
            Assert.Equal(marker, Assert.Single(Directory.GetFileSystemEntries(root)));
        }
        finally
        {
            Directory.Delete(root, true);
        }
    }

    [Theory]
    [InlineData("init")]
    [InlineData("root")]
    [InlineData("status")]
    [InlineData("export")]
    [InlineData("scope", "list")]
    [InlineData("claim", "add", "lost hypothesis", "--origin", "user")]
    public void DamagedChildInstanceStopsDiscoveryAndNothingIsWrittenInTheAncestor(
        params string[] command
    )
    {
        string top = BareDirectory();
        try
        {
            (string ancestor, string child) = DamagedChildLayout(top);
            Dictionary<string, string> ancestorBefore = TestInstance.SnapshotOf(ancestor);
            Dictionary<string, string> childBefore = TestInstance.SnapshotOf(child);
            CliResult result = TestInstance.Run(
                new[] { "--path", child, "--json" }.Concat(command).ToArray()
            );
            Assert.Equal(3, result.ExitCode);
            Assert.Empty(result.Output);
            Assert.Equal("invalid_instance", TestInstance.ErrorCode(result));
            string message = JsonDocument
                .Parse(result.Error)
                .RootElement.GetProperty("message")
                .GetString()!;
            Assert.Contains("restore instance.json", message);
            Assert.DoesNotContain("move it away", message);
            Assert.Equal(
                ancestorBefore.OrderBy(x => x.Key),
                TestInstance.SnapshotOf(ancestor).OrderBy(x => x.Key)
            );
            Assert.Equal(
                childBefore.OrderBy(x => x.Key),
                TestInstance.SnapshotOf(child).OrderBy(x => x.Key)
            );
        }
        finally
        {
            Directory.Delete(top, true);
        }
    }

    [Fact]
    public void DoctorReportsADamagedInstanceInsteadOfAnsweringNoInstance()
    {
        string top = BareDirectory();
        try
        {
            (string ancestor, string child) = DamagedChildLayout(top);
            Dictionary<string, string> ancestorBefore = TestInstance.SnapshotOf(ancestor);
            AssertDoctorReports(child, child, "invalid_instance: damaged");
            Assert.Equal(
                ancestorBefore.OrderBy(x => x.Key),
                TestInstance.SnapshotOf(ancestor).OrderBy(x => x.Key)
            );
        }
        finally
        {
            Directory.Delete(top, true);
        }
    }

    [Fact]
    public void LinkedMarkerWithoutConfigurationIsRejectedRatherThanSkipped()
    {
        string top = BareDirectory();
        string target = Path.Combine(top, "elsewhere");
        string work = Path.Combine(top, "work");
        string link = Path.Combine(work, ".sermofur");
        try
        {
            Directory.CreateDirectory(target);
            Directory.CreateDirectory(work);
            CreateDirectoryLink(link, target);
            InstanceManager manager = new InstanceManager();
            Assert.Equal(
                "unsafe_path",
                Assert.Throws<SermofurException>(() => manager.Discover(work)).Code
            );
            Assert.Equal(
                "unsafe_path",
                Assert.Throws<SermofurException>(() => manager.Initialize(work)).Code
            );
            Assert.Empty(Directory.GetFileSystemEntries(target));
            // A dangling link is still an entry, not an absence.
            Directory.Delete(target);
            Assert.Equal(
                "unsafe_path",
                Assert.Throws<SermofurException>(() => manager.Discover(work)).Code
            );
            Assert.Equal(
                "unsafe_path",
                Assert.Throws<SermofurException>(() => manager.Initialize(work)).Code
            );
            // Doctor refuses a link too, before any report.
            CliResult doctor = TestInstance.Run("--path", work, "--json", "doctor");
            Assert.Equal(4, doctor.ExitCode);
            Assert.Equal("unsafe_path", TestInstance.ErrorCode(doctor));
        }
        finally
        {
            // The link goes first, on its own: a recursive delete of its parent is refused.
            if (new DirectoryInfo(link).LinkTarget is not null)
            {
                TestInstance.DeleteDirectoryLink(link);
            }
            Directory.Delete(top, true);
        }
    }

    [Fact]
    public void MappingTraversalAndSiblingPrefixAreRejected()
    {
        using TestInstance fixture = new TestInstance();
        Assert.False(
            LocalPaths.Contains(Path.Combine(fixture.Root, "a"), Path.Combine(fixture.Root, "a2"))
        );
        foreach (string escape in new[] { "../escape", "..\\escape", "a/../../escape" })
        {
            Assert.Equal(
                "scope_boundary",
                Assert
                    .Throws<SermofurException>(() =>
                        LocalPaths.ResolveMapping(fixture.Root, escape)
                    )
                    .Code
            );
        }
        Assert.Equal(
            "unsafe_path",
            Assert
                .Throws<SermofurException>(() =>
                    LocalPaths.ResolveMapping(fixture.Root, "C:relative")
                )
                .Code
        );
    }

    [Fact]
    public void ContainsDoesNotDoubleTheSeparatorOfAVolumeRoot()
    {
        string temporary = Path.TrimEndingDirectorySeparator(Path.GetTempPath());
        string volume = Path.GetPathRoot(temporary)!;
        Assert.True(LocalPaths.Contains(volume, temporary));
        Assert.True(LocalPaths.Contains(volume, volume));
        Assert.False(LocalPaths.Contains(temporary, volume));
    }

    [Fact]
    public void MissingMappingIsNormalizedLexicallyButNotResolved()
    {
        using TestInstance fixture = new();
        string expected = Path.Combine(fixture.Root, "absent");
        Assert.Equal(expected, LocalPaths.NormalizeMapping(fixture.Root, "absent/"));
        Assert.Equal(
            "invalid_path",
            Assert
                .Throws<SermofurException>(() => LocalPaths.ResolveMapping(fixture.Root, "absent"))
                .Code
        );
    }

    /// <summary>
    /// Instance <c>ancestor</c> holding instance <c>ancestor/child</c> whose <c>instance.json</c>
    /// was lost: <c>memory.db</c> and <c>records/</c> remain. The child is initialized first,
    /// since init refuses to nest under an existing instance.
    /// </summary>
    private static (string Ancestor, string Child) DamagedChildLayout(string top)
    {
        string ancestor = Path.Combine(top, "ancestor");
        string child = Path.Combine(ancestor, "child");
        Directory.CreateDirectory(child);
        InstanceManager manager = new InstanceManager();
        manager.Initialize(child);
        manager.Initialize(ancestor);
        File.Delete(Path.Combine(child, InstanceManager.Marker, InstanceManager.ConfigurationFile));
        return (ancestor, child);
    }

    /// <summary>
    /// Directory link without elevation: a junction on Windows, a symbolic link elsewhere. A
    /// failure fails the test explicitly instead of skipping it.
    /// </summary>
    private static void CreateDirectoryLink(string link, string target)
    {
        if (!OperatingSystem.IsWindows())
        {
            Directory.CreateSymbolicLink(link, target);
            return;
        }
        ProcessStartInfo start = new("cmd.exe")
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        };
        foreach (string argument in new[] { "/c", "mklink", "/J", link, target })
        {
            start.ArgumentList.Add(argument);
        }
        using Process process = Process.Start(start)!;
        string error = process.StandardError.ReadToEnd();
        process.WaitForExit();
        Assert.True(
            process.ExitCode == 0 && (File.GetAttributes(link) & FileAttributes.ReparsePoint) != 0,
            $"Junction creation failed in this environment: {error}"
        );
    }

    private static void AssertInvalidInstance(Func<object> action, string hint)
    {
        SermofurException error = Assert.Throws<SermofurException>(action);
        Assert.Equal("invalid_instance", error.Code);
        Assert.Equal(3, error.ExitCode);
        Assert.Contains(hint, error.Message);
    }

    /// <summary>
    /// The content hash of <paramref name="entry"/> if it is a file; otherwise every path under
    /// it, with the content hash of each file.
    /// </summary>
    private static string DescribeEntry(string entry)
    {
        if (File.Exists(entry))
        {
            return HashOf(entry);
        }
        return string.Join(
            "\n",
            Directory
                .EnumerateFileSystemEntries(entry, "*", SearchOption.AllDirectories)
                .Order(StringComparer.Ordinal)
                .Select(path =>
                    Path.GetRelativePath(entry, path)
                    + (File.Exists(path) ? " " + HashOf(path) : "/")
                )
        );
    }

    private static string HashOf(string file) =>
        Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(file)));

    private static void AssertDoctorReports(string path, string expectedRoot, string expectedDetail)
    {
        CliResult result = TestInstance.Run("--path", path, "--json", "doctor");
        Assert.Equal(5, result.ExitCode);
        DoctorReport report = RecordJson.Read<DoctorReport>(result.Output);
        Assert.Equal("unhealthy", report.Overall);
        Assert.Equal(expectedRoot, report.Root);
        DiagnosticCheck instance = report.Checks.Single(check => check.Name == "instance");
        Assert.Equal("error", instance.Status);
        Assert.Equal(expectedDetail, instance.Detail);
    }

    /// <summary>
    /// Denies listing <paramref name="directory"/> to the current user until disposal, with a
    /// deny ACE. Windows only: on Unix, mode 000 does not stop root, which most CI containers
    /// run as, so the unreadable case is not tested there.
    /// </summary>
    [SupportedOSPlatform("windows")]
    private static IDisposable DenyListingOnWindows(string directory)
    {
        DirectoryInfo info = new DirectoryInfo(directory);
        FileSystemAccessRule rule = new(
            WindowsIdentity.GetCurrent().User!,
            FileSystemRights.ListDirectory,
            AccessControlType.Deny
        );
        DirectorySecurity security = info.GetAccessControl();
        security.AddAccessRule(rule);
        info.SetAccessControl(security);
        return new Restore(() =>
        {
            DirectorySecurity current = info.GetAccessControl();
            current.RemoveAccessRule(rule);
            info.SetAccessControl(current);
        });
    }

    private sealed class Restore(Action undo) : IDisposable
    {
        public void Dispose() => undo();
    }

    /// <summary>Temporary directory outside any instance, removed by the caller.</summary>
    private static string BareDirectory()
    {
        TestInstance.RequireNoEntryAboveTemp();
        string path = Path.Combine(
            TestInstance.TempRoot,
            "sermofur-bare-" + Guid.NewGuid().ToString("N")
        );
        Directory.CreateDirectory(path);
        return path;
    }
}
