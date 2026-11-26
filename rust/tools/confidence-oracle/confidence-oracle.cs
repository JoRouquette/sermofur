// Records what MemoryService.EvaluateConfidence of src/Sermofur.Application answers for crafted
// and random claims and evidence lists. Each case is written with its inputs (RecordJson of the
// claim and of each piece of evidence) and the Confidence (RecordJson) or the exception type.
// Review dates lie far in the past or the future, so the clock never decides.
// Usage: dotnet run confidence-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Application;
using Sermofur.Domain;

Guid claimId = Guid.Parse("0f8fad5b-d9cb-469f-a165-70867728950e");
DateTimeOffset at = new DateTimeOffset(2026, 1, 2, 3, 4, 5, TimeSpan.FromHours(1));
DateTimeOffset past = new DateTimeOffset(2001, 1, 1, 0, 0, 0, TimeSpan.Zero);
DateTimeOffset future = new DateTimeOffset(2199, 1, 1, 0, 0, 0, TimeSpan.Zero);

MemoryRecord Claim(KnowledgeStatus status, MemoryCategory category, DateTimeOffset? review) =>
    new(
        claimId,
        "workspace",
        RecordKind.Claim,
        status,
        1,
        RecordJson.Write(new ClaimContent("c", category, Volatility.Stable, null, review)),
        new Provenance(ActorKind.User, "john", at)
    );

int counter = 0;
MemoryRecord Evidence(
    EvidenceKind kind,
    ActorKind origin,
    string? lineage,
    EvidenceRelation relation = EvidenceRelation.Supports,
    Guid? id = null,
    TimeSpan? offset = null
) =>
    new(
        id ?? new Guid(counter++, 0, 0, new byte[8]),
        "workspace",
        RecordKind.Evidence,
        KnowledgeStatus.Active,
        1,
        RecordJson.Write(new EvidenceContent(claimId, kind, "ref", lineage!, relation)),
        new Provenance(origin, "a", at.ToOffset(offset ?? TimeSpan.FromHours(1)))
    );

MemoryRecord active = Claim(KnowledgeStatus.Active, MemoryCategory.Semantic, null);
const ActorKind U = ActorKind.User;
const ActorKind L = ActorKind.Llm;
Guid shared = Guid.Parse("7c9e6679-7425-40de-944b-e07fc1f90ae7");

