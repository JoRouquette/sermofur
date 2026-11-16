//! The eight tools of `smf mcp serve` (`Tools`, `ToolInput`, `Session` and the call part of
//! `McpBridge` of the oracle). Each becomes smf commands run by the daemon in the scope of the
//! launch folder; writes carry `--origin llm` and the actor of the session, never values of the
//! call.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;

use sermofur_core::daemon_paths::environment_text;
use sermofur_core::discovery::{expand_short_names, get_full_path};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::json::quoted_basic_latin;
use sermofur_core::local_paths::{is_path_fully_qualified, trim_ending_separator};
use sermofur_core::stj::{self, Name, Node};
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::server::CommandOutcome;

use crate::wire::{self, integer};

/// Runs one smf command through the daemon (`ICommandChannel`); a failure of the bridge is an
/// error with its code.
pub trait CommandChannel {
    fn run(&self, argv: Vec<String>) -> impl Future<Output = Result<CommandOutcome>>;
}

/// Kinds of value a tool accepts (`FieldKind`).
#[derive(Clone, Copy)]
pub enum Kind {
    Text,
    Guid,
    Choice(&'static [&'static str]),
    Flag,
    Limit,
}

/// One field of a tool input (`ToolField`): the schema and the validation come from it.
pub struct Field {
    pub name: &'static str,
    pub kind: Kind,
    pub required: bool,
    pub description: &'static str,
    /// The value goes to the CLI as an option value, which cannot start with `--`.
    pub as_option: bool,
}

/// Longest text, as `InputLimits` of the CLI (`ToolField.MaxTextLength`), in UTF-16 units.
pub const MAX_TEXT_LENGTH: usize = 16_384;

const fn field(name: &'static str, kind: Kind, required: bool, description: &'static str) -> Field {
    Field {
        name,
        kind,
        required,
        description,
        as_option: true,
    }
}

const fn positional(
    name: &'static str,
    kind: Kind,
    required: bool,
    description: &'static str,
) -> Field {
    Field {
        as_option: false,
        ..field(name, kind, required, description)
    }
}

/// One tool of the contract (`ToolDefinition`).
pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub fields: &'static [Field],
    pub read_only: bool,
}

const CATEGORIES: &[&str] = &["episodic", "semantic", "procedural"];
const VOLATILITIES: &[&str] = &["stable", "evolving", "volatile"];
const EVIDENCE_KINDS: &[&str] = &[
    "execution",
    "source_code",
    "authoritative_documentation",
    "project_decision",
    "local_documentation",
    "human_observation",
    "user_assertion",
    "llm_assertion",
];
const VERDICTS: &[&str] = &["helpful", "not_applicable", "wrong"];

const KEY: Field = field(
    "key",
    Kind::Text,
    false,
    "Idempotency key: the same key with the same content returns the same object.",
);

