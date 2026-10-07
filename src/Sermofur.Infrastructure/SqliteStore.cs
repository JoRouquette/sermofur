using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

public sealed class SqliteStore : IMemoryStore, IDisposable
{
    private readonly SqliteConnection connection;
    private readonly MarkdownProjection projection;
    private readonly Guid instanceId;

    public SqliteStore(string root, Guid instanceId, bool readOnly = false)
    {
        this.instanceId = instanceId;
        projection = new MarkdownProjection(root);
        string database = InstanceManager.DatabasePath(root);
        LocalPaths.RejectLinks(database);
        if (!File.Exists(database))
        {
            throw new SermofurException("missing_database", "Memory database missing.", 3);
        }

        connection = Open(database, readOnly ? SqliteOpenMode.ReadOnly : SqliteOpenMode.ReadWrite);
    }

    private static SqliteConnection Open(string path, SqliteOpenMode mode)
    {
        SqliteConnectionStringBuilder configuration = new SqliteConnectionStringBuilder
        {
            DataSource = path,
            Mode = mode,
            Pooling = false,
            ForeignKeys = true,
            DefaultTimeout = 5,
        };
        SqliteConnection database = new SqliteConnection(configuration.ToString());
        try
        {
            database.Open();
            return database;
        }
        catch
        {
            database.Dispose();
            throw;
        }
    }

    public static void CreateDatabase(string path, Guid instanceId)
    {
        using SqliteConnection database = Open(path, SqliteOpenMode.ReadWriteCreate);
        SqliteSchema.Initialize(database, instanceId);
    }

    public object Scalar(string sql)
    {
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = sql;
        return command.ExecuteScalar() ?? string.Empty;
    }

    public IReadOnlyList<string> QueryStrings(string sql)
    {
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = sql;
        using SqliteDataReader reader = command.ExecuteReader();
        List<string> result = new List<string>();
        while (reader.Read())
        {
            result.Add(
                string.Join("|", Enumerable.Range(0, reader.FieldCount).Select(reader.GetValue))
            );
        }

        return result;
    }

    public void ValidateSchema()
    {
        if (Convert.ToInt32(Scalar("PRAGMA user_version")) != InstanceManager.SchemaVersion)
        {
            throw new SermofurException(
                "unsupported_schema",
                "Unsupported database schema version.",
                3
            );
        }

        if (
            !Guid.TryParse(
                Convert.ToString(Scalar("SELECT value FROM metadata WHERE key='instance_id'")),
                out Guid storedId
            )
            || storedId != instanceId
        )
        {
            throw new SermofurException("instance_mismatch", "Inconsistent SQLite identity.", 3);
        }
    }

    public IReadOnlyList<Scope> ReadScopes()
    {
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = "SELECT id,kind,parent_id,relative_path FROM scopes ORDER BY id";
        using SqliteDataReader reader = command.ExecuteReader();
        List<Scope> scopes = new List<Scope>();
        while (reader.Read())
        {
            scopes.Add(
                new Scope(
                    reader.GetString(0),
                    Enum.Parse<ScopeKind>(reader.GetString(1)),
                    reader.IsDBNull(2) ? null : reader.GetString(2),
                    reader.IsDBNull(3) ? null : reader.GetString(3)
                )
            );
        }

        return scopes;
    }

    public void AddScope(Scope scope, Action<IReadOnlyList<Scope>> precondition)
    {
        using SqliteTransaction transaction = connection.BeginTransaction(deferred: false);
        // BEGIN IMMEDIATE: no other writer can insert between this read and the INSERT.
        // The registration rules live in the precondition supplied by Application; the tree
        // check only guarantees a valid hierarchy.
        IReadOnlyList<Scope> scopes = ReadScopes();
        precondition(scopes);
        ScopePolicy.ValidateTree(scopes.Append(scope).ToArray());
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "INSERT INTO scopes VALUES($id,$kind,$parent,$path)";
        command.Parameters.AddWithValue("$id", scope.Id);
        command.Parameters.AddWithValue("$kind", scope.Kind.ToString());
        command.Parameters.AddWithValue("$parent", (object?)scope.ParentId ?? DBNull.Value);
        command.Parameters.AddWithValue("$path", (object?)scope.RelativePath ?? DBNull.Value);
        command.ExecuteNonQuery();
        transaction.Commit();
    }

