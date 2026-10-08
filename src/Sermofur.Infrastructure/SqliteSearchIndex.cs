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
        IReadOnlyList<SearchDocument> documents
    )
    {
        SqliteTokenizer tokenizer = new SqliteTokenizer(connection) { Transaction = transaction };
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

    /// <summary>Indexes every claim and RETEX of the registry: migration and rebuild.</summary>
    public static int IndexRecords(SqliteConnection connection, SqliteTransaction transaction)
    {
        List<MemoryRecord> records = new List<MemoryRecord>();
        using (SqliteCommand read = connection.CreateCommand())
        {
            read.Transaction = transaction;
            read.CommandText =
                "SELECT payload FROM records WHERE kind IN ('Claim','Retex') ORDER BY id";
            using SqliteDataReader reader = read.ExecuteReader();
            while (reader.Read())
            {
                records.Add(RecordJson.Read<MemoryRecord>(reader.GetString(0)));
            }
        }
        foreach (MemoryRecord record in records)
        {
            Insert(
                connection,
                transaction,
                record.Id,
                record.ScopeId,
                record.Kind,
                SearchDocuments.For(record)
            );
        }
        return records.Count;
    }
}
