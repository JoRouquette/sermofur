// Records what RecordJson of src/Sermofur.Application writes for the domain records and what
// RecordJson.Read makes of well-formed and crafted JSON: each read is written back with
// RecordJson.Write, or its exception type is kept.
// Usage: dotnet run record-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Application;
using Sermofur.Domain;

Guid id = Guid.Parse("0f8fad5b-d9cb-469f-a165-70867728950e");
Guid other = Guid.Parse("7c9e6679-7425-40de-944b-e07fc1f90ae7");
DateTimeOffset at = new DateTimeOffset(2026, 1, 2, 3, 4, 5, 678, TimeSpan.FromHours(2));
DateTimeOffset utc = new DateTimeOffset(2026, 11, 6, 20, 0, 0, TimeSpan.Zero).AddTicks(1234567);

JsonArray writes = [];
void Written(string type, object value) =>
    writes.Add(new JsonObject { ["type"] = type, ["json"] = RecordJson.Write(value) });

foreach (ScopeKind kind in Enum.GetValues<ScopeKind>())
{
    Written(
        "Scope",
        new Scope(
            "s-" + kind,
            kind,
            kind == ScopeKind.Workspace ? null : "workspace",
            kind == ScopeKind.Repository ? "src/app" : null
        )
    );
}
Written("InstanceConfiguration", new InstanceConfiguration(2, id, utc));
foreach (ActorKind actor in Enum.GetValues<ActorKind>())
{
    Written("Provenance", new Provenance(actor, "john <a&b> 'x' `y` +", at));
}
foreach (MemoryCategory category in Enum.GetValues<MemoryCategory>())
{
    foreach (Volatility volatility in Enum.GetValues<Volatility>())
    {
        Written(
            "ClaimContent",
            new ClaimContent(
                "Le café \"noir\" \u00e9t\u00e9 \U0001F600\n\t",
                category,
                volatility,
                category == MemoryCategory.Decisions ? at : null,
                volatility == Volatility.Volatile ? utc : null
            )
        );
    }
}
foreach (EvidenceKind kind in Enum.GetValues<EvidenceKind>())
{
    Written(
        "EvidenceContent",
        new EvidenceContent(
            id,
            kind,
            "ref",
            "lineage",
            kind == EvidenceKind.Execution
                ? EvidenceRelation.Contradicts
                : EvidenceRelation.Supports,
            kind == EvidenceKind.SourceCode ? other : null,
            kind == EvidenceKind.SourceCode ? "ABCDEF" : null
        )
    );
}
foreach (SourceStatus status in Enum.GetValues<SourceStatus>())
{
    Written(
        "SourceContent",
        new SourceContent(
            "docs/a.md",
            status,
            "00FF",
            1234567890123L,
            status == SourceStatus.Indexed ? utc : null,
            3,
            status == SourceStatus.Rejected ? SourceRejection.TooLarge : null
        )
    );
}
foreach (SourceRejection rejection in Enum.GetValues<SourceRejection>())
{
    Written(
        "SourceContent",
        new SourceContent("x", SourceStatus.Rejected, "", 0, null, 0, rejection)
    );
}
Written("RetexContent", new RetexContent("e", "i", "n"));
foreach (RecordKind kind in Enum.GetValues<RecordKind>())
{
    foreach (KnowledgeStatus status in Enum.GetValues<KnowledgeStatus>())
    {
        Written(
            "MemoryRecord",
            new MemoryRecord(
                id,
                "workspace",
                kind,
                status,
                7,
                "{\"a\":1}",
                new Provenance(ActorKind.Llm, "claude", at)
            )
        );
    }
}
Written(
    "HistoryEntry",
    new HistoryEntry(42L, id, "workspace", null, "active", "created", "john", at)
);
Written(
    "HistoryEntry",
    new HistoryEntry(43L, id, "workspace", "active", "invalidated", "why", "john", utc)
);
foreach (ConfidenceLevel level in Enum.GetValues<ConfidenceLevel>())
{
    Written("Confidence", new Confidence(level, ["a", "b"], 2));
}

