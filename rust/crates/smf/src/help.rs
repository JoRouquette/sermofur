//! The header, the general usage text and the help of each command and group (lot 9), ported
//! from `CommandHelp.cs` and `CommandRunner.cs`.

use sermofur_core::{NEW_LINE, VERSION};

use crate::args::{CommandArguments, END_OF_OPTIONS, HELP_SHORTCUT, VERSION_SHORTCUT};

const REPOSITORY: &str = "https://github.com/JoRouquette/sermofur";

/// Header shared by every help output: `Sermofur <version>`.
pub fn header() -> String {
    format!("Sermofur {VERSION}")
}

/// CLI reference for `version`: the documentation of its release tag, or that of `main` for a
/// build that carries a pre-release label, such as a development build.
pub fn cli_documentation(version: &str) -> String {
    if version.contains('-') {
        format!("{REPOSITORY}/blob/main/docs/cli.md")
    } else {
        format!("{REPOSITORY}/blob/v{version}/docs/cli.md")
    }
}

/// Lines of the usage text between the header and the last sentence, as the .NET tool prints
/// them.
const USAGE_LINES: [&str; 24] = [
    "Global options: [--path DIRECTORY] [--json]",
    "smf init | root | status | doctor | export | migrate",
    "smf scope current | list | tree",
    "smf scope add ID KIND PARENT RELATIVE_PATH",
    "smf claim add TEXT --origin user|llm [--category CATEGORY] [--volatility VOLATILITY] [--actor NAME] [--key KEY]",
    "smf claim list | show ID | invalidate ID --reason TEXT [--actor NAME]",
    "smf evidence add CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm [--contradicts] [--source SOURCE_ID] [--actor NAME] [--key KEY]",
    "smf evidence list | show ID",
    "smf retex add --event TEXT --impact TEXT --next TEXT --origin user|llm [--actor NAME] [--key KEY]",
    "smf retex list | show ID",
    "smf source add FILE --origin user|llm [--actor NAME]",
    "smf source list | show ID | reindex [ID] [--actor NAME]",
    "smf index rebuild",
    "smf recall QUESTION [--limit 1-3]",
    "smf challenge CLAIM_ID | challenge --text TEXT",
    "smf daemon install | uninstall | start | stop | restart | status",
    "smf daemon register | unregister | instances",
    "smf daemon run [--supervise]",
    "smf mcp install | uninstall [--host claude-code|codex] [--scope project|user]",
    "smf mcp serve",
    "smf COMMAND -h | smf GROUP -h       help of one command, or the subcommands of a group",
    "smf --version | -v                  version",
    "--help (-h) and --version (-v) are recognized anywhere before --, except as an option value; --help wins.",
    "-- ends options: every following argument is positional (text starting with --).",
];

/// The general usage text, printed for `smf`, `smf -h` and `smf --help`.
pub fn usage() -> String {
    let mut text = header();
    for line in USAGE_LINES {
        text.push('\n');
        text.push_str(line);
    }
    text.push_str(&format!(
        "\nAn option value cannot start with --; see {} for values and exit codes.",
        cli_documentation(VERSION)
    ));
    text
}

/// One line of a help section: a name and what it means (`HelpEntry`).
pub struct Entry {
    pub name: &'static str,
    pub description: &'static str,
}

const fn entry(name: &'static str, description: &'static str) -> Entry {
    Entry { name, description }
}

/// Help of one command form (`CommandTopic`): what `smf ... -h` prints.
pub struct Topic {
    pub command: &'static str,
    pub arguments: &'static str,
    pub summary: &'static str,
    pub parameters: &'static [Entry],
    pub options: &'static [Entry],
    pub errors: &'static [Entry],
    pub example: &'static str,
}

impl Topic {
    /// First word: the command or the group of subcommands.
    pub fn group(&self) -> &'static str {
        self.command.split(' ').next().unwrap_or(self.command)
    }

    pub fn synopsis(&self) -> String {
        if self.arguments.is_empty() {
            self.command.to_string()
        } else {
            format!("{} {}", self.command, self.arguments)
        }
    }
}

