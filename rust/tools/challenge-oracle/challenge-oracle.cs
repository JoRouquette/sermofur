// Plays challenge scenarios on the real SqliteStore of src/Sermofur.Infrastructure and records what
// ChallengeService of src/Sermofur.Application answers: the five signals (not_applicable,
// review_due, contradiction, source_unavailable, source_changed), their order (status, review
// date, then each evidence in list order, its contradiction before its source signal), the close
// claims to confront, and the refusals. Each scenario runs in a new instance; its steps (scopes,
// records, invalidations, challenges) are recorded in order so that the Rust test replays the same
// script. Answers are the RecordJson text the CLI prints, line breaks as "\n"; a refusal is its
// code, message and exit code; any other exception is its type name. GUIDs and instants are fixed;
// review dates lie far in the past or the future, so the clock never decides.
// Usage: dotnet run challenge-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

Guid instance = Guid.Parse("11111111-2222-4333-8444-555555555555");

Guid Id(int n) => Guid.Parse($"00000000-0000-4000-8000-{n:D12}");
DateTimeOffset At(int day, int offsetMinutes = 0, long extraTicks = 0) =>
    new DateTimeOffset(2026, 3, day, 5, 6, 7, TimeSpan.FromMinutes(offsetMinutes)).AddTicks(
        extraTicks
    );
DateTimeOffset past = new DateTimeOffset(2001, 1, 1, 0, 0, 0, TimeSpan.Zero);
DateTimeOffset pastOffset = new DateTimeOffset(
    2001,
    1,
    1,
    0,
    0,
    0,
    TimeSpan.FromHours(-5)
).AddTicks(1234567);
DateTimeOffset future = new DateTimeOffset(2199, 1, 1, 0, 0, 0, TimeSpan.Zero);
Provenance User(DateTimeOffset at) => new(ActorKind.User, "john", at);
Provenance Llm(DateTimeOffset at) => new(ActorKind.Llm, "claude", at);
const string HashA = "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08";
const string HashB = "60303ae22b998861bce3b28f33eec1be758a213c86c93c076dbe9f558c11c752";

JsonArray scenarios = [];
JsonArray steps = [];
SqliteStore store = null!;
string root = "";

void Scenario(string name, Action body)
{
    root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-challenge-oracle-" + Guid.NewGuid().ToString("N")
    );
    Directory.CreateDirectory(Path.Combine(root, ".sermofur"));
    SqliteStore.CreateDatabase(Path.Combine(root, ".sermofur", "memory.db"), instance);
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

MemoryRecord Raw(
    Guid id,
    string scope,
    RecordKind kind,
    KnowledgeStatus status,
    string content,
    Provenance provenance
) => Create(new MemoryRecord(id, scope, kind, status, 1, content, provenance));

MemoryRecord Claim(
    Guid id,
    string scope,
    string text,
    DateTimeOffset at,
    KnowledgeStatus status = KnowledgeStatus.Proposed,
    DateTimeOffset? review = null
) =>
    Raw(
        id,
        scope,
        RecordKind.Claim,
        status,
        RecordJson.Write(
            new ClaimContent(text, MemoryCategory.Semantic, Volatility.Stable, null, review)
        ),
        User(at)
    );

MemoryRecord Evidence(
    Guid id,
    Guid claim,
    string scope,
    EvidenceKind kind,
    string reference,
    string lineage,
    Provenance provenance,
    EvidenceRelation relation = EvidenceRelation.Supports,
    Guid? source = null,
    string? hash = null
) =>
    Raw(
        id,
        scope,
        RecordKind.Evidence,
        KnowledgeStatus.Proposed,
        RecordJson.Write(
            new EvidenceContent(claim, kind, reference, lineage, relation, source, hash)
        ),
        provenance
    );

MemoryRecord Source(Guid id, string scope, string path, SourceStatus status, string? hash) =>
    Raw(
        id,
        scope,
        RecordKind.Source,
        KnowledgeStatus.Active,
        RecordJson.Write(
            new SourceContent(
                path,
                status,
                hash!,
                4,
                status == SourceStatus.Indexed ? At(9, 120, 5) : null,
                status == SourceStatus.Indexed ? 1 : 0,
                status == SourceStatus.Rejected ? SourceRejection.Binary : null
            )
        ),
        User(At(9))
    );