/// The eight tools, in the order of `Tools.All`.
pub const TOOLS: &[Tool] = &[
    Tool {
        name: "sermofur_status",
        description: "Identity of the Sermofur instance of this project, current scope and counts of the visible objects.",
        fields: &[],
        read_only: true,
    },
    Tool {
        name: "sermofur_context",
        description: "Context of this session: instance, current scope with its visible ancestors, and counts. No content.",
        fields: &[],
        read_only: true,
    },
    Tool {
        name: "sermofur_recall",
        description: "Recalls at most 3 explained results (claims, RETEX, source passages) relevant to a question, among what this project may see. Read only.",
        fields: &[
            positional(
                "question",
                Kind::Text,
                true,
                "The question, in plain words.",
            ),
            field(
                "limit",
                Kind::Limit,
                false,
                "Number of results, 1 to 3. Default: 3.",
            ),
        ],
        read_only: true,
    },
    Tool {
        name: "sermofur_challenge",
        description: "Confronts a claim (claimId) or a text not recorded yet (text) with contradictions, changed sources, status and close claims. Never decides; writes nothing. Give exactly one of claimId and text.",
        fields: &[
            field(
                "claimId",
                Kind::Guid,
                false,
                "Identifier of a visible claim.",
            ),
            field(
                "text",
                Kind::Text,
                false,
                "A statement to confront; cannot start with --.",
            ),
        ],
        read_only: true,
    },
    Tool {
        name: "sermofur_claim",
        description: "Proposes a claim in the current scope. It is recorded as coming from an LLM: its confidence only grows with independent, non-LLM evidence.",
        fields: &[
            positional("text", Kind::Text, true, "The statement."),
            field(
                "category",
                Kind::Choice(CATEGORIES),
                false,
                "Default: semantic.",
            ),
            field(
                "volatility",
                Kind::Choice(VOLATILITIES),
                false,
                "Default: evolving.",
            ),
            KEY,
        ],
        read_only: false,
    },
    Tool {
        name: "sermofur_evidence",
        description: "Records evidence for a visible claim (or against it with contradicts). Evidence declared by an LLM never reinforces a claim. lineage and key cannot start with --.",
        fields: &[
            positional(
                "claimId",
                Kind::Guid,
                true,
                "Identifier of a visible claim.",
            ),
            positional(
                "kind",
                Kind::Choice(EVIDENCE_KINDS),
                true,
                "Kind of evidence.",
            ),
            positional(
                "reference",
                Kind::Text,
                true,
                "What supports it: a path, a link, a short description.",
            ),
            field(
                "lineage",
                Kind::Text,
                true,
                "Common origin of related evidence; evidence of one lineage counts once.",
            ),
            field(
                "contradicts",
                Kind::Flag,
                false,
                "True when the evidence goes against the claim.",
            ),
            field(
                "sourceId",
                Kind::Guid,
                false,
                "Identifier of a visible source the evidence cites.",
            ),
            KEY,
        ],
        read_only: false,
    },
    Tool {
        name: "sermofur_record_retex",
        description: "Records a draft RETEX (lesson learned): what happened, its impact, what to do next. A draft never changes confidence or recall. Values cannot start with --.",
        fields: &[
            field("event", Kind::Text, true, "What happened."),
            field("impact", Kind::Text, true, "Its consequence."),
            field("next", Kind::Text, true, "What to do next time."),
            KEY,
        ],
        read_only: false,
    },
    Tool {
        name: "sermofur_feedback",
        description: "Says whether a recalled object helped, did not apply or was wrong. Kept as a draft RETEX marked as feedback, with no effect on confidence or ranking.",
        fields: &[
            field(
                "targetId",
                Kind::Guid,
                true,
                "Identifier of a visible claim, RETEX or source.",
            ),
            field("verdict", Kind::Choice(VERDICTS), true, "The verdict."),
            field("comment", Kind::Text, false, "Why; cannot start with --."),
            KEY,
        ],
        read_only: false,
    },
];

/// The `result` of `tools/list` (`McpBridge.ListTools`), as the SDK writes it.
pub fn list() -> String {
    let tools: Vec<String> = TOOLS
        .iter()
        .map(|tool| {
            format!(
                "{{\"name\":{},\"description\":{},\"inputSchema\":{},\"annotations\":{{\"destructiveHint\":false,\"openWorldHint\":false,\"readOnlyHint\":{}}}}}",
                quoted_basic_latin(tool.name),
                quoted_basic_latin(tool.description),
                input_schema(tool),
                tool.read_only
            )
        })
        .collect();
    format!("{{\"tools\":[{}]}}", tools.join(","))
}

/// `ToolDefinition.InputSchema`: a closed object, from the fields.
fn input_schema(tool: &Tool) -> String {
    let properties: Vec<String> = tool
        .fields
        .iter()
        .map(|field| format!("{}:{}", quoted_basic_latin(field.name), field_schema(field)))
        .collect();
    let mut schema = format!(
        "{{\"type\":\"object\",\"properties\":{{{}}},\"additionalProperties\":false",
        properties.join(",")
    );
    let required: Vec<String> = tool
        .fields
        .iter()
        .filter(|field| field.required)
        .map(|field| quoted_basic_latin(field.name))
        .collect();
    if !required.is_empty() {
        schema.push_str(&format!(",\"required\":[{}]", required.join(",")));
    }
    schema.push('}');
    schema
}

