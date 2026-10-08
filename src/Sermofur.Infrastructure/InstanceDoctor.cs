using System.Text.Json;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

public sealed record DiagnosticCheck(string Name, string Status, string Detail);

public sealed record DoctorReport(
    string Root,
    string Overall,
    IReadOnlyList<DiagnosticCheck> Checks
);

/// <param name="ownership">Owner and type of entries; the operating system by default.</param>
public sealed class InstanceDoctor(IFileOwnership? ownership = null)
{
    private static readonly string[] UndeliveredCapabilities = ["daemon", "laya", "model", "mcp"];

    public DoctorReport Inspect(string root)
    {
        List<DiagnosticCheck> checks = new List<DiagnosticCheck>();
        string? invalidMarker = new InstanceManager(ownership).DescribeInvalidMarker(root);
        if (invalidMarker is not null)
        {
            checks.Add(new("instance", "error", $"invalid_instance: {invalidMarker}"));
            return Report(root, checks);
        }
        InstanceConfiguration config;
        try
        {
            config = new InstanceManager(ownership).ReadConfiguration(root);
            checks.Add(new("instance", "ok", "Compatible configuration and version."));
        }
        catch (Exception exception) when (IsDiagnosable(exception))
        {
            checks.Add(new("instance", "error", ErrorCode(exception)));
            return Report(root, checks);
        }
        try
        {
            using SqliteStore store = new SqliteStore(root, config.InstanceId, readOnly: true);
            InspectStorage(store, checks);
            IReadOnlyList<Scope> scopes = InspectScopes(root, store, checks);
            InspectMemory(root, store, scopes, checks);
            InspectSearchIndex(store, scopes, checks);
            checks.Add(
                new(
                    "sqlite",
                    "ok",
                    $"Native version {store.Scalar("SELECT sqlite_version()")}; journal {store.Scalar("PRAGMA journal_mode")}."
                )
            );
        }
        catch (Exception exception) when (IsDiagnosable(exception))
        {
            checks.Add(new("instance_storage", "error", ErrorCode(exception)));
        }
        return Report(root, checks);
    }

    private static DoctorReport Report(string root, List<DiagnosticCheck> checks)
    {
        AddUndeliveredCapabilities(checks);
        string overall = checks.Any(c => c.Status == "error")
            ? "unhealthy"
            : "healthy_with_warnings";
        return new DoctorReport(root, overall, checks);
    }

    private static bool IsDiagnosable(Exception exception) =>
        exception
            is SermofurException
                or SqliteException
                or JsonException
                or IOException
                or UnauthorizedAccessException
                or ArgumentException
                or InvalidOperationException;

    private static string ErrorCode(Exception exception) =>
        exception is SermofurException error ? error.Code : "invalid_storage";

    /// <summary>
    /// Checks schema, storage and orphan_records on the database opened read-only.
    /// An invalid schema stops the inspection with an exception.
    /// </summary>
    private static void InspectStorage(SqliteStore store, List<DiagnosticCheck> checks)
    {
        store.ValidateSchema();
        checks.Add(new("schema", "ok", "Consistent SQLite schema and identity."));
        checks.Add(
            new(
                "storage",
                Convert.ToString(store.Scalar("PRAGMA integrity_check")) == "ok" ? "ok" : "error",
                "SQLite integrity check."
            )
        );
        IReadOnlyList<string> foreignKeys = store.QueryStrings("PRAGMA foreign_key_check");
        checks.Add(
            new(
                "orphan_records",
                foreignKeys.Count == 0 ? "ok" : "error",
                $"{foreignKeys.Count} inconsistent references."
            )
        );
    }

    /// <summary>
    /// The full-text index must hold exactly the indexable objects of the registry: claims,
    /// RETEX and indexed sources. Compared as sets of identifiers, without repairing anything.
    /// </summary>
    private static void InspectSearchIndex(
        SqliteStore store,
        IReadOnlyList<Scope> scopes,
        List<DiagnosticCheck> checks
    )
    {
        bool fts5 =
            Convert.ToInt64(store.Scalar("SELECT sqlite_compileoption_used('ENABLE_FTS5')")) == 1;
        checks.Add(new("fts5", fts5 ? "ok" : "error", "SQLite full-text search engine."));
        MemoryRecord[] records = store
            .QueryStrings("SELECT payload FROM records WHERE kind IN ('Claim','Retex','Source')")
            .Select(RecordJson.Read<MemoryRecord>)
            .ToArray();
        // Identity, scope and kind of each indexed object must match the registry: the
        // visibility filter of recall relies on the scope column of the index.
        HashSet<string> indexed = store
            .QueryStrings("SELECT DISTINCT object_id, scope_id, kind FROM search")
            .ToHashSet(StringComparer.Ordinal);
        HashSet<string> expected = records
            .Where(SearchExpectations.IsIndexed)
            .Select(record =>
                $"{record.Id}|{record.ScopeId}|{SqliteSearchIndex.KindOf(record.Kind)}"
            )
            .ToHashSet(StringComparer.Ordinal);
        int missing = expected.Count(entry => !indexed.Contains(entry));
        int extra = indexed.Count(entry => !expected.Contains(entry));
        checks.Add(
            new(
                "search_index",
                missing + extra == 0 ? "ok" : "error",
                $"{missing} objects missing from the index or misplaced, {extra} entries without object; run index rebuild."
            )
        );
        // A source must belong to the narrowest scope that holds its file (FR-002).
        int misplaced = records
            .Where(record => record.Kind == RecordKind.Source)
            .Count(source =>
                SourcePlacement.NarrowerScope(
                    RecordJson.Read<SourceContent>(source.ContentJson).RelativePath,
                    source.ScopeId,
                    scopes,
                    LocalPaths.Comparison
                )
                    is not null
            );
        checks.Add(
            new(
                "source_scopes",
                misplaced == 0 ? "ok" : "error",
                $"{misplaced} sources attached to a broader scope than their file."
            )
        );
    }

