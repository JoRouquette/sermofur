using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

public class MigrationTests
{
    private const string RecordRows =
        "SELECT id,scope_id,kind,claim_id,payload,revision FROM records ORDER BY id";
    private const string HistoryRows =
        "SELECT sequence,object_id,scope_id,payload FROM history ORDER BY sequence";
    private const string KeyRows = "SELECT * FROM idempotency ORDER BY key";
    private const string ScopeRows = "SELECT * FROM scopes ORDER BY id";

    [Fact]
    public void MigrationKeepsEveryObjectHistoryKeyAndProjection()
    {
        using LegacyInstance legacy = new LegacyInstance();
        IReadOnlyList<string> records = legacy.Rows(RecordRows);
        IReadOnlyList<string> history = legacy.Rows(HistoryRows);
        IReadOnlyList<string> keys = legacy.Rows(KeyRows);
        IReadOnlyList<string> scopes = legacy.Rows(ScopeRows);
        Dictionary<string, string> projections = legacy.Projections();
        Assert.Equal(6, records.Count);

        MigrationResult result = InstanceMigration.Migrate(legacy.Root);

        Assert.True(result.Migrated);
        Assert.Equal((1, 2), (result.From, result.To));
        Assert.StartsWith(".sermofur/backups/memory-v1-", result.Backup);
        Assert.Equal(2, legacy.UserVersion());
        Assert.Equal(records, legacy.Rows(RecordRows));
        Assert.Equal(history, legacy.Rows(HistoryRows));
        Assert.Equal(keys, legacy.Rows(KeyRows));
        Assert.Equal(scopes, legacy.Rows(ScopeRows));
        Assert.Equal(projections, legacy.Projections());
        Assert.Equal(
            InstanceManager.SchemaVersion,
            new InstanceManager().ReadConfiguration(legacy.Root).SchemaVersion
        );
        // Four claims and one RETEX are indexed; the evidence is not.
        Assert.Equal("5", legacy.Rows("SELECT count(DISTINCT object_id) FROM search").Single());
        DoctorReport report = new InstanceDoctor().Inspect(legacy.Root);
        Assert.DoesNotContain(report.Checks, check => check.Status == "error");
        Assert.Equal("ok", report.Checks.Single(check => check.Name == "search_index").Status);
    }

    [Fact]
    public void MigratedInstanceAcceptsNewObjectsAndMigrateIsIdempotent()
    {
        using LegacyInstance legacy = new LegacyInstance();
        InstanceMigration.Migrate(legacy.Root);
        InstanceManager manager = new InstanceManager();
        InstanceConfiguration config = manager.ReadConfiguration(legacy.Root);
        using (SqliteStore store = new SqliteStore(legacy.Root, config.InstanceId))
        {
            store.ValidateSchema();
            string clientA = Path.Combine(legacy.Root, "client-a");
            MemoryService memory = new MemoryService(
                store,
                manager.ResolveContext(clientA, legacy.Root, store.ReadScopes())
            );
            Assert.Equal(2, memory.List(RecordKind.Claim).Count);
            memory.CreateClaim(TestInstance.Fact("after migration"), TestInstance.User, null);
        }
        MigrationResult again = InstanceMigration.Migrate(legacy.Root);
        Assert.False(again.Migrated);
        Assert.Null(again.Backup);
        Assert.Single(
            Directory.GetFiles(
                Path.Combine(legacy.Root, ".sermofur", InstanceManager.BackupsDirectory)
            )
        );
    }

    [Fact]
    public void BackupIsAConsistentCopyOfTheFormatOneDatabase()
    {
        using LegacyInstance legacy = new LegacyInstance();
        IReadOnlyList<string> records = legacy.Rows(RecordRows);
        MigrationResult result = InstanceMigration.Migrate(legacy.Root);
        string backup = Path.Combine(legacy.Root, result.Backup!);
        using Microsoft.Data.Sqlite.SqliteConnection connection = new(
            $"Pooling=False;Mode=ReadOnly;Data Source={backup}"
        );
        connection.Open();
        using Microsoft.Data.Sqlite.SqliteCommand command = connection.CreateCommand();
        command.CommandText = "PRAGMA user_version";
        Assert.Equal(1L, command.ExecuteScalar());
        command.CommandText = "SELECT count(*) FROM records";
        Assert.Equal((long)records.Count, command.ExecuteScalar());
    }

