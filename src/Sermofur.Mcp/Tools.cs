using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Domain;

namespace Sermofur.Mcp;

/// <summary>Runs one smf command through the daemon; bridge failures are SermofurExceptions.</summary>
public interface ICommandChannel
{
    Task<CommandOutcome> RunAsync(IReadOnlyList<string> argv, CancellationToken cancellation);
}

/// <summary>Context of one call: validated arguments, actor of the session, channel.</summary>
public sealed record ToolCall(ToolArguments Arguments, string Actor, ICommandChannel Channel);

/// <summary>One tool of the contract (contracts/mcp-tools.md).</summary>
public sealed record ToolDefinition(
    string Name,
    string Description,
    IReadOnlyList<ToolField> Fields,
    Func<ToolCall, CancellationToken, Task<JsonNode>> Execute,
    bool ReadOnly = false
)
{
    /// <summary>JSON Schema of the input: closed object, from the fields.</summary>
    public JsonElement InputSchema()
    {
        JsonObject properties = [];
        foreach (ToolField field in Fields)
        {
            properties[field.Name] = field.Schema();
        }
        JsonObject schema = new JsonObject
        {
            ["type"] = "object",
            ["properties"] = properties,
            ["additionalProperties"] = false,
        };
        string[] required = [.. Fields.Where(field => field.Required).Select(field => field.Name)];
        if (required.Length > 0)
        {
            schema["required"] = new JsonArray([
                .. required.Select(name => JsonValue.Create(name)),
            ]);
        }
        return JsonSerializer.SerializeToElement(schema);
    }
}

/// <summary>
/// The eight tools. Each becomes smf commands run by the daemon in the scope of the launch folder;
/// writes carry <c>--origin llm</c> and the actor of the session, never values of the call
/// (research R2, R4, R6).
/// </summary>
public static class Tools
{
    private static readonly string[] Categories = ["episodic", "semantic", "procedural"];
    private static readonly string[] Volatilities = ["stable", "evolving", "volatile"];
    private static readonly string[] EvidenceKinds =
    [
        "execution",
        "source_code",
        "authoritative_documentation",
        "project_decision",
        "local_documentation",
        "human_observation",
        "user_assertion",
        "llm_assertion",
    ];
    private static readonly string[] Verdicts = ["helpful", "not_applicable", "wrong"];

    private static readonly ToolField Key = new ToolField(
        "key",
        FieldKind.Text,
        false,
        "Idempotency key: the same key with the same content returns the same object."
    );

