// Plays a fixed script on the real SqliteStore of src/Sermofur.Infrastructure (scopes, records,
// evidence links, history, invalidation, a tampered scope row) and records the answer of each
// step, then the rows of every table. History timestamps (UtcNow) become "<date>" and line
// breaks "\n".
// Usage: dotnet run store-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

string root = Path.Combine(
    Path.GetTempPath(),
    "sermofur-store-oracle-" + Guid.NewGuid().ToString("N")
);
Directory.CreateDirectory(Path.Combine(root, ".sermofur"));
Guid instance = Guid.Parse("11111111-2222-4333-8444-555555555555");
string database = Path.Combine(root, ".sermofur", "memory.db");
SqliteStore.CreateDatabase(database, instance);

DateTimeOffset at = new DateTimeOffset(2026, 3, 4, 5, 6, 7, TimeSpan.FromHours(1));
Guid Id(int n) => Guid.Parse($"00000000-0000-4000-8000-{n:D12}");
Provenance user = new Provenance(ActorKind.User, "john", at);
Provenance llm = new Provenance(ActorKind.Llm, "claude", at);

MemoryRecord Claim(int n, string scope, string text, Provenance provenance) =>
    new(
        Id(n),
        scope,
        RecordKind.Claim,
        KnowledgeStatus.Active,
        1,
        RecordJson.Write(new ClaimContent(text, MemoryCategory.Semantic, Volatility.Stable)),
        provenance
    );

MemoryRecord Evidence(int n, string scope, Guid claim) =>
    new(
        Id(n),
        scope,
        RecordKind.Evidence,
        KnowledgeStatus.Active,
        1,
        RecordJson.Write(
            new EvidenceContent(claim, EvidenceKind.SourceCode, "src/a.cs", "lineage")
        ),
        user
    );

MemoryRecord Retex(int n, string scope) =>
    new(
        Id(n),
        scope,
        RecordKind.Retex,
        KnowledgeStatus.Active,
        1,
        RecordJson.Write(new RetexContent("Déploiement raté", "Service coupé", "Ajouter un test")),
        user
    );

string Show(string text) =>
    Regex.Replace(
        text.Replace("\r\n", "\n")
            .Replace(root.Replace("\\", "\\\\"), "<root>")
            .Replace(root, "<root>")
            .Replace('\\', '/'),
        "\"timestamp\": \"[^\"]*\"",
        "\"timestamp\": \"<date>\""
    );

JsonArray steps = [];
SqliteStore store = new SqliteStore(root, instance);
void Step(string name, Func<object?> call)
{
    JsonObject row = new() { ["step"] = name };
    try
    {
        object? value = call();
        row["result"] = value switch
        {
            null => null,
            string text => Show(text),
            _ => Show(RecordJson.Write(value)),
        };
    }
    catch (SermofurException exception)
    {
        row["code"] = exception.Code;
        row["message"] = Show(exception.Message);
        row["exitCode"] = exception.ExitCode;
    }
    catch (SqliteException exception)
    {
        row["code"] = exception.SqliteErrorCode is 5 or 6 ? "storage_busy" : "storage_error";
    }
    catch (Exception exception)
    {
        row["exception"] = exception.GetType().FullName;
    }
    steps.Add(row);
}