List<(string Name, MemoryRecord Claim, MemoryRecord[] Evidence)> cases =
[
    ("no_evidence", active, []),
    (
        "invalidated",
        Claim(KnowledgeStatus.Invalidated, MemoryCategory.Semantic, null),
        [Evidence(EvidenceKind.SourceCode, U, "l")]
    ),
    ("superseded", Claim(KnowledgeStatus.Superseded, MemoryCategory.Semantic, null), []),
    (
        "review_passed",
        Claim(KnowledgeStatus.Active, MemoryCategory.Semantic, past),
        [Evidence(EvidenceKind.SourceCode, U, "l")]
    ),
    (
        "review_future",
        Claim(KnowledgeStatus.Active, MemoryCategory.Semantic, future),
        [Evidence(EvidenceKind.SourceCode, U, "l")]
    ),
    (
        "proposed",
        Claim(KnowledgeStatus.Proposed, MemoryCategory.Semantic, null),
        [Evidence(EvidenceKind.HumanObservation, U, "l")]
    ),
    ("draft_preferences", Claim(KnowledgeStatus.Draft, MemoryCategory.Preferences, null), []),
    (
        "decisions",
        Claim(KnowledgeStatus.Active, MemoryCategory.Decisions, null),
        [Evidence(EvidenceKind.ProjectDecision, U, "d")]
    ),
    (
        "llm_repeated",
        active,
        [
            Evidence(EvidenceKind.LlmAssertion, L, "x"),
            Evidence(EvidenceKind.LlmAssertion, L, "x"),
            Evidence(EvidenceKind.LlmAssertion, L, "y"),
        ]
    ),
    (
        "llm_self_labels_code",
        active,
        [Evidence(EvidenceKind.SourceCode, L, "x"), Evidence(EvidenceKind.Execution, L, "y")]
    ),
    ("execution_declared", active, [Evidence(EvidenceKind.Execution, U, "e")]),
    (
        "one_lineage_many",
        active,
        [
            Evidence(EvidenceKind.UserAssertion, U, "a"),
            Evidence(EvidenceKind.SourceCode, U, "a"),
            Evidence(EvidenceKind.LocalDocumentation, U, "a"),
        ]
    ),
    (
        "two_lineages",
        active,
        [
            Evidence(EvidenceKind.LocalDocumentation, U, "b"),
            Evidence(EvidenceKind.AuthoritativeDocumentation, U, "a"),
        ]
    ),
    (
        "null_lineage",
        active,
        [
            Evidence(EvidenceKind.HumanObservation, U, null),
            Evidence(EvidenceKind.HumanObservation, U, null),
        ]
    ),
    (
        "lineage_case",
        active,
        [
            Evidence(EvidenceKind.HumanObservation, U, "a"),
            Evidence(EvidenceKind.HumanObservation, U, "A"),
        ]
    ),
    (
        "contradicted_by_user",
        active,
        [
            Evidence(EvidenceKind.SourceCode, U, "a"),
            Evidence(EvidenceKind.HumanObservation, U, "b", EvidenceRelation.Contradicts),
        ]
    ),
    (
        "contradicted_by_llm",
        active,
        [
            Evidence(EvidenceKind.SourceCode, U, "a"),
            Evidence(EvidenceKind.LlmAssertion, L, "b", EvidenceRelation.Contradicts),
        ]
    ),
    (
        "contradicted_by_both",
        active,
        [
            Evidence(EvidenceKind.SourceCode, U, "a"),
            Evidence(EvidenceKind.LlmAssertion, L, "b", EvidenceRelation.Contradicts),
            Evidence(EvidenceKind.UserAssertion, U, "c", EvidenceRelation.Contradicts),
            Evidence(EvidenceKind.UserAssertion, U, "c", EvidenceRelation.Contradicts),
        ]
    ),
    (
        "contradiction_null_lineage",
        active,
        [
            Evidence(EvidenceKind.UserAssertion, U, null, EvidenceRelation.Contradicts),
            Evidence(EvidenceKind.UserAssertion, U, "x", EvidenceRelation.Contradicts),
        ]
    ),
    (
        "only_contradictions",
        active,
        [Evidence(EvidenceKind.SourceCode, U, "a", EvidenceRelation.Contradicts)]
    ),
    (
        "duplicate_record",
        active,
        [
            Evidence(EvidenceKind.SourceCode, L, "a", id: shared),
            Evidence(EvidenceKind.SourceCode, L, "a", id: shared),
        ]
    ),
    (
        "duplicate_other_offset",
        active,
        [
            Evidence(EvidenceKind.SourceCode, L, "a", id: shared),
            Evidence(EvidenceKind.SourceCode, L, "a", id: shared, offset: TimeSpan.FromHours(-5)),
        ]
    ),
    (
        "contradiction_equal_to_support",
        active,
        [
            Evidence(EvidenceKind.SourceCode, U, "a", EvidenceRelation.Contradicts, shared),
            Evidence(EvidenceKind.SourceCode, U, "a", EvidenceRelation.Contradicts, shared),
        ]
    ),
];

Random random = new Random(7);
EvidenceKind[] kinds = Enum.GetValues<EvidenceKind>();
MemoryCategory[] categories = Enum.GetValues<MemoryCategory>();
KnowledgeStatus[] statuses = Enum.GetValues<KnowledgeStatus>();
string?[] lineages = ["a", "b", "c", "A", null];
for (int sample = 0; sample < 400; sample++)
{
    MemoryRecord claim = Claim(
        statuses[random.Next(statuses.Length)],
        categories[random.Next(categories.Length)],
        random.Next(5) switch
        {
            0 => past,
            1 => future,
            _ => null,
        }
    );
    List<MemoryRecord> evidence = [];
    int count = random.Next(0, 7);
    for (int i = 0; i < count; i++)
    {
        if (evidence.Count > 0 && random.Next(6) == 0)
        {
            evidence.Add(evidence[random.Next(evidence.Count)]);
            continue;
        }
        evidence.Add(
            Evidence(
                kinds[random.Next(kinds.Length)],
                random.Next(3) == 0 ? L : U,
                lineages[random.Next(lineages.Length)],
                random.Next(4) == 0 ? EvidenceRelation.Contradicts : EvidenceRelation.Supports
            )
        );
    }
    cases.Add(("random_" + sample, claim, [.. evidence]));
}

JsonArray rows = [];
foreach ((string name, MemoryRecord claim, MemoryRecord[] evidence) in cases)
{
    JsonObject row = new()
    {
        ["name"] = name,
        ["claim"] = RecordJson.Write(claim),
        ["evidence"] = new JsonArray([
            .. evidence.Select(e => JsonValue.Create(RecordJson.Write(e))),
        ]),
    };
    try
    {
        row["confidence"] = RecordJson.Write(MemoryService.EvaluateConfidence(claim, evidence));
    }
    catch (Exception exception)
    {
        row["exception"] = exception.GetType().FullName;
    }
    rows.Add(row);
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/confidence-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["cases"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} cases");
