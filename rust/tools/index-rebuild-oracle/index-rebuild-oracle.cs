// Plays index rebuild scenarios (lot 5.3) on the real SqliteStore of src/Sermofur.Infrastructure
// through SourceService.RebuildIndex of src/Sermofur.Application, with the real FileSourceReader
// wrapped to record the paths it reads, or to fail on one of them as the FailingReader of
// SourceTests. Each scenario runs in a new instance; its steps (scopes, records with fixed GUIDs,
// files, raw SQL, rebuilds) are recorded in order so that the Rust test replays the same script.
// A rebuild records its answer (the RecordJson text the CLI prints, line breaks as "\n"; a refusal
// is its code, message and exit code; any other exception is its type name), the paths read, then
// the rows of records, history and search and the names of the projection files. The rows are
// read through SQL that minifies the JSON payloads and drops the instants stamped now
// (indexedAt of a source indexed again, timestamp of history): the same SQL runs on both sides.
// Usage: dotnet run index-rebuild-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

Guid instance = Guid.Parse("11111111-2222-4333-8444-555555555555");
Guid Id(int n) => Guid.Parse($"00000000-0000-4000-8000-{n:D12}");
DateTimeOffset at = new DateTimeOffset(2026, 3, 9, 5, 6, 7, TimeSpan.FromHours(2)).AddTicks(5);
DateTimeOffset indexedBefore = new DateTimeOffset(2001, 2, 3, 4, 5, 6, TimeSpan.Zero);
Provenance user = new(ActorKind.User, "john", at);

// What the rows keep of the JSON payloads: minified, without the instants stamped now.
const string Content = "json_extract(payload,'$.contentJson')";
const string NewState = "json_extract(payload,'$.newState')";
string[] queries =
[
    "SELECT id,scope_id,kind,claim_id,revision,source_path,"
        + "json_remove(json(payload),'$.contentJson'),"
        + $"json_remove(json({Content}),'$.indexedAt'),"
        + $"CASE WHEN json_extract({Content},'$.indexedAt') < '2020' THEN json_extract({Content},'$.indexedAt') "
        + $"WHEN json_extract({Content},'$.indexedAt') IS NULL THEN NULL ELSE 'now' END "
        + "FROM records ORDER BY id",
    "SELECT sequence,object_id,scope_id,json_extract(payload,'$.reason'),json_extract(payload,'$.actor'),"
        + "json_extract(json_extract(payload,'$.previousState'),'$.revision'),"
        + $"json_remove(json({NewState}),'$.contentJson'),"
        + $"json_remove(json(json_extract({NewState},'$.contentJson')),'$.indexedAt') "
        + "FROM history ORDER BY sequence",
    "SELECT object_id,scope_id,kind,passage,length,text FROM search ORDER BY rowid",
];

JsonArray scenarios = [];
JsonArray steps = [];
SqliteStore store = null!;
string root = "";
string database = "";

void Scenario(string name, Action body)
{
    root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-rebuild-oracle-" + Guid.NewGuid().ToString("N")
    );
    Directory.CreateDirectory(Path.Combine(root, ".sermofur"));
    database = Path.Combine(root, ".sermofur", "memory.db");
    SqliteStore.CreateDatabase(database, instance);
    store = new SqliteStore(root, instance);
    steps = [];
    body();
    store.Dispose();
    SqliteConnection.ClearAllPools();
    Directory.Delete(root, recursive: true);
    scenarios.Add(new JsonObject { ["name"] = name, ["steps"] = steps });
}

void AddScope(string id, ScopeKind kind, string parent, string path)
{
    Directory.CreateDirectory(Path.Combine(root, path));
    store.AddScope(new Scope(id, kind, parent, path), _ => { });
    steps.Add(
        new JsonObject
        {
            ["op"] = "addScope",
            ["id"] = id,
            ["kind"] = kind.ToString(),
            ["parent"] = parent,
            ["path"] = path,
        }
    );
}

void Create(MemoryRecord record)
{
    store.CreateRecord(record, null);
    steps.Add(new JsonObject { ["op"] = "create", ["payload"] = RecordJson.Write(record) });
}

