using System.Text;

namespace Sermofur.Cli;

/// <summary>One line of a help section: a name and what it means.</summary>
public sealed record HelpEntry(string Name, string Description);

/// <summary>
/// Help of one command form (<c>claim add</c>, <c>recall</c>...): what <c>smf ... -h</c> prints.
/// </summary>
public sealed record CommandTopic(
    string Command,
    string Arguments,
    string Summary,
    IReadOnlyList<HelpEntry> Parameters,
    IReadOnlyList<HelpEntry> Options,
    IReadOnlyList<HelpEntry> Errors,
    string Example
)
{
    /// <summary>First word: the command or the group of subcommands.</summary>
    public string Group => Command.Split(' ')[0];

    public string Synopsis => Arguments.Length == 0 ? Command : $"{Command} {Arguments}";
}

/// <summary>
/// Catalogue of the help of every command: the source of the help of a group and of a command.
/// The general usage stays a hand-written summary; a test checks that it names every command
/// form and option of this catalogue.
/// </summary>
public static class CommandHelp
{
    private static readonly HelpEntry Origin = new HelpEntry(
        "--origin user|llm",
        "Required, no default. Declared origin of the record."
    );

    private static readonly HelpEntry Actor = new HelpEntry(
        "--actor NAME",
        "Name recorded in the history. Default: local-user."
    );

    private static readonly HelpEntry Key = new HelpEntry(
        "--key KEY",
        "Idempotency key: the same key with the same content returns the same object."
    );

    private static readonly HelpEntry Conflict = new HelpEntry(
        "idempotency_conflict (1)",
        "Key reused with another content."
    );

    private static readonly HelpEntry IdError = new HelpEntry(
        "not_found (1)",
        "Unknown identifier, or one of a scope that is not visible."
    );

    private static readonly HelpEntry McpHost = new HelpEntry(
        "--host claude-code|codex",
        "MCP host. Default: claude-code."
    );

    private static readonly HelpEntry McpScope = new HelpEntry(
        "--scope project|user",
        "Codex only: configuration of the project (.codex/config.toml, loaded when the project is trusted) or of the user. Default: project."
    );

    private static IReadOnlyList<HelpEntry> Entries(params HelpEntry[] entries) => entries;

    private static HelpEntry Id(string name, string what) =>
        new HelpEntry(name, $"Identifier (GUID) of a visible {what}.");