/// `ToolField.Schema`.
fn field_schema(field: &Field) -> String {
    let kind = match field.kind {
        Kind::Text => {
            format!("\"type\":\"string\",\"minLength\":1,\"maxLength\":{MAX_TEXT_LENGTH}")
        }
        Kind::Guid => "\"type\":\"string\",\"format\":\"uuid\"".to_owned(),
        Kind::Choice(choices) => {
            let choices: Vec<String> = choices.iter().map(|c| quoted_basic_latin(c)).collect();
            format!("\"type\":\"string\",\"enum\":[{}]", choices.join(","))
        }
        Kind::Flag => "\"type\":\"boolean\"".to_owned(),
        Kind::Limit => "\"type\":\"integer\",\"minimum\":1,\"maximum\":3".to_owned(),
    };
    format!(
        "{{\"description\":{},{kind}}}",
        quoted_basic_latin(field.description)
    )
}

/// Why a call has no result to give.
pub enum Failure {
    /// A `SermofurException`: an error result with its code and message.
    Refused(SermofurError),
    /// Any other exception of the oracle (a string with a lone surrogate, an output that is not
    /// JSON): the SDK answers -32603.
    Internal,
}

impl From<SermofurError> for Failure {
    fn from(error: SermofurError) -> Self {
        Self::Refused(error)
    }
}

/// The `result` of a `tools/call` (`McpBridge.CallAsync`), or `None` for -32603.
pub async fn call(
    name: &str,
    arguments: Option<&Node>,
    actor: &str,
    channel: &impl CommandChannel,
    log: Option<&DaemonLog>,
) -> Option<String> {
    let outcome = match TOOLS.iter().find(|tool| tool.name == name) {
        None => Err(Failure::Refused(invalid(format!("Unknown tool '{name}'.")))),
        Some(tool) => run(tool, arguments, actor, channel).await,
    };
    let (text, structured, failed) = match outcome {
        Ok(data) => {
            if let Some(log) = log {
                log.write("mcp_call", None, None, None);
            }
            // `data as JsonObject ?? new JsonObject { ["items"] = data }`.
            let text = wire::to_text(&data);
            let structured = if matches!(data, Node::Object(_)) {
                text.clone()
            } else {
                wire::to_text(&Node::Object(vec![(name_of("items"), data)]))
            };
            (text, structured, false)
        }
        Err(Failure::Refused(error)) => {
            if let Some(log) = log {
                log.write("mcp_call", Some(&error.code), None, None);
            }
            let structured = format!(
                "{{\"code\":{},\"message\":{}}}",
                quoted_basic_latin(&error.code),
                quoted_basic_latin(&error.message)
            );
            (
                format!("{}: {}", error.code, error.message),
                structured,
                true,
            )
        }
        Err(Failure::Internal) => return None,
    };
    Some(format!(
        "{{\"content\":[{{\"type\":\"text\",\"text\":{}}}],\"structuredContent\":{structured}{}}}",
        quoted_basic_latin(&text),
        if failed { ",\"isError\":true" } else { "" }
    ))
}

async fn run(
    tool: &Tool,
    arguments: Option<&Node>,
    actor: &str,
    channel: &impl CommandChannel,
) -> std::result::Result<Node, Failure> {
    let values = validate(tool.fields, arguments)?;
    let call = Call {
        values,
        actor,
        channel,
    };
    match tool.name {
        "sermofur_status" => call.json(words(&["status", "--json"])).await,
        "sermofur_context" => call.context().await,
        "sermofur_recall" => {
            let mut argv = words(&["recall"]);
            if let Some(limit) = call.text("limit") {
                argv.extend(["--limit".to_owned(), limit.to_owned()]);
            }
            argv.extend([
                "--json".to_owned(),
                "--".to_owned(),
                call.required("question"),
            ]);
            call.json(argv).await
        }
        "sermofur_challenge" => {
            let by_id = call.text("claimId");
            if by_id.is_some() == call.text("text").is_some() {
                return Err(invalid("Give exactly one of claimId and text.").into());
            }
            let argv = match by_id {
                Some(id) => vec!["challenge".to_owned(), id.to_owned(), "--json".to_owned()],
                None => vec![
                    "challenge".to_owned(),
                    "--text".to_owned(),
                    call.required("text"),
                    "--json".to_owned(),
                ],
            };
            call.json(argv).await
        }
        "sermofur_claim" => {
            let mut argv = call.write("claim", "add");
            call.option(&mut argv, "category");
            call.option(&mut argv, "volatility");
            call.option(&mut argv, "key");
            argv.extend(["--json".to_owned(), "--".to_owned(), call.required("text")]);
            call.json(argv).await
        }
        "sermofur_evidence" => {
            let mut argv = call.write("evidence", "add");
            argv.extend(["--lineage".to_owned(), call.required("lineage")]);
            if call.text("contradicts") == Some("true") {
                argv.push("--contradicts".to_owned());
            }
            if let Some(source) = call.text("sourceId") {
                argv.extend(["--source".to_owned(), source.to_owned()]);
            }
            call.option(&mut argv, "key");
            argv.extend([
                "--json".to_owned(),
                "--".to_owned(),
                call.required("claimId"),
                call.required("kind"),
                call.required("reference"),
            ]);
            call.json(argv).await
        }
        "sermofur_record_retex" => {
            let (event, impact, next) = (
                call.required("event"),
                call.required("impact"),
                call.required("next"),
            );
            call.retex(event, impact, next).await
        }
        _ => call.feedback().await,
    }
}