void Claim(int id, string scope, string text, KnowledgeStatus status = KnowledgeStatus.Proposed) =>
    Create(
        new MemoryRecord(
            Id(id),
            scope,
            RecordKind.Claim,
            status,
            1,
            RecordJson.Write(new ClaimContent(text, MemoryCategory.Semantic, Volatility.Stable)),
            user
        )
    );

void Retex(int id, string scope, string what) =>
    Create(
        new MemoryRecord(
            Id(id),
            scope,
            RecordKind.Retex,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(new RetexContent(what, "impact " + what, "suite " + what)),
            user
        )
    );

void Evidence(int id, int claim, string scope, int? source = null) =>
    Create(
        new MemoryRecord(
            Id(id),
            scope,
            RecordKind.Evidence,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(
                new EvidenceContent(
                    Id(claim),
                    EvidenceKind.LocalDocumentation,
                    "doc.md",
                    "doc",
                    EvidenceRelation.Supports,
                    source is null ? null : Id(source.Value)
                )
            ),
            user
        )
    );

string Hash(string path) =>
    Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(Path.Combine(root, path))));

// A source as source add leaves it, or forged in another state; the hash is the one of the file
// on disk when it exists, so that reading it again finds it unchanged.
void Source(
    int id,
    string scope,
    string? path,
    SourceStatus status = SourceStatus.Indexed,
    SourceRejection? rejection = null,
    string? hash = null,
    int revision = 1
)
{
    string stored =
        hash ?? (path is not null && File.Exists(Path.Combine(root, path)) ? Hash(path) : "");
    Create(
        new MemoryRecord(
            Id(id),
            scope,
            RecordKind.Source,
            KnowledgeStatus.Active,
            revision,
            RecordJson.Write(
                new SourceContent(
                    path!,
                    status,
                    stored,
                    7,
                    status == SourceStatus.Indexed ? indexedBefore : null,
                    status == SourceStatus.Indexed ? 1 : 0,
                    rejection
                )
            ),
            user
        )
    );
}

void Write(string path, string text)
{
    string full = Path.Combine(root, path);
    Directory.CreateDirectory(Path.GetDirectoryName(full)!);
    File.WriteAllText(full, text, new UTF8Encoding(false));
    steps.Add(
        new JsonObject
        {
            ["op"] = "file",
            ["path"] = path,
            ["text"] = text,
        }
    );
}

void Bytes(string path, byte[] bytes)
{
    File.WriteAllBytes(Path.Combine(root, path), bytes);
    steps.Add(
        new JsonObject
        {
            ["op"] = "bytes",
            ["path"] = path,
            ["hex"] = Convert.ToHexStringLower(bytes),
        }
    );
}

void Fill(string path, int count)
{
    File.WriteAllBytes(Path.Combine(root, path), Enumerable.Repeat((byte)'a', count).ToArray());
    steps.Add(
        new JsonObject
        {
            ["op"] = "fill",
            ["path"] = path,
            ["count"] = count,
        }
    );
}

void Mkdir(string path)
{
    Directory.CreateDirectory(Path.Combine(root, path));
    steps.Add(new JsonObject { ["op"] = "mkdir", ["path"] = path });
}

void Remove(string path)
{
    File.Delete(Path.Combine(root, path));
    steps.Add(new JsonObject { ["op"] = "remove", ["path"] = path });
}

void Rmdir(string path)
{
    Directory.Delete(Path.Combine(root, path), recursive: true);
    steps.Add(new JsonObject { ["op"] = "rmdir", ["path"] = path });
}

void Sql(string sql)
{
    using (SqliteConnection raw = new($"Data Source={database};Pooling=False"))
    {
        raw.Open();
        using SqliteCommand command = raw.CreateCommand();
        command.CommandText = sql;
        command.ExecuteNonQuery();
    }
    steps.Add(new JsonObject { ["op"] = "sql", ["sql"] = sql });
}