    private SqliteCommand VisibleCommand(IReadOnlySet<string> visible, string suffix = "")
    {
        SqliteCommand command = connection.CreateCommand();
        string[] names = visible
            .Select(
                (id, index) =>
                {
                    string name = "$scope" + index;
                    command.Parameters.AddWithValue(name, id);
                    return name;
                }
            )
            .ToArray();
        command.CommandText =
            $"SELECT payload FROM records WHERE scope_id IN ({string.Join(',', names)}) {suffix}";
        return command;
    }

    public IReadOnlyList<MemoryRecord> ReadRecords(
        IReadOnlySet<string> visible,
        RecordKind? kind = null
    )
    {
        using SqliteCommand command = VisibleCommand(
            visible,
            kind is null ? "ORDER BY id" : "AND kind=$kind ORDER BY id"
        );
        if (kind is not null)
        {
            command.Parameters.AddWithValue("$kind", kind.ToString());
        }

        using SqliteDataReader reader = command.ExecuteReader();
        List<MemoryRecord> records = new List<MemoryRecord>();
        while (reader.Read())
        {
            records.Add(RecordJson.Read<MemoryRecord>(reader.GetString(0)));
        }

        return records;
    }

    public MemoryRecord? FindRecord(Guid id, IReadOnlySet<string> visible)
    {
        using SqliteCommand command = VisibleCommand(visible, "AND id=$id");
        command.Parameters.AddWithValue("$id", id.ToString());
        string? payload = command.ExecuteScalar() as string;
        return payload is null ? null : RecordJson.Read<MemoryRecord>(payload);
    }

    public IReadOnlyList<HistoryEntry> ReadHistory(Guid id, IReadOnlySet<string> visible)
    {
        if (FindRecord(id, visible) is null)
        {
            return [];
        }

        using SqliteCommand command = connection.CreateCommand();
        command.CommandText =
            "SELECT sequence,payload FROM history WHERE object_id=$id ORDER BY sequence";
        command.Parameters.AddWithValue("$id", id.ToString());
        using SqliteDataReader reader = command.ExecuteReader();
        List<HistoryEntry> entries = new List<HistoryEntry>();
        while (reader.Read())
        {
            entries.Add(
                RecordJson.Read<HistoryEntry>(reader.GetString(1)) with
                {
                    Sequence = reader.GetInt64(0),
                }
            );
        }

        return entries;
    }

    public MemoryRecord CreateRecord(MemoryRecord candidate, string? key)
    {
        using SqliteTransaction transaction = connection.BeginTransaction(deferred: false);
        string fingerprint = MarkdownProjection.Hash(
            RecordJson.Write(
                new
                {
                    candidate.ScopeId,
                    candidate.Kind,
                    candidate.ContentJson,
                    candidate.Provenance.Origin,
                    candidate.Provenance.Actor,
                }
            )
        );
        if (key is not null)
        {
            MemoryRecord? existing = FindIdempotent(candidate, key, fingerprint);
            if (existing is not null)
            {
                transaction.Commit();
                if (!projection.IsCurrentSafe(existing))
                {
                    Project(existing);
                }

                return existing;
            }
        }
        ValidateEvidenceLink(candidate);
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            "INSERT INTO records VALUES($id,$scope,$kind,$claim,$payload,$revision)";
        command.Parameters.AddWithValue("$id", candidate.Id.ToString());
        command.Parameters.AddWithValue("$scope", candidate.ScopeId);
        command.Parameters.AddWithValue("$kind", candidate.Kind.ToString());
        command.Parameters.AddWithValue("$payload", RecordJson.Write(candidate));
        command.Parameters.AddWithValue("$revision", candidate.Revision);
        string? claimId =
            candidate.Kind == RecordKind.Evidence
                ? RecordJson.Read<EvidenceContent>(candidate.ContentJson).ClaimId.ToString()
                : null;
        command.Parameters.AddWithValue("$claim", (object?)claimId ?? DBNull.Value);
        command.ExecuteNonQuery();
        AppendHistory(null, candidate, "create", candidate.Provenance.Actor, transaction);
        if (key is not null)
        {
            InsertIdempotency(candidate, key, fingerprint, transaction);
        }

        transaction.Commit();
        Project(candidate);
        return candidate;
    }

    private void ValidateEvidenceLink(MemoryRecord candidate)
    {
        if (candidate.Kind != RecordKind.Evidence)
        {
            return;
        }

        EvidenceContent content = RecordJson.Read<EvidenceContent>(candidate.ContentJson);
        MemoryRecord? claim = FindRecord(
            content.ClaimId,
            new HashSet<string> { candidate.ScopeId }
        );
        if (claim?.Kind != RecordKind.Claim)
        {
            throw new SermofurException(
                "scope_boundary",
                "Claim of the evidence missing from the scope.",
                4
            );
        }
    }

