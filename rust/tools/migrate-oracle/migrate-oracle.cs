// Plays migration scenarios (lot 6.1) on the real InstanceMigration of src/Sermofur.Infrastructure,
// each on a fresh copy of the format 1 fixture of MigrationTests (tests/Sermofur.Tests/Fixtures/
// instance-v1, copied, never modified). The interruption points are the StepHook of the oracle
// (backup, transaction, database), set by reflection: a crash is an IOException("simulated
// crash"), recorded as {crash}. The race is MigrationThatLosesTheRaceKeepsNoBackupOfItsOwn: a
// second migration runs to completion inside the hook "backup" of the first.
// Steps are recorded in order (raw SQL, file writes and deletions, migrations, dumps) so that the
// Rust test replays the same script. A migration records its answer: the RecordJson text the CLI
// prints (line breaks as "\n", the backup name normalized), or the refusal the CLI prints for the
// exception (code, message, exit code), or the type name of any other exception. A dump records,
// through SQL that the Rust test runs again, sqlite_master, every table and the checks of the
// database, the same for each backup, the names of the backups (normalized), instance.json (line
// breaks of the platform as "\n") and the projections. A lock is held by another connection
// between lock and unlock; under it, a migration or the backup step alone (the private Open and
// Backup, by reflection) records its elapsed time in buckets (<1s, 1-4s, >=4s).
// Usage: dotnet run migrate-oracle.cs -- OUTPUT.json (from rust/tools/migrate-oracle)
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Diagnostics;
using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

string fixture = Path.GetFullPath(
    Path.Combine("..", "..", "..", "tests", "Sermofur.Tests", "Fixtures", "instance-v1", "marker")
);
FieldInfo hook = typeof(InstanceMigration).GetField(
    "StepHook",
    BindingFlags.NonPublic | BindingFlags.Static
)!;

// The private Open and Backup of InstanceMigration, to time the backup alone on a locked source.
MethodInfo openMethod = typeof(InstanceMigration).GetMethod(
    "Open",
    BindingFlags.NonPublic | BindingFlags.Static
)!;
MethodInfo backupMethod = typeof(InstanceMigration).GetMethod(
    "Backup",
    BindingFlags.NonPublic | BindingFlags.Static
)!;
Regex backupName = new Regex("memory-v1-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{32}\\.db");

string[] legacyQueries =
[
    "PRAGMA user_version",
    "SELECT type,name,tbl_name,rootpage,sql FROM sqlite_master ORDER BY rowid",
    "SELECT * FROM metadata ORDER BY rowid",
    "SELECT * FROM scopes ORDER BY rowid",
    "SELECT * FROM records ORDER BY rowid",
    "SELECT * FROM history ORDER BY rowid",
    "SELECT * FROM idempotency ORDER BY rowid",
    "SELECT * FROM sqlite_sequence ORDER BY rowid",
    "PRAGMA integrity_check",
    "PRAGMA foreign_key_check",
];
string[] currentQueries =
[
    .. legacyQueries,
    "SELECT rowid,* FROM search ORDER BY rowid",
    "SELECT * FROM search_terms ORDER BY term,doc,col,offset",
];

JsonArray scenarios = [];
JsonArray steps = [];
string root = "";

// The connection of another process that holds a lock on memory.db (op lock, op unlock).
SqliteConnection? holder = null;

void Scenario(string name, Action body)
{
    root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-migrate-oracle-" + Guid.NewGuid().ToString("N")
    );
    Directory.CreateDirectory(Path.Combine(root, "client-a", "project-x"));
    Directory.CreateDirectory(Path.Combine(root, "client-b"));
    Copy(fixture, Path.Combine(root, ".sermofur"));
    steps = [];
    body();
    SqliteConnection.ClearAllPools();
    Directory.Delete(root, recursive: true);
    scenarios.Add(new JsonObject { ["name"] = name, ["steps"] = steps });
}

void Copy(string source, string target)
{
    Directory.CreateDirectory(target);
    foreach (string file in Directory.GetFiles(source))
    {
        File.Copy(file, Path.Combine(target, Path.GetFileName(file)));
    }
    foreach (string directory in Directory.GetDirectories(source))
    {
        Copy(directory, Path.Combine(target, Path.GetFileName(directory)));
    }
}

string Database() => Path.Combine(root, ".sermofur", "memory.db");

