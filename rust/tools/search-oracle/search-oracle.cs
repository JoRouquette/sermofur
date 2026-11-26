// Builds an instance with the real SqliteStore of src/Sermofur.Infrastructure (claims, RETEX,
// evidence and sources in several scopes, one claim invalidated), then records the read side of
// the search index: tokenizer output, question terms, hits, corpus statistics, summaries and
// passage texts; then the index writes of SqliteSearchIndex (internal, reached by reflection):
// remove, clear and index records, each followed by a probe. The setup is recorded as actions so
// that the Rust test replays the same script. Nothing is nondeterministic: GUIDs and instants
// are fixed, and history (the only UtcNow) is not recorded.
// Usage: dotnet run search-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

string root = Path.Combine(
    Path.GetTempPath(),
    "sermofur-search-oracle-" + Guid.NewGuid().ToString("N")
);
Directory.CreateDirectory(Path.Combine(root, ".sermofur"));
Guid instance = Guid.Parse("11111111-2222-4333-8444-555555555555");
string database = Path.Combine(root, ".sermofur", "memory.db");
SqliteStore.CreateDatabase(database, instance);

Type index = typeof(SqliteStore).Assembly.GetType("Sermofur.Infrastructure.SqliteSearchIndex")!;
object? Call(string method, params object?[] arguments) =>
    index.GetMethod(method, BindingFlags.Public | BindingFlags.Static)!.Invoke(null, arguments);

Guid Id(int n) => Guid.Parse($"00000000-0000-4000-8000-{n:D12}");
DateTimeOffset At(int day, int offsetMinutes, long extraTicks) =>
    new DateTimeOffset(2026, 3, day, 5, 6, 7, TimeSpan.FromMinutes(offsetMinutes)).AddTicks(
        extraTicks
    );
Provenance By(ActorKind origin, string actor, DateTimeOffset at) => new(origin, actor, at);

MemoryRecord Record(
    int n,
    string scope,
    RecordKind kind,
    KnowledgeStatus status,
    string content,
    Provenance provenance
) => new(Id(n), scope, kind, status, 1, content, provenance);

string ClaimJson(string text) =>
    RecordJson.Write(new ClaimContent(text, MemoryCategory.Semantic, Volatility.Stable));
string RetexJson(string e, string i, string a) => RecordJson.Write(new RetexContent(e, i, a));
string SourceJson(string path, SourceStatus status) =>
    RecordJson.Write(
        new SourceContent(
            path,
            status,
            "ABC",
            12,
            status == SourceStatus.Indexed ? At(9, 0, 0) : null,
            status == SourceStatus.Indexed ? 3 : 0,
            status == SourceStatus.Rejected ? SourceRejection.Binary : null
        )
    );

string longText = string.Join(" ", Enumerable.Range(0, 2500).Select(i => "mot" + (i % 37)));
string repeated = "deploy deploy DEPLOY Deploy déploy Déploiement déploiement DÉPLOIEMENT";

JsonArray setup = [];
SqliteStore store = new SqliteStore(root, instance);