const ORIGIN: Entry = entry(
    "--origin user|llm",
    "Required, no default. Declared origin of the record.",
);
const ACTOR: Entry = entry(
    "--actor NAME",
    "Name recorded in the history. Default: local-user.",
);
const KEY: Entry = entry(
    "--key KEY",
    "Idempotency key: the same key with the same content returns the same object.",
);
const CONFLICT: Entry = entry(
    "idempotency_conflict (1)",
    "Key reused with another content.",
);
const ID_ERROR: Entry = entry(
    "not_found (1)",
    "Unknown identifier, or one of a scope that is not visible.",
);
const MCP_HOST: Entry = entry(
    "--host claude-code|codex",
    "MCP host. Default: claude-code.",
);
const MCP_SCOPE: Entry = entry(
    "--scope project|user",
    "Codex only: configuration of the project (.codex/config.toml, loaded when the project is trusted) or of the user. Default: project.",
);
// The seconds of `InstanceRegistry.DefaultLockTimeout` and `DaemonCommands.DefaultStatusTimeout`,
// written out; a test ties them to the constants of the port.
const REGISTRY_BUSY: Entry = entry(
    "registry_busy (3)",
    "Another smf process has been changing the registry for 10 s, or another program holds it open; nothing changed, run again.",
);
const NOT_INSTALLED: Entry = entry("daemon_unavailable (3)", "The service is not installed.");

const fn topic(
    command: &'static str,
    arguments: &'static str,
    summary: &'static str,
    parameters: &'static [Entry],
    options: &'static [Entry],
    errors: &'static [Entry],
    example: &'static str,
) -> Topic {
    Topic {
        command,
        arguments,
        summary,
        parameters,
        options,
        errors,
        example,
    }
}