    [Fact]
    public void OtherCommandsRefuseFormatOneWithoutTouchingIt()
    {
        using LegacyInstance legacy = new LegacyInstance();
        Dictionary<string, string> before = TestInstance.SnapshotOf(legacy.Root);
        CliResult status = TestInstance.Run("--path", legacy.Root, "--json", "status");
        Assert.Equal(3, status.ExitCode);
        Assert.Equal("migration_required", TestInstance.ErrorCode(status));
        CliResult add = TestInstance.Run(
            "--path",
            legacy.Root,
            "--json",
            "claim",
            "add",
            "x",
            "--origin",
            "user"
        );
        Assert.Equal("migration_required", TestInstance.ErrorCode(add));
        Assert.Equal(0, TestInstance.Run("--path", legacy.Root, "root").ExitCode);
        DoctorReport report = new InstanceDoctor().Inspect(legacy.Root);
        Assert.Contains(report.Checks, check => check.Detail == "migration_required");
        Assert.Equal(
            before.OrderBy(x => x.Key),
            TestInstance.SnapshotOf(legacy.Root).OrderBy(x => x.Key)
        );
    }

    [Theory]
    [InlineData("backup")]
    [InlineData("transaction")]
    public void InterruptionBeforeCommitLeavesFormatOneThenMigrationResumes(string step)
    {
        using LegacyInstance legacy = new LegacyInstance();
        IReadOnlyList<string> records = legacy.Rows(RecordRows);
        InstanceMigration.StepHook = name =>
        {
            if (name == step)
            {
                throw new IOException("simulated crash");
            }
        };
        try
        {
            Assert.Throws<IOException>(() => InstanceMigration.Migrate(legacy.Root));
        }
        finally
        {
            InstanceMigration.StepHook = null;
        }
        Assert.Equal(1, legacy.UserVersion());
        Assert.Equal(records, legacy.Rows(RecordRows));
        Assert.Equal(
            "migration_required",
            TestInstance.ErrorCode(TestInstance.Run("--path", legacy.Root, "--json", "status"))
        );
        Assert.True(InstanceMigration.Migrate(legacy.Root).Migrated);
        Assert.Equal(records, legacy.Rows(RecordRows));
    }

    [Fact]
    public void InterruptionBeforeTheConfigurationIsAnUnfinishedMigrationThatMigrateCompletes()
    {
        using LegacyInstance legacy = new LegacyInstance();
        InstanceMigration.StepHook = name =>
        {
            if (name == "database")
            {
                throw new IOException("simulated crash");
            }
        };
        try
        {
            Assert.Throws<IOException>(() => InstanceMigration.Migrate(legacy.Root));
        }
        finally
        {
            InstanceMigration.StepHook = null;
        }
        Assert.Equal(2, legacy.UserVersion());
        Assert.Equal(
            "migration_required",
            TestInstance.ErrorCode(TestInstance.Run("--path", legacy.Root, "--json", "status"))
        );
        MigrationResult result = InstanceMigration.Migrate(legacy.Root);
        Assert.True(result.Migrated);
        Assert.Null(result.Backup);
        Assert.Equal(0, TestInstance.Run("--path", legacy.Root, "--json", "status").ExitCode);
    }

    [Fact]
    public void MigrateCommandReportsTheResult()
    {
        using LegacyInstance legacy = new LegacyInstance();
        CliResult first = TestInstance.Run("--path", legacy.Root, "--json", "migrate");
        Assert.Equal(0, first.ExitCode);
        Assert.Contains("\"migrated\": true", first.Output);
        CliResult second = TestInstance.Run("--path", legacy.Root, "--json", "migrate");
        Assert.Contains("\"migrated\": false", second.Output);
    }
}
