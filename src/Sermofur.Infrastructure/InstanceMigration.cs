using System.Globalization;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

public sealed record MigrationResult(bool Migrated, int From, int To, string? Backup);

/// <summary>
/// Explicit migration of an instance from format 1 to format 2 (ADR 0012): consistency check,
/// backup through the SQLite backup API, one IMMEDIATE transaction for the database, then
/// <c>instance.json</c> replaced atomically. A database already in format 2 with a format 1
/// configuration is an unfinished migration that only needs its last step.
/// </summary>
public static class InstanceMigration
{
    /// <summary>Test hook: called with the name of each step, may throw to simulate a crash.</summary>
    [ThreadStatic]
    internal static Action<string>? StepHook;

    public static MigrationResult Migrate(string root)
    {
        InstanceManager manager = new InstanceManager();
        InstanceConfiguration configuration = manager.ReadConfiguration(root, allowLegacy: true);
        string database = InstanceManager.DatabasePath(root);
        LocalPaths.RejectLinks(database);
        if (!File.Exists(database))
        {
            throw new SermofurException("missing_database", "Memory database missing.", 3);
        }
        int version;
        string? backup = null;
        using (SqliteConnection connection = Open(database))
        {
            version = Convert.ToInt32(Scalar(connection, "PRAGMA user_version"));
            CheckIdentity(connection, configuration.InstanceId);
            if (version == InstanceManager.LegacySchemaVersion)
            {
                RequireConsistent(connection);
                backup = Backup(connection, root);
                StepHook?.Invoke("backup");
                if (!MigrateDatabase(connection))
                {
                    // Another migrate committed first: nothing left to do on the database.
                    version = InstanceManager.SchemaVersion;
                }
            }
            else if (version != InstanceManager.SchemaVersion)
            {
                throw new SermofurException(
                    "unsupported_schema",
                    "Unsupported database schema version.",
                    3
                );
            }
        }
        StepHook?.Invoke("database");
        bool migrated =
            version == InstanceManager.LegacySchemaVersion
            || configuration.SchemaVersion == InstanceManager.LegacySchemaVersion;
        if (configuration.SchemaVersion == InstanceManager.LegacySchemaVersion)
        {
            WriteConfiguration(
                root,
                configuration with
                {
                    SchemaVersion = InstanceManager.SchemaVersion,
                }
            );
        }
        return new MigrationResult(
            migrated,
            migrated ? InstanceManager.LegacySchemaVersion : InstanceManager.SchemaVersion,
            InstanceManager.SchemaVersion,
            backup is null ? null : LocalPaths.RelativizeMapping(root, backup)
        );
    }

    private static SqliteConnection Open(string path)
    {
        // Foreign keys off: the records table is rebuilt; they are checked before commit.
        SqliteConnection connection = new SqliteConnection(
            new SqliteConnectionStringBuilder
            {
                DataSource = path,
                Mode = SqliteOpenMode.ReadWrite,
                Pooling = false,
                ForeignKeys = false,
                DefaultTimeout = 5,
            }.ToString()
        );
        connection.Open();
        return connection;
    }

    private static object Scalar(
        SqliteConnection connection,
        string sql,
        SqliteTransaction? transaction = null
    )
    {
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = sql;
        return command.ExecuteScalar() ?? string.Empty;
    }

    private static void CheckIdentity(SqliteConnection connection, Guid instanceId)
    {
        object stored = Scalar(connection, "SELECT value FROM metadata WHERE key='instance_id'");
        if (!Guid.TryParse(Convert.ToString(stored), out Guid id) || id != instanceId)
        {
            throw new SermofurException("instance_mismatch", "Inconsistent SQLite identity.", 3);
        }
    }

    private static void RequireConsistent(SqliteConnection connection)
    {
        if (
            Convert.ToString(Scalar(connection, "PRAGMA integrity_check")) != "ok"
            || Convert.ToString(Scalar(connection, "PRAGMA foreign_key_check")) != string.Empty
        )
        {
            throw new SermofurException(
                "storage_error",
                "The instance is not consistent; run smf doctor before migrating.",
                3
            );
        }
    }

    private static string Backup(SqliteConnection connection, string root)
    {
        string directory = Path.Combine(
            root,
            InstanceManager.Marker,
            InstanceManager.BackupsDirectory
        );
        LocalPaths.RejectLinks(directory);
        Directory.CreateDirectory(directory);
        string stamp = DateTimeOffset.UtcNow.ToString(
            "yyyyMMdd'T'HHmmss'Z'",
            CultureInfo.InvariantCulture
        );
        string target = Path.Combine(directory, $"memory-v1-{stamp}-{Guid.NewGuid():N}.db");
        using SqliteConnection destination = new SqliteConnection(
            new SqliteConnectionStringBuilder
            {
                DataSource = target,
                Mode = SqliteOpenMode.ReadWriteCreate,
                Pooling = false,
            }.ToString()
        );
        destination.Open();
        connection.BackupDatabase(destination);
        return target;
    }

    /// <summary>
    /// Migrates under an IMMEDIATE transaction; the version is read again under that lock, so
    /// that two concurrent migrations never both rebuild the database. Returns false when it
    /// was already migrated.
    /// </summary>
    private static bool MigrateDatabase(SqliteConnection connection)
    {
        using SqliteTransaction transaction = connection.BeginTransaction(deferred: false);
        if (
            Convert.ToInt32(Scalar(connection, "PRAGMA user_version", transaction))
            != InstanceManager.LegacySchemaVersion
        )
        {
            return false;
        }
        SqliteSchema.MigrateVersion1(connection, transaction);
        SqliteSearchIndex.IndexRecords(connection, transaction);
        using (SqliteCommand version = connection.CreateCommand())
        {
            version.Transaction = transaction;
            version.CommandText = $"PRAGMA user_version={InstanceManager.SchemaVersion}";
            version.ExecuteNonQuery();
        }
        if (
            Convert.ToString(Scalar(connection, "PRAGMA foreign_key_check", transaction))
            != string.Empty
        )
        {
            throw new SermofurException(
                "storage_error",
                "Migration broke a reference; nothing changed.",
                3
            );
        }
        StepHook?.Invoke("transaction");
        transaction.Commit();
        return true;
    }

    private static void WriteConfiguration(string root, InstanceConfiguration configuration)
    {
        string file = Path.Combine(root, InstanceManager.Marker, InstanceManager.ConfigurationFile);
        LocalPaths.RejectLinks(file);
        string temporary = file + $".{Guid.NewGuid():N}.tmp";
        try
        {
            File.WriteAllText(temporary, RecordJson.Write(configuration));
            File.Move(temporary, file, overwrite: true);
        }
        finally
        {
            if (File.Exists(temporary))
            {
                File.Delete(temporary);
            }
        }
    }
}
