using Sermofur.Domain;

namespace Sermofur.Application;

/// <summary>A source with its history, as shown by <c>source show</c>.</summary>
public sealed record SourceView(MemoryRecord Source, IReadOnlyList<HistoryEntry> History);

/// <summary>
/// Declared sources of the current scope: explicit add, explicit reindexing. A source belongs
/// to the narrowest scope whose directory holds the file, never to an ancestor that would make
/// it visible to sibling scopes (FR-002).
/// </summary>
public sealed class SourceService
{
    private readonly IMemoryStore store;
    private readonly ISourceReader reader;
    private readonly IPathResolver paths;
    private readonly MemoryContext context;
    private readonly IReadOnlyList<Scope> scopes;

    public SourceService(
        IMemoryStore store,
        ISourceReader reader,
        IPathResolver paths,
        MemoryContext context
    )
    {
        this.store = store;
        this.reader = reader;
        this.paths = paths;
        scopes = store.ReadScopes();
        ScopePolicy.ValidateTree(scopes);
        this.context = context with
        {
            VisibleScopes = ScopePolicy.VisibleAncestors(context.ScopeId, scopes),
        };
    }

    public IReadOnlyList<MemoryRecord> List() =>
        store.ReadRecords(context.VisibleScopes, RecordKind.Source);

    public SourceView Show(Guid id)
    {
        MemoryRecord source = Get(id);
        return new SourceView(source, store.ReadHistory(id, context.VisibleScopes));
    }

    public MemoryRecord Add(string file, Provenance provenance)
    {
        MemoryService.ValidateText(provenance.Actor);
        if (!Enum.IsDefined(provenance.Origin))
        {
            throw new SermofurException("invalid_input", "Invalid origin.");
        }
        string relative = Locate(file);
        SourceSnapshot snapshot = reader.Read(context.Root, relative);
        RefuseUnindexable(snapshot);
        MemoryRecord? saved = store.SaveSource(
            relative,
            existing =>
            {
                if (existing is null)
                {
                    SourceContent content = Indexed(relative, snapshot);
                    return new SourceChange(
                        new MemoryRecord(
                            Guid.NewGuid(),
                            context.ScopeId,
                            RecordKind.Source,
                            KnowledgeStatus.Active,
                            1,
                            RecordJson.Write(content),
                            provenance
                        ),
                        "create",
                        provenance.Actor
                    );
                }
                if (existing.ScopeId != context.ScopeId)
                {
                    throw new SermofurException(
                        "scope_boundary",
                        "This file is already declared as a source of another scope.",
                        4
                    );
                }
                return Transition(existing, snapshot, provenance.Actor).Change;
            },
            snapshot.Passages
        );
        return saved!;
    }

    /// <summary>
    /// Reads again the sources of the current scope (or one of them) and records what changed;
    /// never reads a file that is not a declared source.
    /// </summary>
    public IReadOnlyList<ReindexEntry> Reindex(Guid? id, string actor)
    {
        MemoryService.ValidateText(actor);
        IReadOnlyList<MemoryRecord> sources = store.ReadRecords(
            new HashSet<string>(StringComparer.Ordinal) { context.ScopeId },
            RecordKind.Source
        );
        if (id is not null)
        {
            sources = sources.Where(source => source.Id == id).ToArray();
            if (sources.Count == 0)
            {
                // Same answer for an unknown source and one of another scope.
                throw new SermofurException("not_found", "Object missing or inaccessible.");
            }
        }
        List<ReindexEntry> entries = new List<ReindexEntry>();
        foreach (MemoryRecord source in sources)
        {
            SourceContent previous = Content(source);
            SourceSnapshot snapshot = reader.Read(context.Root, previous.RelativePath);
            ReindexOutcome outcome = ReindexOutcome.Unchanged;
            MemoryRecord? saved = store.SaveSource(
                previous.RelativePath,
                current =>
                {
                    if (current is null || current.Id != source.Id)
                    {
                        throw new SermofurException(
                            "storage_busy",
                            "Source changed concurrently; retry.",
                            3
                        );
                    }
                    (SourceChange? change, ReindexOutcome result) = Transition(
                        current,
                        snapshot,
                        actor
                    );
                    outcome = result;
                    return change;
                },
                snapshot.Passages
            );
            entries.Add(
                new ReindexEntry(
                    source.Id,
                    previous.RelativePath,
                    outcome,
                    previous.Hash.Length == 0 ? null : previous.Hash,
                    Content(saved!).Hash
                )
            );
        }
        return entries;
    }