    public static IReadOnlyList<CommandTopic> Topics { get; } =
    [
        new(
            "init",
            "",
            "Creates an instance (.sermofur) in the context directory, or returns the existing one. Refused inside another instance.",
            [],
            [],
            Entries(
                new HelpEntry("invalid_path (1)", "The directory does not exist."),
                new HelpEntry("nested_instance (4)", "A valid instance exists above."),
                new HelpEntry(
                    "invalid_instance (3)",
                    "The nearest .sermofur entry is not a valid instance."
                ),
                new HelpEntry("unsafe_path (4)", "Network path, link or junction.")
            ),
            "smf init"
        ),
        new(
            "root",
            "",
            "Prints the root of the nearest instance going up from the context directory.",
            [],
            [],
            [],
            "smf root"
        ),
        new(
            "status",
            "",
            "Identity of the instance, current scope and counts of the visible objects.",
            [],
            [],
            [],
            "smf status --json"
        ),
        new(
            "doctor",
            "",
            "Read-only health check: integrity, schema, scopes, mappings, index, projections. Exit 5 when unhealthy, 0 when healthy, with or without warnings.",
            [],
            [],
            Entries(new HelpEntry("unsafe_path (4)", "The .sermofur entry is a link.")),
            "smf doctor"
        ),
        new(
            "export",
            "",
            "Rebuilds the Markdown projections of the visible objects and lists their files.",
            [],
            [],
            [],
            "smf export"
        ),
        new(
            "migrate",
            "",
            "Migrates an instance from format 1 (0.1) to format 2: consistency check, backup in .sermofur/backups/, one transaction. Resumes after an interruption; nothing to do on format 2.",
            [],
            [],
            Entries(
                new HelpEntry(
                    "storage_error (3)",
                    "The instance is not consistent: run smf doctor first."
                )
            ),
            "smf migrate"
        ),
        new(
            "scope current",
            "",
            "Current scope (from the context directory) and its visible ancestors.",
            [],
            [],
            [],
            "smf scope current"
        ),
        new(
            "scope list",
            "",
            "Visible scopes: the current one and its ancestors, never siblings or descendants.",
            [],
            [],
            [],
            "smf scope list"
        ),
        new("scope tree", "", "Same as scope list in this version.", [], [], [], "smf scope tree"),
        new(
            "scope add",
            "ID KIND PARENT RELATIVE_PATH",
            "Registers a direct child of the current scope, mapped to an existing directory. The mapping is stored as the folder is named on disk.",
            Entries(
                new HelpEntry("ID", "New identifier: [a-z][a-z0-9-]{0,63}."),
                new HelpEntry(
                    "KIND",
                    "client, project, repository or task, compatible with the parent."
                ),
                new HelpEntry("PARENT", "The current scope (workspace at the instance root)."),
                new HelpEntry(
                    "RELATIVE_PATH",
                    "Directory relative to the instance root, inside the parent's directory."
                )
            ),
            [],
            Entries(
                new HelpEntry("invalid_scope (1)", "Bad identifier or kind, or identifier taken."),
                new HelpEntry(
                    "duplicate_mapping (1)",
                    "Directory already mapped by another scope."
                ),
                new HelpEntry("invalid_path (1)", "The directory does not exist."),
                new HelpEntry(
                    "scope_boundary (4)",
                    "Wrong parent, or mapping outside the parent or overlapping another branch."
                ),
                new HelpEntry(
                    "unsafe_path (4)",
                    "Absolute path, drive, link, junction or network path, or an alias."
                )
            ),
            "smf scope add acme client workspace acme"
        ),
        new(
            "claim add",
            "TEXT --origin user|llm",
            "Records a proposed claim in the current scope. Its confidence is computed when read, from its evidence.",
            Entries(
                new HelpEntry(
                    "TEXT",
                    "The statement. Put it after -- if it starts with -- or is -h or -v."
                )
            ),
            Entries(
                Origin,
                new HelpEntry(
                    "--category CATEGORY",
                    "episodic, semantic, procedural, preferences or decisions. Default: semantic. preferences and decisions require --origin user."
                ),
                new HelpEntry(
                    "--volatility VOLATILITY",
                    "stable, evolving or volatile. Default: evolving."
                ),
                Actor,
                Key
            ),
            Entries(
                new HelpEntry(
                    "invalid_arguments (1)",
                    "Missing origin, bad value or unknown option."
                ),
                new HelpEntry(
                    "user_choice_required (1)",
                    "preferences or decisions with --origin llm."
                ),
                Conflict
            ),
            "smf claim add \"The billing API paginates with cursors\" --origin user"
        ),
        new("claim list", "", "Visible claims.", [], [], [], "smf claim list --json"),
        new(
            "claim show",
            "ID",
            "The claim with its evidence, its explained confidence and its history.",
            Entries(Id("ID", "claim")),
            [],
            Entries(IdError),
            "smf claim show 7f8c1a3e-0000-4000-8000-000000000001"
        ),
        new(
            "claim invalidate",
            "ID --reason TEXT",
            "Invalidates a claim of the current scope, with an auditable reason.",
            Entries(Id("ID", "claim of the current scope")),
            Entries(
                new HelpEntry("--reason TEXT", "Required. Why the claim no longer holds."),
                Actor
            ),
            Entries(
                IdError,
                new HelpEntry("scope_boundary (4)", "The claim belongs to an ancestor scope.")
            ),
            "smf claim invalidate 7f8c1a3e-0000-4000-8000-000000000001 --reason \"Replaced by cursors v2\""
        ),
        new(
            "evidence add",
            "CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm",
            "Records evidence for (or with --contradicts, against) a claim, in the claim's scope. Evidence of the same lineage counts as one origin.",
            Entries(
                Id("CLAIM_ID", "claim"),
                new HelpEntry(
                    "KIND",
                    "execution, source_code, authoritative_documentation, project_decision, local_documentation, human_observation, user_assertion or llm_assertion."
                ),
                new HelpEntry("REFERENCE", "Where the evidence can be checked (file, URL, run...).")
            ),
            Entries(
                new HelpEntry("--lineage ORIGIN", "Required. Common origin of related evidence."),
                Origin,
                new HelpEntry(
                    "--contradicts",
                    "Evidence against the claim. From a non-LLM origin, it caps confidence at medium."
                ),
                new HelpEntry(
                    "--source SOURCE_ID",
                    "Visible indexed source supporting the evidence; its hash is kept."
                ),
                Actor,
                Key
            ),
            Entries(
                IdError,
                new HelpEntry("scope_boundary (4)", "The claim is not in the current scope."),
                new HelpEntry(
                    "source_unavailable (1)",
                    "The source is missing, unreadable or rejected."
                ),
                Conflict
            ),
            "smf evidence add 7f8c1a3e-0000-4000-8000-000000000001 source_code src/Billing/Pagination.cs --lineage billing-repo --origin user"
        ),
        new("evidence list", "", "Visible evidence.", [], [], [], "smf evidence list"),
        new(
            "evidence show",
            "ID",
            "One piece of evidence.",
            Entries(Id("ID", "evidence")),
            [],
            Entries(IdError),
            "smf evidence show 7f8c1a3e-0000-4000-8000-000000000002"
        ),
        new(
            "retex add",
            "--event TEXT --impact TEXT --next TEXT --origin user|llm",
            "Records a lesson learned (RETEX) as a draft; nothing is learned implicitly.",
            [],
            Entries(
                new HelpEntry("--event TEXT", "Required. What happened."),
                new HelpEntry("--impact TEXT", "Required. What it caused."),
                new HelpEntry("--next TEXT", "Required. What to do next time."),
                Origin,
                Actor,
                Key
            ),
            Entries(
                new HelpEntry("invalid_arguments (1)", "Missing option or unknown option."),
                Conflict
            ),
            "smf retex add --event \"Cache cleared on deploy\" --impact \"Slow start\" --next \"Warm the cache\" --origin user"
        ),
        new("retex list", "", "Visible RETEX.", [], [], [], "smf retex list"),
        new(
            "retex show",
            "ID",
            "One RETEX.",
            Entries(Id("ID", "RETEX")),
            [],
            Entries(IdError),
            "smf retex show 7f8c1a3e-0000-4000-8000-000000000003"
        ),
        new(
            "source add",
            "FILE --origin user|llm",
            "Declares a local UTF-8 text file (at most 1 MiB) as a source of the current scope: read once, hashed and indexed. Idempotent by path.",
            Entries(
                new HelpEntry(
                    "FILE",
                    "Relative to the context directory; inside the current scope's directory and outside any narrower scope."
                )
            ),
            Entries(Origin, Actor),
            Entries(
                new HelpEntry("source_rejected (1)", "Empty, binary or larger than 1 MiB."),
                new HelpEntry("invalid_path (1)", "Missing, a folder, or unreadable."),
                new HelpEntry(
                    "scope_boundary (4)",
                    "Outside the scope's directory, in a narrower scope, or a source of another scope."
                ),
                new HelpEntry(
                    "unsafe_path (4)",
                    "Link, junction, network path, alias, or a file of .sermofur."
                )
            ),
            "smf source add docs/adr/0001.md --origin user"
        ),
        new("source list", "", "Visible sources.", [], [], [], "smf source list"),
        new(
            "source show",
            "ID",
            "A source with its history.",
            Entries(Id("ID", "source")),
            [],
            Entries(IdError),
            "smf source show 7f8c1a3e-0000-4000-8000-000000000004"
        ),
        new(
            "source reindex",
            "[ID]",
            "Reads again the sources of the current scope (or one of them): unchanged, modified, missing, unreadable, rejected, restored, or skipped when another command changed it meanwhile.",
            Entries(
                new HelpEntry("ID", "Optional. Identifier (GUID) of a source of the current scope.")
            ),
            Entries(Actor),
            Entries(IdError),
            "smf source reindex"
        ),
        new(
            "index rebuild",
            "",
            "Rebuilds the full-text index of the whole instance in one write transaction and reads every source again. Other commands wait up to 5 s, then fail with storage_busy.",
            [],
            [],
            Entries(
                new HelpEntry("storage_busy (3)", "Another command holds the instance: run again.")
            ),
            "smf index rebuild"
        ),
        new(
            "recall",
            "QUESTION",
            "At most 3 explained results among visible claims, RETEX and source passages. The question is plain text; case and accents are ignored. Writes nothing.",
            Entries(new HelpEntry("QUESTION", "Free text.")),
            Entries(new HelpEntry("--limit 1-3", "Number of results. Default: 3.")),
            Entries(
                new HelpEntry("invalid_arguments (1)", "Limit outside 1-3."),
                new HelpEntry("invalid_input (1)", "No searchable term in the question.")
            ),
            "smf recall \"how does billing paginate\""
        ),
        new(
            "challenge",
            "CLAIM_ID | --text TEXT",
            "Signals about a claim or a text: contradictions, changed or unavailable sources, status, review date, and at most 3 close claims to confront. Decides nothing, writes nothing.",
            Entries(Id("CLAIM_ID", "claim")),
            Entries(
                new HelpEntry("--text TEXT", "A text that is not a claim yet, instead of CLAIM_ID.")
            ),
            Entries(IdError),
            "smf challenge --text \"Billing paginates with offsets\""
        ),
        new(
            "daemon install",
            "",
            "Installs the daemon as a service of your session (no administrator rights) and starts it. Run again: nothing changes when the same version is installed; another version or executable replaces the service.",
            [],
            [],
            Entries(
                new HelpEntry(
                    "service_manager_unavailable (3)",
                    "No service manager in this session (container, WSL without systemd); the CLI keeps working directly."
                ),
                new HelpEntry(
                    "service_install_failed (3)",
                    "The service manager refused the service; the previous state is restored."
                ),
                new HelpEntry(
                    "daemon_unavailable (3)",
                    "The service is registered but the daemon did not answer; see its journal."
                )
            ),
            "smf daemon install"
        ),
        new(
            "daemon uninstall",
            "",
            "Stops and removes the service. Instances, backups and the registry are left untouched.",
            [],
            [],
            [],
            "smf daemon uninstall"
        ),
        new(
            "daemon start",
            "",
            "Starts the installed service.",
            [],
            [],
            Entries(new HelpEntry("daemon_unavailable (3)", "The service is not installed.")),
            "smf daemon start"
        ),
        new(
            "daemon stop",
            "",
            "Stops the daemon; commands then run directly. Stop it before updating smf on Windows.",
            [],
            [],
            Entries(new HelpEntry("daemon_unavailable (3)", "The service is not installed.")),
            "smf daemon stop"
        ),
        new(
            "daemon restart",
            "",
            "Stops then starts the service: the way to load a new version of smf.",
            [],
            [],
            Entries(new HelpEntry("daemon_unavailable (3)", "The service is not installed.")),
            "smf daemon restart"
        ),
        new(
            "daemon status",
            "",
            "State of the daemon: absent, installed_stopped, running, version_mismatch, foreign_endpoint or service_manager_unavailable; version, process, start time, open instances, clients.",
            [],
            [],
            [],
            "smf daemon status --json"
        ),
        new(
            "daemon register",
            "",
            "Lets the daemon serve the instance found from the context directory, with the checks of a direct command. Idempotent.",
            [],
            [],
            Entries(
                new HelpEntry(
                    "invalid_registry (3)",
                    "The registry cannot be read; it is left untouched."
                )
            ),
            "smf daemon register"
        ),
        new(
            "daemon unregister",
            "",
            "Stops serving an instance, found from the context directory or registered at that exact path.",
            [],
            [],
            Entries(new HelpEntry("not_registered (1)", "No such instance in the registry.")),
            "smf daemon unregister --path ~/work"
        ),
        new(
            "daemon instances",
            "",
            "Instances the daemon may serve; missing marks one no longer found where it was registered.",
            [],
            [],
            [],
            "smf daemon instances"
        ),
        new(
            "daemon run",
            "",
            "Serves in the foreground until Ctrl+C: what the service runs, useful to diagnose. --supervise restarts the daemon after an abnormal exit (Windows service).",
            [],
            Entries(
                new HelpEntry(
                    "--supervise",
                    "Runs the daemon as a child and restarts it within a second after a crash."
                )
            ),
            Entries(
                new HelpEntry("daemon_already_running (3)", "A daemon already serves this user."),
                new HelpEntry(
                    "foreign_endpoint (4)",
                    "The endpoint or its folder belongs to another account."
                )
            ),
            "smf daemon run"
        ),
        new(
            "mcp install",
            "",
            "Declares the Sermofur MCP server to a host, changing only its own entry: .mcp.json of the context directory for Claude Code, the [mcp_servers.sermofur] table of .codex/config.toml (or of the Codex user configuration) for Codex. Idempotent; says what remains to do (daemon, registration, trust).",
            [],
            Entries(McpHost, McpScope),
            Entries(
                new HelpEntry(
                    "invalid_mcp_config (3)",
                    "The file cannot be changed safely (not a JSON object, mcpServers not an object, TOML the edit cannot handle); it is left untouched."
                )
            ),
            "smf mcp install --host codex"
        ),
        new(
            "mcp uninstall",
            "",
            "Removes the Sermofur entry of the host configuration, and nothing else.",
            [],
            Entries(McpHost, McpScope),
            Entries(
                new HelpEntry(
                    "invalid_mcp_config (3)",
                    "The file cannot be read safely; it is left untouched."
                )
            ),
            "smf mcp uninstall"
        ),
        new(
            "mcp serve",
            "",
            "The MCP server on stdio, started by the host (Claude Code), not by hand. Its tools run through the daemon in the scope of the project; what they write comes from an LLM.",
            [],
            [],
            [],
            "smf mcp serve"
        ),
    ];