HashSet<string> all = ["workspace", "acme", "web"];
HashSet<string> acme = ["workspace", "acme"];
Step("scopes_initial", () => store.ReadScopes());
Step(
    "add_client",
    () =>
    {
        store.AddScope(new Scope("acme", ScopeKind.Client, "workspace", "acme"), _ => { });
        return null;
    }
);
Step(
    "add_project",
    () =>
    {
        store.AddScope(new Scope("web", ScopeKind.Project, "acme", "acme/web"), _ => { });
        return null;
    }
);
Step(
    "add_repository_under_workspace",
    () =>
    {
        store.AddScope(new Scope("repo", ScopeKind.Repository, "workspace", "repo"), _ => { });
        return null;
    }
);
Step(
    "add_refused_by_precondition",
    () =>
    {
        store.AddScope(
            new Scope("other", ScopeKind.Client, "workspace", "other"),
            _ => throw new SermofurException("scope_conflict", "Refused by the caller.", 4)
        );
        return null;
    }
);
Step(
    "add_duplicate",
    () =>
    {
        store.AddScope(new Scope("acme", ScopeKind.Client, "workspace", "acme2"), _ => { });
        return null;
    }
);
Step("scopes_after", () => store.ReadScopes());
Step(
    "create_claim_workspace",
    () => store.CreateRecord(Claim(1, "workspace", "Le café est noir & <fort>", user), null)
);
Step(
    "create_claim_acme",
    () => store.CreateRecord(Claim(2, "acme", "La facture part le 5", llm), null)
);
Step("create_retex_web", () => store.CreateRecord(Retex(3, "web"), null));
Step("create_evidence_acme", () => store.CreateRecord(Evidence(4, "acme", Id(2)), null));
Step(
    "create_evidence_other_scope",
    () => store.CreateRecord(Evidence(5, "workspace", Id(2)), null)
);
Step("create_evidence_unknown_claim", () => store.CreateRecord(Evidence(6, "acme", Id(99)), null));
Step("create_evidence_on_retex", () => store.CreateRecord(Evidence(7, "web", Id(3)), null));
Step("create_duplicate_id", () => store.CreateRecord(Claim(1, "workspace", "Autre", user), null));
Step("create_unknown_scope", () => store.CreateRecord(Claim(8, "nowhere", "x", user), null));
Step("read_all", () => store.ReadRecords(all));
Step("read_acme_claims", () => store.ReadRecords(acme, RecordKind.Claim));
Step("read_none", () => store.ReadRecords(new HashSet<string>()));
Step("find_visible", () => store.FindRecord(Id(2), acme));
Step("find_invisible", () => store.FindRecord(Id(3), acme));
Step("history_claim", () => store.ReadHistory(Id(2), acme));
Step("history_invisible", () => store.ReadHistory(Id(3), acme));
Step("invalidate", () => store.InvalidateRecord(Id(2), "acme", "Obsolète", "john"));
Step("invalidate_again", () => store.InvalidateRecord(Id(2), "acme", "Encore", "john"));
Step("invalidate_other_scope", () => store.InvalidateRecord(Id(1), "acme", "x", "john"));
Step("invalidate_retex", () => store.InvalidateRecord(Id(3), "web", "x", "john"));
Step("invalidate_unknown", () => store.InvalidateRecord(Id(98), "acme", "x", "john"));
Step("history_after", () => store.ReadHistory(Id(2), acme));

// Idempotency keys: per scope and operation; the fingerprint ignores the identifier.
Step("idem_create", () => store.CreateRecord(Claim(10, "acme", "Clé", user), "k1"));
Step("idem_replay", () => store.CreateRecord(Claim(10, "acme", "Clé", user), "k1"));
Step("idem_replay_other_id", () => store.CreateRecord(Claim(11, "acme", "Clé", user), "k1"));
Step("idem_conflict", () => store.CreateRecord(Claim(12, "acme", "Autre clé", user), "k1"));
Step("idem_conflict_actor", () => store.CreateRecord(Claim(15, "acme", "Clé", llm), "k1"));
Step("idem_other_scope", () => store.CreateRecord(Claim(13, "workspace", "Clé", user), "k1"));
Step("idem_other_operation", () => store.CreateRecord(Retex(14, "web"), "k1"));
Step("idem_evidence", () => store.CreateRecord(Evidence(16, "acme", Id(10)), "k2"));
Step("idem_evidence_replay", () => store.CreateRecord(Evidence(17, "acme", Id(10)), "k2"));
store.Dispose();

// An idempotency row whose object is gone: the replay finds it orphaned.
using (
    SqliteConnection raw = new SqliteConnection(
        $"Data Source={database};Pooling=False;Foreign Keys=False"
    )
)
{
    raw.Open();
    MemoryRecord orphan = Claim(18, "acme", "Orpheline", user);
    string hash = MarkdownProjection.Hash(
        RecordJson.Write(
            new
            {
                orphan.ScopeId,
                orphan.Kind,
                orphan.ContentJson,
                orphan.Provenance.Origin,
                orphan.Provenance.Actor,
            }
        )
    );
    using SqliteCommand insert = raw.CreateCommand();
    insert.CommandText =
        "INSERT INTO idempotency VALUES('acme','Claim','k3',$hash,'00000000-0000-4000-8000-000000000097')";
    insert.Parameters.AddWithValue("$hash", hash);
    insert.ExecuteNonQuery();
}
store = new SqliteStore(root, instance);
Step("idem_orphan", () => store.CreateRecord(Claim(18, "acme", "Orpheline", user), "k3"));