JsonArray Rows(string sql)
{
    using SqliteConnection raw = new($"Data Source={database};Pooling=False;Mode=ReadOnly");
    raw.Open();
    using SqliteCommand command = raw.CreateCommand();
    command.CommandText = sql;
    using SqliteDataReader reader = command.ExecuteReader();
    JsonArray rows = [];
    while (reader.Read())
    {
        JsonArray row = [];
        for (int i = 0; i < reader.FieldCount; i++)
        {
            row.Add(
                reader.IsDBNull(i) ? null
                : reader.GetFieldType(i) == typeof(long) ? JsonValue.Create(reader.GetInt64(i))
                : JsonValue.Create(reader.GetString(i))
            );
        }
        rows.Add(row);
    }
    return rows;
}

JsonNode Answer(Func<object> call)
{
    try
    {
        return RecordJson.Write(call()).Replace("\r\n", "\n");
    }
    catch (SermofurException error)
    {
        return new JsonObject
        {
            ["code"] = error.Code,
            ["message"] = error.Message,
            ["exitCode"] = error.ExitCode,
        };
    }
    catch (Exception error)
    {
        return new JsonObject { ["exception"] = error.GetType().Name };
    }
}

void Rebuild(string scope, string? failing = null)
{
    JsonArray reads = [];
    RecordingReader reader = new(new FileSourceReader(new FileOwnership()), reads, failing);
    MemoryContext context = new(root, scope, new HashSet<string>());
    JsonNode answer = Answer(() =>
        new SourceService(store, reader, new LocalPathResolver(), context).RebuildIndex()
    );
    string records = Path.Combine(root, ".sermofur", "records");
    JsonArray projections = [];
    if (Directory.Exists(records))
    {
        foreach (
            string file in Directory
                .EnumerateFiles(records)
                .Select(Path.GetFileName)
                .Order(StringComparer.Ordinal)!
        )
        {
            projections.Add(file);
        }
    }
    JsonArray tables = [];
    foreach (string sql in queries)
    {
        tables.Add(new JsonObject { ["sql"] = sql, ["rows"] = Rows(sql) });
    }
    steps.Add(
        new JsonObject
        {
            ["op"] = "rebuild",
            ["scope"] = scope,
            ["failing"] = failing,
            ["answer"] = answer,
            ["reads"] = reads,
            ["projections"] = projections,
            ["tables"] = tables,
        }
    );
}

// SourceTests.IndexRebuildRestoresADesynchronizedIndex, with every kind of record: claims and
// RETEX indexed whatever their status, evidence never, a source read again.
Scenario(
    "desynchronized",
    () =>
    {
        Claim(1, "workspace", "le cache chaud");
        Claim(2, "workspace", "un claim invalidé", KnowledgeStatus.Invalidated);
        Retex(3, "workspace", "panne");
        Write("doc.md", "cache chaud\n\nsecond paragraphe");
        Source(4, "workspace", "doc.md");
        Evidence(5, 1, "workspace", 4);
        Sql("DELETE FROM search");
        Rebuild("workspace");
        Rebuild("workspace");
        Sql(
            "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES('x','workspace','claim',0,1,'orphelin')"
        );
        Rebuild("workspace");
    }
);

// SourceTests.IndexRebuildReadsEverySourceAgainAndCountsOnlyVisibleObjects, plus a project
// below a client: its objects are not visible from the client, nor the client's from it.
Scenario(
    "visible_counts",
    () =>
    {
        AddScope("a", ScopeKind.Client, "workspace", "a");
        AddScope("b", ScopeKind.Client, "workspace", "b");
        AddScope("p", ScopeKind.Project, "a", "a/p");
        Write("a/a.md", "texte de a");
        Source(10, "a", "a/a.md");
        Write("b/b.md", "texte de b");
        Source(20, "b", "b/b.md");
        Claim(21, "b", "claim de b");
        Write("a/p/p.md", "texte de p");
        Source(30, "p", "a/p/p.md");
        Claim(31, "p", "claim de p");
        Retex(32, "workspace", "commun");
        Write("b/b.md", "texte de b modifié");
        Write("a/p/p.md", "texte de p modifié");
        Rebuild("a");
        Rebuild("b");
        Write("a/a.md", "texte de a modifié");
        Write("b/b.md", "texte de b modifié encore");
        Rebuild("workspace");
        Write("a/a.md", "texte de a revu");
        Rebuild("p");
        Rebuild("p");
    }
);