fn words(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn name_of(text: &str) -> Name {
    Name {
        text: text.to_owned(),
        lone_surrogate: false,
    }
}

fn invalid(message: impl Into<String>) -> SermofurError {
    SermofurError::new("invalid_input", message, 1)
}

/// `ToolArguments.Validate`: no unknown field (in the order given), then each field in its
/// order: required, type, length, choice, identifier. A null value is no value.
fn validate(
    fields: &[Field],
    arguments: Option<&Node>,
) -> std::result::Result<Vec<(&'static str, String)>, Failure> {
    // `Dictionary<string, JsonElement>`: a repeated key keeps its first place and its last value.
    let mut given: Vec<(&str, &Node)> = Vec::new();
    let mut place: HashMap<&str, usize> = HashMap::new();
    if let Some(Node::Object(entries)) = arguments {
        for (name, value) in entries {
            if name.lone_surrogate {
                return Err(Failure::Internal);
            }
            match place.get(name.text.as_str()) {
                Some(&index) => given[index].1 = value,
                None => {
                    place.insert(&name.text, given.len());
                    given.push((&name.text, value));
                }
            }
        }
    }
    if let Some((name, _)) = given
        .iter()
        .find(|(name, _)| !fields.iter().any(|field| field.name == *name))
    {
        return Err(invalid(format!("Unknown field '{name}'.")).into());
    }
    let mut values = Vec::new();
    for field in fields {
        match place.get(field.name).map(|&index| given[index]) {
            None | Some((_, Node::Null)) => {
                if field.required {
                    return Err(invalid(format!("Field '{}' is required.", field.name)).into());
                }
            }
            Some((_, value)) => values.push((field.name, read(field, value)?)),
        }
    }
    Ok(values)
}

/// `ToolArguments.Read`.
fn read(field: &Field, value: &Node) -> std::result::Result<String, Failure> {
    let name = field.name;
    match field.kind {
        Kind::Flag => {
            return match value {
                Node::Bool(flag) => Ok(flag.to_string()),
                _ => Err(invalid(format!("Field '{name}' is a boolean.")).into()),
            };
        }
        Kind::Limit => {
            return match value {
                Node::Number(raw) => match integer::<i32>(raw) {
                    Some(limit @ 1..=3) => Ok(limit.to_string()),
                    _ => Err(limit_refusal(name)),
                },
                _ => Err(limit_refusal(name)),
            };
        }
        _ => {}
    }
    let Node::Text {
        text,
        lone_surrogate,
    } = value
    else {
        return Err(invalid(format!("Field '{name}' is a string.")).into());
    };
    if *lone_surrogate {
        // `GetString` refuses invalid UTF-16 with an `InvalidOperationException`.
        return Err(Failure::Internal);
    }
    let length = text.encode_utf16().count();
    if length == 0 || length > MAX_TEXT_LENGTH || text.contains('\0') {
        return Err(invalid(format!(
            "Field '{name}' takes 1 to {MAX_TEXT_LENGTH} characters, without NUL."
        ))
        .into());
    }
    if field.as_option && text.starts_with("--") {
        return Err(invalid(format!("Field '{name}' cannot start with --.")).into());
    }
    if matches!(field.kind, Kind::Guid) && sermofur_core::guid::try_parse_exact_d(text).is_none() {
        return Err(invalid(format!("Field '{name}' is an identifier (GUID).")).into());
    }
    if let Kind::Choice(choices) = field.kind
        && !choices.contains(&text.as_str())
    {
        return Err(invalid(format!("Field '{name}' is one of: {}.", choices.join(", "))).into());
    }
    Ok(text.clone())
}

fn limit_refusal(name: &str) -> Failure {
    invalid(format!("Field '{name}' is an integer from 1 to 3.")).into()
}

/// One call: validated values, actor of the session, channel (`ToolCall`).
struct Call<'a, C> {
    values: Vec<(&'static str, String)>,
    actor: &'a str,
    channel: &'a C,
}

impl<C: CommandChannel> Call<'_, C> {
    fn text(&self, name: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }

    /// A required value, present after validation.
    fn required(&self, name: &str) -> String {
        self.text(name).unwrap_or_default().to_owned()
    }

    fn write(&self, command: &str, subcommand: &str) -> Vec<String> {
        words(&[
            command, subcommand, "--origin", "llm", "--actor", self.actor,
        ])
    }

    fn option(&self, argv: &mut Vec<String>, name: &str) {
        if let Some(value) = self.text(name) {
            argv.extend([format!("--{name}"), value.to_owned()]);
        }
    }

    /// Runs a command and returns its JSON output, or its error.
    async fn json(&self, argv: Vec<String>) -> std::result::Result<Node, Failure> {
        let outcome = self.channel.run(argv).await?;
        if outcome.exit_code != 0 {
            return Err(error_of(&outcome).into());
        }
        match stj::parse(&outcome.stdout) {
            Ok(Node::Null) => {
                Err(SermofurError::new("protocol_error", "The command returned no JSON.", 3).into())
            }
            Ok(node) => Ok(node),
            Err(_) => Err(Failure::Internal),
        }
    }

    async fn context(&self) -> std::result::Result<Node, Failure> {
        let status = self.json(words(&["status", "--json"])).await?;
        let scopes = self.json(words(&["scope", "current", "--json"])).await?;
        // `status["root"]` of a `JsonNode` that is not an object throws.
        let Node::Object(entries) = &status else {
            return Err(Failure::Internal);
        };
        let get = |name: &str| {
            entries
                .iter()
                .rev()
                .find(|(key, _)| key.text == name)
                .map_or(Node::Null, |(_, value)| value.clone())
        };
        let counts = ["claims", "evidence", "retex", "sources"]
            .iter()
            .map(|name| (name_of(name), get(name)))
            .collect();
        Ok(Node::Object(vec![
            (name_of("root"), get("root")),
            (name_of("instance"), get("configuration")),
            (name_of("scope"), get("scope")),
            (name_of("scopes"), scopes),
            (name_of("counts"), Node::Object(counts)),
        ]))
    }

    /// Visible target first (claim, then RETEX, then source), then a marked draft RETEX.
    async fn feedback(&self) -> std::result::Result<Node, Failure> {
        let target = self.required("targetId");
        let mut kind = None;
        for candidate in ["claim", "retex", "source"] {
            let shown = self
                .channel
                .run(words(&[candidate, "show", &target, "--json"]))
                .await?;
            if shown.exit_code == 0 {
                kind = Some(candidate);
                break;
            }
            let error = error_of(&shown);
            if error.code != "not_found" && error.code != "wrong_kind" {
                return Err(error.into());
            }
        }
        let Some(kind) = kind else {
            return Err(SermofurError::new(
                "not_found",
                "Unknown identifier, or one of a scope that is not visible.",
                1,
            )
            .into());
        };
        let comment = self.text("comment").unwrap_or("none").to_owned();
        let event = format!("feedback {} on {kind} {target}", self.required("verdict"));
        self.retex(event, comment, "to review by Reflect/Learn".to_owned())
            .await
    }

    async fn retex(
        &self,
        event: String,
        impact: String,
        next: String,
    ) -> std::result::Result<Node, Failure> {
        let mut argv = self.write("retex", "add");
        argv.extend([
            "--event".to_owned(),
            event,
            "--impact".to_owned(),
            impact,
            "--next".to_owned(),
            next,
        ]);
        self.option(&mut argv, "key");
        argv.push("--json".to_owned());
        self.json(argv).await
    }
}