    /// <summary>Errors any command may give, kept out of the per-command lists.</summary>
    public const string CommonErrors =
        "Any command: invalid_arguments (1) for a bad command line or a malformed ID, invalid_input (1) for an empty or blank text. On an instance: no_instance (2), migration_required (3, except migrate, root and doctor), storage_busy (3).";

    /// <summary>Header shared by every help output.</summary>
    public static string Header => $"Sermofur {ProductVersion.Current}";

    /// <summary>
    /// Help asked by the command line: a command, a group, or null for the general usage.
    /// Options and their values are skipped; nothing after <c>--</c> is looked at.
    /// </summary>
    public static string? Find(IReadOnlyList<string> arguments)
    {
        List<string> words = Positionals(arguments);
        if (words.Count == 0)
        {
            return null;
        }
        CommandTopic? topic =
            (words.Count > 1 ? Topic($"{words[0]} {words[1]}") : null) ?? Topic(words[0]);
        if (topic is not null)
        {
            return Command(topic);
        }
        CommandTopic[] group = Topics.Where(topic => topic.Group == words[0]).ToArray();
        return group.Length == 0 ? null : Group(words[0], group);
    }

    private static CommandTopic? Topic(string command) =>
        Topics.FirstOrDefault(topic => topic.Command == command);

