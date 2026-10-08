using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Domain;

namespace Sermofur.Infrastructure;

/// <summary>
/// Writes of the FTS5 <c>search</c> table. Always called inside the transaction that writes the
/// object, so that the index never diverges from the registry.
/// </summary>
internal static class SqliteSearchIndex
{
    public static string KindOf(RecordKind kind) =>
        kind switch
        {
            RecordKind.Claim => "claim",
            RecordKind.Retex => "retex",
            RecordKind.Source => "source",
            _ => throw new ArgumentOutOfRangeException(nameof(kind), kind, "Kind not indexed."),
        };

    public static void Insert(
        SqliteConnection connection,
        SqliteTransaction transaction,
        Guid objectId,
        string scopeId,
        RecordKind kind,
        IReadOnlyList<SearchDocument> documents,
        SqliteTokenizer? shared = null
    )
    {
        // One tokenizer per batch: its temporary tables are created once.
        SqliteTokenizer tokenizer =
            shared ?? new SqliteTokenizer(connection) { Transaction = transaction };
        foreach (SearchDocument document in documents)
        {
            using SqliteCommand command = connection.CreateCommand();
            command.Transaction = transaction;
            command.CommandText =
                "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES($id,$scope,$kind,$passage,$length,$text)";
            command.Parameters.AddWithValue("$id", objectId.ToString());
            command.Parameters.AddWithValue("$scope", scopeId);
            command.Parameters.AddWithValue("$kind", KindOf(kind));
            command.Parameters.AddWithValue("$passage", document.Passage);
            command.Parameters.AddWithValue("$length", tokenizer.Tokenize(document.Text).Count);
            command.Parameters.AddWithValue("$text", document.Text);
            command.ExecuteNonQuery();
        }
    }

    public static void Remove(
        SqliteConnection connection,
        SqliteTransaction transaction,
        Guid objectId
    )
    {
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "DELETE FROM search WHERE object_id=$id";
        command.Parameters.AddWithValue("$id", objectId.ToString());
        command.ExecuteNonQuery();
    }

    /// <summary>Removes every entry of the index.</summary>
    public static void Clear(SqliteConnection connection, SqliteTransaction transaction)
    {
        using SqliteCommand command = connection.CreateCommand();
        command.Transaction = transaction;
        command.CommandText = "DELETE FROM search";
        command.ExecuteNonQuery();
    }

    /// <summary>Indexes every claim and RETEX of the registry: migration and rebuild.</summary>
    /// <remarks>Streams the registry: one record in memory at a time.</remarks>
    public static int IndexRecords(
        SqliteConnection connection,
        SqliteTransaction transaction,
        SqliteTokenizer? shared = null
    )
    {
        SqliteTokenizer tokenizer =
            shared ?? new SqliteTokenizer(connection) { Transaction = transaction };
        int count = 0;
        using SqliteCommand read = connection.CreateCommand();
        read.Transaction = transaction;
        read.CommandText =
            "SELECT payload FROM records WHERE kind IN ('Claim','Retex') ORDER BY id";
        using SqliteDataReader reader = read.ExecuteReader();
        while (reader.Read())
        {
            MemoryRecord record = RecordJson.Read<MemoryRecord>(reader.GetString(0));
            Insert(
                connection,
                transaction,
                record.Id,
                record.ScopeId,
                record.Kind,
                SearchDocuments.For(record),
                tokenizer
            );
            count++;
        }
        return count;
    }
}
