// Plays recall scenarios on the real SqliteStore of src/Sermofur.Infrastructure and records what
// RecallService of src/Sermofur.Application answers: Recall (BM25 on visible passages, weights,
// order, tie-breaks, complement, excerpt, rounding, errors) and CloseClaims (lot 4.3). Each
// scenario runs in a new instance; its steps (scopes, records, invalidations, source passages,
// questions) are recorded in order so that the Rust test replays the same script. Answers are
// the RecordJson text the CLI prints, line breaks as "\n"; a refusal is its code, message and
// exit code. GUIDs and instants are fixed; review dates lie far in the past or the future, so
// the clock never decides. Excerpts depend on the culture of the machine: record on Windows.
// Usage: dotnet run recall-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

Guid instance = Guid.Parse("11111111-2222-4333-8444-555555555555");
Type index = typeof(SqliteStore).Assembly.GetType("Sermofur.Infrastructure.SqliteSearchIndex")!;

Guid Id(int n) => Guid.Parse($"00000000-0000-4000-8000-{n:D12}");
DateTimeOffset At(int day, int offsetMinutes = 0, long extraTicks = 0) =>
    new DateTimeOffset(2026, 3, day, 5, 6, 7, TimeSpan.FromMinutes(offsetMinutes)).AddTicks(
        extraTicks
    );
DateTimeOffset past = new DateTimeOffset(2001, 1, 1, 0, 0, 0, TimeSpan.Zero);
DateTimeOffset future = new DateTimeOffset(2199, 1, 1, 0, 0, 0, TimeSpan.Zero);
Provenance User(DateTimeOffset at) => new(ActorKind.User, "john", at);
Provenance Llm(DateTimeOffset at) => new(ActorKind.Llm, "claude", at);

string ClaimJson(string text, DateTimeOffset? lastVerified = null, DateTimeOffset? review = null) =>
    RecordJson.Write(
        new ClaimContent(text, MemoryCategory.Semantic, Volatility.Stable, lastVerified, review)
    );

JsonArray scenarios = [];
JsonArray steps = [];
SqliteStore store = null!;
string root = "";
string database = "";

