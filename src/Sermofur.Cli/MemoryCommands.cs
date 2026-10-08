using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Cli;

internal static class MemoryCommands
{
    public static object Execute(CommandArguments args, MemoryService memory, RecordKind kind)
    {
        if (args.Positionals.Count < 2)
        {
            throw new SermofurException("invalid_arguments", "Memory subcommand required.");
        }
        string action = args.Positionals[1];
        if (action == "list")
        {
            args.RequireCount(2);
            args.ValidateUsed();
            return memory.List(kind);
        }
        if (action == "show")
        {
            args.RequireCount(3);
            args.ValidateUsed();
            Guid id = CommandArguments.ParseId(args.Positionals[2]);
            MemoryRecord record = memory.Get(id);
            if (record.Kind != kind)
            {
                throw new SermofurException("not_found", "Object missing or inaccessible.");
            }
            return kind == RecordKind.Claim ? memory.Explain(id) : record;
        }
        if (action == "invalidate" && kind == RecordKind.Claim)
        {
            args.RequireCount(3);
            string reason = args.Required("reason");
            string actor = args.Option("actor", "local-user")!;
            args.ValidateUsed();
            return memory.Invalidate(CommandArguments.ParseId(args.Positionals[2]), reason, actor);
        }
        if (action != "add")
        {
            throw new SermofurException("invalid_arguments", "Unknown memory subcommand.");
        }
        return Add(args, memory, kind);
    }

    private static object Add(CommandArguments args, MemoryService memory, RecordKind kind)
    {
        // The origin stays declarative but never implicit: no default value.
        Provenance provenance = new(
            CommandArguments.ParseEnum<ActorKind>(args.Required("origin")),
            args.Option("actor", "local-user")!,
            DateTimeOffset.UtcNow
        );
        string? key = args.Option("key");
        switch (kind)
        {
            case RecordKind.Claim:
                return AddClaim(args, memory, provenance, key);
            case RecordKind.Evidence:
                return AddEvidence(args, memory, provenance, key);
            case RecordKind.Retex:
                args.RequireCount(2);
                RetexContent content = new(
                    args.Required("event"),
                    args.Required("impact"),
                    args.Required("next")
                );
                args.ValidateUsed();
                return memory.CreateRetex(content, provenance, key);
            default:
                throw new SermofurException("invalid_arguments", "Unknown kind.");
        }
    }

    private static MemoryRecord AddClaim(
        CommandArguments args,
        MemoryService memory,
        Provenance provenance,
        string? key
    )
    {
        args.RequireCount(3);
        MemoryCategory category = CommandArguments.ParseEnum<MemoryCategory>(
            args.Option("category", "semantic")!
        );
        Volatility volatility = CommandArguments.ParseEnum<Volatility>(
            args.Option("volatility", "evolving")!
        );
        args.ValidateUsed();
        return memory.CreateClaim(
            new ClaimContent(args.Positionals[2], category, volatility),
            provenance,
            key
        );
    }

    private static MemoryRecord AddEvidence(
        CommandArguments args,
        MemoryService memory,
        Provenance provenance,
        string? key
    )
    {
        args.RequireCount(5);
        string lineage = args.Required("lineage");
        EvidenceRelation relation = args.Flag("contradicts")
            ? EvidenceRelation.Contradicts
            : EvidenceRelation.Supports;
        string? source = args.Option("source");
        args.ValidateUsed();
        return memory.CreateEvidence(
            new EvidenceContent(
                CommandArguments.ParseId(args.Positionals[2]),
                CommandArguments.ParseEnum<EvidenceKind>(args.Positionals[3]),
                args.Positionals[4],
                lineage,
                relation,
                source is null ? null : CommandArguments.ParseId(source)
            ),
            provenance,
            key
        );
    }
}