/// `ErrorOf`: the JSON error the command wrote on stderr, or `storage_error` when it cannot be
/// read (`JsonDocument`: case kept, the last of repeated names).
fn error_of(outcome: &CommandOutcome) -> SermofurError {
    let unreadable = || {
        SermofurError::new(
            "storage_error",
            "The command failed without a readable error.",
            outcome.exit_code,
        )
    };
    let Ok(Node::Object(entries)) = stj::parse(&outcome.stderr) else {
        return unreadable();
    };
    let get = |name: &str| {
        entries
            .iter()
            .rev()
            .find(|(key, _)| key.text == name)
            .map(|(_, value)| value)
    };
    // `GetString`: a string, or null for null; any other kind throws.
    let string = |name: &str| match get(name) {
        Some(Node::Null) => Some(None),
        Some(Node::Text {
            text,
            lone_surrogate: false,
        }) => Some(Some(text.clone())),
        _ => None,
    };
    match (string("code"), string("message")) {
        (Some(code), Some(message)) => SermofurError::new(
            code.unwrap_or_else(|| "storage_error".to_owned()),
            message.unwrap_or_default(),
            outcome.exit_code,
        ),
        _ => unreadable(),
    }
}

/// `Session.ProjectDirectoryVariable`: Claude Code sets it to the project root.
pub const PROJECT_DIRECTORY_VARIABLE: &str = "CLAUDE_PROJECT_DIR";

