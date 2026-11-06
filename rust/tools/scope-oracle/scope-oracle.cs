// Records what ScopePolicy of src/Sermofur.Domain answers: ExpectedParent of each kind, then
// ValidateTree and VisibleAncestors of every identifier (plus one unknown) on crafted trees and
// on random trees, which are written with their answers.
// Usage: dotnet run scope-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;

static Scope S(string id, ScopeKind kind, string? parent, string? path = null) =>
    new(id, kind, parent, path);

const ScopeKind W = ScopeKind.Workspace;
const ScopeKind C = ScopeKind.Client;
const ScopeKind P = ScopeKind.Project;
const ScopeKind R = ScopeKind.Repository;
const ScopeKind T = ScopeKind.Task;

List<(string Name, Scope[] Tree)> trees =
[
    (
        "documented",
        [
            S("workspace", W, null),
            S("a", C, "workspace", "a"),
            S("b", C, "workspace", "b"),
            S("ap", P, "a", "a/project"),
        ]
    ),
    (
        "full_chain",
        [
            S("workspace", W, null),
            S("c", C, "workspace", "c"),
            S("p", P, "c", "c/p"),
            S("r", R, "p", "c/p/r"),
            S("t", T, "r", "c/p/r/t"),
        ]
    ),
    ("cycle_without_root", [S("a", C, "b", "a"), S("b", P, "a", "b")]),
    ("repository_under_workspace", [S("workspace", W, null), S("repo", R, "workspace", "repo")]),
    ("empty", []),
    ("only_root", [S("workspace", W, null)]),
    ("root_with_path", [S("workspace", W, null, "x")]),
    ("root_wrong_id", [S("root", W, null)]),
    ("root_wrong_kind", [S("workspace", C, null)]),
    ("root_id_case", [S("Workspace", W, null)]),
    ("two_roots", [S("workspace", W, null), S("other", W, null)]),
    ("duplicate_id", [S("workspace", W, null), S("a", C, "workspace"), S("a", C, "workspace")]),
    (
        "ids_differ_by_case",
        [S("workspace", W, null), S("a", C, "workspace"), S("A", C, "workspace")]
    ),
    ("parent_case", [S("workspace", W, null), S("a", C, "workspace"), S("ap", P, "A")]),
    ("missing_parent", [S("workspace", W, null), S("a", C, "nowhere")]),
    ("self_parent", [S("workspace", W, null), S("a", C, "a")]),
    ("cycle_beside_root", [S("workspace", W, null), S("a", C, "b"), S("b", C, "a")]),
    (
        "task_under_project",
        [S("workspace", W, null), S("c", C, "workspace"), S("p", P, "c"), S("t", T, "p")]
    ),
    ("client_under_client", [S("workspace", W, null), S("c", C, "workspace"), S("d", C, "c")]),
    ("workspace_kind_child", [S("workspace", W, null), S("w2", W, "workspace")]),
    ("undefined_kind", [S("workspace", W, null), S("x", (ScopeKind)7, "workspace")]),
    ("undefined_root_kind", [S("workspace", (ScopeKind)9, null)]),
    ("order_child_first", [S("ap", P, "a"), S("a", C, "workspace"), S("workspace", W, null)]),
    (
        "hierarchy_before_cycle",
        [S("workspace", W, null), S("r", R, "workspace"), S("a", C, "b"), S("b", C, "a")]
    ),
    (
        "cycle_before_hierarchy",
        [S("workspace", W, null), S("a", C, "b"), S("b", C, "a"), S("r", R, "workspace")]
    ),
    ("empty_id", [S("workspace", W, null), S("", C, "workspace")]),
];

Random random = new Random(42);
string[] pool = ["workspace", "a", "b", "c", "A", "x", ""];
for (int sample = 0; sample < 300; sample++)
{
    int count = random.Next(0, 9);
    List<Scope> tree = [];
    for (int i = 0; i < count; i++)
    {
        string id = random.Next(3) == 0 ? pool[random.Next(pool.Length)] : "s" + random.Next(12);
        ScopeKind kind = (ScopeKind)random.Next(0, 6);
        string? parent = random.Next(4) switch
        {
            0 => null,
            1 => pool[random.Next(pool.Length)],
            _ => tree.Count > 0 ? tree[random.Next(tree.Count)].Id : "workspace",
        };
        string? path = random.Next(3) == 0 ? null : id;
        tree.Add(S(id, kind, parent, path));
    }
    if (sample % 3 == 0)
    {
        tree.Insert(0, S("workspace", W, null));
    }
    trees.Add(("random_" + sample, [.. tree]));
}

JsonObject Outcome(Func<object?> call)
{
    try
    {
        object? value = call();
        return value is IReadOnlySet<string> set
            ? new JsonObject
            {
                ["visible"] = new JsonArray([
                    .. set.Order(StringComparer.Ordinal).Select(id => JsonValue.Create(id)),
                ]),
            }
            : new JsonObject { ["ok"] = true };
    }
    catch (SermofurException exception)
    {
        return new JsonObject
        {
            ["code"] = exception.Code,
            ["message"] = exception.Message,
            ["exitCode"] = exception.ExitCode,
        };
    }
    catch (Exception exception)
    {
        return new JsonObject { ["exception"] = exception.GetType().FullName };
    }
}

JsonArray rows = [];
foreach ((string name, Scope[] tree) in trees)
{
    JsonArray scopes = [];
    foreach (Scope scope in tree)
    {
        scopes.Add(
            new JsonObject
            {
                ["id"] = scope.Id,
                ["kind"] = (int)scope.Kind,
                ["parentId"] = scope.ParentId,
                ["relativePath"] = scope.RelativePath,
            }
        );
    }
    JsonObject visible = [];
    foreach (string id in tree.Select(s => s.Id).Append("unknown").Distinct())
    {
        visible[id] = Outcome(() => ScopePolicy.VisibleAncestors(id, tree));
    }
    rows.Add(
        new JsonObject
        {
            ["name"] = name,
            ["scopes"] = scopes,
            ["validate"] = Outcome(() =>
            {
                ScopePolicy.ValidateTree(tree);
                return null;
            }),
            ["visibleAncestors"] = visible,
        }
    );
}

JsonArray parents = [];
foreach (int kind in Enumerable.Range(0, 7))
{
    parents.Add(
        new JsonObject
        {
            ["kind"] = kind,
            ["expected"] = Outcome(() =>
                ScopePolicy.ExpectedParent((ScopeKind)kind) is { } p
                    ? new HashSet<string> { ((int)p).ToString() }
                    : new HashSet<string>()
            ),
        }
    );
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/scope-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["expectedParent"] = parents,
    ["trees"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} trees");
