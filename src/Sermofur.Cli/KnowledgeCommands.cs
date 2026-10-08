using System.Globalization;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

namespace Sermofur.Cli;

/// <summary>Commands of format 2: sources, index rebuild, recall and challenge.</summary>
internal static class KnowledgeCommands
{
    public static bool IsKnowledgeCommand(string command) =>
        command is "source" or "index" or "recall" or "challenge";

    /// <summary>True when the command writes: the store is then opened read-write.</summary>
    public static bool Writes(CommandArguments args) =>
        args.Positionals[0] switch
        {
            "source" => args.Positionals.Count > 1 && args.Positionals[1] is "add" or "reindex",
            "index" => true,
            _ => false,
        };

    public static object Execute(
        CommandArguments args,
        SqliteStore store,
        MemoryService memory,
        MemoryContext context,
        string path
    )
    {
        // Each command builds only the services it uses.
        SourceService Sources() =>
            new(store, new FileSourceReader(new FileOwnership()), new LocalPathResolver(), context);
        RecallService Recalls() => new(store, store, context);
        return args.Positionals[0] switch
        {
            "source" => Source(args, Sources(), path),
            "index" => Index(args, Sources()),
            "recall" => Recall(args, Recalls()),
            _ => Challenge(args, new ChallengeService(memory, Recalls())),
        };
    }

    private static object Source(CommandArguments args, SourceService sources, string path)
    {
        if (args.Positionals.Count < 2)
        {
            throw new SermofurException("invalid_arguments", "Source subcommand required.");
        }
        string action = args.Positionals[1];
        switch (action)
        {
            case "add":
            {
                args.RequireCount(3);
                Provenance provenance = new(
                    CommandArguments.ParseEnum<ActorKind>(args.Required("origin")),
                    args.Option("actor", "local-user")!,
                    DateTimeOffset.UtcNow
                );
                args.ValidateUsed();
                // A relative file is relative to the context directory (--path), like the scope.
                return sources.Add(Path.GetFullPath(args.Positionals[2], path), provenance);
            }
            case "list":
                args.RequireCount(2);
                args.ValidateUsed();
                return sources.List();
            case "show":
                args.RequireCount(3);
                args.ValidateUsed();
                return sources.Show(CommandArguments.ParseId(args.Positionals[2]));
            case "reindex":
            {
                if (args.Positionals.Count is not (2 or 3))
                {
                    throw new SermofurException("invalid_arguments", "Wrong number of arguments.");
                }
                Guid? id =
                    args.Positionals.Count == 3
                        ? CommandArguments.ParseId(args.Positionals[2])
                        : null;
                string actor = args.Option("actor", "local-user")!;
                args.ValidateUsed();
                return new { sources = sources.Reindex(id, actor) };
            }
            default:
                throw new SermofurException("invalid_arguments", "Unknown source subcommand.");
        }
    }

    private static object Index(CommandArguments args, SourceService sources)
    {
        args.RequireCount(2);
        if (args.Positionals[1] != "rebuild")
        {
            throw new SermofurException("invalid_arguments", "Unknown index subcommand.");
        }
        args.ValidateUsed();
        return sources.RebuildIndex();
    }

    private static object Recall(CommandArguments args, RecallService recall)
    {
        args.RequireCount(2);
        string? limit = args.Option("limit");
        args.ValidateUsed();
        int parsed = RecallService.MaxResults;
        if (
            limit is not null
            && !int.TryParse(limit, NumberStyles.None, CultureInfo.InvariantCulture, out parsed)
        )
        {
            throw new SermofurException("invalid_arguments", "The limit is between 1 and 3.");
        }
        return recall.Recall(args.Positionals[1], parsed);
    }

    private static object Challenge(CommandArguments args, ChallengeService challenge)
    {
        string? text = args.Option("text");
        args.ValidateUsed();
        if (text is not null)
        {
            args.RequireCount(1);
            return challenge.Challenge(text);
        }
        args.RequireCount(2);
        return challenge.Challenge(CommandArguments.ParseId(args.Positionals[1]));
    }
}