void Sql(string sql)
{
    using (
        SqliteConnection connection = new SqliteConnection(
            $"Pooling=False;Foreign Keys=False;Data Source={Database()}"
        )
    )
    {
        connection.Open();
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = sql;
        command.ExecuteNonQuery();
    }
    steps.Add(new JsonObject { ["op"] = "sql", ["sql"] = sql });
}

void Write(string path, string text)
{
    File.WriteAllText(Path.Combine(root, path), text, new UTF8Encoding(false));
    steps.Add(
        new JsonObject
        {
            ["op"] = "write",
            ["path"] = path,
            ["text"] = text,
        }
    );
}

void Remove(string path)
{
    File.Delete(Path.Combine(root, path));
    steps.Add(new JsonObject { ["op"] = "remove", ["path"] = path });
}

string Normalize(string text) => backupName.Replace(text, "memory-v1-<stamp>-<id>.db");

JsonNode? Value(object value) =>
    value switch
    {
        DBNull => null,
        long number => JsonValue.Create(number),
        string text => JsonValue.Create(text),
        double real => JsonValue.Create(
            "real:" + real.ToString("R", System.Globalization.CultureInfo.InvariantCulture)
        ),
        byte[] bytes => JsonValue.Create("blob:" + Convert.ToHexStringLower(bytes)),
        _ => throw new InvalidOperationException(value.GetType().Name),
    };

JsonArray DumpDatabase(string file)
{
    JsonArray tables = [];
    using SqliteConnection connection = new SqliteConnection(
        $"Pooling=False;Mode=ReadOnly;Data Source={file}"
    );
    connection.Open();
    using SqliteCommand version = connection.CreateCommand();
    version.CommandText = "PRAGMA user_version";
    string[] queries = (long)version.ExecuteScalar()! == 1 ? legacyQueries : currentQueries;
    foreach (string query in queries)
    {
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = query;
        using SqliteDataReader reader = command.ExecuteReader();
        JsonArray rows = [];
        while (reader.Read())
        {
            JsonArray row = [];
            for (int i = 0; i < reader.FieldCount; i++)
            {
                row.Add(Value(reader.GetValue(i)));
            }
            rows.Add(row);
        }
        tables.Add(new JsonObject { ["sql"] = query, ["rows"] = rows });
    }
    return tables;
}

void Dump()
{
    string marker = Path.Combine(root, ".sermofur");
    string backups = Path.Combine(marker, "backups");
    JsonArray copies = [];
    if (Directory.Exists(backups))
    {
        foreach (string file in Directory.GetFiles(backups).Order(StringComparer.Ordinal))
        {
            copies.Add(
                new JsonObject
                {
                    ["name"] = Normalize(Path.GetFileName(file)),
                    // A backup that failed on a locked source stays as an empty file.
                    ["database"] =
                        new FileInfo(file).Length == 0 ? new JsonArray() : DumpDatabase(file),
                }
            );
        }
    }
    JsonObject projections = [];
    foreach (
        string file in Directory
            .GetFiles(Path.Combine(marker, "records"))
            .Order(StringComparer.Ordinal)
    )
    {
        projections[Path.GetFileName(file)] = File.ReadAllText(file);
    }
    steps.Add(
        new JsonObject
        {
            ["op"] = "dump",
            ["database"] = DumpDatabase(Database()),
            ["backups"] = copies,
            ["instance"] = File.ReadAllText(Path.Combine(marker, "instance.json"))
                .Replace(Environment.NewLine, "\n"),
            ["projections"] = projections,
        }
    );
}

JsonNode Answer(Func<MigrationResult> migrate) =>
    Outcome(() => Normalize(RecordJson.Write(migrate()).Replace(Environment.NewLine, "\n")));

JsonNode Outcome(Func<JsonNode> body)
{
    try
    {
        return body();
    }
    catch (IOException exception) when (exception.Message == "simulated crash")
    {
        return new JsonObject { ["crash"] = true };
    }
    catch (SermofurException exception)
    {
        return Refusal(exception.Code, exception.Message, exception.ExitCode);
    }
    catch (SqliteException exception)
    {
        return Refusal(
            exception.SqliteErrorCode is 5 or 6 ? "storage_busy" : "storage_error",
            "Storage unavailable or inconsistent; run doctor.",
            3
        );
    }
    catch (Exception exception)
        when (exception
                is JsonException
                    or IOException
                    or UnauthorizedAccessException
                    or ArgumentException
                    or InvalidOperationException
        )
    {
        return Refusal("invalid_storage_or_path", "Invalid path or data; run doctor.", 3);
    }
    catch (Exception exception)
    {
        return new JsonObject { ["exception"] = exception.GetType().Name };
    }
}