// Reads: (type, JSON). Each is written back, or its exception type is kept.
const string Evidence =
    "{\"claimId\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"kind\":\"source_code\",\"reference\":\"r\",\"lineageId\":\"l\"";
const string Claim = "{\"text\":\"t\",\"category\":\"semantic\",\"volatility\":\"stable\"";
List<(string Type, string Json)> reads =
[
    ("EvidenceContent", Evidence + "}"),
    (
        "EvidenceContent",
        Evidence
            + ",\"relation\":\"contradicts\",\"sourceId\":\"7c9e6679-7425-40de-944b-e07fc1f90ae7\",\"sourceHash\":\"AB\"}"
    ),
    ("EvidenceContent", Evidence + ",\"relation\":null}"),
    ("EvidenceContent", Evidence + ",\"sourceId\":null,\"sourceHash\":null}"),
    ("EvidenceContent", "{\"kind\":\"execution\",\"reference\":\"r\",\"lineageId\":\"l\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":\"Contradicts\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":\"CONTRADICTS\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":1}"),
    ("EvidenceContent", Evidence + ",\"relation\":\"1\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":\"other\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":\"supports, contradicts\"}"),
    ("EvidenceContent", Evidence + ",\"relation\":\" supports\"}"),
    (
        "EvidenceContent",
        "{\"claimId\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"kind\":\"SourceCode\",\"reference\":\"r\",\"lineageId\":\"l\"}"
    ),
    (
        "EvidenceContent",
        "{\"claimId\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"kind\":\"sourcecode\",\"reference\":\"r\",\"lineageId\":\"l\"}"
    ),
    (
        "EvidenceContent",
        "{\"claimId\":\"0F8FAD5B-D9CB-469F-A165-70867728950E\",\"kind\":\"execution\",\"reference\":\"r\",\"lineageId\":\"l\"}"
    ),
    (
        "EvidenceContent",
        "{\"claimId\":\"{0f8fad5b-d9cb-469f-a165-70867728950e}\",\"kind\":\"execution\",\"reference\":\"r\",\"lineageId\":\"l\"}"
    ),
    (
        "EvidenceContent",
        "{\"claimId\":\"0f8fad5bd9cb469fa16570867728950e\",\"kind\":\"execution\",\"reference\":\"r\",\"lineageId\":\"l\"}"
    ),
    ("EvidenceContent", Evidence + ",\"reference\":\"second\"}"),
    ("EvidenceContent", Evidence + ",\"Reference\":\"case\"}"),
    ("EvidenceContent", Evidence + ",\"extra\":{\"deep\":[1,2]}}"),
    ("ClaimContent", Claim + "}"),
    (
        "ClaimContent",
        Claim + ",\"lastVerified\":\"2026-01-02T03:04:05+02:00\",\"reviewAfter\":\"2026-01-02\"}"
    ),
    ("ClaimContent", Claim + ",\"lastVerified\":\"2026-01-02T03:04:05Z\"}"),
    ("ClaimContent", Claim + ",\"lastVerified\":\"not a date\"}"),
    ("ClaimContent", "{\"category\":\"semantic\",\"volatility\":\"stable\"}"),
    ("ClaimContent", "{\"text\":null,\"category\":\"semantic\",\"volatility\":\"stable\"}"),
    ("ClaimContent", "{\"text\":\"t\"}"),
    ("ClaimContent", "{\"text\":\"t\",\"category\":null,\"volatility\":\"stable\"}"),
    ("ClaimContent", "{\"text\":5,\"category\":\"semantic\",\"volatility\":\"stable\"}"),
    ("ClaimContent", "{\"text\":\"\\ud800\",\"category\":\"semantic\",\"volatility\":\"stable\"}"),
    ("ClaimContent", "{\"text\":\"t\",\"category\":\"semantic\",\"volatility\":\"stable\",}"),
    ("ClaimContent", "{\"text\":\"t\",\"category\":\"semantic\",\"volatility\":\"stable\"} x"),
    ("ClaimContent", "[]"),
    ("ClaimContent", "null"),
    ("ClaimContent", ""),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":5,\"indexedAt\":null,\"passages\":1}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"rejected\",\"hash\":\"H\",\"size\":5,\"indexedAt\":null,\"passages\":1,\"rejection\":\"too_large\"}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":5.0,\"passages\":1}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":1e3,\"passages\":1}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":\"5\",\"passages\":1}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":9223372036854775808,\"passages\":1}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":5,\"passages\":2147483648}"
    ),
    (
        "SourceContent",
        "{\"relativePath\":\"a\",\"status\":\"indexed\",\"hash\":\"H\",\"size\":-5,\"passages\":-1}"
    ),
    ("RetexContent", "{\"event\":\"e\",\"impact\":\"i\",\"nextAction\":\"n\"}"),
    ("RetexContent", "{\"event\":\"e\"}"),
    (
        "MemoryRecord",
        "{\"id\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"scopeId\":\"workspace\",\"kind\":\"claim\",\"status\":\"active\",\"revision\":1,\"contentJson\":\"{}\",\"provenance\":{\"origin\":\"user\",\"actor\":\"a\",\"timestamp\":\"2026-01-02T03:04:05+00:00\"}}"
    ),
    (
        "MemoryRecord",
        "{\"id\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"scopeId\":\"workspace\",\"kind\":\"claim\",\"status\":\"active\",\"revision\":1,\"contentJson\":\"{}\"}"
    ),
    (
        "HistoryEntry",
        "{\"sequence\":1,\"objectId\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"scopeId\":\"w\",\"previousState\":null,\"newState\":\"active\",\"reason\":\"r\",\"actor\":\"a\",\"timestamp\":\"2026-01-02T03:04:05+00:00\"}"
    ),
    // Enumeration lists: an OR that is not a member, and a list that mixes a JSON name and a
    // .NET name.
    (
        "ClaimContent",
        "{\"text\":\"t\",\"category\":\"semantic\",\"volatility\":\"evolving, volatile\"}"
    ),
    (
        "ClaimContent",
        "{\"text\":\"t\",\"category\":\"procedural, semantic\",\"volatility\":\"stable\"}"
    ),
    ("EvidenceContent", Evidence + ",\"kind\":\"source_code, SourceCode\"}"),
    ("EvidenceContent", Evidence + ",\"kind\":\"execution, source_code\"}"),
    // Depth 64 and 65 (the top-level object is depth 1) and a lone surrogate, in a value that
    // no property reads.
    ("RetexContent", "{\"event\":\"e\",\"x\":" + new string('[', 63) + new string(']', 63) + "}"),
    ("RetexContent", "{\"event\":\"e\",\"x\":" + new string('[', 64) + new string(']', 64) + "}"),
    ("RetexContent", "{\"event\":\"e\",\"x\":\"\\ud800\"}"),
];

