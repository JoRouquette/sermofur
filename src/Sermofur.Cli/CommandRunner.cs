using System.Text.Json;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;
using Sermofur.Mcp;

namespace Sermofur.Cli;

/// <param name="mode"><c>daemon</c> when the daemon runs the command for a client, <c>direct</c>
/// otherwise; reported by <c>status</c>.</param>
public sealed class CommandRunner(TextWriter output, TextWriter error, string mode = "direct")
{
    private static string CliDocumentation =>
        ProductVersion.CliDocumentation(ProductVersion.Current);

    private static readonly string Usage = $"""
        {CommandHelp.Header}
        Global options: [--path DIRECTORY] [--json]
        smf init | root | status | doctor | export | migrate
        smf scope current | list | tree
        smf scope add ID KIND PARENT RELATIVE_PATH
        smf claim add TEXT --origin user|llm [--category CATEGORY] [--volatility VOLATILITY] [--actor NAME] [--key KEY]
        smf claim list | show ID | invalidate ID --reason TEXT [--actor NAME]
        smf evidence add CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm [--contradicts] [--source SOURCE_ID] [--actor NAME] [--key KEY]
        smf evidence list | show ID
        smf retex add --event TEXT --impact TEXT --next TEXT --origin user|llm [--actor NAME] [--key KEY]
        smf retex list | show ID
        smf source add FILE --origin user|llm [--actor NAME]
        smf source list | show ID | reindex [ID] [--actor NAME]
        smf index rebuild
        smf recall QUESTION [--limit 1-3]
        smf challenge CLAIM_ID | challenge --text TEXT
        smf daemon install | uninstall | start | stop | restart | status
        smf daemon register | unregister | instances
        smf daemon run [--supervise]
        smf mcp install | uninstall | serve [--host claude-code|codex] [--scope project|user]
        smf COMMAND -h | smf GROUP -h       help of one command, or the subcommands of a group
        smf --version | -v                  version
        --help (-h) and --version (-v) are recognized anywhere before --, except as an option value; --help wins.
        -- ends options: every following argument is positional (text starting with --).
        An option value cannot start with --; see {CliDocumentation} for values and exit codes.
        """;

    /// <summary>Runs a command from the current directory of this process.</summary>
    public int Run(string[] arguments) => Run(arguments, Directory.GetCurrentDirectory());

    /// <summary>
    /// Runs a command as if launched from <paramref name="workingDirectory"/>: the daemon runs
    /// the commands of its clients this way, so nothing here may read the current directory of
    /// the process (research R4).
    /// </summary>
    public int Run(string[] arguments, string workingDirectory)
    {
        // Read ahead: a parsing error must already honour the requested format.
        bool json = CommandArguments.HasFlag(arguments, "json");
        try
        {
            if (
                arguments.Length == 0
                || CommandArguments.Asks(
                    arguments,
                    CommandArguments.Help,
                    CommandArguments.HelpShortcut
                )
            )
            {
                output.WriteLine(CommandHelp.Find(arguments) ?? Usage);
                return 0;
            }
            if (
                CommandArguments.Asks(
                    arguments,
                    CommandArguments.Version,
                    CommandArguments.VersionShortcut
                )
            )
            {
                output.WriteLine(
                    json
                        ? RecordJson.Write(new { version = ProductVersion.Current })
                        : CommandHelp.Header
                );
                return 0;
            }
            CommandArguments parsed = new CommandArguments(arguments);
            if (parsed.Positionals.Count > 0 && parsed.Positionals[0] == "daemon")
            {
                return new DaemonCommands(output, Write).Run(parsed, json, workingDirectory);
            }
            if (parsed.Positionals.Count > 0 && parsed.Positionals[0] == "mcp")
            {
                return new McpCommands(Write).Run(parsed, json, workingDirectory);
            }
            return Execute(parsed, json, workingDirectory);
        }
        catch (SermofurException exception)
        {
            WriteError(exception.Code, exception.Message, json);
            return exception.ExitCode;
        }
        catch (SqliteException exception)
        {
            string code = exception.SqliteErrorCode is 5 or 6 ? "storage_busy" : "storage_error";
            WriteError(code, "Storage unavailable or inconsistent; run doctor.", json);
            return 3;
        }
        catch (Exception exception)
            when (exception
                    is JsonException
                        or IOException
                        or UnauthorizedAccessException
                        or ArgumentException
                        or InvalidOperationException
            )
        {
            WriteError("invalid_storage_or_path", "Invalid path or data; run doctor.", json);
            return 3;
        }
    }

