using System.Text.Json;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Cli;

public sealed class CommandRunner(TextWriter output, TextWriter error)
{
    private const string Usage = """
        Sermofur 0.1
        Global options: [--path DIRECTORY] [--json]
        smf init | root | status | doctor | export
        smf scope current | list | tree
        smf scope add ID KIND PARENT RELATIVE_PATH
        smf claim add TEXT --origin user|llm [--category CATEGORY] [--volatility VOLATILITY] [--actor NAME] [--key KEY]
        smf claim list | show ID | invalidate ID --reason TEXT [--actor NAME]
        smf evidence add CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm [--actor NAME] [--key KEY]
        smf evidence list | show ID
        smf retex add --event TEXT --impact TEXT --next TEXT --origin user|llm [--actor NAME] [--key KEY]
        smf retex list | show ID
        --help is recognized anywhere before -- and prints this help.
        -- ends options: every following argument is positional (text starting with --).
        An option value cannot start with --; see https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md for values and exit codes.
        """;

    public int Run(string[] arguments)
    {
        // Read ahead: a parsing error must already honour the requested format.
        bool json = CommandArguments.HasFlag(arguments, "json");
        try
        {
            if (arguments.Length == 0 || CommandArguments.HasFlag(arguments, "help"))
            {
                output.WriteLine(Usage);
                return 0;
            }
            return Execute(new CommandArguments(arguments), json);
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

    private int Execute(CommandArguments args, bool json)
    {
        string path = args.Option("path", Directory.GetCurrentDirectory())!;
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
            // An invalid .sermofur entry (foreign, damaged, unreadable) is reported by doctor rather than refused before inspection; a link is still refused.
            return RunInstanceCommand(args, manager.DiscoverForDiagnosis(path), json);
        }
        string root = manager.Discover(path);
        if (command == "root")
        {
            return RunInstanceCommand(args, root, json);
        }
        InstanceConfiguration config = manager.ReadConfiguration(root);
        bool mutating =
            command == "export"
            || (args.Positionals.Count > 1 && args.Positionals[1] is "add" or "invalidate");
        using SqliteStore store = new(root, config.InstanceId, readOnly: !mutating);
        store.ValidateSchema();
        MemoryContext context = manager.ResolveContext(path, root, store.ReadScopes());
        MemoryService memory = new(store, context);
        ScopeService scopes = new(store, new LocalPathResolver(), context);
        Write(Dispatch(args, memory, scopes, context, config), json);
        return 0;
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
        DoctorReport report = new InstanceDoctor().Inspect(root);
        Write(report, json);
        return report.Overall == "unhealthy" ? 5 : 0;
    }

    private static object Dispatch(
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
                mode = "bootstrap",
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