/// Catalogue of the help of every command (`CommandHelp.Topics`), in the order of the oracle.
pub const TOPICS: [Topic; 40] = [
    topic(
        "init",
        "",
        "Creates an instance (.sermofur) in the context directory, or returns the existing one. Refused inside another instance.",
        &[],
        &[],
        &[
            entry("invalid_path (1)", "The directory does not exist."),
            entry("nested_instance (4)", "A valid instance exists above."),
            entry(
                "invalid_instance (3)",
                "The nearest .sermofur entry is not a valid instance.",
            ),
            entry("unsafe_path (4)", "Network path, link or junction."),
        ],
        "smf init",
    ),
    topic(
        "root",
        "",
        "Prints the root of the nearest instance going up from the context directory.",
        &[],
        &[],
        &[],
        "smf root",
    ),
    topic(
        "status",
        "",
        "Identity of the instance, current scope and counts of the visible objects.",
        &[],
        &[],
        &[],
        "smf status --json",
    ),
    topic(
        "doctor",
        "",
        "Read-only health check: integrity, schema, scopes, mappings, index, projections. Exit 5 when unhealthy, 0 when healthy, with or without warnings.",
        &[],
        &[],
        &[entry("unsafe_path (4)", "The .sermofur entry is a link.")],
        "smf doctor",
    ),
    topic(
        "export",
        "",
        "Rebuilds the Markdown projections of the visible objects and lists their files.",
        &[],
        &[],
        &[],
        "smf export",
    ),
    topic(
        "migrate",
        "",
        "Migrates an instance from format 1 (0.1) to format 2: consistency check, backup in .sermofur/backups/, one transaction. Resumes after an interruption; nothing to do on format 2.",
        &[],
        &[],
        &[entry(
            "storage_error (3)",
            "The instance is not consistent: run smf doctor first.",
        )],
        "smf migrate",
    ),
    topic(
        "scope current",
        "",
        "Current scope (from the context directory) and its visible ancestors.",
        &[],
        &[],
        &[],
        "smf scope current",
    ),
    topic(
        "scope list",
        "",
        "Visible scopes: the current one and its ancestors, never siblings or descendants.",
        &[],
        &[],
        &[],
        "smf scope list",
    ),
    topic(
        "scope tree",
        "",
        "Same as scope list in this version.",
        &[],
        &[],
        &[],
        "smf scope tree",
    ),
    topic(
        "scope add",
        "ID KIND PARENT RELATIVE_PATH",
        "Registers a direct child of the current scope, mapped to an existing directory. The mapping is stored as the folder is named on disk.",
        &[
            entry("ID", "New identifier: [a-z][a-z0-9-]{0,63}."),
            entry(
                "KIND",
                "client, project, repository or task, compatible with the parent.",
            ),
            entry(
                "PARENT",
                "The current scope (workspace at the instance root).",
            ),
            entry(
                "RELATIVE_PATH",
                "Directory relative to the instance root, inside the parent's directory.",
            ),
        ],
        &[],
        &[
            entry(
                "invalid_scope (1)",
                "Bad identifier or kind, or identifier taken.",
            ),
            entry(
                "duplicate_mapping (1)",
                "Directory already mapped by another scope.",
            ),
            entry("invalid_path (1)", "The directory does not exist."),
            entry(
                "scope_boundary (4)",
                "Wrong parent, or mapping outside the parent or overlapping another branch.",
            ),
            entry(
                "unsafe_path (4)",
                "Absolute path, drive, link, junction or network path, or an alias.",
            ),
        ],
        "smf scope add acme client workspace acme",
    ),
    topic(
        "claim add",
        "TEXT --origin user|llm",
        "Records a proposed claim in the current scope. Its confidence is computed when read, from its evidence.",
        &[entry(
            "TEXT",
            "The statement. Put it after -- if it starts with -- or is -h or -v.",
        )],
        &[
            ORIGIN,
            entry(
                "--category CATEGORY",
                "episodic, semantic, procedural, preferences or decisions. Default: semantic. preferences and decisions require --origin user.",
            ),
            entry(
                "--volatility VOLATILITY",
                "stable, evolving or volatile. Default: evolving.",
            ),
            ACTOR,
            KEY,
        ],
        &[
            entry(
                "invalid_arguments (1)",
                "Missing origin, bad value or unknown option.",
            ),
            entry(
                "user_choice_required (1)",
                "preferences or decisions with --origin llm.",
            ),
            CONFLICT,
        ],
        "smf claim add \"The billing API paginates with cursors\" --origin user",
    ),
    topic(
        "claim list",
        "",
        "Visible claims.",
        &[],
        &[],
        &[],
        "smf claim list --json",
    ),
    topic(
        "claim show",
        "ID",
        "The claim with its evidence, its explained confidence and its history.",
        &[entry("ID", "Identifier (GUID) of a visible claim.")],
        &[],
        &[ID_ERROR],
        "smf claim show 7f8c1a3e-0000-4000-8000-000000000001",
    ),
    topic(
        "claim invalidate",
        "ID --reason TEXT",
        "Invalidates a claim of the current scope, with an auditable reason.",
        &[entry(
            "ID",
            "Identifier (GUID) of a visible claim of the current scope.",
        )],
        &[
            entry("--reason TEXT", "Required. Why the claim no longer holds."),
            ACTOR,
        ],
        &[
            ID_ERROR,
            entry(
                "scope_boundary (4)",
                "The claim belongs to an ancestor scope.",
            ),
        ],
        "smf claim invalidate 7f8c1a3e-0000-4000-8000-000000000001 --reason \"Replaced by cursors v2\"",
    ),
    topic(
        "evidence add",
        "CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm",
        "Records evidence for (or with --contradicts, against) a claim, in the claim's scope. Evidence of the same lineage counts as one origin.",
        &[
            entry("CLAIM_ID", "Identifier (GUID) of a visible claim."),
            entry(
                "KIND",
                "execution, source_code, authoritative_documentation, project_decision, local_documentation, human_observation, user_assertion or llm_assertion.",
            ),
            entry(
                "REFERENCE",
                "Where the evidence can be checked (file, URL, run...).",
            ),
        ],
        &[
            entry(
                "--lineage ORIGIN",
                "Required. Common origin of related evidence.",
            ),
            ORIGIN,
            entry(
                "--contradicts",
                "Evidence against the claim. From a non-LLM origin, it caps confidence at medium.",
            ),
            entry(
                "--source SOURCE_ID",
                "Visible indexed source supporting the evidence; its hash is kept.",
            ),
            ACTOR,
            KEY,
        ],
        &[
            ID_ERROR,
            entry(
                "scope_boundary (4)",
                "The claim is not in the current scope.",
            ),
            entry(
                "source_unavailable (1)",
                "The source is missing, unreadable or rejected.",
            ),
            CONFLICT,
        ],
        "smf evidence add 7f8c1a3e-0000-4000-8000-000000000001 source_code src/Billing/Pagination.cs --lineage billing-repo --origin user",
    ),
    topic(
        "evidence list",
        "",
        "Visible evidence.",
        &[],
        &[],
        &[],
        "smf evidence list",
    ),
    topic(
        "evidence show",
        "ID",
        "One piece of evidence.",
        &[entry("ID", "Identifier (GUID) of a visible evidence.")],
        &[],
        &[ID_ERROR],
        "smf evidence show 7f8c1a3e-0000-4000-8000-000000000002",
    ),
    topic(
        "retex add",
        "--event TEXT --impact TEXT --next TEXT --origin user|llm",
        "Records a lesson learned (RETEX) as a draft; nothing is learned implicitly.",
        &[],
        &[
            entry("--event TEXT", "Required. What happened."),
            entry("--impact TEXT", "Required. What it caused."),
            entry("--next TEXT", "Required. What to do next time."),
            ORIGIN,
            ACTOR,
            KEY,
        ],
        &[
            entry("invalid_arguments (1)", "Missing option or unknown option."),
            CONFLICT,
        ],
        "smf retex add --event \"Cache cleared on deploy\" --impact \"Slow start\" --next \"Warm the cache\" --origin user",
    ),
    topic(
        "retex list",
        "",
        "Visible RETEX.",
        &[],
        &[],
        &[],
        "smf retex list",
    ),
    topic(
        "retex show",
        "ID",
        "One RETEX.",
        &[entry("ID", "Identifier (GUID) of a visible RETEX.")],
        &[],
        &[ID_ERROR],
        "smf retex show 7f8c1a3e-0000-4000-8000-000000000003",
    ),
    topic(
        "source add",
        "FILE --origin user|llm",
        "Declares a local UTF-8 text file (at most 1 MiB) as a source of the current scope: read once, hashed and indexed. Idempotent by path.",
        &[entry(
            "FILE",
            "Relative to the context directory; inside the current scope's directory and outside any narrower scope.",
        )],
        &[ORIGIN, ACTOR],
        &[
            entry("source_rejected (1)", "Empty, binary or larger than 1 MiB."),
            entry("invalid_path (1)", "Missing, a folder, or unreadable."),
            entry(
                "scope_boundary (4)",
                "Outside the scope's directory, in a narrower scope, or a source of another scope.",
            ),
            entry(
                "unsafe_path (4)",
                "Link, junction, network path, alias, or a file of .sermofur.",
            ),
        ],
        "smf source add docs/adr/0001.md --origin user",
    ),
    topic(
        "source list",
        "",
        "Visible sources.",
        &[],
        &[],
        &[],
        "smf source list",
    ),
    topic(
        "source show",
        "ID",
        "A source with its history.",
        &[entry("ID", "Identifier (GUID) of a visible source.")],
        &[],
        &[ID_ERROR],
        "smf source show 7f8c1a3e-0000-4000-8000-000000000004",
    ),
    topic(
        "source reindex",
        "[ID]",
        "Reads again the sources of the current scope (or one of them): unchanged, modified, missing, unreadable, rejected, restored, or skipped when another command changed it meanwhile.",
        &[entry(
            "ID",
            "Optional. Identifier (GUID) of a source of the current scope.",
        )],
        &[ACTOR],
        &[ID_ERROR],
        "smf source reindex",
    ),
    topic(
        "index rebuild",
        "",
        "Rebuilds the full-text index of the whole instance in one write transaction and reads every source again. Other commands wait up to 5 s, then fail with storage_busy.",
        &[],
        &[],
        &[entry(
            "storage_busy (3)",
            "Another command holds the instance: run again.",
        )],
        "smf index rebuild",
    ),
    topic(
        "recall",
        "QUESTION",
        "At most 3 explained results among visible claims, RETEX and source passages. The question is plain text; case and accents are ignored. Writes nothing.",
        &[entry("QUESTION", "Free text.")],
        &[entry("--limit 1-3", "Number of results. Default: 3.")],
        &[
            entry("invalid_arguments (1)", "Limit outside 1-3."),
            entry("invalid_input (1)", "No searchable term in the question."),
        ],
        "smf recall \"how does billing paginate\"",
    ),
    topic(
        "challenge",
        "CLAIM_ID | --text TEXT",
        "Signals about a claim or a text: contradictions, changed or unavailable sources, status, review date, and at most 3 close claims to confront. Decides nothing, writes nothing.",
        &[entry("CLAIM_ID", "Identifier (GUID) of a visible claim.")],
        &[entry(
            "--text TEXT",
            "A text that is not a claim yet, instead of CLAIM_ID.",
        )],
        &[ID_ERROR],
        "smf challenge --text \"Billing paginates with offsets\"",
    ),
    topic(
        "daemon install",
        "",
        "Installs the daemon as a service of your session (no administrator rights) and starts it. Run again: nothing changes when the same version is installed; another version or executable replaces the service.",
        &[],
        &[],
        &[
            entry(
                "service_manager_unavailable (3)",
                "No service manager in this session (container, WSL without systemd); the CLI keeps working directly.",
            ),
            entry(
                "service_install_failed (3)",
                "The service manager refused the service; the previous state is restored.",
            ),
            entry(
                "daemon_unavailable (3)",
                "The service is registered but the daemon did not answer; see its journal.",
            ),
        ],
        "smf daemon install",
    ),
    topic(
        "daemon uninstall",
        "",
        "Stops and removes the service. Instances, backups and the registry are left untouched.",
        &[],
        &[],
        &[],
        "smf daemon uninstall",
    ),
    topic(
        "daemon start",
        "",
        "Starts the installed service.",
        &[],
        &[],
        &[NOT_INSTALLED],
        "smf daemon start",
    ),
    topic(
        "daemon stop",
        "",
        "Stops the daemon; commands then run directly. Stop it before updating smf on Windows, and close the hosts that run smf mcp serve: both hold the files of the tool.",
        &[],
        &[],
        &[NOT_INSTALLED],
        "smf daemon stop",
    ),
    topic(
        "daemon restart",
        "",
        "Stops then starts the service: the way to load a new version of smf.",
        &[],
        &[],
        &[NOT_INSTALLED],
        "smf daemon restart",
    ),
    topic(
        "daemon status",
        "",
        "State of the daemon: absent, installed_stopped, running, version_mismatch, foreign_endpoint or service_manager_unavailable; whether a running daemon answers within 5 s (answering), version, process, start time, open instances, clients.",
        &[],
        &[],
        &[],
        "smf daemon status --json",
    ),
    topic(
        "daemon register",
        "",
        "Lets the daemon serve the instance found from the context directory, with the checks of a direct command. Idempotent.",
        &[],
        &[],
        &[
            entry(
                "invalid_registry (3)",
                "The registry cannot be read; it is left untouched.",
            ),
            REGISTRY_BUSY,
        ],
        "smf daemon register",
    ),
    topic(
        "daemon unregister",
        "",
        "Stops serving an instance, found from the context directory or registered at that exact path.",
        &[],
        &[],
        &[
            entry("not_registered (1)", "No such instance in the registry."),
            REGISTRY_BUSY,
        ],
        "smf daemon unregister --path ~/work",
    ),
    topic(
        "daemon instances",
        "",
        "Instances the daemon may serve; missing marks one no longer found where it was registered.",
        &[],
        &[],
        &[],
        "smf daemon instances",
    ),
    topic(
        "daemon run",
        "",
        "Serves in the foreground: what the service runs, useful to diagnose. A first Ctrl+C or SIGTERM lets the running commands finish, a second one stops at once. --supervise restarts the daemon after an abnormal exit (Windows service).",
        &[],
        &[entry(
            "--supervise",
            "Runs the daemon as a child and restarts it within a second after a crash.",
        )],
        &[
            entry(
                "daemon_already_running (3)",
                "A daemon already serves this user.",
            ),
            entry(
                "foreign_endpoint (4)",
                "The endpoint or its folder belongs to another account.",
            ),
        ],
        "smf daemon run",
    ),
    topic(
        "mcp install",
        "",
        "Declares the Sermofur MCP server to a host, changing only its own entry: .mcp.json of the context directory for Claude Code, the [mcp_servers.sermofur] table of .codex/config.toml (or of the Codex user configuration) for Codex. Idempotent; says what remains to do (daemon, registration, trust).",
        &[],
        &[MCP_HOST, MCP_SCOPE],
        &[entry(
            "invalid_mcp_config (3)",
            "The file cannot be changed safely (not a JSON object, mcpServers not an object, TOML the edit cannot handle); it is left untouched.",
        )],
        "smf mcp install --host codex",
    ),
    topic(
        "mcp uninstall",
        "",
        "Removes the Sermofur entry of the host configuration, and nothing else.",
        &[],
        &[MCP_HOST, MCP_SCOPE],
        &[entry(
            "invalid_mcp_config (3)",
            "The file cannot be read safely; it is left untouched.",
        )],
        "smf mcp uninstall",
    ),
    topic(
        "mcp serve",
        "",
        "The MCP server on stdio, started by the host (Claude Code, Codex), not by hand. Its tools run through the daemon in the scope of the project; what they write comes from an LLM.",
        &[],
        &[],
        &[],
        "smf mcp serve",
    ),
];

