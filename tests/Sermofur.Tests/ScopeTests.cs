using System.Text.Json;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class ScopeTests
{
    private static string ScopeOf(CliResult result) =>
        JsonDocument.Parse(result.Output).RootElement.GetProperty("scope").GetString()!;

    private static SermofurException Rejected(Action register) =>
        Assert.Throws<SermofurException>(register);

    [Fact]
    public void MissingMappedDirectoryDoesNotBlockOtherContextsAndDoctorWarns()
    {
        using TestInstance fixture = new();
        string b;
        using (SqliteStore store = fixture.Open())
        {
            string a = fixture.Client(store, "a");
            b = fixture.Client(store, "b");
            Directory.Delete(a);
        }
        CliResult fromRoot = TestInstance.Run("--path", fixture.Root, "--json", "status");
        Assert.Equal(0, fromRoot.ExitCode);
        Assert.Equal(ScopePolicy.WorkspaceScopeId, ScopeOf(fromRoot));
        CliResult fromOtherScope = TestInstance.Run("--path", b, "--json", "status");
        Assert.Equal(0, fromOtherScope.ExitCode);
        Assert.Equal("b", ScopeOf(fromOtherScope));
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("healthy_with_warnings", report.Overall);
        DiagnosticCheck mappings = report.Checks.Single(check => check.Name == "scope_mappings");
        Assert.Equal("warning", mappings.Status);
        Assert.Equal("1 missing mappings.", mappings.Detail);
    }

    [Theory]
    [InlineData("a", "a/b")]
    [InlineData("a/b", "a")]
    public void ClientsCannotNestInsideEachOther(string first, string second)
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "b"));
        using SqliteStore store = fixture.Open();
        fixture
            .Scopes(store)
            .Register(new Scope("first", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, first));
        SermofurException error = Rejected(() =>
            fixture
                .Scopes(store)
                .Register(
                    new Scope("second", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, second)
                )
        );
        Assert.Equal("scope_boundary", error.Code);
        Assert.Equal(4, error.ExitCode);
        Assert.Equal(2, store.ReadScopes().Count);
    }

    [Theory]
    [InlineData("b/x")]
    [InlineData("a/../b/x")]
    public void ProjectOfOneClientCannotLiveInAnotherClientTree(string mapping)
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "b", "x"));
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        fixture.Client(store, "b");
        SermofurException error = Rejected(() =>
            fixture.Scopes(store, a).Register(new Scope("pa", ScopeKind.Project, "a", mapping))
        );
        Assert.Equal("scope_boundary", error.Code);
        Assert.Equal(4, error.ExitCode);
    }

    [Fact]
    public void SiblingProjectsCannotOverlap()
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "p1", "sub"));
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        fixture.Scopes(store, a).Register(new Scope("p1", ScopeKind.Project, "a", "a/p1"));
        SermofurException error = Rejected(() =>
            fixture.Scopes(store, a).Register(new Scope("p2", ScopeKind.Project, "a", "a/p1/sub"))
        );
        Assert.Equal("scope_boundary", error.Code);
    }

    [Fact]
    public void TakenIdGivesTheSameGenericErrorWhoeverOwnsIt()
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "p"));
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "q"));
        Directory.CreateDirectory(Path.Combine(fixture.Root, "b", "p"));
        using SqliteStore store = fixture.Open();
        string a = fixture.Client(store, "a");
        string b = fixture.Client(store, "b");
        fixture.Scopes(store, a).Register(new Scope("pa", ScopeKind.Project, "a", "a/p"));
        SermofurException visibleOwner = Rejected(() =>
            fixture.Scopes(store, a).Register(new Scope("pa", ScopeKind.Project, "a", "a/q"))
        );
        SermofurException hiddenOwner = Rejected(() =>
            fixture.Scopes(store, b).Register(new Scope("pa", ScopeKind.Project, "b", "b/p"))
        );
        Assert.Equal("invalid_scope", visibleOwner.Code);
        Assert.Equal(1, visibleOwner.ExitCode);
        Assert.Equal(visibleOwner.Code, hiddenOwner.Code);
        Assert.Equal(visibleOwner.ExitCode, hiddenOwner.ExitCode);
        Assert.Equal(visibleOwner.Message, hiddenOwner.Message);
    }

    [Fact]
    public void DuplicateIdThroughCliExitsWithInvalidScope()
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "other"));
        using (SqliteStore store = fixture.Open())
        {
            fixture.Client(store, "a");
        }
        Dictionary<string, string> before = fixture.Snapshot();
        CliResult result = TestInstance.Run(
            "--path",
            fixture.Root,
            "--json",
            "scope",
            "add",
            "a",
            "client",
            ScopePolicy.WorkspaceScopeId,
            "other"
        );
        Assert.Equal(1, result.ExitCode);
        Assert.Equal("invalid_scope", TestInstance.ErrorCode(result));
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void IdenticalMappingIsDuplicateMapping()
    {
        using TestInstance fixture = new();
        using SqliteStore store = fixture.Open();
        fixture.Client(store, "a");
        SermofurException error = Rejected(() =>
            fixture
                .Scopes(store)
                .Register(new Scope("c", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "a"))
        );
        Assert.Equal("duplicate_mapping", error.Code);
        Assert.Equal(1, error.ExitCode);
    }

    [Theory]
    [InlineData("x\n", ScopeKind.Client)]
    [InlineData("X", ScopeKind.Client)]
    [InlineData("x", ScopeKind.Project)]
    [InlineData("x", ScopeKind.Workspace)]
    public void InvalidSlugOrKindIsAnInputError(string id, ScopeKind kind)
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "x"));
        using SqliteStore store = fixture.Open();
        SermofurException error = Rejected(() =>
            fixture.Scopes(store).Register(new Scope(id, kind, ScopePolicy.WorkspaceScopeId, "x"))
        );
        Assert.Equal("invalid_scope", error.Code);
        Assert.Equal(1, error.ExitCode);
        Assert.Single(store.ReadScopes());
    }

    [Fact]
    public void NulInMappingIsAnInputErrorBeforeAnyPathCall()
    {
        using TestInstance fixture = new();
        Dictionary<string, string> before = fixture.Snapshot();
        using (SqliteStore store = fixture.Open())
        {
            SermofurException error = Rejected(() =>
                fixture
                    .Scopes(store)
                    .Register(
                        new Scope("x", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "a\0b")
                    )
            );
            Assert.Equal("invalid_scope", error.Code);
            Assert.Equal(1, error.ExitCode);
            Assert.Single(store.ReadScopes());
        }
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void OverlongMappingIsAnInputError()
    {
        using TestInstance fixture = new();
        using SqliteStore store = fixture.Open();
        string mapping = new('a', InputLimits.MaxTextLength + 1);
        SermofurException error = Rejected(() =>
            fixture
                .Scopes(store)
                .Register(new Scope("x", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, mapping))
        );
        Assert.Equal("invalid_scope", error.Code);
        Assert.Equal(1, error.ExitCode);
        Assert.Single(store.ReadScopes());
    }

    [Theory]
    [InlineData("a", "duplicate_mapping", 1)]
    [InlineData("a/b", "scope_boundary", 4)]
    public void ConcurrentConflictingScopeIsCaughtUnderTheTransaction(
        string mapping,
        string expectedCode,
        int expectedExit
    )
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "b"));
        using SqliteStore store = fixture.Open();
        // The service reads the scopes before the concurrent writer; the raw insertion happens just
        // before the AddScope transaction, like a second process that would have won the race.
        RacingStore racing = new(
            store,
            InstanceManager.DatabasePath(fixture.Root),
            "INSERT INTO scopes VALUES('rival','Client','workspace','a')"
        );
        ScopeService service = new(
            racing,
            new LocalPathResolver(),
            fixture.Manager.ResolveContext(fixture.Root, fixture.Root, store.ReadScopes())
        );
        SermofurException error = Rejected(() =>
            service.Register(
                new Scope("late", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, mapping)
            )
        );
        Assert.Equal(expectedCode, error.Code);
        Assert.Equal(expectedExit, error.ExitCode);
        Assert.True(racing.PreconditionSawRival);
        Assert.Equal(
            ["rival", ScopePolicy.WorkspaceScopeId],
            store.ReadScopes().Select(scope => scope.Id).ToArray()
        );
    }

    [Fact]
    public void ChildOfAnotherContextIsABoundaryError()
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "p"));
        using SqliteStore store = fixture.Open();
        fixture.Client(store, "a");
        SermofurException error = Rejected(() =>
            fixture.Scopes(store).Register(new Scope("pa", ScopeKind.Project, "a", "a/p"))
        );
        Assert.Equal("scope_boundary", error.Code);
        Assert.Equal(4, error.ExitCode);
    }

    [Theory]
    [InlineData("x\\y")]
    [InlineData("x/y")]
    [InlineData("x/./y/")]
    public void MappingIsStoredWithForwardSlashesWhateverTheInput(string mapping)
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "x", "y"));
        using SqliteStore store = fixture.Open();
        Scope registered = fixture
            .Scopes(store)
            .Register(new Scope("c", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, mapping));
        Assert.Equal("x/y", registered.RelativePath);
        Assert.Equal("x/y", store.ReadScopes().Single(scope => scope.Id == "c").RelativePath);
    }

    [Fact]
    public void StoredBackslashMappingIsStillRead()
    {
        using TestInstance fixture = new();
        string mapped = Path.Combine(fixture.Root, "x", "y");
        Directory.CreateDirectory(mapped);
        using (
            SqliteConnection database = new(
                "Pooling=False;Data Source=" + InstanceManager.DatabasePath(fixture.Root)
            )
        )
        {
            database.Open();
            using SqliteCommand command = database.CreateCommand();
            command.CommandText = "INSERT INTO scopes VALUES('legacy','Client','workspace','x\\y')";
            command.ExecuteNonQuery();
        }
        using (SqliteStore store = fixture.Open(readOnly: true))
        {
            Assert.Equal(
                "legacy",
                fixture.Manager.ResolveContext(mapped, fixture.Root, store.ReadScopes()).ScopeId
            );
        }
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("ok", report.Checks.Single(check => check.Name == "scope_mappings").Status);
        Assert.Equal("ok", report.Checks.Single(check => check.Name == "scope_overlap").Status);
    }

    [Fact]
    public void ForwardSlashMappingOfAStoredBackslashMappingIsADuplicate()
    {
        using TestInstance fixture = new();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "x", "y"));
        using (
            SqliteConnection database = new(
                "Pooling=False;Data Source=" + InstanceManager.DatabasePath(fixture.Root)
            )
        )
        {
            database.Open();
            using SqliteCommand command = database.CreateCommand();
            command.CommandText = "INSERT INTO scopes VALUES('legacy','Client','workspace','x\\y')";
            command.ExecuteNonQuery();
        }
        using SqliteStore store = fixture.Open();
        SermofurException error = Rejected(() =>
            fixture
                .Scopes(store)
                .Register(new Scope("other", ScopeKind.Client, ScopePolicy.WorkspaceScopeId, "x/y"))
        );
        Assert.Equal("duplicate_mapping", error.Code);
        Assert.Equal(1, error.ExitCode);
        Assert.Equal(
            ["legacy", ScopePolicy.WorkspaceScopeId],
            store.ReadScopes().Select(scope => scope.Id).Order(StringComparer.Ordinal).ToArray()
        );
    }
}