JsonObject Refusal(string code, string message, int exit) =>
    new JsonObject
    {
        ["code"] = code,
        ["message"] = message,
        ["exitCode"] = exit,
    };

// A migration, interrupted at `crash` when given.
void Migrate(string? crash = null)
{
    hook.SetValue(
        null,
        crash is null
            ? null
            : (Action<string>)(
                step =>
                {
                    if (step == crash)
                    {
                        throw new IOException("simulated crash");
                    }
                }
            )
    );
    JsonNode answer;
    try
    {
        answer = Answer(() => InstanceMigration.Migrate(root));
    }
    finally
    {
        hook.SetValue(null, null);
    }
    steps.Add(
        new JsonObject
        {
            ["op"] = "migrate",
            ["crash"] = crash,
            ["answer"] = answer,
        }
    );
}

// Elapsed time of a step that may wait for a lock, in buckets wide enough for any machine.
string Bucket(Stopwatch watch) =>
    watch.Elapsed.TotalSeconds < 1 ? "<1s"
    : watch.Elapsed.TotalSeconds >= 4 ? ">=4s"
    : "1-4s";

// Another connection takes a lock on memory.db and keeps it until Unlock.
void Lock(string sql)
{
    holder = new SqliteConnection($"Pooling=False;Foreign Keys=False;Data Source={Database()}");
    holder.Open();
    using (SqliteCommand command = holder.CreateCommand())
    {
        command.CommandText = sql;
        command.ExecuteNonQuery();
    }
    steps.Add(new JsonObject { ["op"] = "lock", ["sql"] = sql });
}

void Unlock()
{
    holder!.Dispose();
    holder = null;
    steps.Add(new JsonObject { ["op"] = "unlock" });
}

// A migration with its elapsed time.
void TimedMigrate()
{
    Stopwatch watch = Stopwatch.StartNew();
    JsonNode answer = Answer(() => InstanceMigration.Migrate(root));
    steps.Add(
        new JsonObject
        {
            ["op"] = "migrate",
            ["crash"] = null,
            ["answer"] = answer,
            ["elapsed"] = Bucket(watch),
        }
    );
}

// The backup step alone (InstanceMigration.Open then Backup), with its elapsed time: the
// consistency checks of Migrate read the source first, so a lock taken before them is met
// there, and a lock met by the backup itself comes only from a race.
void TimedBackup()
{
    Stopwatch watch = Stopwatch.StartNew();
    JsonNode answer = Outcome(() =>
    {
        using SqliteConnection connection = (SqliteConnection)
            openMethod.Invoke(null, [Database()])!;
        try
        {
            backupMethod.Invoke(null, [connection, root]);
        }
        catch (TargetInvocationException exception)
        {
            ExceptionDispatchInfo.Capture(exception.InnerException!).Throw();
        }
        return "done";
    });
    steps.Add(
        new JsonObject
        {
            ["op"] = "backup",
            ["answer"] = answer,
            ["elapsed"] = Bucket(watch),
        }
    );
}

// A second migration runs to completion between the backup of the first and its lock.
void Race()
{
    JsonNode? winner = null;
    hook.SetValue(
        null,
        (Action<string>)(
            step =>
            {
                if (step == "backup" && winner is null)
                {
                    hook.SetValue(null, null);
                    winner = Answer(() => InstanceMigration.Migrate(root));
                }
            }
        )
    );
    JsonNode loser;
    try
    {
        loser = Answer(() => InstanceMigration.Migrate(root));
    }
    finally
    {
        hook.SetValue(null, null);
    }
    steps.Add(
        new JsonObject
        {
            ["op"] = "race",
            ["winner"] = winner,
            ["loser"] = loser,
        }
    );
}

const string Configuration =
    "{\"schemaVersion\":SCHEMA,\"instanceId\":\"ID\",\"createdAt\":\"2026-10-08T10:16:34.8807593+00:00\"}";
string Config(int schema, string id) =>
    Configuration.Replace("SCHEMA", schema.ToString()).Replace("ID", id);
const string FixtureId = "d0c43e10-ec6f-45c3-a15b-4b1413b8cfab";
const string OtherId = "0f8fad5b-d9cb-469f-a165-70867728950e";