JsonArray reading = [];
foreach ((string type, string json) in reads)
{
    JsonObject row = new() { ["type"] = type, ["input"] = json };
    try
    {
        object value = type switch
        {
            "EvidenceContent" => RecordJson.Read<EvidenceContent>(json),
            "ClaimContent" => RecordJson.Read<ClaimContent>(json),
            "SourceContent" => RecordJson.Read<SourceContent>(json),
            "RetexContent" => RecordJson.Read<RetexContent>(json),
            "MemoryRecord" => RecordJson.Read<MemoryRecord>(json),
            "HistoryEntry" => RecordJson.Read<HistoryEntry>(json),
            _ => throw new InvalidOperationException(type),
        };
        row["written"] = RecordJson.Write(value);
    }
    catch (Exception exception)
    {
        row["exception"] = exception.GetType().FullName;
        if (exception is SermofurException sermofur)
        {
            row["code"] = sermofur.Code;
        }
    }
    reading.Add(row);
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/record-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["writes"] = writes,
    ["reads"] = reading,
    // The offset of local time on the date-only row (2026-01-02), which the port is given.
    ["localOffsetMinutes"] = TimeZoneInfo.Local.GetUtcOffset(new DateTime(2026, 1, 2)).TotalMinutes,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{writes.Count} writes, {reading.Count} reads");