    private int Execute(CommandArguments args, bool json, string workingDirectory)
    {
        string path = Path.GetFullPath(args.Option("path", workingDirectory)!, workingDirectory);
        if (args.Positionals.Count == 0)
        {
            throw new SermofurException("invalid_arguments", "Command required.");
        }
        string command = args.Positionals[0];
        InstanceManager manager = new();
        if (command == "init")
        {
            args.RequireCount(1);
            args.ValidateUsed();
            Write(manager.Initialize(path), json);
            return 0;
        }
        if (command == "doctor")
        {
            // The doctor command reports an invalid .sermofur entry (foreign, damaged, unreadable)
            // instead of failing before its report; a link is still refused.
            return RunInstanceCommand(args, manager.DiscoverForDiagnosis(path), json);
        }
        string root = manager.Discover(path);
        if (command == "root")
        {
            return RunInstanceCommand(args, root, json);
        }
        if (command == "migrate")
        {
            args.RequireCount(1);
            args.ValidateUsed();
            Write(InstanceMigration.Migrate(root), json);
            return 0;
        }
        InstanceConfiguration config = manager.ReadConfiguration(root);
        bool knowledge = KnowledgeCommands.IsKnowledgeCommand(command);
        using SqliteStore store = new(root, config.InstanceId, readOnly: !Writes(args));
        store.ValidateSchema();
        MemoryContext context = manager.ResolveContext(path, root, store.ReadScopes());
        MemoryService memory = new(store, context);
        if (knowledge)
        {
            Write(KnowledgeCommands.Execute(args, store, memory, context, path), json);
            return 0;
        }
        ScopeService scopes = new(store, new LocalPathResolver(), context);
        Write(Dispatch(args, memory, scopes, context, config), json);
        return 0;
    }

    /// <summary>
    /// True when the command may write to the instance: the store is then opened read-write, and
    /// the daemon runs it alone on its instance.
    /// </summary>
    internal static bool Writes(CommandArguments args)
    {
        string command = args.Positionals[0];
        if (command is "init" or "migrate")
        {
            return true;
        }
        return KnowledgeCommands.IsKnowledgeCommand(command)
            ? KnowledgeCommands.Writes(args)
            : command == "export"
                || (args.Positionals.Count > 1 && args.Positionals[1] is "add" or "invalidate");
    }

    private int RunInstanceCommand(CommandArguments args, string root, bool json)
    {
        args.RequireCount(1);
        args.ValidateUsed();
        if (args.Positionals[0] == "root")
        {
            Write(json ? new { root } : root, json);
            return 0;
        }
        DoctorReport report = new InstanceDoctor(
            null,
            () =>
                DaemonProbe.Check(
                    DaemonPaths.ForCurrentUser,
                    new FileOwnership(),
                    ProductVersion.Current
                ),
            instanceRoot =>
                McpDeclaration.Declarations(instanceRoot) is { Count: > 0 } declared
                    ? new DiagnosticCheck(
                        "mcp",
                        "ok",
                        $"Declared to {string.Join(", ", declared)}."
                    )
                    : new DiagnosticCheck(
                        "mcp",
                        "warning",
                        "Not declared to an MCP host: smf mcp install, or smf mcp install --host codex."
                    )
        ).Inspect(root);
        Write(report, json);
        return report.Overall == "unhealthy" ? 5 : 0;
    }

    private object Dispatch(
        CommandArguments args,
        MemoryService memory,
        ScopeService scopes,
        MemoryContext context,
        InstanceConfiguration config
    )
    {
        string command = args.Positionals[0];
        if (command == "status")
        {
            args.RequireCount(1);
            args.ValidateUsed();
            IReadOnlyList<MemoryRecord> records = memory.List();
            return new
            {
                root = context.Root,
                configuration = config,
                scope = context.ScopeId,
                claims = records.Count(r => r.Kind == RecordKind.Claim),
                evidence = records.Count(r => r.Kind == RecordKind.Evidence),
                retex = records.Count(r => r.Kind == RecordKind.Retex),
                sources = records.Count(r => r.Kind == RecordKind.Source),
                mode,
                laya = "unavailable",
            };
        }
        if (command == "scope")
        {
            return RunScope(args, scopes);
        }
        if (command == "export")
        {
            args.RequireCount(1);
            args.ValidateUsed();
            return new { files = memory.Export() };
        }
        if (command is not ("claim" or "evidence" or "retex"))
        {
            throw new SermofurException("invalid_arguments", "Unknown command.");
        }
        return MemoryCommands.Execute(
            args,
            memory,
            CommandArguments.ParseEnum<RecordKind>(command)
        );
    }

    private static object RunScope(CommandArguments args, ScopeService scopes)
    {
        if (args.Positionals.Count < 2)
        {
            throw new SermofurException("invalid_arguments", "Scope subcommand required.");
        }
        string action = args.Positionals[1];
        if (action == "add")
        {
            args.RequireCount(6);
            args.ValidateUsed();
            Scope candidate = new(
                args.Positionals[2],
                CommandArguments.ParseEnum<ScopeKind>(args.Positionals[3], "invalid_scope"),
                args.Positionals[4],
                args.Positionals[5]
            );
            return scopes.Register(candidate);
        }
        args.RequireCount(2);
        args.ValidateUsed();
        return action switch
        {
            "current" => scopes.Current(),
            "list" or "tree" => scopes.List(),
            _ => throw new SermofurException("invalid_arguments", "Unknown scope subcommand."),
        };
    }

    private void Write(object result, bool json)
    {
        if (result is DoctorReport report && !json)
        {
            output.WriteLine("Sermofur Doctor");
            foreach (DiagnosticCheck check in report.Checks)
            {
                output.WriteLine($"  {check.Status}: {check.Name} — {check.Detail}");
            }
            output.WriteLine($"Overall: {report.Overall}");
        }
        else if (result is string text && !json)
        {
            output.WriteLine(text);
        }
        else
        {
            output.WriteLine(RecordJson.Write(result));
        }
    }

    private void WriteError(string code, string message, bool json) =>
        error.WriteLine(json ? RecordJson.Write(new { code, message }) : $"smf: {code}: {message}");
}