    public static IReadOnlyList<ToolDefinition> All { get; } =
    [
        new(
            "sermofur_status",
            "Identity of the Sermofur instance of this project, current scope and counts of the visible objects.",
            [],
            (call, cancellation) => Json(call, ["status", "--json"], cancellation),
            ReadOnly: true
        ),
        new(
            "sermofur_context",
            "Context of this session: instance, current scope with its visible ancestors, and counts. No content.",
            [],
            Context,
            ReadOnly: true
        ),
        new(
            "sermofur_recall",
            "Recalls at most 3 explained results (claims, RETEX, source passages) relevant to a question, among what this project may see. Read only.",
            [
                new ToolField(
                    "question",
                    FieldKind.Text,
                    true,
                    "The question, in plain words.",
                    AsOption: false
                ),
                new ToolField(
                    "limit",
                    FieldKind.Limit,
                    false,
                    "Number of results, 1 to 3. Default: 3."
                ),
            ],
            Recall,
            ReadOnly: true
        ),
        new(
            "sermofur_challenge",
            "Confronts a claim (claimId) or a text not recorded yet (text) with contradictions, changed sources, status and close claims. Never decides; writes nothing. Give exactly one of claimId and text.",
            [
                new ToolField("claimId", FieldKind.Guid, false, "Identifier of a visible claim."),
                new ToolField(
                    "text",
                    FieldKind.Text,
                    false,
                    "A statement to confront; cannot start with --."
                ),
            ],
            Challenge,
            ReadOnly: true
        ),
        new(
            "sermofur_claim",
            "Proposes a claim in the current scope. It is recorded as coming from an LLM: its confidence only grows with independent, non-LLM evidence.",
            [
                new ToolField("text", FieldKind.Text, true, "The statement.", AsOption: false),
                new ToolField(
                    "category",
                    FieldKind.Choice,
                    false,
                    "Default: semantic.",
                    Categories
                ),
                new ToolField(
                    "volatility",
                    FieldKind.Choice,
                    false,
                    "Default: evolving.",
                    Volatilities
                ),
                Key,
            ],
            Claim
        ),
        new(
            "sermofur_evidence",
            "Records evidence for a visible claim (or against it with contradicts). Evidence declared by an LLM never reinforces a claim. lineage and key cannot start with --.",
            [
                new ToolField(
                    "claimId",
                    FieldKind.Guid,
                    true,
                    "Identifier of a visible claim.",
                    AsOption: false
                ),
                new ToolField(
                    "kind",
                    FieldKind.Choice,
                    true,
                    "Kind of evidence.",
                    EvidenceKinds,
                    AsOption: false
                ),
                new ToolField(
                    "reference",
                    FieldKind.Text,
                    true,
                    "What supports it: a path, a link, a short description.",
                    AsOption: false
                ),
                new ToolField(
                    "lineage",
                    FieldKind.Text,
                    true,
                    "Common origin of related evidence; evidence of one lineage counts once."
                ),
                new ToolField(
                    "contradicts",
                    FieldKind.Flag,
                    false,
                    "True when the evidence goes against the claim."
                ),
                new ToolField(
                    "sourceId",
                    FieldKind.Guid,
                    false,
                    "Identifier of a visible source the evidence cites."
                ),
                Key,
            ],
            Evidence
        ),
        new(
            "sermofur_record_retex",
            "Records a draft RETEX (lesson learned): what happened, its impact, what to do next. A draft never changes confidence or recall. Values cannot start with --.",
            [
                new ToolField("event", FieldKind.Text, true, "What happened."),
                new ToolField("impact", FieldKind.Text, true, "Its consequence."),
                new ToolField("next", FieldKind.Text, true, "What to do next time."),
                Key,
            ],
            Retex
        ),
        new(
            "sermofur_feedback",
            "Says whether a recalled object helped, did not apply or was wrong. Kept as a draft RETEX marked as feedback, with no effect on confidence or ranking.",
            [
                new ToolField(
                    "targetId",
                    FieldKind.Guid,
                    true,
                    "Identifier of a visible claim, RETEX or source."
                ),
                new ToolField("verdict", FieldKind.Choice, true, "The verdict.", Verdicts),
                new ToolField("comment", FieldKind.Text, false, "Why; cannot start with --."),
                Key,
            ],
            Feedback
        ),
    ];

    private static async Task<JsonNode> Context(ToolCall call, CancellationToken cancellation)
    {
        JsonNode status = await Json(call, ["status", "--json"], cancellation);
        JsonNode scopes = await Json(call, ["scope", "current", "--json"], cancellation);
        return new JsonObject
        {
            ["root"] = status["root"]?.DeepClone(),
            ["instance"] = status["configuration"]?.DeepClone(),
            ["scope"] = status["scope"]?.DeepClone(),
            ["scopes"] = scopes.DeepClone(),
            ["counts"] = new JsonObject
            {
                ["claims"] = status["claims"]?.DeepClone(),
                ["evidence"] = status["evidence"]?.DeepClone(),
                ["retex"] = status["retex"]?.DeepClone(),
                ["sources"] = status["sources"]?.DeepClone(),
            },
        };
    }

    private static Task<JsonNode> Recall(ToolCall call, CancellationToken cancellation)
    {
        List<string> argv = ["recall"];
        if (call.Arguments.Has("limit"))
        {
            argv.AddRange(["--limit", call.Arguments.Text("limit")!]);
        }
        argv.AddRange(["--json", "--", call.Arguments.Text("question")!]);
        return Json(call, argv, cancellation);
    }

    private static Task<JsonNode> Challenge(ToolCall call, CancellationToken cancellation)
    {
        bool byId = call.Arguments.Has("claimId");
        if (byId == call.Arguments.Has("text"))
        {
            throw new SermofurException("invalid_input", "Give exactly one of claimId and text.");
        }
        return Json(
            call,
            byId
                ? ["challenge", call.Arguments.Text("claimId")!, "--json"]
                : ["challenge", "--text", call.Arguments.Text("text")!, "--json"],
            cancellation
        );
    }