/// Errors any command may give, kept out of the per-command lists.
pub const COMMON_ERRORS: &str = "Any command: invalid_arguments (1) for a bad command line or a malformed ID, invalid_input (1) for an empty or blank text. On an instance: no_instance (2), migration_required (3, except migrate, root and doctor), storage_busy (3).";

/// Help asked by the command line (`CommandHelp.Find`): a command, a group, or `None` for the
/// general usage. Options and their values are skipped; nothing after `--` is looked at.
pub fn find(arguments: &[String]) -> Option<String> {
    let words = positionals(arguments);
    let first = words.first()?;
    let topic = words
        .get(1)
        .and_then(|second| topic_of(&format!("{first} {second}")))
        .or_else(|| topic_of(first));
    if let Some(topic) = topic {
        return Some(command(topic));
    }
    let group: Vec<&Topic> = TOPICS.iter().filter(|t| t.group() == *first).collect();
    (!group.is_empty()).then(|| group_help(first, &group))
}

fn topic_of(command: &str) -> Option<&'static Topic> {
    TOPICS.iter().find(|topic| topic.command == command)
}

fn positionals(arguments: &[String]) -> Vec<&str> {
    let mut words = Vec::new();
    let mut index = 0;
    while index < arguments.len() {
        let argument = arguments[index].as_str();
        if argument == END_OF_OPTIONS {
            break;
        }
        if argument.starts_with("--") {
            if CommandArguments::takes_value(arguments, index) {
                index += 1;
            }
        } else if argument != HELP_SHORTCUT && argument != VERSION_SHORTCUT {
            words.push(argument);
        }
        index += 1;
    }
    words
}