// Every transition of a source read again, under the system actor.
Scenario(
    "transitions",
    () =>
    {
        Write("same.md", "inchangé");
        Source(1, "workspace", "same.md");
        Write("changed.md", "avant");
        Source(2, "workspace", "changed.md");
        Write("changed.md", "après");
        Write("gone.md", "bientôt parti");
        Source(3, "workspace", "gone.md");
        Remove("gone.md");
        Write("back.md", "de retour");
        Source(4, "workspace", "back.md", SourceStatus.Missing, hash: "0000");
        Source(5, "workspace", "still-missing.md", SourceStatus.Missing, hash: "0000");
        Write("empty.md", "  \n ");
        Source(6, "workspace", "empty.md", hash: "abcd");
        Bytes("binary.md", [0x41, 0x00, 0x42]);
        Source(7, "workspace", "binary.md", SourceStatus.Rejected, SourceRejection.Binary, "abcd");
        Bytes("latin1.md", [0x63, 0x61, 0x66, 0xE9]);
        Source(8, "workspace", "latin1.md", hash: "abcd");
        Fill("large.md", 1024 * 1024 + 1);
        Source(9, "workspace", "large.md", hash: "abcd");
        Mkdir("folder.md");
        Source(10, "workspace", "folder.md", hash: "abcd");
        Source(11, "workspace", ".sermofur/memory.db", hash: "abcd");
        Source(12, "workspace", "../outside.md", hash: "abcd");
        Write("bom.md", "﻿avec BOM");
        Source(13, "workspace", "bom.md", SourceStatus.Rejected, SourceRejection.Empty, "abcd");
        Write("revised.md", "révisé");
        Source(14, "workspace", "revised.md", revision: 4);
        Write("long.md", "premier\n\n" + new string('x', 2500) + "\n\nfin");
        Source(15, "workspace", "long.md", hash: "abcd");
        Rebuild("workspace");
        Rebuild("workspace");
    }
);

// SourceTests.IndexRebuildDropsALostSourceAndRollsBackOnFailure: a failure on either source
// rolls back the change of the other one, then the lost source is dropped.
Scenario(
    "lost_and_rollback",
    () =>
    {
        Write("kept.md", "texte conservé");
        Source(1, "workspace", "kept.md");
        Write("lost.md", "texte perdu");
        Source(2, "workspace", "lost.md");
        Claim(3, "workspace", "un claim");
        Write("kept.md", "texte conservé et révisé");
        Sql("DELETE FROM search WHERE kind='claim'");
        Rebuild("workspace", "lost.md");
        Rebuild("workspace", "kept.md");
        Remove("lost.md");
        Rebuild("workspace");
        Rebuild("workspace");
    }
);

// The projections are written after the commit: a failure leaves the rebuild done and answers
// projection_pending for the first changed source.
Scenario(
    "projection_pending",
    () =>
    {
        Write("one.md", "un");
        Source(1, "workspace", "one.md", hash: "abcd");
        Write("two.md", "deux");
        Source(2, "workspace", "two.md", hash: "abcd");
        Rmdir(".sermofur/records");
        Write(".sermofur/records", "un fichier à la place du dossier");
        Rebuild("workspace");
        Remove(".sermofur/records");
        Rebuild("workspace");
    }
);

// An empty instance, and a source without path (altered base only).
Scenario(
    "empty_and_altered",
    () =>
    {
        Rebuild("workspace");
        Write("doc.md", "texte");
        Source(1, "workspace", null, hash: "abcd");
        Rebuild("workspace");
    }
);

store = null!;
JsonObject result = new()
{
    ["generator"] = "rust/tools/index-rebuild-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
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

/// The real reader, recording every path read, failing on one as SourceTests.FailingReader.
sealed class RecordingReader(ISourceReader inner, JsonArray reads, string? failing) : ISourceReader
{
    public SourceSnapshot Read(string root, string relativePath)
    {
        reads.Add(relativePath);
        return failing is not null && relativePath == failing
            ? throw new IOException("simulated read failure")
            : inner.Read(root, relativePath);
    }
}