    private MemoryRecord? FindIdempotent(MemoryRecord candidate, string key, string fingerprint)
    {
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText =
            "SELECT content_hash,object_id FROM idempotency WHERE scope_id=$scope AND operation=$kind AND key=$key";
        command.Parameters.AddWithValue("$scope", candidate.ScopeId);
        command.Parameters.AddWithValue("$kind", candidate.Kind.ToString());
        command.Parameters.AddWithValue("$key", key);
        using SqliteDataReader reader = command.ExecuteReader();
        if (!reader.Read())
        {
            return null;
        }

        if (reader.GetString(0) != fingerprint)
        {
            throw new SermofurException(
                "idempotency_conflict",
                "Key reused with different content."
            );
        }

        Guid id = Guid.Parse(reader.GetString(1));
        reader.Close();
        return FindRecord(id, new HashSet<string> { candidate.ScopeId })
            ?? throw new SermofurException(
                "orphan_idempotency",
                "Orphan idempotency reference.",
                3
            );
    }

    private void InsertIdempotency(
        MemoryRecord record,
        string key,
        string hash,
        SqliteTransaction transaction
    )
    {
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "INSERT INTO idempotency VALUES($scope,$operation,$key,$hash,$id)";
        command.Parameters.AddWithValue("$scope", record.ScopeId);
        command.Parameters.AddWithValue("$operation", record.Kind.ToString());
        command.Parameters.AddWithValue("$key", key);
        command.Parameters.AddWithValue("$hash", hash);
        command.Parameters.AddWithValue("$id", record.Id.ToString());
        command.ExecuteNonQuery();
    }

    public MemoryRecord InvalidateRecord(Guid id, string scopeId, string reason, string actor)
    {
        using SqliteTransaction transaction = connection.BeginTransaction(deferred: false);
        MemoryRecord previous =
            FindRecord(id, new HashSet<string> { scopeId })
            ?? throw new SermofurException("not_found", "Object missing or inaccessible.");
        if (previous.Kind != RecordKind.Claim)
        {
            throw new SermofurException("wrong_kind", "A claim is required.");
        }

        if (previous.Status == KnowledgeStatus.Invalidated)
        {
            transaction.Commit();
            return previous;
        }
        MemoryRecord updated = previous with
        {
            Status = KnowledgeStatus.Invalidated,
            Revision = previous.Revision + 1,
        };
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "UPDATE records SET payload=$payload,revision=$revision WHERE id=$id";
        command.Parameters.AddWithValue("$payload", RecordJson.Write(updated));
        command.Parameters.AddWithValue("$revision", updated.Revision);
        command.Parameters.AddWithValue("$id", id.ToString());
        command.ExecuteNonQuery();
        AppendHistory(previous, updated, reason, actor, transaction);
        transaction.Commit();
        Project(updated);
        return updated;
    }

    private void AppendHistory(
        MemoryRecord? previous,
        MemoryRecord current,
        string reason,
        string actor,
        SqliteTransaction transaction
    )
    {
        HistoryEntry entry = new HistoryEntry(
            0,
            current.Id,
            current.ScopeId,
            previous is null ? null : RecordJson.Write(previous),
            RecordJson.Write(current),
            reason,
            actor,
            DateTimeOffset.UtcNow
        );
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText =
            "INSERT INTO history(object_id,scope_id,payload) VALUES($id,$scope,$payload)";
        command.Parameters.AddWithValue("$id", current.Id.ToString());
        command.Parameters.AddWithValue("$scope", current.ScopeId);
        command.Parameters.AddWithValue("$payload", RecordJson.Write(entry));
        command.ExecuteNonQuery();
    }

    private void Project(MemoryRecord record)
    {
        try
        {
            projection.Write(record);
        }
        catch (Exception exception)
            when (exception is IOException or UnauthorizedAccessException or SermofurException)
        {
            throw new SermofurException(
                "projection_pending",
                $"Object {record.Id} saved; projection to rebuild with export.",
                3
            );
        }
    }

    public IReadOnlyList<string> Export(IReadOnlySet<string> visible)
    {
        IReadOnlyList<MemoryRecord> records = ReadRecords(visible);
        foreach (MemoryRecord record in records)
        {
            Project(record);
        }

        return records.Select(projection.PathFor).ToArray();
    }

    public void Dispose() => connection.Dispose();
}