    private static void AddUndeliveredCapabilities(List<DiagnosticCheck> checks)
    {
        foreach (string component in UndeliveredCapabilities)
        {
            checks.Add(new(component, "warning", "Capability not delivered in this version."));
        }
    }

    /// <summary>
    /// Checks scopes, scope_mappings and scope_overlap; returns the scopes read for the memory
    /// checks. An invalid tree stops the inspection with an exception.
    /// </summary>
    private static IReadOnlyList<Scope> InspectScopes(
        string root,
        SqliteStore store,
        List<DiagnosticCheck> checks
    )
    {
        IReadOnlyList<Scope> scopes = store.ReadScopes();
        ScopePolicy.ValidateTree(scopes);
        checks.Add(new("scopes", "ok", "Valid scope tree."));
        int missing = CountMissingMappings(root, scopes);
        checks.Add(
            new("scope_mappings", missing == 0 ? "ok" : "warning", $"{missing} missing mappings.")
        );
        // Same rules as scope add, on lexically normalized mappings: detects an overlap entered
        // outside the CLI. The detail only gives the number of pairs.
        int conflicts = ScopeService.MappingConflicts(scopes, root, new LocalPathResolver()).Count;
        checks.Add(
            new(
                "scope_overlap",
                conflicts == 0 ? "ok" : "error",
                $"{conflicts} conflicting mapping pairs."
            )
        );
        return scopes;
    }

    /// <summary>
    /// A mapped directory that disappeared is reported without stopping the checks; a mapping
    /// outside the instance or turned into a link remains an error.
    /// </summary>
    private static int CountMissingMappings(string root, IReadOnlyList<Scope> scopes)
    {
        int missing = 0;
        foreach (Scope scope in scopes.Where(s => s.RelativePath is not null))
        {
            string mapped = LocalPaths.NormalizeMapping(root, scope.RelativePath!);
            if (!Directory.Exists(mapped))
            {
                missing++;
                continue;
            }
            LocalPaths.ValidateDirectory(mapped);
        }
        return missing;
    }

    private static void InspectMemory(
        string root,
        SqliteStore store,
        IReadOnlyList<Scope> scopes,
        List<DiagnosticCheck> checks
    )
    {
        HashSet<string> visible = scopes.Select(s => s.Id).ToHashSet(StringComparer.Ordinal);
        IReadOnlyList<MemoryRecord> records = store.ReadRecords(visible);
        int invalid = 0;
        foreach (MemoryRecord record in records)
        {
            if (!visible.Contains(record.ScopeId) || record.Id == Guid.Empty || record.Revision < 1)
            {
                invalid++;
            }

            switch (record.Kind)
            {
                case RecordKind.Claim:
                    RecordJson.Read<ClaimContent>(record.ContentJson);
                    break;
                case RecordKind.Retex:
                    RecordJson.Read<RetexContent>(record.ContentJson);
                    break;
                case RecordKind.Source:
                    RecordJson.Read<SourceContent>(record.ContentJson);
                    break;
                case RecordKind.Evidence:
                    EvidenceContent evidence = RecordJson.Read<EvidenceContent>(record.ContentJson);
                    MemoryRecord? claim = records.SingleOrDefault(r => r.Id == evidence.ClaimId);
                    if (
                        claim is null
                        || claim.Kind != RecordKind.Claim
                        || claim.ScopeId != record.ScopeId
                    )
                    {
                        invalid++;
                    }

                    break;
            }
            IReadOnlyList<HistoryEntry> history = store.ReadHistory(record.Id, visible);
            if (history.Count == 0 || RecordJson.Read<MemoryRecord>(history[^1].NewState) != record)
            {
                invalid++;
            }
        }
        checks.Add(
            new(
                "memory_consistency",
                invalid == 0 ? "ok" : "error",
                $"{invalid} inconsistencies detected."
            )
        );
        MarkdownProjection projection = new MarkdownProjection(root);
        int stale = records.Count(record => !projection.IsCurrent(record));
        checks.Add(
            new(
                "projections",
                stale == 0 ? "ok" : "warning",
                $"{stale} missing or diverging projections; run export to rebuild."
            )
        );
        int rowErrors = store
            .QueryStrings(
                """
                SELECT id FROM records WHERE
                    id != json_extract(payload,'$.id') OR scope_id != json_extract(payload,'$.scopeId') OR
                    revision != json_extract(payload,'$.revision') OR lower(kind) != json_extract(payload,'$.kind')
                """
            )
            .Count;
        checks.Add(
            new(
                "record_columns",
                rowErrors == 0 ? "ok" : "error",
                $"{rowErrors} inconsistent metadata rows."
            )
        );
    }
}
