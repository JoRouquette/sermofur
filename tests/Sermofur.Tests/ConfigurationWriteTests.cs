using Sermofur.Daemon;
using Sermofur.Daemon.Services;
using Sermofur.Domain;
using Sermofur.Infrastructure;
using Sermofur.Mcp;

namespace Sermofur.Tests;

/// <summary>
/// Writes of configuration (issue #12): what a link in place of the target means, which
/// variables reach a supervised daemon, and a <c>service.json</c> whose shape does not hold.
/// </summary>
public class ConfigurationWriteTests
{
    /// <summary>The options the configuration of a host is written with.</summary>
    private static readonly WriteOptions Follow = new WriteOptions(
        Policy: LinkPolicy.FollowToTarget
    );

    [FileLinkFact]
    public void TheStateOfSermofurIsNotWrittenThroughALink()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "elsewhere.json");
            File.WriteAllText(target, "{\"kept\":true}");
            string link = Path.Combine(folder, "instances.json");
            File.CreateSymbolicLink(link, target);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                AtomicFile.Write(link, "{\"written\":true}"u8.ToArray())
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Equal(4, refusal.ExitCode);
            Assert.Contains(link, refusal.Message);
            Assert.Equal("{\"kept\":true}", File.ReadAllText(target));
            Assert.NotNull(new FileInfo(link).LinkTarget);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void TheConfigurationOfAHostIsWrittenInTheTargetOfItsLink()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "dotfiles-mcp.json");
            File.WriteAllText(target, "{}");
            string link = Path.Combine(folder, ".mcp.json");
            File.CreateSymbolicLink(link, target);
            AtomicFile.Write(link, "{\"a\":1}"u8.ToArray(), Follow);
            Assert.Equal("{\"a\":1}", File.ReadAllText(target));
            Assert.NotNull(new FileInfo(link).LinkTarget);
            Assert.Equal(
                [".mcp.json", "dotfiles-mcp.json"],
                Directory.GetFiles(folder).Select(Path.GetFileName).Order()
            );
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void ALinkThroughASecondLinkStillReachesItsFinalTarget()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "real.toml");
            File.WriteAllText(target, "a = 1\n");
            string middle = Path.Combine(folder, "middle.toml");
            File.CreateSymbolicLink(middle, target);
            string link = Path.Combine(folder, "config.toml");
            File.CreateSymbolicLink(link, middle);
            AtomicFile.Write(link, "a = 2\n"u8.ToArray(), Follow);
            Assert.Equal("a = 2\n", File.ReadAllText(target));
            Assert.NotNull(new FileInfo(link).LinkTarget);
            Assert.NotNull(new FileInfo(middle).LinkTarget);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void ALinkThatLeadsNowhereIsRefusedByItsOwnReason()
    {
        string folder = NewFolder();
        try
        {
            string missing = Path.Combine(folder, "gone.toml");
            string link = Path.Combine(folder, "config.toml");
            File.CreateSymbolicLink(link, missing);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                AtomicFile.Write(link, "a = 2\n"u8.ToArray(), Follow)
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Contains("does not exist", refusal.Message);
            Assert.False(File.Exists(missing));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void ALinkThatGoesInACircleIsRefused()
    {
        string folder = NewFolder();
        try
        {
            string first = Path.Combine(folder, "first.toml");
            string second = Path.Combine(folder, "second.toml");
            File.CreateSymbolicLink(first, second);
            File.CreateSymbolicLink(second, first);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                AtomicFile.Write(first, "a = 2\n"u8.ToArray(), Follow)
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Contains("circle", refusal.Message);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [UnixFact]
    [System.Runtime.Versioning.UnsupportedOSPlatform("windows")]
    public void TheFinalTargetOfALinkKeepsItsOwnPermissions()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "real.toml");
            File.WriteAllText(target, "a = 1\n");
            // Readable by the group, by neither the others nor nobody: a mode the write can
            // only keep exactly, so widening it and narrowing it both show.
            UnixFileMode own =
                UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.GroupRead;
            File.SetUnixFileMode(target, own);
            string link = Path.Combine(folder, "config.toml");
            File.CreateSymbolicLink(link, target);
            AtomicFile.Write(
                link,
                "a = 2\n"u8.ToArray(),
                new WriteOptions(
                    UnixFileMode.UserRead | UnixFileMode.OtherRead,
                    LinkPolicy.FollowToTarget
                )
            );
            Assert.Equal(own, File.GetUnixFileMode(target));
            Assert.Equal("a = 2\n", File.ReadAllText(target));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void AFolderIsNeverWrittenAsAFile()
    {
        string folder = NewFolder();
        try
        {
            string inside = Path.Combine(folder, "state");
            Directory.CreateDirectory(inside);
            foreach (LinkPolicy policy in new[] { LinkPolicy.Refuse, LinkPolicy.FollowToTarget })
            {
                SermofurException refusal = Assert.Throws<SermofurException>(() =>
                    AtomicFile.Write(inside, "{}"u8.ToArray(), new WriteOptions(Policy: policy))
                );
                Assert.Equal("unsafe_path", refusal.Code);
                Assert.Contains("folder", refusal.Message);
            }
            Assert.Empty(Directory.GetFiles(inside));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void ASupervisedDaemonGetsTheCarriedVariablesOnly()
    {
        List<string> dropped = [];
        ServiceDefinition installed = Definition(
            new()
            {
                ["DOTNET_ROOT"] = Absolute("dotnet"),
                ["PATH"] = string.Join(
                    Path.PathSeparator,
                    [Absolute("tools"), "relative-folder", Absolute("more")]
                ),
                ["COMSPEC"] = Absolute("shell"),
                ["LD_PRELOAD"] = Absolute("evil.so"),
            }
        );
        List<string> narrowed = [];
        System.Diagnostics.ProcessStartInfo start = Supervisor.ChildStart(
            Definition([]),
            installed,
            dropped.Add,
            narrowed.Add
        );
        Assert.Equal(Absolute("dotnet"), start.Environment["DOTNET_ROOT"]);
        Assert.Equal(
            string.Join(Path.PathSeparator, [Absolute("tools"), Absolute("more")]),
            start.Environment["PATH"]
        );
        // The child inherits the environment of the supervisor: what matters is that the
        // recorded value of a variable nobody carries never reaches it.
        Assert.NotEqual(Absolute("shell"), Value(start, "COMSPEC"));
        Assert.NotEqual(Absolute("evil.so"), Value(start, "LD_PRELOAD"));
        Assert.Equal(["COMSPEC", "LD_PRELOAD"], dropped.Order());
        // A PATH that keeps some of its entries is reported too: a daemon that no longer finds
        // a tool is diagnosed from the journal, not by reading service.json.
        Assert.Equal(["PATH"], narrowed);
    }

    [Fact]
    public void ACarriedVariableWithoutAnAbsolutePathIsDropped()
    {
        List<string> dropped = [];
        ServiceDefinition installed = Definition(
            new() { ["DOTNET_ROOT"] = "dotnet", ["PATH"] = "tools" }
        );
        System.Diagnostics.ProcessStartInfo start = Supervisor.ChildStart(
            Definition([]),
            installed,
            dropped.Add
        );
        Assert.NotEqual("dotnet", Value(start, "DOTNET_ROOT"));
        Assert.NotEqual("tools", Value(start, "PATH"));
        Assert.Equal(["DOTNET_ROOT", "PATH"], dropped.Order());
    }

    [Theory]
    [InlineData("{\"executable\":\"")]
    [InlineData("{}")]
    [InlineData("{\"executable\":null,\"arguments\":null,\"version\":null,\"environment\":null}")]
    [InlineData("{\"executable\":\"  \",\"arguments\":[\"daemon\",\"run\"],\"version\":\"0.4.2\"}")]
    [InlineData("{\"executable\":\"@@\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"\"}")]
    [InlineData("{\"executable\":\"@@\",\"arguments\":[\"mcp\",\"serve\"],\"version\":\"0.4.2\"}")]
    [InlineData("{\"executable\":\"@@\",\"arguments\":[\"run\",\"daemon\"],\"version\":\"0.4.2\"}")]
    [InlineData(
        "{\"executable\":\"@@\",\"arguments\":[\"daemon\",\"stop\",\"run\"],\"version\":\"0.4.2\"}"
    )]
    [InlineData("{\"executable\":\"@@\",\"arguments\":[null,\"run\"],\"version\":\"0.4.2\"}")]
    [InlineData("{\"executable\":\"@@\",\"arguments\":[],\"version\":\"0.4.2\"}")]
    public void AServiceDefinitionThatDoesNotHoldReadsAsNothingInstalled(string content)
    {
        string folder = NewFolder();
        try
        {
            DaemonPaths paths = PathsIn(folder);
            File.WriteAllText(
                InstalledDefinition.File(paths),
                // The path is put in as JSON, not raw: a Windows path holds backslashes, and a
                // raw one would make the document invalid, so the case would pass for the wrong
                // reason.
                content.Replace("@@", Escaped(Absolute("smf")))
            );
            Assert.Null(InstalledDefinition.Read(paths));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void AServiceDefinitionWithoutItsEnvironmentReadsAsOneWithNoVariable()
    {
        string folder = NewFolder();
        try
        {
            DaemonPaths paths = PathsIn(folder);
            File.WriteAllText(
                InstalledDefinition.File(paths),
                $"{{\"executable\":\"{Escaped(Absolute("smf"))}\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"0.4.2\"}}"
            );
            ServiceDefinition read = Assert.IsType<ServiceDefinition>(
                InstalledDefinition.Read(paths)
            );
            Assert.Empty(read.Environment);
            Assert.Equal("0.4.2", read.Version);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [Fact]
    public void AWrittenServiceDefinitionReadsBackWhole()
    {
        string folder = NewFolder();
        try
        {
            DaemonPaths paths = PathsIn(folder);
            ServiceDefinition written = new ServiceDefinition(
                Absolute("smf"),
                ["daemon", "run", "--supervise"],
                "0.4.2",
                new Dictionary<string, string> { ["DOTNET_ROOT"] = Absolute("dotnet") }
            );
            InstalledDefinition.Write(paths, written);
            ServiceDefinition read = Assert.IsType<ServiceDefinition>(
                InstalledDefinition.Read(paths)
            );
            Assert.Equal(written.Executable, read.Executable);
            Assert.Equal(written.Arguments, read.Arguments);
            Assert.Equal(Absolute("dotnet"), read.Environment["DOTNET_ROOT"]);
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    /// <summary>
    /// A chain of ten links reaches its target; the eleventh is refused for its length, which
    /// is not the circle the chain of a dotfiles manager would be.
    /// </summary>
    [FileLinkFact]
    public void TenLinksAreFollowedAndTheEleventhIsRefused()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "real.toml");
            File.WriteAllText(target, "a = 1\n");
            string current = target;
            for (int step = 1; step <= 10; step++)
            {
                string link = Path.Combine(folder, $"step-{step}.toml");
                File.CreateSymbolicLink(link, current);
                current = link;
            }
            AtomicFile.Write(current, "a = 2\n"u8.ToArray(), Follow);
            Assert.Equal("a = 2\n", File.ReadAllText(target));
            string eleventh = Path.Combine(folder, "step-11.toml");
            File.CreateSymbolicLink(eleventh, current);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                AtomicFile.Write(eleventh, "a = 3\n"u8.ToArray(), Follow)
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Contains("more than 10 steps", refusal.Message);
            Assert.DoesNotContain("circle", refusal.Message);
            Assert.Equal("a = 2\n", File.ReadAllText(target));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void ALinkThatEndsOnAFolderAfterAStepIsRefused()
    {
        string folder = NewFolder();
        try
        {
            string inside = Path.Combine(folder, "state");
            Directory.CreateDirectory(inside);
            string middle = Path.Combine(folder, "middle.toml");
            File.CreateSymbolicLink(middle, inside);
            string link = Path.Combine(folder, "config.toml");
            File.CreateSymbolicLink(link, middle);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                AtomicFile.Write(link, "a = 2\n"u8.ToArray(), Follow)
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Contains("end on the folder", refusal.Message);
            Assert.Empty(Directory.GetFiles(inside));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    /// <summary>
    /// The policy is held by the writers, not only by AtomicFile: the one that records the
    /// service refuses a link in place of its file without being told to.
    /// </summary>
    [FileLinkFact]
    public void TheWriterOfTheServiceDefinitionRefusesALinkInItsPlace()
    {
        string folder = NewFolder();
        try
        {
            DaemonPaths paths = PathsIn(folder);
            string target = Path.Combine(folder, "elsewhere.json");
            File.WriteAllText(target, "{\"kept\":true}");
            File.CreateSymbolicLink(InstalledDefinition.File(paths), target);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                InstalledDefinition.Write(paths, Definition([]))
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Equal("{\"kept\":true}", File.ReadAllText(target));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    [FileLinkFact]
    public void TheWriterOfTheRegistryRefusesALinkInItsPlace()
    {
        using TestInstance fixture = new TestInstance();
        string folder = NewFolder();
        try
        {
            DaemonPaths paths = PathsIn(folder);
            string target = Path.Combine(folder, "elsewhere.json");
            // A registry the reader accepts, so the refusal comes from the write and not from
            // reading something that is not a registry.
            string kept = "{\"version\":1,\"instances\":[]}";
            File.WriteAllText(target, kept);
            File.CreateSymbolicLink(paths.RegistryFile, target);
            SermofurException refusal = Assert.Throws<SermofurException>(() =>
                new InstanceRegistry(paths.RegistryFile).Register(fixture.Root, DateTimeOffset.Now)
            );
            Assert.Equal("unsafe_path", refusal.Code);
            Assert.Equal(kept, File.ReadAllText(target));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    /// <summary>
    /// The other half of the same invariant: the writer of a host configuration follows the link
    /// without being told to, and the report carries the file it ended on, as a field of its own
    /// and in a line for the reader.
    /// </summary>
    [FileLinkFact]
    public void TheWriterOfAHostConfigurationFollowsTheLinkAndNamesWhereItWrote()
    {
        string folder = NewFolder();
        try
        {
            string target = Path.Combine(folder, "dotfiles-mcp.json");
            File.WriteAllText(target, "{}");
            string link = Path.Combine(folder, McpConfigFile.FileName);
            File.CreateSymbolicLink(link, target);
            DeclarationReport report = McpDeclaration.Install(folder);
            Assert.True(report.Changed);
            Assert.Contains("sermofur", File.ReadAllText(target));
            Assert.NotNull(new FileInfo(link).LinkTarget);
            Assert.Equal(target, report.Replaced);
            Assert.Equal(link, report.File);
            Assert.Contains(report.Next, line => line.Contains(target));
        }
        finally
        {
            Directory.Delete(folder, true);
        }
    }

    /// <summary>The path as a JSON document holds it, backslashes and all.</summary>
    private static string Escaped(string path) => path.Replace("\\", "\\\\");

    /// <summary>What the child would read for a variable, null when it reads nothing.</summary>
    private static string? Value(System.Diagnostics.ProcessStartInfo start, string name) =>
        start.Environment.TryGetValue(name, out string? value) ? value : null;

    /// <summary>An absolute path of this operating system, named after what it stands for.</summary>
    private static string Absolute(string name) =>
        OperatingSystem.IsWindows() ? $"C:\\smf-test\\{name}" : $"/smf-test/{name}";

    private static ServiceDefinition Definition(Dictionary<string, string> environment) =>
        new ServiceDefinition(Absolute("smf"), ["daemon", "run"], "0.4.2", environment);

    private static DaemonPaths PathsIn(string folder) =>
        new DaemonPaths(folder, folder, Path.Combine(folder, "endpoint"));

    private static string NewFolder()
    {
        string folder = Path.Combine(
            TestInstance.TempRoot,
            "smf-config-" + Guid.NewGuid().ToString("N")[..8]
        );
        Directory.CreateDirectory(folder);
        return folder;
    }
}
