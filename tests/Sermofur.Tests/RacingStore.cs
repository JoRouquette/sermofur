using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Tests;

/// <summary>
/// Store that simulates a concurrent writer: just before <see cref="AddScope"/> (or
/// <see cref="SaveSource"/> when <paramref name="raceSaveSource"/> is set), it runs raw SQL on a
/// separate connection, then delegates to the real store and observes what the store saw.
/// </summary>
internal sealed class RacingStore(
    IMemoryStore inner,
    string databasePath,
    string rivalSql,
    bool raceSaveSource = false
) : IMemoryStore
{
    public bool PreconditionSawRival { get; private set; }

    private void Race()
    {
        using SqliteConnection rival = new("Pooling=False;Data Source=" + databasePath);
        rival.Open();
        using SqliteCommand command = rival.CreateCommand();
        command.CommandText = rivalSql;
        command.ExecuteNonQuery();
    }

    public void AddScope(Scope scope, Action<IReadOnlyList<Scope>> precondition)
    {
        if (!raceSaveSource)
        {
            Race();
        }
        inner.AddScope(
            scope,
            scopes =>
            {
                PreconditionSawRival = scopes.Any(existing => existing.Id == "rival");
                precondition(scopes);
            }
        );
    }

    public IReadOnlyList<Scope> ReadScopes() => inner.ReadScopes();

    public IReadOnlyList<MemoryRecord> ReadRecords(
        IReadOnlySet<string> visible,
        RecordKind? kind = null
    ) => inner.ReadRecords(visible, kind);

    public IReadOnlyList<RecordSummary> ReadSummaries(IReadOnlySet<string> visible) =>
        inner.ReadSummaries(visible);

    public MemoryRecord? FindRecord(Guid id, IReadOnlySet<string> visible) =>
        inner.FindRecord(id, visible);

    public IReadOnlyList<HistoryEntry> ReadHistory(Guid id, IReadOnlySet<string> visible) =>
        inner.ReadHistory(id, visible);

    public MemoryRecord CreateRecord(MemoryRecord candidate, string? key) =>
        inner.CreateRecord(candidate, key);

    public MemoryRecord InvalidateRecord(Guid id, string scopeId, string reason, string actor) =>
        inner.InvalidateRecord(id, scopeId, reason, actor);

    public IReadOnlyList<string> Export(IReadOnlySet<string> visible) => inner.Export(visible);

    public MemoryRecord? SaveSource(
        string relativePath,
        Func<MemoryRecord?, IReadOnlyList<Scope>, SourceChange?> decide,
        IReadOnlyList<SearchDocument> passages
    )
    {
        if (raceSaveSource)
        {
            Race();
        }
        return inner.SaveSource(
            relativePath,
            (existing, scopes) =>
            {
                PreconditionSawRival = scopes.Any(scope => scope.Id == "rival");
                return decide(existing, scopes);
            },
            passages
        );
    }

    public IReadOnlyList<MemoryRecord> RebuildIndex(Func<MemoryRecord, SourceRebuild> decide) =>
        inner.RebuildIndex(decide);
}