    /// <summary>
    /// Rebuilds the whole index: claims and RETEX from the registry, then every source of the
    /// instance read again with the same transitions as <see cref="Reindex"/> (history included),
    /// so that the registry and the index agree afterwards. Files are read before any write
    /// transaction. Counts only cover visible objects, so that nothing of another scope leaks.
    /// </summary>
    public IndexRebuild RebuildIndex(string actor)
    {
        MemoryService.ValidateText(actor);
        HashSet<string> all = scopes.Select(scope => scope.Id).ToHashSet(StringComparer.Ordinal);
        (MemoryRecord Source, SourceSnapshot Snapshot)[] sources = store
            .ReadRecords(all, RecordKind.Source)
            .Select(source => (source, reader.Read(context.Root, Content(source).RelativePath)))
            .ToArray();
        store.ResetIndex();
        int changed = 0;
        foreach ((MemoryRecord source, SourceSnapshot snapshot) in sources)
        {
            ReindexOutcome outcome = ReindexOutcome.Unchanged;
            store.SaveSource(
                Content(source).RelativePath,
                current =>
                {
                    if (current is null || current.Id != source.Id)
                    {
                        throw new SermofurException(
                            "storage_busy",
                            "Source changed concurrently; retry.",
                            3
                        );
                    }
                    (SourceChange? change, ReindexOutcome result) = Transition(
                        current,
                        snapshot,
                        actor
                    );
                    outcome = result;
                    return change;
                },
                snapshot.Passages,
                reindex: true
            );
            if (
                outcome != ReindexOutcome.Unchanged
                && context.VisibleScopes.Contains(source.ScopeId)
            )
            {
                changed++;
            }
        }
        int indexed = store.ReadRecords(context.VisibleScopes).Count(SearchExpectations.IsIndexed);
        return new IndexRebuild(indexed, changed);
    }

    public static SourceContent Content(MemoryRecord source) =>
        RecordJson.Read<SourceContent>(source.ContentJson);

    private MemoryRecord Get(Guid id)
    {
        MemoryRecord record =
            store.FindRecord(id, context.VisibleScopes)
            ?? throw new SermofurException("not_found", "Object missing or inaccessible.");
        return record.Kind == RecordKind.Source
            ? record
            : throw new SermofurException("not_found", "Object missing or inaccessible.");
    }

    /// <summary>Stored relative path of a file of the current scope, or a refusal.</summary>
    private string Locate(string file)
    {
        if (string.IsNullOrWhiteSpace(file) || InputLimits.IsOutOfBounds(file))
        {
            throw new SermofurException("invalid_input", InputLimits.TextMessage);
        }
        string full = Path.GetFullPath(file);
        if (!paths.Contains(context.Root, full))
        {
            throw new SermofurException("scope_boundary", "Source outside the instance.", 4);
        }
        if (paths.Contains(Path.Combine(context.Root, ".sermofur"), full))
        {
            throw new SermofurException(
                "unsafe_path",
                "Sermofur's own files cannot be sources.",
                4
            );
        }
        Scope current = scopes.Single(scope => scope.Id == context.ScopeId);
        string directory = current.RelativePath is null
            ? context.Root
            : paths.Normalize(context.Root, current.RelativePath);
        if (!paths.Contains(directory, full))
        {
            throw new SermofurException(
                "scope_boundary",
                "The file is outside the directory of the current scope.",
                4
            );
        }
        bool narrower = scopes.Any(scope =>
            scope.Id != context.ScopeId
            && scope.RelativePath is not null
            && ScopePolicy.VisibleAncestors(scope.Id, scopes).Contains(context.ScopeId)
            && paths.Contains(paths.Normalize(context.Root, scope.RelativePath), full)
        );
        if (narrower)
        {
            throw new SermofurException(
                "scope_boundary",
                "The file belongs to a narrower scope; add it from that scope's directory.",
                4
            );
        }
        return paths.Relativize(context.Root, full);
    }

