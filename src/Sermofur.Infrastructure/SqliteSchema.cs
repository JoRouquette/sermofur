using Microsoft.Data.Sqlite;

namespace Sermofur.Infrastructure;

internal static class SqliteSchema
{
    public static void Initialize(SqliteConnection database, Guid instanceId)
    {
        using SqliteTransaction transaction = database.BeginTransaction(deferred: false);
        using SqliteCommand command = database.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = """
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
            INSERT INTO metadata VALUES('instance_id',$instance);
            INSERT INTO scopes VALUES('workspace','Workspace',NULL,NULL);
            PRAGMA user_version=1;
            """;
        command.Parameters.AddWithValue("$instance", instanceId.ToString());
        command.ExecuteNonQuery();
        transaction.Commit();
    }
}
