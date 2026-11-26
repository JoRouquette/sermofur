// Records two pure rules of src/Sermofur.Application used by source add and source reindex (lot 5.2):
// - "placements": SourcePlacement.NarrowerScope on crafted and pseudo-random scope trees, for each
//   stored path and each owner scope, compared as LocalPaths.Comparison (without case on Windows
//   only): record on Windows and on Linux;
// - "transitions": the private SourceService.Transition (table of transitions of the data model)
//   for every pair of a stored state and a snapshot state, hash equal or not. A freshly indexed
//   content is stamped now: its indexedAt is checked to be recent, then written as the fixed
//   instant 1999-01-01T00:00:00+00:00 so that the table is stable.
// Each record is the RecordJson text, line breaks as "\n".
// Usage: dotnet run source-service-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Reflection;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Static;
MethodInfo transitionMethod = typeof(SourceService).GetMethod("Transition", Hidden)!;
DateTimeOffset stamp = new DateTimeOffset(1999, 1, 1, 0, 0, 0, TimeSpan.Zero);
DateTimeOffset indexedBefore = new DateTimeOffset(2026, 3, 4, 5, 6, 7, TimeSpan.FromHours(2));
string Json(object value) => RecordJson.Write(value).Replace("\r\n", "\n");

// Placements.
JsonArray placements = [];

void Placement(string name, List<Scope> scopes, IEnumerable<string> paths)
{
    JsonArray cases = [];
    foreach (string path in paths)
    {
        foreach (Scope owner in scopes)
        {
            Scope? narrower = SourcePlacement.NarrowerScope(
                path,
                owner.Id,
                scopes,
                LocalPaths.Comparison
            );
            cases.Add(
                new JsonObject
                {
                    ["path"] = path,
                    ["owner"] = owner.Id,
                    ["narrower"] = narrower?.Id,
                }
            );
        }
    }
    placements.Add(
        new JsonObject
        {
            ["name"] = name,
            ["scopes"] = JsonNode.Parse(Json(scopes)),
            ["cases"] = cases,
        }
    );
}

string[] crafted =
[
    "a/notes.md",
    "A/notes.md",
    "a",
    "a/",
    "ab/notes.md",
    "a/p/x.md",
    "a/P/x.md",
    "a/p",
    "a/pq/x.md",
    "b/x.md",
    "x.md",
    "",
    ".",
    "a/p/r/t/deep.md",
    "é/x.md",
    "É/x.md",
];
Placement(
    "crafted",
    [
        new("workspace", ScopeKind.Workspace, null, null),
        new("a", ScopeKind.Client, "workspace", "a"),
        new("b", ScopeKind.Client, "workspace", "b/"),
        new("p", ScopeKind.Project, "a", "a/p"),
        new("r", ScopeKind.Repository, "p", "a/p"),
        new("t", ScopeKind.Task, "r", "/a/p/r/t/"),
        new("u", ScopeKind.Task, "r", "a/p/r"),
        new("e", ScopeKind.Client, "workspace", "É"),
    ],
    crafted
);
Placement(
    "root mappings",
    [
        new("workspace", ScopeKind.Workspace, null, "/"),
        new("a", ScopeKind.Client, "workspace", ""),
        new("b", ScopeKind.Client, "workspace", "//"),
        new("p", ScopeKind.Project, "a", "A"),
        new("q", ScopeKind.Project, "b", "a"),
    ],
    crafted
);
Random random = new Random(52);
string[] pieces = ["a", "A", "b", "p", "P", "r", "é", "É", "ab", "ss", "ß", "ı", "I"];
string Pick() => pieces[random.Next(pieces.Length)];
string RandomPath(int depth) =>
    string.Join("/", Enumerable.Range(0, depth).Select(_ => Pick()))
    + (random.Next(4) == 0 ? "/" : "");
for (int tree = 0; tree < 40; tree++)
{
    List<Scope> scopes = [new("workspace", ScopeKind.Workspace, null, null)];
    int count = random.Next(1, 9);
    for (int i = 0; i < count; i++)
    {
        // A parent of the expected kind, when there is one.
        Scope parent = scopes[random.Next(scopes.Count)];
        if (parent.Kind == ScopeKind.Task)
        {
            continue;
        }
        ScopeKind kind = parent.Kind + 1;
        string? mapping = random.Next(8) == 0 ? null : RandomPath(random.Next(0, 4));
        scopes.Add(new($"s{i}", kind, parent.Id, mapping));
    }
    List<string> paths = [];
    for (int i = 0; i < 12; i++)
    {
        paths.Add(RandomPath(random.Next(1, 5)));
    }
    Placement($"random {tree}", scopes, paths);
}