// Markdown projection: a stale file is rewritten by a replay, export rebuilds every visible
// one, and a file in place of the records folder leaves the record saved but not projected.
string records = Path.Combine(root, ".sermofur", "records");
File.WriteAllText(Path.Combine(records, Id(10) + ".md"), "stale");
Step("idem_replay_rewrites_stale", () => store.CreateRecord(Claim(10, "acme", "Clé", user), "k1"));

// The bytes of three projections before any export, carriage returns shown as <CR>: after the
// stale replay, after an invalidation (revision 2), after a creation.
string Projection(int n) =>
    File.ReadAllText(Path.Combine(records, Id(n) + ".md")).Replace("\r", "<CR>");
Step("projection_after_replay", () => Projection(10));
Step("projection_after_invalidate", () => Projection(2));
Step("projection_after_create", () => Projection(16));
File.Delete(Path.Combine(records, Id(1) + ".md"));
Step("export_acme", () => store.Export(acme));
Directory.Move(records, records + "-moved");
File.WriteAllText(records, "not a folder");
Step(
    "create_projection_pending",
    () => store.CreateRecord(Claim(19, "acme", "Sans projection", user), null)
);
Step("invalidate_projection_pending", () => store.InvalidateRecord(Id(19), "acme", "x", "john"));
Step("export_projection_pending", () => store.Export(acme));
File.Delete(records);
Directory.Move(records + "-moved", records);
Step("export_all", () => store.Export(all));
store.Dispose();

// A scope row whose kind is a number: Enum.Parse accepts it, and ValidateTree refuses it.
using (SqliteConnection raw = new SqliteConnection($"Data Source={database};Pooling=False"))
{
    raw.Open();
    using SqliteCommand insert = raw.CreateCommand();
    insert.CommandText = "INSERT INTO scopes VALUES('odd','7','workspace',NULL)";
    insert.ExecuteNonQuery();
}
store = new SqliteStore(root, instance);
Step("scopes_numeric_kind", () => store.ReadScopes());
Step(
    "validate_numeric_kind",
    () =>
    {
        ScopePolicy.ValidateTree(store.ReadScopes());
        return null;
    }
);
store.Dispose();
using (SqliteConnection raw = new SqliteConnection($"Data Source={database};Pooling=False"))
{
    raw.Open();
    using SqliteCommand update = raw.CreateCommand();
    update.CommandText = "UPDATE scopes SET kind='client' WHERE id='odd'";
    update.ExecuteNonQuery();
}
store = new SqliteStore(root, instance);
Step("scopes_lowercase_kind", () => store.ReadScopes());
store.Dispose();

JsonObject Table(string sql)
{
    using SqliteConnection raw = new SqliteConnection(
        $"Data Source={database};Pooling=False;Mode=ReadOnly"
    );
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
                : JsonValue.Create(Show(reader.GetString(i)))
            );
        }
        rows.Add(row);
    }
    return new JsonObject { ["sql"] = sql, ["rows"] = rows };
}

JsonArray tables =
[
    Table("SELECT key,value FROM metadata ORDER BY key"),
    Table("SELECT id,kind,parent_id,relative_path FROM scopes ORDER BY id"),
    Table("SELECT id,scope_id,kind,claim_id,payload,revision,source_path FROM records ORDER BY id"),
    Table("SELECT sequence,object_id,scope_id,payload FROM history ORDER BY sequence"),
    Table("SELECT object_id,scope_id,kind,passage,length,text FROM search ORDER BY rowid"),
    Table("SELECT scope_id,operation,key,content_hash,object_id FROM idempotency ORDER BY key"),
];

// The projection files: name and text, sorted by name.
JsonObject projections = [];
foreach (
    string file in Directory
        .EnumerateFiles(Path.Combine(root, ".sermofur", "records"))
        .Order(StringComparer.Ordinal)
)
{
    projections[Path.GetFileName(file)] = Show(File.ReadAllText(file));
}
SqliteConnection.ClearAllPools();
Directory.Delete(root, recursive: true);

JsonObject result = new()
{
    ["generator"] = "rust/tools/store-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["steps"] = steps,
    ["tables"] = tables,
    ["projections"] = projections,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{steps.Count} steps");
