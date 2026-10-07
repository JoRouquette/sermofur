using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class DoctorTests
{
    [Fact]
    public void DoctorLeavesPersistentFilesUnchanged()
    {
        using TestInstance fixture = new TestInstance();
        using (SqliteStore store = fixture.Open())
        {
            fixture.Memory(store).CreateClaim(TestInstance.Fact(), TestInstance.User, null);
        }

        Dictionary<string, string> before = fixture.Snapshot();
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("healthy_with_warnings", report.Overall);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
        Assert.DoesNotContain(report.Checks, check => check.Status == "error");
        Assert.Equal("warning", report.Checks.Single(c => c.Name == "laya").Status);
    }

    [Fact]
    public void FutureSchemaIsRejectedWithoutMigration()
    {
        using TestInstance fixture = new TestInstance();
        using (
            SqliteConnection database = new SqliteConnection(
                "Pooling=False;Data Source=" + InstanceManager.DatabasePath(fixture.Root)
            )
        )
        {
            database.Open();
            using SqliteCommand command = database.CreateCommand();
            command.CommandText = "PRAGMA user_version=999";
            command.ExecuteNonQuery();
        }
        Dictionary<string, string> before = fixture.Snapshot();
        Assert.Equal("unhealthy", new InstanceDoctor().Inspect(fixture.Root).Overall);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
        using SqliteStore store = fixture.Open(true);
        Assert.Equal(
            "unsupported_schema",
            Assert.Throws<SermofurException>(store.ValidateSchema).Code
        );
    }

    [Fact]
    public void MissingAndCorruptDatabaseAreNotCreatedOrRepaired()
    {
        using TestInstance fixture = new TestInstance();
        string database = InstanceManager.DatabasePath(fixture.Root);
        File.Delete(database);
        Assert.Equal("unhealthy", new InstanceDoctor().Inspect(fixture.Root).Overall);
        Assert.False(File.Exists(database));
        File.WriteAllText(database, "not sqlite");
        Dictionary<string, string> before = fixture.Snapshot();
        Assert.Equal("unhealthy", new InstanceDoctor().Inspect(fixture.Root).Overall);
        Assert.Equal(before.OrderBy(x => x.Key), fixture.Snapshot().OrderBy(x => x.Key));
    }

    [Fact]
    public void OverlappingScopeInsertedOutsideTheCliIsAnError()
    {
        using TestInstance fixture = new TestInstance();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "b"));
        using (SqliteStore store = fixture.Open())
        {
            fixture.Client(store, "a");
        }
        using (
            SqliteConnection database = new SqliteConnection(
                "Pooling=False;Data Source=" + InstanceManager.DatabasePath(fixture.Root)
            )
        )
        {
            database.Open();
            using SqliteCommand command = database.CreateCommand();
            command.CommandText = "INSERT INTO scopes VALUES('nested','Client','workspace','a/b')";
            command.ExecuteNonQuery();
        }
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        DiagnosticCheck overlap = report.Checks.Single(check => check.Name == "scope_overlap");
        Assert.Equal("error", overlap.Status);
        Assert.Equal("1 conflicting mapping pairs.", overlap.Detail);
        Assert.Equal("unhealthy", report.Overall);
    }

    [Fact]
    public void RepositorySharingItsProjectPathIsNotAnOverlap()
    {
        using TestInstance fixture = new TestInstance();
        Directory.CreateDirectory(Path.Combine(fixture.Root, "a", "p"));
        using (SqliteStore store = fixture.Open())
        {
            string a = fixture.Client(store, "a");
            fixture.Scopes(store, a).Register(new Scope("p", ScopeKind.Project, "a", "a/p"));
            string p = Path.Combine(a, "p");
            fixture.Scopes(store, p).Register(new Scope("r", ScopeKind.Repository, "p", "a/p"));
        }
        DoctorReport report = new InstanceDoctor().Inspect(fixture.Root);
        Assert.Equal("ok", report.Checks.Single(check => check.Name == "scope_overlap").Status);
    }

    [Fact]
    public void SchemaAndInstanceIdentityMismatchIsDiagnosed()
    {
        using TestInstance fixture = new TestInstance();
        InstanceConfiguration config = fixture.Manager.ReadConfiguration(fixture.Root) with
        {
            InstanceId = Guid.NewGuid(),
        };
        File.WriteAllText(
            Path.Combine(fixture.Root, ".sermofur", "instance.json"),
            RecordJson.Write(config)
        );
        Assert.Contains(
            new InstanceDoctor().Inspect(fixture.Root).Checks,
            c => c.Status == "error" && c.Detail == "instance_mismatch"
        );
    }
}