// MigrationKeepsEveryObjectHistoryKeyAndProjection, BackupIsAConsistentCopyOfTheFormatOneDatabase
// and MigratedInstanceAcceptsNewObjectsAndMigrateIsIdempotent (the second migration).
Scenario(
    "migrate",
    () =>
    {
        Dump();
        Migrate();
        Dump();
        Migrate();
        Dump();
    }
);

// InterruptionBeforeCommitLeavesFormatOneThenMigrationResumes and
// InterruptionBeforeTheConfigurationIsAnUnfinishedMigrationThatMigrateCompletes.
foreach (string step in new[] { "backup", "transaction", "database" })
{
    Scenario(
        "interrupted_at_" + step,
        () =>
        {
            Migrate(step);
            Dump();
            Migrate();
            Dump();
            Migrate();
        }
    );
}

// MigrationThatLosesTheRaceKeepsNoBackupOfItsOwn, with E1: the loser answers migrated true.
Scenario(
    "race",
    () =>
    {
        Race();
        Dump();
    }
);

// A configuration already in format 2 over a database in format 1: migrated, not rewritten.
Scenario(
    "configuration_2_database_1",
    () =>
    {
        Write(".sermofur/instance.json", Config(2, FixtureId));
        Migrate();
        Dump();
    }
);

// Every refusal leaves the instance as it was.
Scenario(
    "refusals",
    () =>
    {
        Write(".sermofur/instance.json", Config(3, FixtureId));
        Migrate();
        Write(".sermofur/instance.json", Config(1, OtherId));
        Migrate();
        Write(".sermofur/instance.json", Config(1, FixtureId));
        Sql($"UPDATE metadata SET value='{OtherId}' WHERE key='instance_id'");
        Migrate();
        Sql("UPDATE metadata SET value='not a guid' WHERE key='instance_id'");
        Migrate();
        Sql($"UPDATE metadata SET value='{FixtureId}' WHERE key='instance_id'");
        Sql("PRAGMA user_version=3");
        Migrate();
        Sql("PRAGMA user_version=0");
        Migrate();
        // Above Int32.MaxValue: the header keeps 32 bits, read back as -1294967296.
        Sql("PRAGMA user_version=3000000000");
        Migrate();
        Sql("PRAGMA user_version=1");
        Sql("INSERT INTO history(object_id,scope_id,payload) VALUES('nope','workspace','{}')");
        Migrate();
        Sql("DELETE FROM history WHERE object_id='nope'");
        Write(".sermofur/backups", "a file in place of the folder");
        Migrate();
        Remove(".sermofur/backups");
        Dump();
        Sql("ALTER TABLE metadata RENAME TO metadata_old");
        Migrate();
        Sql("ALTER TABLE metadata_old RENAME TO metadata");
        Remove(".sermofur/memory.db");
        Migrate();
    }
);

// A lock held by another connection: EXCLUSIVE (no reader), then RESERVED with a pending
// write (readers allowed, no other writer).
Scenario(
    "locked",
    () =>
    {
        Lock("BEGIN EXCLUSIVE");
        TimedBackup();
        TimedMigrate();
        Unlock();
        Lock("BEGIN IMMEDIATE; UPDATE metadata SET value=value WHERE key='instance_id'");
        TimedMigrate();
        Unlock();
        Dump();
    }
);

// PRAGMA integrity_check reports an index that does not match its table.
Scenario(
    "inconsistent",
    () =>
    {
        Sql(
            "CREATE TABLE t(a,b); INSERT INTO t VALUES(1,2); CREATE INDEX t_a ON t(a); "
                + "PRAGMA writable_schema=ON; "
                + "UPDATE sqlite_master SET sql='CREATE INDEX t_a ON t(b)' WHERE name='t_a'; "
                + "PRAGMA writable_schema=OFF"
        );
        Migrate();
        Dump();
    }
);

JsonObject result = new()
{
    ["generator"] = "rust/tools/migrate-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["sqlite"] = SqliteVersion(),
    ["scenarios"] = scenarios,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ) + "\n",
    new UTF8Encoding(false)
);
Console.WriteLine($"{scenarios.Count} scenarios");

static string SqliteVersion()
{
    using SqliteConnection connection = new SqliteConnection("Data Source=:memory:");
    connection.Open();
    using SqliteCommand command = connection.CreateCommand();
    command.CommandText = "SELECT sqlite_version()";
    return (string)command.ExecuteScalar()!;
}