/// `Session.DefaultActor`.
pub const DEFAULT_ACTOR: &str = "mcp-client";

const MAX_ACTOR_LENGTH: usize = 64;

/// `Session.LaunchFolder`: `CLAUDE_PROJECT_DIR` when set and absolute, the working directory
/// otherwise; full, in its long form, final separator trimmed.
///
/// `LongPath.Of` expands the whole path or nothing; [`expand_short_names`] expands the longest
/// part that exists: they differ only for a folder that does not exist.
pub fn launch_folder(project: Option<String>, working_directory: &Path) -> Result<String> {
    let folder = match project {
        Some(project) if !project.trim().is_empty() && is_path_fully_qualified(&project) => {
            Path::new(&project).to_path_buf()
        }
        _ => working_directory.to_path_buf(),
    };
    let long = expand_short_names(&get_full_path(&folder)?);
    Ok(trim_ending_separator(&long.to_string_lossy()))
}

/// [`launch_folder`] from the environment of the process.
pub fn launch_folder_of_process(working_directory: &Path) -> Result<String> {
    launch_folder(
        environment_text(PROJECT_DIRECTORY_VARIABLE),
        working_directory,
    )
}

/// `Session.Actor`: the name the host gives at initialization, lower case, `[a-z0-9-]`, at most
/// 64 characters; `mcp-client` when nothing is left.
pub fn actor(client_name: Option<&str>) -> String {
    let Some(name) = client_name.filter(|name| !name.trim().is_empty()) else {
        return DEFAULT_ACTOR.to_owned();
    };
    let mut actor = String::new();
    let mut dash = false;
    // `ToLowerInvariant` per UTF-16 unit: only the Kelvin sign (to `k`) reaches ASCII from
    // outside it; `İ` stays itself (measured, golden `mcp-serve.json`). Any other unit
    // outside `[a-z0-9]` is a separator, as one unit or as the two of a surrogate pair.
    for character in name.trim().chars() {
        let lower = match character {
            '\u{212A}' => 'k',
            other => other.to_ascii_lowercase(),
        };
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            actor.push(lower);
            dash = false;
        } else if !dash && !actor.is_empty() {
            actor.push('-');
            dash = true;
        }
        if actor.len() >= MAX_ACTOR_LENGTH {
            break;
        }
    }
    let normalized = actor.trim_matches('-');
    if normalized.is_empty() {
        DEFAULT_ACTOR.to_owned()
    } else {
        normalized.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Port of `McpTests.ToolsAreNamedAndSessionFolderAndActorAreDerived`.
    #[test]
    fn tools_are_named_and_session_folder_and_actor_are_derived() {
        let mut names: Vec<&str> = TOOLS.iter().map(|tool| tool.name).collect();
        names.sort_unstable();
        assert_eq!(
            names,
            [
                "sermofur_challenge",
                "sermofur_claim",
                "sermofur_context",
                "sermofur_evidence",
                "sermofur_feedback",
                "sermofur_recall",
                "sermofur_record_retex",
                "sermofur_status",
            ]
        );
        assert_eq!(actor(Some("Claude Code")), "claude-code");
        assert_eq!(actor(Some("  ***  ")), "mcp-client");
        assert_eq!(actor(None), "mcp-client");
        assert_eq!(actor(Some(&"a".repeat(100))).len(), 64);
        assert_eq!(actor(Some("Claude Code É")), "claude-code");
        assert_eq!(actor(Some("İ\u{212A}")), "k");
        assert_eq!(actor(Some("aİb")), "a-b");
        let temporary = std::env::temp_dir();
        let project = temporary.to_string_lossy().into_owned();
        let expected = trim_ending_separator(
            &expand_short_names(&get_full_path(&temporary).expect("full")).to_string_lossy(),
        );
        assert_eq!(
            launch_folder(Some(project), Path::new("/elsewhere")).expect("folder"),
            expected
        );
        assert_eq!(
            launch_folder(Some("relative".to_owned()), &temporary).expect("folder"),
            expected
        );
        assert_eq!(launch_folder(None, &temporary).expect("folder"), expected);
        if cfg!(windows) {
            // %TEMP% is often given in its 8.3 short form: it comes back in the long form.
            assert!(
                !launch_folder(None, &temporary)
                    .expect("folder")
                    .contains('~')
            );
        }
    }

    fn outcome(exit_code: i32, stderr: &str) -> CommandOutcome {
        CommandOutcome {
            exit_code,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn the_error_of_a_command_is_read_as_the_oracle_reads_it() {
        let read = |stderr| {
            let error = error_of(&outcome(3, stderr));
            (error.code, error.message, error.exit_code)
        };
        assert_eq!(
            read("{\"code\":\"a\",\"message\":\"b\",\"code\":\"c\"}\n"),
            ("c".to_owned(), "b".to_owned(), 3)
        );
        assert_eq!(
            read("{\"code\":null,\"message\":null}"),
            ("storage_error".to_owned(), String::new(), 3)
        );
        for unreadable in [
            "",
            "[]",
            "{\"code\":1,\"message\":\"m\"}",
            "{\"code\":\"x\"}",
        ] {
            assert_eq!(read(unreadable).0, "storage_error", "{unreadable}");
        }
    }

    proptest::proptest! {
        /// Whatever the host name: a non-empty actor of at most 64 characters of `[a-z0-9-]`,
        /// without a dash at either end or two in a row.
        #[test]
        fn an_actor_is_always_a_safe_name(
            name in proptest::option::of(r"\PC{0,100}|[a-zA-Z0-9 _.\u{130}\u{212A}\u{e9}-]{0,100}")
        ) {
            let actor = actor(name.as_deref());
            proptest::prop_assert!(!actor.is_empty() && actor.len() <= MAX_ACTOR_LENGTH, "{}", actor);
            proptest::prop_assert!(
                actor.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
                "{}",
                actor
            );
            proptest::prop_assert!(
                !actor.starts_with('-') && !actor.ends_with('-') && !actor.contains("--"),
                "{}",
                actor
            );
        }
    }

    /// Arguments with 200 000 distinct names are refused in linear time (review of lot 8.4).
    #[test]
    fn many_argument_names_are_checked_in_linear_time() {
        let entries: Vec<String> = (0..200_000).map(|i| format!(r#""k{i}":1"#)).collect();
        let arguments = stj::parse(&format!("{{{}}}", entries.join(","))).expect("JSON");
        let started = std::time::Instant::now();
        assert!(validate(TOOLS[0].fields, Some(&arguments)).is_err());
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }
}