/// `StringBuilder.AppendLine`: the line, then the line break of the platform.
fn line(text: &mut String, value: &str) {
    text.push_str(value);
    text.push_str(NEW_LINE);
}

fn command(topic: &Topic) -> String {
    let mut text = String::new();
    line(&mut text, &header());
    line(
        &mut text,
        &format!(
            "Usage: smf [--path DIRECTORY] [--json] {}",
            topic.synopsis()
        ),
    );
    line(&mut text, "");
    line(&mut text, topic.summary);
    section(&mut text, "Arguments", topic.parameters);
    section(&mut text, "Options", topic.options);
    section(&mut text, "Errors (code, exit)", topic.errors);
    line(&mut text, "");
    line(&mut text, COMMON_ERRORS);
    line(&mut text, "");
    line(&mut text, "Example:");
    line(&mut text, &format!("  {}", topic.example));
    line(&mut text, "");
    text.push_str(&format!("Reference: {}", cli_documentation(VERSION)));
    text
}

fn group_help(name: &str, topics: &[&Topic]) -> String {
    let mut text = String::new();
    line(&mut text, &header());
    line(
        &mut text,
        &format!("Usage: smf [--path DIRECTORY] [--json] {name} SUBCOMMAND ..."),
    );
    line(&mut text, "");
    line(&mut text, "Subcommands:");
    // The catalogue is ASCII: a `char` is a UTF-16 unit of `PadRight`.
    let width = topics.iter().map(|t| t.synopsis().len()).max().unwrap_or(0);
    for topic in topics {
        let synopsis = topic.synopsis();
        line(
            &mut text,
            &format!("  {synopsis:<width$}  {}", first_sentence(topic.summary)),
        );
    }
    line(&mut text, "");
    text.push_str(&format!(
        "Run smf {name} SUBCOMMAND -h for the details of one subcommand."
    ));
    text
}

