using Microsoft.Data.Sqlite;

namespace Sermofur.Infrastructure;

internal static class SqliteSchema
{
    /// <summary>Format 1 (CLI 0.1): kept to read and migrate existing instances, and for tests.</summary>
    internal const string Version1 = """
        CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
        CREATE TABLE scopes(id TEXT PRIMARY KEY, kind TEXT NOT NULL, parent_id TEXT REFERENCES scopes(id),
            relative_path TEXT COLLATE NOCASE);
        CREATE TABLE records(id TEXT PRIMARY KEY, scope_id TEXT NOT NULL REFERENCES scopes(id),
            kind TEXT NOT NULL CHECK(kind IN ('Claim','Evidence','Retex')), claim_id TEXT REFERENCES records(id),
            payload TEXT NOT NULL CHECK(json_valid(payload)), revision INTEGER NOT NULL CHECK(revision>0));
        CREATE INDEX records_scope_kind ON records(scope_id,kind);
        CREATE TABLE history(sequence INTEGER PRIMARY KEY AUTOINCREMENT, object_id TEXT NOT NULL REFERENCES records(id),
            scope_id TEXT NOT NULL REFERENCES scopes(id), payload TEXT NOT NULL CHECK(json_valid(payload)));
        CREATE TABLE idempotency(scope_id TEXT NOT NULL REFERENCES scopes(id), operation TEXT NOT NULL,
            key TEXT NOT NULL, content_hash TEXT NOT NULL, object_id TEXT NOT NULL REFERENCES records(id),
            PRIMARY KEY(scope_id,operation,key));
        """;

    /// <summary>Format 2 records table: sources, and the source path for its uniqueness.</summary>
    private const string RecordsVersion2 = """
        CREATE TABLE {0}(id TEXT PRIMARY KEY, scope_id TEXT NOT NULL REFERENCES scopes(id),
            kind TEXT NOT NULL CHECK(kind IN ('Claim','Evidence','Retex','Source')),
            claim_id TEXT REFERENCES records(id),
            payload TEXT NOT NULL CHECK(json_valid(payload)), revision INTEGER NOT NULL CHECK(revision>0),
            source_path TEXT COLLATE NOCASE CHECK(source_path IS NULL OR kind='Source'));
        """;

    private const string IndexesVersion2 = """
        CREATE INDEX records_scope_kind ON records(scope_id,kind);
        CREATE UNIQUE INDEX records_source_path ON records(source_path) WHERE source_path IS NOT NULL;
        CREATE VIRTUAL TABLE search USING fts5(object_id UNINDEXED, scope_id UNINDEXED, kind UNINDEXED,
            passage UNINDEXED, length UNINDEXED, text, tokenize='unicode61 remove_diacritics 2');
        CREATE VIRTUAL TABLE search_terms USING fts5vocab(search, instance);
        """;

    public static void Initialize(SqliteConnection database, Guid instanceId)
    {
        using SqliteTransaction transaction = database.BeginTransaction(deferred: false);
        using SqliteCommand command = database.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = $"""
            CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
            CREATE TABLE scopes(id TEXT PRIMARY KEY, kind TEXT NOT NULL, parent_id TEXT REFERENCES scopes(id),
                relative_path TEXT COLLATE NOCASE);
            {string.Format(RecordsVersion2, "records")}
            CREATE TABLE history(sequence INTEGER PRIMARY KEY AUTOINCREMENT, object_id TEXT NOT NULL REFERENCES records(id),
                scope_id TEXT NOT NULL REFERENCES scopes(id), payload TEXT NOT NULL CHECK(json_valid(payload)));
            CREATE TABLE idempotency(scope_id TEXT NOT NULL REFERENCES scopes(id), operation TEXT NOT NULL,
                key TEXT NOT NULL, content_hash TEXT NOT NULL, object_id TEXT NOT NULL REFERENCES records(id),
                PRIMARY KEY(scope_id,operation,key));
            {IndexesVersion2}
            INSERT INTO metadata VALUES('instance_id',$instance);
            INSERT INTO scopes VALUES('workspace','Workspace',NULL,NULL);
            PRAGMA user_version={InstanceManager.SchemaVersion};
            """;
        command.Parameters.AddWithValue("$instance", instanceId.ToString());
        command.ExecuteNonQuery();
        transaction.Commit();
    }

    /// <summary>
    /// Rebuilds a format 1 database as format 2 inside the caller's transaction: the records table
    /// is recreated (SQLite cannot change a CHECK constraint), rows copied unchanged, then the
    /// search index is created. The connection must have foreign keys disabled: history and
    /// idempotency keep referring to <c>records</c> by name, which the rename restores.
    /// </summary>
    public static void MigrateVersion1(SqliteConnection database, SqliteTransaction transaction)
    {
        using SqliteCommand command = database.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = $"""
            {string.Format(RecordsVersion2, "records_v2")}
            INSERT INTO records_v2(id,scope_id,kind,claim_id,payload,revision,source_path)
                SELECT id,scope_id,kind,claim_id,payload,revision,NULL FROM records;
            DROP TABLE records;
            ALTER TABLE records_v2 RENAME TO records;
            {IndexesVersion2}
            """;
        command.ExecuteNonQuery();
    }
}