void AddScope(string id, ScopeKind kind, string parent, string path)
{
    store.AddScope(new Scope(id, kind, parent, path), _ => { });
    setup.Add(
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
    setup.Add(new JsonObject { ["op"] = "create", ["payload"] = RecordJson.Write(record) });
}

void Invalidate(int n, string scope)
{
    store.InvalidateRecord(Id(n), scope, "Obsolète", "john");
    setup.Add(
        new JsonObject
        {
            ["op"] = "invalidate",
            ["id"] = Id(n).ToString(),
            ["scope"] = scope,
        }
    );
}

// Source passages, written by another connection as SourceService would through SaveSource.
void InsertPassages(int n, string scope, params string[] texts)
{
    using SqliteConnection raw = new SqliteConnection($"Data Source={database};Pooling=False");
    raw.Open();
    using SqliteTransaction transaction = raw.BeginTransaction();
    SearchDocument[] documents = texts.Select((text, i) => new SearchDocument(i, text)).ToArray();
    Call("Insert", raw, transaction, Id(n), scope, RecordKind.Source, documents, null);
    transaction.Commit();
    JsonArray passages = [];
    foreach (SearchDocument document in documents)
    {
        passages.Add(new JsonArray(document.Passage, document.Text));
    }
    setup.Add(
        new JsonObject
        {
            ["op"] = "insertPassages",
            ["id"] = Id(n).ToString(),
            ["scope"] = scope,
            ["passages"] = passages,
        }
    );
}

Provenance user = By(ActorKind.User, "john", At(4, 60, 0));
AddScope("acme", ScopeKind.Client, "workspace", "acme");
AddScope("web", ScopeKind.Project, "acme", "acme/web");
AddScope("beta", ScopeKind.Client, "workspace", "beta");
Create(
    Record(
        1,
        "workspace",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("Déploiement à chaud"),
        user
    )
);
Create(
    Record(
        2,
        "workspace",
        RecordKind.Retex,
        KnowledgeStatus.Active,
        RetexJson("event", "impact", "next"),
        By(ActorKind.Llm, "claude", At(5, 0, 1234567))
    )
);
Create(
    Record(
        3,
        "acme",
        RecordKind.Claim,
        KnowledgeStatus.Proposed,
        ClaimJson("Élan, CAFÉ-crème; naïve Straße 42 ½ x_y ß"),
        By(ActorKind.User, "john", At(6, -330, 10))
    )
);
Create(
    Record(
        4,
        "acme",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("日本語のテキスト mixed with ASCII"),
        user
    )
);
Create(
    Record(
        5,
        "web",
        RecordKind.Claim,
        KnowledgeStatus.Draft,
        ClaimJson("emoji 🙂 between words"),
        By(ActorKind.Llm, "claude", At(7, 840, 0))
    )
);
Create(
    Record(
        6,
        "web",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("tabs\tand\nlines\r\nend"),
        user
    )
);
Create(Record(7, "beta", RecordKind.Claim, KnowledgeStatus.Active, ClaimJson(repeated), user));
Create(Record(8, "beta", RecordKind.Claim, KnowledgeStatus.Superseded, ClaimJson(longText), user));
Create(
    Record(
        9,
        "acme",
        RecordKind.Retex,
        KnowledgeStatus.Active,
        RetexJson("Déploiement raté", "Service coupé, café renversé", "Ajouter un test v2.1"),
        user
    )
);
Create(
    Record(
        10,
        "acme",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("Le café est noir & <fort>"),
        user
    )
);
Invalidate(10, "acme");
Create(
    Record(
        11,
        "acme",
        RecordKind.Evidence,
        KnowledgeStatus.Active,
        RecordJson.Write(
            new EvidenceContent(Id(3), EvidenceKind.SourceCode, "src/a.cs", "lineage")
        ),
        user
    )
);
Create(
    Record(
        12,
        "web",
        RecordKind.Source,
        KnowledgeStatus.Active,
        SourceJson("acme/web/notes.md", SourceStatus.Indexed),
        user
    )
);
InsertPassages(
    12,
    "web",
    "Première passe : déploiement du café",
    "Deuxième passe 2026-11-06 version 3.14",
    "café café café"
);
Create(
    Record(
        13,
        "beta",
        RecordKind.Source,
        KnowledgeStatus.Active,
        SourceJson("beta/image.bin", SourceStatus.Rejected),
        user
    )
);
Create(
    Record(
        14,
        "beta",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("!? \" -- "),
        By(ActorKind.User, "john", At(8, 0, 9999999))
    )
);
Create(
    Record(
        15,
        "workspace",
        RecordKind.Claim,
        KnowledgeStatus.Active,
        ClaimJson("version 3.14 et 2026-11-06, 1e10 0x1F 007 ²³ Ⅻ ﬁn ＡＢＣ"),
        user
    )
);

JsonObject result = new()
{
    ["generator"] = "rust/tools/search-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["setup"] = setup,
};

// Facts of SearchIndexTests.FullTextSearchIsAvailableAndClaimsAreIndexedOnCreation.
result["facts"] = new JsonObject
{
    ["fts5"] = (long)store.Scalar("SELECT sqlite_compileoption_used('ENABLE_FTS5')"),
    ["match"] = new JsonArray(
        store
            .QueryStrings("SELECT object_id FROM search WHERE search MATCH '\"deploiement\"'")
            .Select(s => (JsonNode)s)
            .ToArray()
    ),
    ["count"] = (long)store.Scalar("SELECT count(*) FROM search"),
};

JsonArray Strings(IEnumerable<string> values) => [.. values.Select(v => (JsonNode)v)];

result["indexTerms"] = Strings(
    store.QueryStrings(
        "SELECT doc, term, offset FROM search_terms WHERE col='text' ORDER BY doc, offset"
    )
);
result["searchRows"] = Strings(
    store.QueryStrings(
        "SELECT rowid, object_id, scope_id, kind, passage, length FROM search ORDER BY rowid"
    )
);

string[] texts =
[
    "",
    " ",
    "Élan, CAFÉ-crème; naïve Straße 42 ½ x_y ß",
    "日本語のテキスト mixed with ASCII",
    "emoji 🙂 between words",
    "tabs\tand\nlines\r\nend",
    "a\" OR \"b",
    "NEAR(a b) AND col:x*",
    "^start - not",
    "!? \" --",
    "Déploiement DÉPLOIEMENT deploiement",
    "Ça Œuvre ÆON ﬁn ﬀ Ǆ ǅ ǆ İstanbul ıi",
    "ἀρχὴ Ἀθῆναι ΣΊΣΥΦΟΣ σίσυφος",
    "Привет Ёлка ёлка",
    "مرحبا بالعالم ١٢٣",
    "שָׁלוֹם עולם",
    "한국어 텍스트 ㄱㄴ",
    "ไทยภาษา",
    "été café ñ",
    "ＡＢＣ ａｂｃ １２３",
    "²³ ¼ Ⅻ ⅻ ① ⓐ",
    "private \U000F0000",
    "zero\u200Bwidth joi\u200Dner non\u00A0breaking",
    "a\u0000b c",
    "under_score dash-dash dot.dot slash/slash apos'trophe",
    "3.14 2026-11-06 1e10 0x1F 007 -5 +7",
    "👩‍💻 🏳️‍🌈 🇫🇷 #hash @at $dollar %",
    "𝔘𝔫𝔦𝔠𝔬𝔡𝔢 𝟙𝟚𝟛",
    "ǅungla DŽ",
    repeated,
    "Ǳ ǲ ǳ ẞ ß ﬆ",
    longText,
    new string('x', 16384),
    string.Concat(Enumerable.Repeat("ab ", 5000)),
];
JsonArray tokenized = [];
foreach (string text in texts)
{
    tokenized.Add(
        new JsonObject { ["text"] = text, ["terms"] = Strings(store.Tokenizer.Tokenize(text)) }
    );
}
result["tokenize"] = tokenized;

Regex bare = new Regex(@"^[\p{L}\p{N}\p{Co}]+$");
string[] questions =
[
    .. texts,
    "a\" OR \"b",
    "Café CAFE cafe café",
    "the a of le la les de",
    string.Join(" ", Enumerable.Range(0, 40).Select(i => "t" + i)),
    string.Join(" ", Enumerable.Range(0, 40).Select(i => "t" + (i % 5))),
    string.Join(" ", Enumerable.Range(0, 31).Select(i => "u" + i)) + " u0 u31 u32 u33",
    "déploiement café 3.14",
];
JsonArray queries = [];
foreach (string question in questions)
{
    IReadOnlyList<string> terms = SearchTerms.Query(store.Tokenizer, question);
    queries.Add(
        new JsonObject
        {
            ["query"] = question,
            ["terms"] = Strings(terms),
            ["bare"] = terms.All(term => bare.IsMatch(term)),
        }
    );
}
result["queries"] = queries;

string[][] visibleSets =
[
    [],
    ["workspace"],
    ["workspace", "acme"],
    ["workspace", "acme", "web"],
    ["workspace", "beta"],
    ["acme", "beta", "web", "workspace"],
    ["web"],
    ["nowhere"],
];
string[] all = ["acme", "beta", "web", "workspace"];

JsonArray HitRows(IReadOnlyList<TermHit> hits) =>
    [
        .. hits.Select(hit =>
            (JsonNode)
                new JsonArray(
                    hit.Document,
                    hit.ObjectId.ToString(),
                    hit.ScopeId,
                    hit.Kind,
                    hit.Passage,
                    hit.Length,
                    hit.Term,
                    hit.Frequency
                )
        ),
    ];

JsonObject Corpus(string[] visible)
{
    VisibleCorpus corpus = store.Corpus(visible.ToHashSet());
    return new JsonObject
    {
        ["visible"] = Strings(visible),
        ["documents"] = corpus.Documents,
        ["averageLength"] = JsonSerializer.Serialize(corpus.AverageLength),
    };
}

JsonObject Hits(string[] terms, string[] visible) =>
    new()
    {
        ["terms"] = Strings(terms),
        ["visible"] = Strings(visible),
        ["hits"] = HitRows(store.Hits(terms, visible.ToHashSet())),
    };

string[][] termSets =
[
    [],
    ["deploiement"],
    ["cafe", "deploiement"],
    ["Cafe"],
    ["deploy", "deploiement", "cafe", "cafe"],
    ["mot0", "mot36", "mot37"],
    ["日本語のテキスト", "mixed", "ascii"],
    ["3", "14", "2026", "x_y", "x", "y"],
    ["passe", "version", "event", "next"],
    ["absent"],
    [.. SearchTerms.Query(store.Tokenizer, "Déploiement CAFÉ, naïve straße ﬁn")],
];
JsonArray hitsOut = [];
foreach (string[] terms in termSets)
{
    foreach (string[] visible in visibleSets)
    {
        hitsOut.Add(Hits(terms, visible));
    }
}
result["hits"] = hitsOut;

JsonArray corpusOut = [];
foreach (string[] visible in visibleSets)
{
    corpusOut.Add(Corpus(visible));
}
result["corpus"] = corpusOut;

JsonArray summariesOut = [];
foreach (string[] visible in visibleSets)
{
    JsonArray rows = [];
    foreach (RecordSummary summary in store.ReadSummaries(visible.ToHashSet()))
    {
        rows.Add(
            new JsonArray(
                summary.Id.ToString(),
                summary.Kind.ToString(),
                summary.Status.ToString(),
                summary.CreatedAt.ToString("o")
            )
        );
    }
    summariesOut.Add(new JsonObject { ["visible"] = Strings(visible), ["rows"] = rows });
}
result["summaries"] = summariesOut;

JsonArray passagesOut = [];
long last = (long)store.Scalar("SELECT max(rowid) FROM search");
foreach (long document in Enumerable.Range(-1, (int)last + 4).Select(i => (long)i))
{
    passagesOut.Add(new JsonArray(document, store.PassageText(document)));
}
passagesOut.Add(new JsonArray(long.MaxValue, store.PassageText(long.MaxValue)));
result["passages"] = passagesOut;

// SearchExpectations.IsIndexed on every kind and source status, and the documents of a record.
JsonArray indexedOut = [];
MemoryRecord[] candidates =
[
    Record(1, "w", RecordKind.Claim, KnowledgeStatus.Invalidated, ClaimJson("x"), user),
    Record(1, "w", RecordKind.Retex, KnowledgeStatus.Active, RetexJson("a", "b", "c"), user),
    Record(1, "w", RecordKind.Evidence, KnowledgeStatus.Active, ClaimJson("x"), user),
    .. Enum.GetValues<SourceStatus>()
        .Select(status =>
            Record(1, "w", RecordKind.Source, KnowledgeStatus.Active, SourceJson("p", status), user)
        ),
];
foreach (MemoryRecord candidate in candidates)
{
    JsonArray documents = [];
    foreach (SearchDocument document in SearchDocuments.For(candidate))
    {
        documents.Add(new JsonArray(document.Passage, document.Text));
    }
    indexedOut.Add(
        new JsonObject
        {
            ["payload"] = RecordJson.Write(candidate),
            ["indexed"] = SearchExpectations.IsIndexed(candidate),
            ["documents"] = documents,
        }
    );
}
result["indexed"] = indexedOut;

// Index writes by another connection; the store sees them at once (its cache key changes).
string[] probeTerms = ["cafe", "deploiement", "mot0", "event", "passe"];
result["probeTerms"] = Strings(probeTerms);
JsonArray writes = [];
void Write(string op, Func<SqliteConnection, SqliteTransaction, object?> call, Guid? id = null)
{
    object? value;
    using (SqliteConnection raw = new SqliteConnection($"Data Source={database};Pooling=False"))
    {
        raw.Open();
        using SqliteTransaction transaction = raw.BeginTransaction();
        value = call(raw, transaction);
        transaction.Commit();
    }
    writes.Add(
        new JsonObject
        {
            ["op"] = op,
            ["id"] = id?.ToString(),
            ["result"] = value is int count ? count : null,
            ["corpus"] = Corpus(all),
            ["hits"] = HitRows(store.Hits(probeTerms, all.ToHashSet())),
            ["searchRows"] = Strings(
                store.QueryStrings(
                    "SELECT rowid, object_id, scope_id, kind, passage, length FROM search ORDER BY rowid"
                )
            ),
        }
    );
}
_ = store.Corpus(all.ToHashSet());
Write("remove", (c, t) => Call("Remove", c, t, Id(3)), Id(3));
Write("remove", (c, t) => Call("Remove", c, t, Id(12)), Id(12));
Write("remove", (c, t) => Call("Remove", c, t, Id(99)), Id(99));
Write("clear", (c, t) => Call("Clear", c, t));
Write("indexRecords", (c, t) => Call("IndexRecords", c, t, null));
result["writes"] = writes;

// The same connection: its own commits change the cache key too (total_changes).
JsonObject own = new() { ["before"] = Corpus(all) };
store.CreateRecord(
    Record(16, "acme", RecordKind.Claim, KnowledgeStatus.Active, ClaimJson("café tardif"), user),
    null
);
own["payload"] = RecordJson.Write(
    Record(16, "acme", RecordKind.Claim, KnowledgeStatus.Active, ClaimJson("café tardif"), user)
);
own["after"] = Corpus(all);
own["hits"] = HitRows(store.Hits(probeTerms, all.ToHashSet()));
result["ownWrite"] = own;

store.Dispose();
SqliteConnection.ClearAllPools();
Directory.Delete(root, recursive: true);

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
Console.WriteLine($"{tokenized.Count} texts, {queries.Count} queries, {hitsOut.Count} hit sets");