// Transitions.
(SourceStatus Status, SourceRejection? Rejection)[] states =
[
    (SourceStatus.Indexed, null),
    (SourceStatus.Missing, null),
    (SourceStatus.Unreadable, null),
    (SourceStatus.Rejected, SourceRejection.TooLarge),
    (SourceStatus.Rejected, SourceRejection.Binary),
    (SourceStatus.Rejected, SourceRejection.Empty),
    (SourceStatus.Rejected, SourceRejection.UnsafePath),
    // Forged: a rejected state without its reason, and a missing state with one.
    (SourceStatus.Rejected, null),
    (SourceStatus.Missing, SourceRejection.Binary),
];
const string Hash = "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881";
const string Other = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
JsonArray transitions = [];
int number = 0;
foreach ((SourceStatus Status, SourceRejection? Rejection) before in states)
{
    foreach ((SourceStatus Status, SourceRejection? Rejection) after in states)
    {
        foreach (string? snapshotHash in new[] { Hash, Other, "" })
        {
            foreach (string? storedHash in new[] { Hash, "" })
            // A stored source without a path (altered base) only matters to an indexed snapshot.
            foreach (
                string? path in after.Status == SourceStatus.Indexed
                    ? new[] { "dossier/Doc.md", null }
                    : new[] { "dossier/Doc.md" }
            )
            {
                number++;
                SourceContent previous = new(
                    path!,
                    before.Status,
                    storedHash,
                    7,
                    before.Status == SourceStatus.Indexed || storedHash.Length > 0
                        ? indexedBefore
                        : null,
                    before.Status == SourceStatus.Indexed ? 2 : 0,
                    before.Rejection
                );
                MemoryRecord current = new(
                    Guid.Parse($"00000000-0000-4000-8000-{number:D12}"),
                    "client-a",
                    RecordKind.Source,
                    KnowledgeStatus.Active,
                    3,
                    RecordJson.Write(previous),
                    new Provenance(ActorKind.Llm, "claude", indexedBefore)
                );
                List<SearchDocument> passages =
                    after.Status == SourceStatus.Indexed
                        ? [new SearchDocument(0, "un"), new SearchDocument(1, "deux")]
                        : [];
                SourceSnapshot snapshot = new(
                    after.Status,
                    snapshotHash,
                    after.Status == SourceStatus.Missing ? 0 : 11,
                    passages,
                    after.Rejection
                );
                object result = transitionMethod.Invoke(null, [current, snapshot, "tester"])!;
                (SourceChange? change, ReindexOutcome outcome) = ((
                    SourceChange?,
                    ReindexOutcome
                ))result;
                JsonNode? changed = null;
                if (change is not null)
                {
                    SourceContent next = RecordJson.Read<SourceContent>(change.Next.ContentJson);
                    if (outcome is ReindexOutcome.Modified or ReindexOutcome.Restored)
                    {
                        if (
                            (DateTimeOffset.UtcNow - next.IndexedAt!.Value).Duration()
                            > TimeSpan.FromMinutes(1)
                        )
                        {
                            throw new InvalidOperationException("indexedAt is not now");
                        }
                        next = next with { IndexedAt = stamp };
                    }
                    MemoryRecord record = change.Next with { ContentJson = RecordJson.Write(next) };
                    changed = new JsonObject
                    {
                        ["next"] = Json(record),
                        ["reason"] = change.Reason,
                        ["actor"] = change.Actor,
                    };
                }
                transitions.Add(
                    new JsonObject
                    {
                        ["current"] = Json(current),
                        ["snapshot"] = new JsonObject
                        {
                            ["status"] = after.Status.ToString(),
                            ["hash"] = snapshotHash,
                            ["size"] = snapshot.Size,
                            ["passages"] = passages.Count,
                            ["rejection"] = after.Rejection?.ToString(),
                        },
                        ["outcome"] = outcome.ToString(),
                        ["change"] = changed,
                    }
                );
            }
        }
    }
}

File.WriteAllText(
    args[0],
    new JsonObject
    {
        ["system"] = OperatingSystem.IsWindows() ? "windows" : "unix",
        ["placements"] = placements,
        ["transitions"] = transitions,
    }.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{placements.Count} placements, {transitions.Count} transitions");
