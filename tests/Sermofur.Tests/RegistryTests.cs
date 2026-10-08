using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class RegistryTests
{
    [Fact]
    public void RegisteringIsIdempotentAndListed()
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        RegisteredInstance first = registry.Register(
            Path.Combine(fixture.Root),
            DateTimeOffset.Now
        );
        Directory.CreateDirectory(Path.Combine(fixture.Root, "deep", "er"));
        RegisteredInstance again = registry.Register(
            Path.Combine(fixture.Root, "deep", "er"),
            DateTimeOffset.Now.AddHours(1)
        );
        Assert.Equal(first, again);
        RegistryEntry entry = Assert.Single(registry.List());
        Assert.Equal((fixture.Root, false), (entry.Root, entry.Missing));
    }

    [Fact]
    public void RefusalsAreThoseOfTheDirectCli()
    {
        using LegacyInstance legacy = new LegacyInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        Assert.Equal(
            "migration_required",
            Assert
                .Throws<SermofurException>(() => registry.Register(legacy.Root, DateTimeOffset.Now))
                .Code
        );
        using TestInstance fixture = new TestInstance();
        InstanceRegistry foreign = new InstanceRegistry(
            home.Registry,
            new InstanceManager(new ForeignOwnership())
        );
        Assert.Equal(
            ("foreign_owner", 4),
            Code(
                Assert.Throws<SermofurException>(() =>
                    foreign.Register(fixture.Root, DateTimeOffset.Now)
                )
            )
        );
        Assert.Equal(
            "no_instance",
            Assert
                .Throws<SermofurException>(() =>
                    registry.Register(TestInstance.TempRoot, DateTimeOffset.Now)
                )
                .Code
        );
        Assert.False(File.Exists(home.Registry));
    }

    [Theory]
    [InlineData("not json")]
    [InlineData("{\"version\":2,\"instances\":[]}")]
    [InlineData("{\"version\":1}")]
    [InlineData("{\"version\":1,\"instances\":[{\"root\":\"relative\",\"registeredAt\":\"x\"}]}")]
    public void UnusableRegistryIsReportedAndLeftUntouched(string content)
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        Directory.CreateDirectory(Path.GetDirectoryName(home.Registry)!);
        File.WriteAllText(home.Registry, content);
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        Assert.Equal(
            ("invalid_registry", 3),
            Code(Assert.Throws<SermofurException>(() => registry.Read()))
        );
        Assert.Throws<SermofurException>(() => registry.Register(fixture.Root, DateTimeOffset.Now));
        Assert.Equal(content, File.ReadAllText(home.Registry));
    }

    [Fact]
    public void OversizedRegistryIsRefused()
    {
        using Home home = new Home();
        Directory.CreateDirectory(Path.GetDirectoryName(home.Registry)!);
        File.WriteAllText(home.Registry, new string(' ', InstanceRegistry.MaxBytes + 1));
        Assert.Equal(
            "invalid_registry",
            Assert.Throws<SermofurException>(() => new InstanceRegistry(home.Registry).Read()).Code
        );
    }

    [Fact]
    public void MovedInstanceIsMissingAndRemovableByItsRegisteredPath()
    {
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        string root;
        using (TestInstance fixture = new TestInstance())
        {
            root = registry.Register(fixture.Root, DateTimeOffset.Now).Root;
        }
        Assert.True(Assert.Single(registry.List()).Missing);
        Assert.Equal(root, registry.Unregister(root).Root);
        Assert.Empty(registry.Read());
        SermofurException refusal = Assert.Throws<SermofurException>(() =>
            registry.Unregister(root)
        );
        Assert.Equal(("not_registered", 1), Code(refusal));
    }

    [Fact]
    public void UnregisteringFromInsideTheInstanceFindsIt()
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        registry.Register(fixture.Root, DateTimeOffset.Now);
        string sub = Directory.CreateDirectory(Path.Combine(fixture.Root, "sub")).FullName;
        registry.Unregister(sub);
        Assert.Empty(registry.Read());
    }

    [Fact]
    public void GateServesOnlyRegisteredRootsWithoutReadingOutside()
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        registry.Register(fixture.Root, DateTimeOffset.Now);
        ServingGate gate = new ServingGate(registry, new InstanceManager(new NoReadOwnership()));
        string outside = Path.Combine(
            TestInstance.TempRoot,
            "smf-outside-" + Guid.NewGuid().ToString("N")
        );
        Assert.Null(gate.Resolve(outside));
        Assert.Null(gate.Resolve(fixture.Root + "-sibling"));
        Assert.Equal(0, gate.OpenCount);
    }

    [Fact]
    public void GateRefusesANestedInstanceThatIsNotRegistered()
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        registry.Register(fixture.Root, DateTimeOffset.Now);
        string nested = Directory.CreateDirectory(Path.Combine(fixture.Root, "nested")).FullName;
        // Init refuses nesting: build the entry by hand, as a copied folder would.
        Directory.CreateDirectory(Path.Combine(nested, InstanceManager.Marker));
        File.Copy(
            Path.Combine(fixture.Root, InstanceManager.Marker, InstanceManager.ConfigurationFile),
            Path.Combine(nested, InstanceManager.Marker, InstanceManager.ConfigurationFile)
        );
        ServingGate gate = new ServingGate(registry);
        Assert.Equal(fixture.Root, gate.Resolve(fixture.Root));
        Assert.Null(gate.Resolve(nested));
    }

    [Fact]
    public void GateClosesIdleAndUnregisteredInstances()
    {
        using TestInstance fixture = new TestInstance();
        using Home home = new Home();
        InstanceRegistry registry = new InstanceRegistry(home.Registry);
        registry.Register(fixture.Root, DateTimeOffset.Now);
        DateTimeOffset now = DateTimeOffset.Now;
        ServingGate gate = new ServingGate(registry, null, TimeSpan.FromMinutes(10), () => now);
        Assert.Equal(fixture.Root, gate.Resolve(fixture.Root));
        Assert.Equal(1, gate.OpenCount);
        now = now.AddMinutes(11);
        Assert.Equal(0, gate.OpenCount);
        Assert.Equal(fixture.Root, gate.Resolve(fixture.Root));
        registry.Unregister(fixture.Root);
        File.SetLastWriteTimeUtc(home.Registry, DateTime.UtcNow.AddSeconds(5));
        Assert.Null(gate.Resolve(fixture.Root));
        Assert.Equal(0, gate.OpenCount);
    }

    private static (string, int) Code(SermofurException exception) =>
        (exception.Code, exception.ExitCode);

    /// <summary>A registry file in its own temporary folder.</summary>
    private sealed class Home : IDisposable
    {
        private readonly string folder = TestDaemon.NewHome();

        public string Registry => Path.Combine(folder, "config", "instances.json");

        public void Dispose() => TestDaemon.DeleteHome(folder);
    }

    private sealed class ForeignOwnership : IFileOwnership
    {
        public EntryStatus? Inspect(string path) =>
            Directory.Exists(path) ? new EntryStatus(false, true, false)
            : File.Exists(path) ? new EntryStatus(true, false, false)
            : null;
    }

    private sealed class NoReadOwnership : IFileOwnership
    {
        public EntryStatus? Inspect(string path) =>
            throw new InvalidOperationException($"The gate read {path}.");
    }
}