void Invalidate(Guid id, string scope, string reason)
{
    store.InvalidateRecord(id, scope, reason, "john");
    steps.Add(
        new JsonObject
        {
            ["op"] = "invalidate",
            ["id"] = id.ToString(),
            ["scope"] = scope,
            ["reason"] = reason,
        }
    );
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

ChallengeService Service(string scope)
{
    MemoryContext context = new(root, scope, new HashSet<string>());
    return new ChallengeService(
        new MemoryService(store, context),
        new RecallService(store, store, context)
    );
}

void Challenge(string scope, Guid id) =>
    steps.Add(
        new JsonObject
        {
            ["op"] = "challenge",
            ["scope"] = scope,
            ["id"] = id.ToString(),
            ["answer"] = Answer(() => Service(scope).Challenge(id)),
        }
    );

void ChallengeText(string scope, string text) =>
    steps.Add(
        new JsonObject
        {
            ["op"] = "challengeText",
            ["scope"] = scope,
            ["text"] = text,
            ["answer"] = Answer(() => Service(scope).Challenge(text)),
        }
    );

// ChallengeTests.IndependentContradictionIsSignaledAndCapsConfidence.
Scenario(
    "independent_contradiction",
    () =>
    {
        Claim(Id(1), "workspace", "le build passe en 3 min", At(1));
        Evidence(
            Id(2),
            Id(1),
            "workspace",
            EvidenceKind.SourceCode,
            "pipeline.yml",
            "ci",
            User(At(2))
        );
        Challenge("workspace", Id(1));
        Evidence(
            Id(3),
            Id(1),
            "workspace",
            EvidenceKind.HumanObservation,
            "run 42 : 9 min",
            "observation",
            User(At(3, 120, 42)),
            EvidenceRelation.Contradicts
        );
        Challenge("workspace", Id(1));
        Claim(Id(4), "workspace", "le build passe en 9 min", At(4));
        Challenge("workspace", Id(1));
        Challenge("workspace", Id(4));
    }
);

// ChallengeTests.LlmContradictionIsNotedWithoutRefuting.
Scenario(
    "llm_contradiction",
    () =>
    {
        Claim(Id(1), "workspace", "l'API répond en JSON", At(1));
        Evidence(Id(2), Id(1), "workspace", EvidenceKind.SourceCode, "api.cs", "code", User(At(2)));
        Evidence(
            Id(3),
            Id(1),
            "workspace",
            EvidenceKind.LlmAssertion,
            "elle répond en XML",
            "llm",
            Llm(At(3)),
            EvidenceRelation.Contradicts
        );
        Challenge("workspace", Id(1));
        // Every evidence kind, names in snake case; quotes and line breaks in the reference.
        Claim(Id(10), "workspace", "chaque genre de preuve", At(4));
        int n = 0;
        foreach (EvidenceKind kind in Enum.GetValues<EvidenceKind>())
        {
            Evidence(
                Id(20 + n),
                Id(10),
                "workspace",
                kind,
                $"réf \"{n}\"\r\n<b>&",
                $"lignée {n}",
                n % 2 == 0 ? User(At(5, -90, n)) : Llm(At(5, 330, n)),
                EvidenceRelation.Contradicts
            );
            n++;
        }
        Challenge("workspace", Id(10));
    }
);

// ChallengeTests.FormatOneEvidenceReadsAsSupportWithoutSource: a format 1 payload signals
// nothing; a relation or source written by hand reads as declared.
Scenario(
    "format_one_evidence",
    () =>
    {
        Claim(Id(1), "workspace", "ancien format", At(1));
        Raw(
            Id(2),
            "workspace",
            RecordKind.Evidence,
            KnowledgeStatus.Proposed,
            """{"claimId":"00000000-0000-4000-8000-000000000001","kind":"source_code","reference":"r","lineageId":"l"}""",
            User(At(2))
        );
        Challenge("workspace", Id(1));
        Raw(
            Id(3),
            "workspace",
            RecordKind.Evidence,
            KnowledgeStatus.Proposed,
            """{"claimId":"00000000-0000-4000-8000-000000000001","kind":"Execution","relation":"CONTRADICTS"}""",
            User(At(3))
        );
        Challenge("workspace", Id(1));
    }
);

// ChallengeTests.ChangedAndUnavailableSourcesAreSignaledWithTheirHashes: each state of a cited
// source, on evidence of its own.
Scenario(
    "sources",
    () =>
    {
        Source(Id(1), "workspace", "adr.md", SourceStatus.Indexed, HashA);
        Source(Id(2), "workspace", "docs/changed.md", SourceStatus.Indexed, HashB);
        Source(Id(3), "workspace", "docs/missing.md", SourceStatus.Missing, HashA);
        Source(Id(4), "workspace", "docs/unreadable.md", SourceStatus.Unreadable, HashA);
        Source(Id(5), "workspace", "image.bin", SourceStatus.Rejected, null);
        Source(Id(6), "workspace", "docs/nohash.md", SourceStatus.Indexed, null);
        Claim(Id(10), "workspace", "la base est postgres", At(1));
        Evidence(
            Id(11),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "adr.md",
            "adr",
            User(At(2)),
            source: Id(1),
            hash: HashA
        );
        Challenge("workspace", Id(10));
        Evidence(
            Id(12),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "changed",
            "adr",
            User(At(2)),
            source: Id(2),
            hash: HashA
        );
        Challenge("workspace", Id(10));
        Evidence(
            Id(13),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "missing",
            "doc",
            User(At(2)),
            source: Id(3),
            hash: HashA
        );
        Evidence(
            Id(14),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "unreadable",
            "doc",
            User(At(2)),
            source: Id(4),
            hash: HashA
        );
        Evidence(
            Id(15),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "rejected",
            "doc",
            User(At(2)),
            source: Id(5),
            hash: null
        );
        // Hashes compared as ordinal text, null included.
        Evidence(
            Id(16),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "case",
            "doc",
            User(At(2)),
            source: Id(2),
            hash: HashB.ToUpperInvariant()
        );
        Evidence(
            Id(17),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "unrecorded",
            "doc",
            User(At(2)),
            source: Id(1),
            hash: null
        );
        Evidence(
            Id(18),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "both null",
            "doc",
            User(At(2)),
            source: Id(6),
            hash: null
        );
        Evidence(
            Id(19),
            Id(10),
            "workspace",
            EvidenceKind.LocalDocumentation,
            "now null",
            "doc",
            User(At(2)),
            source: Id(6),
            hash: HashA
        );
        Challenge("workspace", Id(10));
    }
);

// ChallengeTests.EvidenceCitingASourceMovedToANarrowerScopeIsUnavailableWithoutItsPath, an
// unknown source, a source of a sibling, and a cited identifier that is not a source.
Scenario(
    "unreachable_sources",
    () =>
    {
        AddScope("a", ScopeKind.Client, "workspace", "a");
        AddScope("b", ScopeKind.Client, "workspace", "b");
        Source(Id(1), "a", "a/plan.md", SourceStatus.Indexed, HashA);
        Claim(Id(10), "workspace", "le plan existe", At(1));
        Evidence(
            Id(11),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "plan",
            "plan",
            User(At(2)),
            source: Id(1),
            hash: HashA
        );
        Challenge("workspace", Id(10));
        Challenge("a", Id(10));
        Challenge("b", Id(10));
        Evidence(
            Id(12),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "inconnue",
            "plan",
            User(At(2)),
            source: Id(99),
            hash: HashA
        );
        Claim(Id(20), "workspace", "un claim cité comme source", At(3));
        Evidence(
            Id(13),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "claim",
            "plan",
            User(At(2)),
            source: Id(20),
            hash: null
        );
        Evidence(
            Id(14),
            Id(10),
            "workspace",
            EvidenceKind.ProjectDecision,
            "claim haché",
            "plan",
            User(At(2)),
            source: Id(20),
            hash: HashA
        );
        Challenge("workspace", Id(10));
        Challenge("a", Id(10));
    }
);

// ChallengeTests.StatusAndReviewDateAreSignaled, then every status, the history, the review
// date with an offset, and the interleaving of the signals of each evidence.
Scenario(
    "status_and_review",
    () =>
    {
        Claim(Id(1), "workspace", "ancien fait", At(1));
        Invalidate(Id(1), "workspace", "remplacé par la v2");
        Challenge("workspace", Id(1));
        Claim(Id(2), "workspace", "fait à revoir", At(2), review: past);
        Challenge("workspace", Id(2));
        Claim(Id(3), "workspace", "fait à revoir plus tard", At(3), review: future);
        Challenge("workspace", Id(3));
        Claim(Id(4), "workspace", "fait remplacé", At(4), KnowledgeStatus.Superseded, pastOffset);
        Challenge("workspace", Id(4));
        Claim(Id(5), "workspace", "fait invalidé sans motif", At(5), KnowledgeStatus.Invalidated);
        Challenge("workspace", Id(5));
        Invalidate(Id(5), "workspace", "");
        Challenge("workspace", Id(5));
        Claim(Id(6), "workspace", "fait actif", At(6), KnowledgeStatus.Active);
        Claim(Id(7), "workspace", "fait brouillon", At(7), KnowledgeStatus.Draft);
        Challenge("workspace", Id(6));
        Challenge("workspace", Id(7));
        // Evidence in list order: each contradiction before its own source signal.
        Source(Id(30), "workspace", "docs/a.md", SourceStatus.Indexed, HashB);
        Source(Id(31), "workspace", "docs/b.md", SourceStatus.Missing, HashA);
        Claim(Id(8), "workspace", "fait très contesté", At(8), review: past);
        Invalidate(Id(8), "workspace", "trop de doutes");
        Invalidate(Id(8), "workspace", "second motif");
        Evidence(
            Id(43),
            Id(8),
            "workspace",
            EvidenceKind.Execution,
            "e3",
            "x",
            User(At(9)),
            EvidenceRelation.Contradicts,
            Id(31),
            HashA
        );
        Evidence(
            Id(41),
            Id(8),
            "workspace",
            EvidenceKind.Execution,
            "e1",
            "x",
            User(At(9)),
            EvidenceRelation.Supports,
            Id(30),
            HashA
        );
        Evidence(
            Id(42),
            Id(8),
            "workspace",
            EvidenceKind.Execution,
            "e2",
            "y",
            Llm(At(9)),
            EvidenceRelation.Contradicts,
            Id(30),
            HashA
        );
        Evidence(
            Id(44),
            Id(1),
            "workspace",
            EvidenceKind.Execution,
            "autre claim",
            "x",
            User(At(9)),
            EvidenceRelation.Contradicts
        );
        Challenge("workspace", Id(8));
    }
);

// ChallengeTests.CloseClaimsAreToConfrontNeverContradictionsAndNothingIsWritten.
Scenario(
    "close_claims",
    () =>
    {
        Claim(Id(1), "workspace", "le cache redis expire après une heure", At(1));
        for (int index = 0; index < 5; index++)
        {
            Claim(Id(2 + index), "workspace", $"redis expire {index}", At(2 + index));
        }
        Challenge("workspace", Id(1));
        ChallengeText("workspace", "redis expire vite");
        // Never a RETEX or a source; non-applicable claims are left out.
        Raw(
            Id(20),
            "workspace",
            RecordKind.Retex,
            KnowledgeStatus.Draft,
            RecordJson.Write(new RetexContent("orage", "orage", "orage")),
            User(At(9))
        );
        Claim(Id(21), "workspace", "orage orage", At(9), KnowledgeStatus.Invalidated);
        Claim(Id(22), "workspace", "orage prévu", At(9));
        Evidence(Id(23), Id(22), "workspace", EvidenceKind.SourceCode, "meteo", "m", User(At(9)));
        ChallengeText("workspace", "orage");
        Challenge("workspace", Id(22));
        Challenge("workspace", Id(21));
        ChallengeText("workspace", "introuvable");
    }
);

// ChallengeTests.ClaimOfAnotherClientAnswersLikeAnUnknownIdentifier, then other kinds and the
// scopes that see a claim.
Scenario(
    "visibility",
    () =>
    {
        AddScope("a", ScopeKind.Client, "workspace", "a");
        AddScope("b", ScopeKind.Client, "workspace", "b");
        AddScope("p", ScopeKind.Project, "a", "a/p");
        Claim(Id(1), "a", "secret de a", At(1));
        Challenge("b", Id(1));
        Challenge("b", Id(99));
        ChallengeText("b", "secret");
        Challenge("a", Id(1));
        Challenge("p", Id(1));
        Challenge("workspace", Id(1));
        ChallengeText("p", "secret");
        // Evidence shares the scope of its claim; narrower scopes see it too.
        Evidence(
            Id(2),
            Id(1),
            "a",
            EvidenceKind.Execution,
            "test",
            "t",
            User(At(2)),
            EvidenceRelation.Contradicts
        );
        Challenge("a", Id(1));
        Challenge("p", Id(1));
        Challenge("b", Id(1));
        // Other kinds answer like an unknown identifier.
        Source(Id(3), "a", "a/doc.md", SourceStatus.Indexed, HashA);
        Raw(
            Id(4),
            "a",
            RecordKind.Retex,
            KnowledgeStatus.Draft,
            RecordJson.Write(new RetexContent("e", "i", "n")),
            User(At(3))
        );
        Challenge("a", Id(2));
        Challenge("p", Id(2));
        Challenge("a", Id(3));
        Challenge("a", Id(4));
        Challenge("nowhere", Id(1));
        ChallengeText("nowhere", "secret");
    }
);

// Refusals of a text, then a claim whose text holds no searchable term.
Scenario(
    "refusals",
    () =>
    {
        Claim(Id(1), "workspace", "!? \" --", At(1));
        Claim(Id(2), "workspace", "cache chaud", At(2));
        ChallengeText("workspace", "cache");
        ChallengeText("workspace", "");
        ChallengeText("workspace", " \t\n");
        ChallengeText("workspace", "!? \" --");
        ChallengeText("workspace", "a\0b");
        ChallengeText("workspace", new string('x', 16384));
        ChallengeText("workspace", new string('x', 16385));
        ChallengeText("workspace", "\"cache\" OR *");
        Challenge("workspace", Id(1));
        Invalidate(Id(1), "workspace", "vide");
        Challenge("workspace", Id(1));
    }
);

store = null!;
JsonObject result = new()
{
    ["generator"] = "rust/tools/challenge-oracle",
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