void Scenario(string name, Action body)
{
    root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-recall-oracle-" + Guid.NewGuid().ToString("N")
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

MemoryRecord Create(MemoryRecord record)
{
    store.CreateRecord(record, null);
    steps.Add(new JsonObject { ["op"] = "create", ["payload"] = RecordJson.Write(record) });
    return record;
}

MemoryRecord Claim(
    Guid id,
    string scope,
    string text,
    DateTimeOffset at,
    KnowledgeStatus status = KnowledgeStatus.Proposed,
    DateTimeOffset? review = null,
    DateTimeOffset? lastVerified = null
) =>
    Create(
        new MemoryRecord(
            id,
            scope,
            RecordKind.Claim,
            status,
            1,
            ClaimJson(text, lastVerified, review),
            User(at)
        )
    );

MemoryRecord Retex(
    Guid id,
    string scope,
    string e,
    string i,
    string a,
    DateTimeOffset at,
    KnowledgeStatus status = KnowledgeStatus.Draft
) =>
    Create(
        new MemoryRecord(
            id,
            scope,
            RecordKind.Retex,
            status,
            1,
            RecordJson.Write(new RetexContent(e, i, a)),
            User(at)
        )
    );

MemoryRecord Evidence(
    Guid id,
    Guid claim,
    string scope,
    EvidenceKind kind,
    string lineage,
    Provenance provenance,
    EvidenceRelation relation = EvidenceRelation.Supports
) =>
    Create(
        new MemoryRecord(
            id,
            scope,
            RecordKind.Evidence,
            KnowledgeStatus.Proposed,
            1,
            RecordJson.Write(new EvidenceContent(claim, kind, "ref", lineage, relation)),
            provenance
        )
    );

void Invalidate(Guid id, string scope)
{
    store.InvalidateRecord(id, scope, "Obsolète", "john");
    steps.Add(
        new JsonObject
        {
            ["op"] = "invalidate",
            ["id"] = id.ToString(),
            ["scope"] = scope,
        }
    );
}

// Source passages, written by another connection as SourceService does through SaveSource.
void InsertPassages(Guid id, string scope, IReadOnlyList<SearchDocument> documents)
{
    using (SqliteConnection raw = new SqliteConnection($"Data Source={database};Pooling=False"))
    {
        raw.Open();
        using SqliteTransaction transaction = raw.BeginTransaction();
        index
            .GetMethod("Insert", BindingFlags.Public | BindingFlags.Static)!
            .Invoke(null, [raw, transaction, id, scope, RecordKind.Source, documents, null]);
        transaction.Commit();
    }
    JsonArray passages = [];
    foreach (SearchDocument document in documents)
    {
        passages.Add(new JsonArray(document.Passage, document.Text));
    }
    steps.Add(
        new JsonObject
        {
            ["op"] = "insertPassages",
            ["id"] = id.ToString(),
            ["scope"] = scope,
            ["passages"] = passages,
        }
    );
}

MemoryRecord Source(
    Guid id,
    string scope,
    string path,
    string text,
    DateTimeOffset at,
    SourceStatus status = SourceStatus.Indexed
)
{
    IReadOnlyList<SearchDocument> passages = SourcePassages.Split(text);
    MemoryRecord record = Create(
        new MemoryRecord(
            id,
            scope,
            RecordKind.Source,
            KnowledgeStatus.Active,
            1,
            RecordJson.Write(
                new SourceContent(
                    path,
                    status,
                    "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08",
                    text.Length,
                    status == SourceStatus.Indexed ? At(9, 120, 5) : null,
                    status == SourceStatus.Indexed ? passages.Count : 0,
                    status == SourceStatus.Rejected ? SourceRejection.Binary : null
                )
            ),
            User(at)
        )
    );
    if (status == SourceStatus.Indexed)
    {
        InsertPassages(id, scope, passages);
    }
    return record;
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
}

MemoryContext Context(string scope) => new(root, scope, new HashSet<string>());

void Recall(string scope, string query, int limit = RecallService.MaxResults) =>
    steps.Add(
        new JsonObject
        {
            ["op"] = "recall",
            ["scope"] = scope,
            ["query"] = query,
            ["limit"] = limit,
            ["answer"] = Answer(() =>
                new RecallService(store, store, Context(scope)).Recall(query, limit)
            ),
        }
    );

void Close(string scope, string text, Guid? exclude = null) =>
    steps.Add(
        new JsonObject
        {
            ["op"] = "close",
            ["scope"] = scope,
            ["query"] = text,
            ["exclude"] = exclude?.ToString(),
            ["answer"] = Answer(() =>
                new RecallService(store, store, Context(scope)).CloseClaims(text, exclude)
            ),
        }
    );

// RecallTests.RecallReturnsAtMostThreeExplainedResultsInAStableOrder.
Scenario(
    "stable_order",
    () =>
    {
        for (int n = 0; n < 5; n++)
        {
            Claim(Id(n + 1), "workspace", $"le cache {n} se vide au déploiement", At(1 + n));
        }
        Retex(Id(6), "workspace", "Cache vidé", "Lenteur", "Préchauffer le cache", At(7));
        Recall("workspace", "CACHE déploiement");
        Recall("workspace", "CACHE déploiement");
        Recall("workspace", "cache", 1);
        Recall("workspace", "cache", 2);
        Recall("workspace", "préchauffer lenteur");
    }
);

// RecallTests.ConfidenceCanLiftALessRelevantClaimAboveTheMostRelevantOne.
Scenario(
    "confidence_lift",
    () =>
    {
        Claim(Id(1), "workspace", "le cache se vide", At(1));
        Claim(Id(2), "workspace", "le cache se vide au redémarrage", At(2));
        for (int n = 0; n < 4; n++)
        {
            Claim(
                Id(3 + n),
                "workspace",
                $"le cache {n} se vide rarement selon les mesures du lot",
                At(3 + n)
            );
        }
        Recall("workspace", "cache vide");
        Evidence(Id(10), Id(2), "workspace", EvidenceKind.SourceCode, "code", User(At(10)));
        for (int limit = 1; limit <= RecallService.MaxResults; limit++)
        {
            Recall("workspace", "cache vide", limit);
        }
    }
);

// Each weight, alone on a token of its own, then all together on a shared token.
Scenario(
    "weights",
    () =>
    {
        Claim(Id(1), "workspace", "alpha none", At(1));
        Claim(Id(2), "workspace", "alpha medium", At(1));
        Evidence(Id(12), Id(2), "workspace", EvidenceKind.HumanObservation, "l", User(At(2)));
        Claim(Id(3), "workspace", "alpha high", At(1));
        Evidence(Id(13), Id(3), "workspace", EvidenceKind.SourceCode, "l", User(At(2)));
        Claim(Id(4), "workspace", "alpha capped", At(1));
        Evidence(Id(14), Id(4), "workspace", EvidenceKind.SourceCode, "l", User(At(2)));
        Evidence(
            Id(24),
            Id(4),
            "workspace",
            EvidenceKind.Execution,
            "m",
            User(At(2)),
            EvidenceRelation.Contradicts
        );
        Claim(Id(5), "workspace", "alpha llm", At(1));
        Evidence(Id(15), Id(5), "workspace", EvidenceKind.SourceCode, "l", Llm(At(2)));
        Claim(Id(6), "workspace", "alpha overdue", At(1), review: past);
        Evidence(Id(16), Id(6), "workspace", EvidenceKind.SourceCode, "l", User(At(2)));
        Claim(Id(7), "workspace", "alpha later", At(1), review: future, lastVerified: At(3, 60));
        Evidence(
            Id(17),
            Id(7),
            "workspace",
            EvidenceKind.AuthoritativeDocumentation,
            "l",
            User(At(2))
        );
        Claim(Id(8), "workspace", "alpha draft", At(1), KnowledgeStatus.Draft);
        Evidence(Id(18), Id(8), "workspace", EvidenceKind.ProjectDecision, "l", User(At(2)));
        foreach (
            string token in new[]
            {
                "none",
                "medium",
                "high",
                "capped",
                "llm",
                "overdue",
                "later",
                "draft",
            }
        )
        {
            Recall("workspace", token);
        }
        Recall("workspace", "alpha");
        Recall("workspace", "alpha", 1);
        Recall("workspace", "alpha none medium high");
    }
);

// RecallTests.ContentOfASiblingNeverChangesPresenceOrderOrScoreOfVisibleResults.
Scenario(
    "sibling_isolation",
    () =>
    {
        AddScope("a", ScopeKind.Client, "workspace", "a");
        AddScope("b", ScopeKind.Client, "workspace", "b");
        Claim(Id(1), "a", "migration de la base postgres", At(1));
        Claim(Id(2), "a", "postgres tourne en version 16", At(2));
        Claim(Id(3), "a", "la migration se fait la nuit", At(3));
        Recall("a", "migration postgres");
        for (int n = 0; n < 20; n++)
        {
            Claim(Id(10 + n), "b", "postgres postgres postgres migration", At(4, 0, n));
        }
        Source(Id(40), "b", "b/doc.md", "migration postgres migration postgres", At(5));
        Recall("a", "migration postgres");
        Recall("b", "migration postgres");
        Recall("workspace", "migration postgres");
    }
);

// Visibility follows the ancestors only.
Scenario(
    "nested_scopes",
    () =>
    {
        AddScope("c", ScopeKind.Client, "workspace", "c");
        AddScope("p", ScopeKind.Project, "c", "c/p");
        AddScope("r", ScopeKind.Repository, "p", "c/p/r");
        Claim(Id(1), "workspace", "kappa racine", At(1));
        Claim(Id(2), "c", "kappa client", At(2));
        Claim(Id(3), "p", "kappa projet", At(3));
        Claim(Id(4), "r", "kappa dépôt", At(4));
        Recall("workspace", "kappa");
        Recall("c", "kappa");
        Recall("p", "kappa");
        Recall("r", "kappa");
        Recall("r", "dépôt");
        Recall("p", "dépôt");
    }
);

// RecallTests.NonApplicableKnowledgeIsNeverPresentedAsAFact, then the complement order.
Scenario(
    "not_applicable",
    () =>
    {
        Claim(Id(1), "workspace", "redis redis redis redis cache", At(1));
        Invalidate(Id(1), "workspace");
        Claim(Id(2), "workspace", "le cache utilise valkey, pas redis", At(2));
        Recall("workspace", "redis");
        Recall("workspace", "redis", 1);
        Recall("workspace", "redis", 2);
        Claim(Id(3), "workspace", "redis remplacé", At(3), KnowledgeStatus.Superseded);
        Retex(
            Id(4),
            "workspace",
            "redis tombé",
            "panne",
            "surveiller",
            At(4),
            KnowledgeStatus.Superseded
        );
        Claim(Id(5), "workspace", "redis redis encore", At(5), KnowledgeStatus.Invalidated);
        Recall("workspace", "redis");
        Recall("workspace", "redis", 1);
        Recall("workspace", "redis", 2);
        Recall("workspace", "panne");
        Close("workspace", "redis");
    }
);

// RecallTests.SourcesComeBackWithTheirBestPassageAndFreshness, plus several passages.
Scenario(
    "sources",
    () =>
    {
        string filler = string.Join("\n\n", Enumerable.Repeat(new string('x', 1500), 3));
        Source(
            Id(1),
            "workspace",
            "guide.md",
            filler + "\n\nLe préchauffage du cache prend dix minutes.\n\n" + filler,
            At(1)
        );
        Recall("workspace", "prechauffage");
        Source(
            Id(2),
            "workspace",
            "notes/multi.md",
            string.Join(
                "\n\n",
                Enumerable.Range(0, 6).Select(n => new string('y', 1200) + $" omega{n % 3} delta")
            ),
            At(2)
        );
        Source(Id(3), "workspace", "image.bin", "omega omega", At(3), SourceStatus.Rejected);
        Claim(Id(4), "workspace", "omega1 delta en clair", At(4));
        Retex(Id(5), "workspace", "omega1", "delta", "rien", At(5));
        // Passages of an object that has no record: never a result.
        InsertPassages(Id(6), "workspace", [new SearchDocument(0, "omega1 orphelin")]);
        Recall("workspace", "omega1");
        Recall("workspace", "omega2 delta");
        Recall("workspace", "delta", 2);
        Close("workspace", "omega1 delta");
        Close("workspace", "omega1 delta", Id(4));
    }
);

// RecallTests.QuestionSyntaxIsPlainText.
Scenario(
    "plain_text",
    () =>
    {
        Claim(Id(1), "workspace", "cache chaud", At(1));
        Recall("workspace", "\"cache\" OR *");
        Recall("workspace", "NEAR(cache, x) AND col:text");
        Recall("workspace", "introuvable");
        Recall("workspace", "cache^2 -chaud \"chaud");
    }
);

// Refusals and their order: text before limit, terms after limit.
Scenario(
    "refusals",
    () =>
    {
        Recall("workspace", "cache");
        Recall("workspace", "");
        Recall("workspace", " \t\n");
        Recall("workspace", "!? \" --");
        Recall("workspace", "!? \" --", 0);
        Recall("workspace", "", 0);
        Recall("workspace", "cache", 0);
        Recall("workspace", "cache", 4);
        Recall("workspace", "cache", -1);
        Recall("workspace", "a\0b");
        Recall("workspace", new string('x', 16384));
        Recall("workspace", new string('x', 16385));
        Recall("nowhere", "cache");
        Close("workspace", "");
        Close("workspace", "!?");
        Close("workspace", "cache");
    }
);

// Equal scores: most recent first, then identifier (Guid.CompareTo), creation compared as an
// instant.
Scenario(
    "ties",
    () =>
    {
        Claim(Guid.Parse("80000000-0000-4000-8000-000000000001"), "workspace", "tie one", At(1));
        Claim(Guid.Parse("7fffffff-0000-4000-8000-000000000001"), "workspace", "tie one", At(1));
        Claim(
            Guid.Parse("00000001-0000-4000-8000-000000000001"),
            "workspace",
            "tie one",
            At(1, 60)
        );
        Claim(Guid.Parse("00000000-ffff-4000-8000-000000000001"), "workspace", "tie one", At(1));
        Recall("workspace", "tie one");
        Claim(Guid.Parse("00000002-0000-4000-8000-000000000001"), "workspace", "tie two", At(2));
        Claim(
            Guid.Parse("00000002-0000-4000-8000-000000000002"),
            "workspace",
            "tie two",
            At(2, -120, 72000000000)
        );
        Claim(Guid.Parse("00000002-0000-4000-ff00-000000000001"), "workspace", "tie two", At(2));
        Claim(
            Guid.Parse("00000002-0000-4000-8000-ff0000000001"),
            "workspace",
            "tie two",
            At(2, 0, 1)
        );
        Claim(Guid.Parse("00000002-0000-c000-8000-000000000001"), "workspace", "tie two", At(2));
        Recall("workspace", "tie two");
        Recall("workspace", "two", 2);
        Retex(
            Guid.Parse("00000003-0000-4000-8000-000000000001"),
            "workspace",
            "tie",
            "two",
            "x",
            At(2, 0, 1)
        );
        Recall("workspace", "tie two");
        Close("workspace", "tie two");
    }
);

// Early stop of the lazy weighting: lifted claims far down the order of relevance.
Scenario(
    "early_stop",
    () =>
    {
        for (int n = 0; n < 30; n++)
        {
            string text =
                "zeta "
                + string.Join(" ", Enumerable.Repeat("pad", n))
                + (n % 4 == 0 ? " zeta" : "");
            Claim(Id(n + 1), "workspace", text, At(1 + n % 20, 0, n));
            if (n % 3 == 0)
            {
                Evidence(
                    Id(100 + n),
                    Id(n + 1),
                    "workspace",
                    EvidenceKind.SourceCode,
                    "l",
                    User(At(25))
                );
            }
            if (n % 5 == 0)
            {
                Evidence(
                    Id(200 + n),
                    Id(n + 1),
                    "workspace",
                    EvidenceKind.HumanObservation,
                    "k",
                    User(At(25))
                );
            }
        }
        for (int limit = 1; limit <= RecallService.MaxResults; limit++)
        {
            Recall("workspace", "zeta", limit);
            Recall("workspace", "zeta pad", limit);
        }
        Close("workspace", "zeta pad");
    }
);

// Excerpts: 300 UTF-16 units around the first matched term, line breaks flattened, case and
// accents ignored, surrogate pairs never cut. Each claim holds a token of its own.
string Pad(int count) => new string('.', count);
string[] excerpts =
[
    "short text with exc0 inside\r\nand\nlines",
    Pad(150) + " exc1 " + Pad(150),
    Pad(400) + " exc2 " + Pad(400),
    "exc3 " + Pad(400),
    Pad(600) + " exc4",
    Pad(295) + "exc5",
    Pad(296) + "exc6",
    Pad(200) + "\r\n" + Pad(200) + " exc7 \n\n" + Pad(200),
    Pad(300) + " Préchauffage EXC8 " + Pad(300),
    Pad(300) + " PRÉCHAUFFAGE exc9 préchauffage " + Pad(300),
    // A surrogate pair at the start of the window, then at its end.
    Pad(299) + "🙂" + " exc10 " + Pad(400),
    Pad(300) + " exc11 " + Pad(193) + "🙂" + Pad(300),
    Pad(300) + " exc12 " + Pad(192) + "🙂" + Pad(300),
    "🙂🙂🙂" + Pad(400) + " exc13 " + Pad(5) + "🙂🙂",
    // The first matched term in the order of the hits, not of the question.
    Pad(500) + " exd14 " + Pad(50) + " exa14 " + Pad(500),
    // A term found inside a longer word before its own occurrence.
    Pad(350) + " preexc15 " + Pad(300) + " exc15 " + Pad(300),
    // Decomposed accents and uppercase.
    Pad(350) + " Café EXC16 " + Pad(350),
    Pad(350) + " ÇA exc17 " + Pad(350),
    Pad(350) + " İstanbul exc18 " + Pad(350),
    // Exotic: ligatures, sharp s, full width, soft hyphen, Kelvin sign, dotless i.
    Pad(350) + " ﬁnal fin exc19 " + Pad(350) + " fin",
    Pad(350) + " Straße strasse exc20 " + Pad(350),
    Pad(350) + " ｅｘｃ２１ exc21 " + Pad(350),
    Pad(350) + " ex\u00ADc22 exc22 " + Pad(350),
    Pad(350) + " \u212Aelvin kelvin exc23 " + Pad(350),
    Pad(350) + " æther aether exc24 " + Pad(350),
    Pad(350) + " ıi exc25 " + Pad(350),
    Pad(350) + " Ǆungla džungla exc26 " + Pad(350),
    Pad(350) + " zero\u200Bwidth zerowidth exc27 " + Pad(350),
    Pad(350) + " ὈΔΥΣΣΕΎΣ οδυσσευς exc28 " + Pad(350),
    Pad(350) + " Ёлка елка exc29 " + Pad(350),
];
Scenario(
    "excerpts",
    () =>
    {
        for (int n = 0; n < excerpts.Length; n++)
        {
            Claim(Id(n + 1), "workspace", excerpts[n], At(1, 0, n));
        }
        string[] questions =
        [
            "exc0",
            "exc1",
            "exc2",
            "exc3",
            "exc4",
            "exc5",
            "exc6",
            "exc7",
            "prechauffage exc8",
            "exc9 prechauffage",
            "exc10",
            "exc11",
            "exc12",
            "exc13",
            "exd14 exa14",
            "exa14 exd14",
            "exc15",
            "cafe exc16",
            "ca exc17",
            "istanbul exc18",
            "fin exc19",
            "strasse exc20",
            "exc21",
            "exc22",
            "kelvin exc23",
            "aether exc24",
            "i exc25",
            "džungla exc26",
            "zerowidth exc27",
            "οδυσσευς exc28",
            "елка exc29",
        ];
        foreach (string question in questions)
        {
            Recall("workspace", question, 1);
        }
    }
);

// Excerpts over the collation table (equal forms found before the plain term), then cases that
// depend on the neighbours of a character: contractions, two-part and preposed vowels, grapheme
// boundaries.
(string Text, string Question)[] collation =
[
    ("Œuvre oeuvre col1", "oeuvre col1"),
    ("Łódź lodz col2", "lodz col2"),
    ("Ørsted orsted col3", "orsted col3"),
    ("Đakovo dakovo col4", "dakovo col4"),
    ("ĳs ijs col5", "ijs col5"),
    ("ﬃx ffix col6", "ffix col6"),
    ("ſoup soup col7", "soup col7"),
    ("ǲemal dzemal col8", "dzemal col8"),
    ("豈 豈 col9", "豈 col9"),
    ("ΣΊΣΥΦΟΣ σισυφος col10", "σισυφος col10"),
    ("x\U0001F469\u200Dab ab col11", "ab col11"),
    ("ки\u0306т кит ctx1", "кит ctx1"),
    ("با\u0654ب باب ctx2", "باب ctx2"),
    ("ক\u09C7\u09BE ক\u09C7 ctx3", "ক\u09C7 ctx3"),
    ("เก ก ctx4", "ก ctx4"),
    ("x\u1DD5y ao ctx5", "ao ctx5"),
    ("\u0601ab ab ctx6", "ab ctx6"),
    ("\u1100\u1161\u11A8 가 ctx7", "가 ctx7"),
];
Scenario(
    "collation",
    () =>
    {
        for (int n = 0; n < collation.Length; n++)
        {
            Claim(
                Id(n + 1),
                "workspace",
                Pad(350) + " " + collation[n].Text + " " + Pad(350),
                At(1, 0, n)
            );
        }
        foreach ((string _, string question) in collation)
        {
            Recall("workspace", question, 1);
        }
    }
);

// Questions of many terms: 32 distinct terms at most, in their order of appearance.
Scenario(
    "many_terms",
    () =>
    {
        Claim(Id(1), "workspace", "t0 t5 t31", At(1));
        Claim(Id(2), "workspace", "t32 t39", At(2));
        Claim(Id(3), "workspace", "t5 t5 t5", At(3));
        Recall("workspace", string.Join(" ", Enumerable.Range(0, 40).Select(i => "t" + i)));
        Recall("workspace", string.Join(" ", Enumerable.Range(0, 40).Select(i => "t" + (39 - i))));
        Recall("workspace", "T5 t5 T0 t0");
    }
);

store = null!;
JsonObject result = new()
{
    ["generator"] = "rust/tools/recall-oracle",
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