    private static void RefuseUnindexable(SourceSnapshot snapshot)
    {
        switch (snapshot.Status)
        {
            case SourceStatus.Missing:
                throw new SermofurException("invalid_path", "Source file missing or not a file.");
            case SourceStatus.Unreadable:
                throw new SermofurException(
                    "invalid_path",
                    "Source file cannot be read; check permissions."
                );
            case SourceStatus.Rejected when snapshot.Rejection == SourceRejection.UnsafePath:
                throw new SermofurException(
                    "unsafe_path",
                    "Links, junctions and network paths cannot be sources.",
                    4
                );
            case SourceStatus.Rejected:
                throw new SermofurException(
                    "source_rejected",
                    RejectionMessage(snapshot.Rejection)
                );
        }
    }

    private static string RejectionMessage(SourceRejection? rejection) =>
        rejection switch
        {
            SourceRejection.TooLarge => "Source larger than 1 MiB.",
            SourceRejection.Binary =>
                "Source is not UTF-8 text (binary, NUL byte or special file).",
            SourceRejection.Empty => "Source is empty.",
            _ => "Source cannot be indexed.",
        };

    private static SourceContent Indexed(string relative, SourceSnapshot snapshot) =>
        new(
            relative,
            SourceStatus.Indexed,
            snapshot.Hash,
            snapshot.Size,
            DateTimeOffset.UtcNow,
            snapshot.Passages.Count
        );

    /// <summary>Table of transitions of the data model; null change when nothing changed.</summary>
    private static (SourceChange? Change, ReindexOutcome Outcome) Transition(
        MemoryRecord current,
        SourceSnapshot snapshot,
        string actor
    )
    {
        SourceContent previous = Content(current);
        if (snapshot.Status == SourceStatus.Indexed)
        {
            if (previous.Status == SourceStatus.Indexed && previous.Hash == snapshot.Hash)
            {
                return (null, ReindexOutcome.Unchanged);
            }
            ReindexOutcome outcome =
                previous.Status == SourceStatus.Indexed
                    ? ReindexOutcome.Modified
                    : ReindexOutcome.Restored;
            return (
                Change(current, Indexed(previous.RelativePath, snapshot), outcome, actor),
                outcome
            );
        }
        if (previous.Status == snapshot.Status && previous.Rejection == snapshot.Rejection)
        {
            return (null, ReindexOutcome.Unchanged);
        }
        ReindexOutcome lost = snapshot.Status switch
        {
            SourceStatus.Missing => ReindexOutcome.Missing,
            SourceStatus.Unreadable => ReindexOutcome.Unreadable,
            _ => ReindexOutcome.Rejected,
        };
        SourceContent kept = previous with
        {
            Status = snapshot.Status,
            Rejection = snapshot.Rejection,
            Passages = 0,
        };
        return (Change(current, kept, lost, actor), lost);
    }

    private static SourceChange Change(
        MemoryRecord current,
        SourceContent content,
        ReindexOutcome outcome,
        string actor
    ) =>
        new(
            current with
            {
                Revision = current.Revision + 1,
                ContentJson = RecordJson.Write(content),
            },
            outcome.ToString().ToLowerInvariant(),
            actor
        );
}
