using Microsoft.Data.Sqlite;
using Sermofur.Application;

namespace Sermofur.Infrastructure;

/// <summary>
/// Tokenizes with the exact tokenizer of the <c>search</c> table: the text goes through a
/// temporary FTS5 table of the same configuration and its terms are read back from an
/// <c>fts5vocab</c> table. A reimplementation would diverge (unicode61 uses Unicode 6.1 tables).
/// The temporary tables live in the connection's temp database, writable even when the main
/// database is opened read-only.
/// </summary>
internal sealed class SqliteTokenizer(SqliteConnection connection) : ITokenizer
{
    private bool created;

    /// <summary>Transaction of the caller, when tokenizing inside a write.</summary>
    public SqliteTransaction? Transaction { get; set; }

    public IReadOnlyList<string> Tokenize(string text)
    {
        EnsureTables();
        Execute("DELETE FROM temp.tokenizer");
        using (SqliteCommand insert = Command("INSERT INTO temp.tokenizer(text) VALUES($text)"))
        {
            insert.Parameters.AddWithValue("$text", text);
            insert.ExecuteNonQuery();
        }
        List<string> terms = new List<string>();
        using (
            SqliteCommand read = Command("SELECT term FROM temp.tokenizer_terms ORDER BY offset")
        )
        using (SqliteDataReader reader = read.ExecuteReader())
        {
            while (reader.Read())
            {
                terms.Add(reader.GetString(0));
            }
        }
        Execute("DELETE FROM temp.tokenizer");
        return terms;
    }

    private void EnsureTables()
    {
        if (created)
        {
            return;
        }
        Execute(
            $"""
            CREATE VIRTUAL TABLE IF NOT EXISTS temp.tokenizer USING fts5(text,
                tokenize='{SqliteSchema.SearchTokenizer}');
            CREATE VIRTUAL TABLE IF NOT EXISTS temp.tokenizer_terms USING fts5vocab(temp, tokenizer, instance);
            """
        );
        created = true;
    }

    private void Execute(string sql)
    {
        using SqliteCommand command = Command(sql);
        command.ExecuteNonQuery();
    }

    private SqliteCommand Command(string sql)
    {
        SqliteCommand command = connection.CreateCommand();
        command.Transaction = Transaction;
        command.CommandText = sql;
        return command;
    }
}