    private static List<string> Positionals(IReadOnlyList<string> arguments)
    {
        List<string> words = [];
        for (int index = 0; index < arguments.Count; index++)
        {
            string argument = arguments[index];
            if (argument == CommandArguments.EndOfOptions)
            {
                break;
            }
            if (CommandArguments.IsShortcut(argument))
            {
                continue;
            }
            if (argument.StartsWith("--", StringComparison.Ordinal))
            {
                if (CommandArguments.TakesValue(arguments, index))
                {
                    index++;
                }
                continue;
            }
            words.Add(argument);
        }
        return words;
    }

    private static string Command(CommandTopic topic)
    {
        StringBuilder text = new();
        text.AppendLine(Header);
        text.AppendLine($"Usage: smf [--path DIRECTORY] [--json] {topic.Synopsis}");
        text.AppendLine();
        text.AppendLine(topic.Summary);
        Section(text, "Arguments", topic.Parameters);
        Section(text, "Options", topic.Options);
        Section(text, "Errors (code, exit)", topic.Errors);
        text.AppendLine();
        text.AppendLine(CommonErrors);
        text.AppendLine();
        text.AppendLine("Example:");
        text.AppendLine($"  {topic.Example}");
        text.AppendLine();
        text.Append($"Reference: {ProductVersion.CliDocumentation(ProductVersion.Current)}");
        return text.ToString();
    }