    private static Task<JsonNode> Claim(ToolCall call, CancellationToken cancellation)
    {
        List<string> argv = Write(call, "claim", "add");
        Option(argv, call, "category");
        Option(argv, call, "volatility");
        Option(argv, call, "key");
        argv.AddRange(["--json", "--", call.Arguments.Text("text")!]);
        return Json(call, argv, cancellation);
    }

    private static Task<JsonNode> Evidence(ToolCall call, CancellationToken cancellation)
    {
        List<string> argv = Write(call, "evidence", "add");
        argv.AddRange(["--lineage", call.Arguments.Text("lineage")!]);
        if (call.Arguments.Flag("contradicts"))
        {
            argv.Add("--contradicts");
        }
        if (call.Arguments.Has("sourceId"))
        {
            argv.AddRange(["--source", call.Arguments.Text("sourceId")!]);
        }
        Option(argv, call, "key");
        argv.AddRange([
            "--json",
            "--",
            call.Arguments.Text("claimId")!,
            call.Arguments.Text("kind")!,
            call.Arguments.Text("reference")!,
        ]);
        return Json(call, argv, cancellation);
    }

    private static Task<JsonNode> Retex(ToolCall call, CancellationToken cancellation) =>
        RecordRetex(
            call,
            call.Arguments.Text("event")!,
            call.Arguments.Text("impact")!,
            call.Arguments.Text("next")!,
            cancellation
        );

    /// <summary>Visible target first (claim, then RETEX, then source), then a marked draft RETEX.</summary>
    private static async Task<JsonNode> Feedback(ToolCall call, CancellationToken cancellation)
    {
        string target = call.Arguments.Text("targetId")!;
        string? kind = null;
        foreach (string candidate in new[] { "claim", "retex", "source" })
        {
            CommandOutcome shown = await call.Channel.RunAsync(
                [candidate, "show", target, "--json"],
                cancellation
            );
            if (shown.ExitCode == 0)
            {
                kind = candidate;
                break;
            }
            if (ErrorOf(shown).Code is not ("not_found" or "wrong_kind"))
            {
                throw ErrorOf(shown);
            }
        }
        if (kind is null)
        {
            throw new SermofurException(
                "not_found",
                "Unknown identifier, or one of a scope that is not visible."
            );
        }
        return await RecordRetex(
            call,
            $"feedback {call.Arguments.Text("verdict")} on {kind} {target}",
            call.Arguments.Text("comment") ?? "none",
            "to review by Reflect/Learn",
            cancellation
        );
    }

    private static Task<JsonNode> RecordRetex(
        ToolCall call,
        string evt,
        string impact,
        string next,
        CancellationToken cancellation
    )
    {
        List<string> argv = Write(call, "retex", "add");
        argv.AddRange(["--event", evt, "--impact", impact, "--next", next]);
        Option(argv, call, "key");
        argv.Add("--json");
        return Json(call, argv, cancellation);
    }

    private static List<string> Write(ToolCall call, string command, string subcommand) =>
        [command, subcommand, "--origin", "llm", "--actor", call.Actor];

    private static void Option(List<string> argv, ToolCall call, string name)
    {
        if (call.Arguments.Has(name))
        {
            argv.AddRange([$"--{name}", call.Arguments.Text(name)!]);
        }
    }

    /// <summary>Runs a command and returns its JSON output, or throws its error.</summary>
    private static async Task<JsonNode> Json(
        ToolCall call,
        IReadOnlyList<string> argv,
        CancellationToken cancellation
    )
    {
        CommandOutcome outcome = await call.Channel.RunAsync(argv, cancellation);
        if (outcome.ExitCode != 0)
        {
            throw ErrorOf(outcome);
        }
        return JsonNode.Parse(outcome.Stdout)
            ?? throw new SermofurException("protocol_error", "The command returned no JSON.", 3);
    }

    private static SermofurException ErrorOf(CommandOutcome outcome)
    {
        try
        {
            JsonElement error = JsonDocument.Parse(outcome.Stderr).RootElement;
            return new SermofurException(
                error.GetProperty("code").GetString() ?? "storage_error",
                error.GetProperty("message").GetString() ?? "",
                outcome.ExitCode
            );
        }
        catch (Exception exception)
            when (exception is JsonException or KeyNotFoundException or InvalidOperationException)
        {
            return new SermofurException(
                "storage_error",
                "The command failed without a readable error.",
                outcome.ExitCode
            );
        }
    }
}