fn section(text: &mut String, title: &str, entries: &[Entry]) {
    if entries.is_empty() {
        return;
    }
    line(text, "");
    line(text, &format!("{title}:"));
    let width = entries.iter().map(|e| e.name.len()).max().unwrap_or(0);
    for entry in entries {
        line(
            text,
            &format!("  {:<width$}  {}", entry.name, entry.description),
        );
    }
}

fn first_sentence(summary: &str) -> &str {
    summary.find(". ").map_or(summary, |end| &summary[..=end])
}

#[cfg(test)]
mod tests {
    use super::{COMMON_ERRORS, TOPICS, cli_documentation, header, usage};

    /// Runs the command line in this process, as `TestInstance.Run`.
    fn run(values: &[&str]) -> (i32, String, String) {
        let arguments: Vec<String> = values.iter().map(|value| (*value).to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = crate::runner::run(&arguments, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("UTF-8"),
            String::from_utf8(err).expect("UTF-8"),
        )
    }

    /// Port of `CliTests.EveryCommandHasItsOwnHelp`.
    #[test]
    fn every_command_has_its_own_help() {
        let (_, usage, _) = run(&["-h"]);
        let temp = std::env::temp_dir();
        let temp = temp.to_str().expect("UTF-8 path");
        for topic in &TOPICS {
            // The general usage has a line for every command form, with every option.
            let words: Vec<&str> = topic.command.split(' ').collect();
            let lines: Vec<&str> = usage
                .split('\n')
                .map(str::trim_end)
                .filter(|text| {
                    text.starts_with(&format!("smf {}", topic.command))
                        || if words.len() > 1 {
                            text.starts_with(&format!("smf {}", topic.group()))
                                && text.contains(&format!("| {}", words[words.len() - 1]))
                        } else {
                            text.starts_with("smf init")
                                && text.contains(&format!("| {}", topic.command))
                        }
                })
                .collect();
            assert_eq!(lines.len(), 1, "{}: {lines:?}", topic.command);
            for option in topic.options {
                let name = option.name.split(' ').next().expect("a word");
                assert!(lines[0].contains(name), "{}: {name}", topic.command);
            }
            let mut arguments = vec!["--path", temp];
            arguments.extend(&words);
            arguments.push("-h");
            let (code, help, _) = run(&arguments);
            assert_eq!(code, 0);
            assert!(help.starts_with("Sermofur 0.0.0-dev"));
            assert!(help.contains(&format!(
                "Usage: smf [--path DIRECTORY] [--json] {}",
                topic.synopsis()
            )));
            assert!(help.contains(topic.summary));
            assert!(help.contains(topic.example));
            assert!(topic.example.starts_with("smf "));
            for entry in topic
                .options
                .iter()
                .chain(topic.parameters)
                .chain(topic.errors)
            {
                assert!(
                    help.contains(entry.name),
                    "{}: {}",
                    topic.command,
                    entry.name
                );
            }
            assert!(help.contains(COMMON_ERRORS));
        }
        let expected = [
            "init",
            "root",
            "status",
            "doctor",
            "export",
            "migrate",
            "scope current",
            "scope list",
            "scope tree",
            "scope add",
            "claim add",
            "claim list",
            "claim show",
            "claim invalidate",
            "evidence add",
            "evidence list",
            "evidence show",
            "retex add",
            "retex list",
            "retex show",
            "source add",
            "source list",
            "source show",
            "source reindex",
            "index rebuild",
            "recall",
            "challenge",
            "daemon install",
            "daemon uninstall",
            "daemon start",
            "daemon stop",
            "daemon restart",
            "daemon status",
            "daemon register",
            "daemon unregister",
            "daemon instances",
            "daemon run",
            "mcp install",
            "mcp uninstall",
            "mcp serve",
        ];
        let mut expected = expected.to_vec();
        expected.sort_unstable();
        let mut actual: Vec<&str> = TOPICS.iter().map(|topic| topic.command).collect();
        actual.sort_unstable();
        assert_eq!(actual, expected);
    }

    /// Port of `CliTests.GroupHelpListsItsSubcommands` (the four groups of the oracle, plus
    /// the four others).
    #[test]
    fn group_help_lists_its_subcommands() {
        for group in [
            "claim", "source", "scope", "index", "evidence", "retex", "daemon", "mcp",
        ] {
            let (code, help, _) = run(&[group, "-h"]);
            assert_eq!(code, 0);
            assert!(help.contains(&format!(
                "Usage: smf [--path DIRECTORY] [--json] {group} SUBCOMMAND"
            )));
            for topic in TOPICS.iter().filter(|topic| topic.group() == group) {
                assert!(
                    help.contains(&topic.synopsis()),
                    "{group}: {}",
                    topic.command
                );
            }
        }
    }

    /// Port of `CliTests.UnknownCommandHelpFallsBackToTheGeneralUsage`.
    #[test]
    fn unknown_command_help_falls_back_to_the_general_usage() {
        assert!(run(&["nothing", "-h"]).1.contains("Global options"));
        assert!(run(&["--path", ".", "-h"]).1.contains("Global options"));
    }

    /// Port of `CliTests.UsageGivesOneLinePerFormWithoutMixingOptionValuesAndSubcommands`.
    #[test]
    fn usage_gives_one_line_per_form_without_mixing_option_values_and_subcommands() {
        let (code, output, _) = run(&["--help"]);
        assert_eq!(code, 0);
        assert!(output.contains("smf claim list | show ID | invalidate ID --reason TEXT"));
        assert!(output.contains("smf evidence list | show ID"));
        assert!(output.contains("smf retex list | show ID"));
        assert!(
            output.contains("--help (-h) and --version (-v) are recognized anywhere before --")
        );
        assert!(!output.contains("llm|list"));
        assert!(!output.contains("tree|add"));
    }

    /// Port of `CliTests.UsageShowsTheBuildVersionAndTheMatchingCliReference`, without the
    /// anchor on the .NET assembly version.
    #[test]
    fn usage_shows_the_build_version_and_the_matching_cli_reference() {
        let (_, output, _) = run(&["--help"]);
        assert!(output.starts_with(&format!("Sermofur {}", sermofur_core::VERSION)));
        assert!(output.contains(&cli_documentation(sermofur_core::VERSION)));
    }

    /// Port of `CliTests.HelpAnywhereShowsUsageBeforeInstanceResolution`: in a folder without
    /// an instance, where the command would exit 2.
    #[test]
    fn help_anywhere_shows_usage_before_instance_resolution() {
        let lines: [&[&str]; 5] = [
            &["--help"],
            &["status", "--help"],
            &["claim", "add", "x", "--help", "--json"],
            &["--json", "--help", "--", "init"],
            &["claim", "add", "x", "--origin", "--help"],
        ];
        for line in lines {
            let outside = std::env::temp_dir().join(format!(
                "sermofur-help-{}",
                sermofur_core::guid::Uuid::new_v4().simple()
            ));
            std::fs::create_dir(&outside).expect("the folder");
            let mut arguments = vec!["--path", outside.to_str().expect("UTF-8 path")];
            arguments.extend(line);
            let (code, output, error) = run(&arguments);
            let created = outside.join(".sermofur").exists();
            std::fs::remove_dir_all(&outside).expect("removed");
            assert_eq!(code, 0, "{line:?}");
            assert!(output.starts_with("Sermofur 0.0.0-dev"), "{line:?}");
            assert_eq!(error, "", "{line:?}");
            assert!(!created, "{line:?}");
        }
    }

    /// The seconds written in the catalogue are those of the constants of the port.
    #[test]
    fn the_timeouts_of_the_catalogue_are_those_of_the_port() {
        let text = |command: &str| {
            let topic = TOPICS
                .iter()
                .find(|t| t.command == command)
                .expect("a topic");
            let errors: Vec<&str> = topic.errors.iter().map(|e| e.description).collect();
            format!("{} {}", topic.summary, errors.join(" "))
        };
        let lock = sermofur_daemon::registry::DEFAULT_LOCK_TIMEOUT.as_secs();
        let status = crate::daemon_commands::DEFAULT_STATUS_TIMEOUT.as_secs();
        assert!(text("daemon register").contains(&format!("for {lock} s,")));
        assert!(text("daemon unregister").contains(&format!("for {lock} s,")));
        assert!(text("daemon status").contains(&format!("within {status} s ")));
    }

    #[test]
    fn header_names_the_tool_and_its_version() {
        assert_eq!(header(), "Sermofur 0.0.0-dev");
    }

    #[test]
    fn a_prerelease_points_to_the_documentation_of_main() {
        assert_eq!(
            cli_documentation("0.0.0-dev"),
            "https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md"
        );
        assert_eq!(
            cli_documentation("0.4.3"),
            "https://github.com/JoRouquette/sermofur/blob/v0.4.3/docs/cli.md"
        );
    }

    #[test]
    fn usage_starts_with_the_header_and_ends_with_the_documentation_link() {
        let text = usage();
        assert!(text.starts_with("Sermofur 0.0.0-dev\nGlobal options:"));
        assert!(text.ends_with("docs/cli.md for values and exit codes."));
    }
}