    private static string Group(string name, IReadOnlyList<CommandTopic> topics)
    {
        StringBuilder text = new();
        text.AppendLine(Header);
        text.AppendLine($"Usage: smf [--path DIRECTORY] [--json] {name} SUBCOMMAND ...");
        text.AppendLine();
        text.AppendLine("Subcommands:");
        int width = topics.Max(topic => topic.Synopsis.Length);
        foreach (CommandTopic topic in topics)
        {
            text.AppendLine($"  {topic.Synopsis.PadRight(width)}  {FirstSentence(topic.Summary)}");
        }
        text.AppendLine();
        text.Append($"Run smf {name} SUBCOMMAND -h for the details of one subcommand.");
        return text.ToString();
    }

    private static void Section(StringBuilder text, string title, IReadOnlyList<HelpEntry> entries)
    {
        if (entries.Count == 0)
        {
            return;
        }
        text.AppendLine();
        text.AppendLine($"{title}:");
        int width = entries.Max(entry => entry.Name.Length);
        foreach (HelpEntry entry in entries)
        {
            text.AppendLine($"  {entry.Name.PadRight(width)}  {entry.Description}");
        }
    }

    private static string FirstSentence(string summary)
    {
        int end = summary.IndexOf(". ", StringComparison.Ordinal);
        return end < 0 ? summary : summary[..(end + 1)];
    }
}
