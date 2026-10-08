using Microsoft.Data.Sqlite;
using Sermofur.Infrastructure;

namespace Sermofur.Tests;

/// <summary>
/// Copy of a format 1 instance built by the published Sermofur 0.1.1 tool (Fixtures/instance-v1):
/// scopes workspace, client-a, client-b, project-x; 4 claims (one invalidated), 1 piece of
/// evidence, 1 RETEX, one idempotency key, their projections.
/// </summary>
public sealed class LegacyInstance : IDisposable
{
    public string Root { get; } =
        Path.Combine(TestInstance.TempRoot, "sermofur-test-" + Guid.NewGuid().ToString("N"));

    public LegacyInstance()
    {
        TestInstance.RequireNoEntryAboveTemp();
        Directory.CreateDirectory(Path.Combine(Root, "client-a", "project-x"));
        Directory.CreateDirectory(Path.Combine(Root, "client-b"));
        string fixture = Path.Combine(
            AppContext.BaseDirectory,
            "Fixtures",
            "instance-v1",
            "marker"
        );
        CopyDirectory(fixture, Path.Combine(Root, InstanceManager.Marker));
    }

    public string Database => InstanceManager.DatabasePath(Root);

    /// <summary>Rows of a table as text, ordered: compares contents before and after.</summary>
    public IReadOnlyList<string> Rows(string sql)
    {
        using SqliteConnection connection = new SqliteConnection(
            $"Pooling=False;Mode=ReadOnly;Data Source={Database}"
        );
        connection.Open();
        using SqliteCommand command = connection.CreateCommand();
        command.CommandText = sql;
        using SqliteDataReader reader = command.ExecuteReader();
        List<string> rows = new List<string>();
        while (reader.Read())
        {
            rows.Add(
                string.Join("|", Enumerable.Range(0, reader.FieldCount).Select(reader.GetValue))
            );
        }
        return rows;
    }

    public long UserVersion() => long.Parse(Rows("PRAGMA user_version").Single());

    public Dictionary<string, string> Projections() =>
        Directory
            .GetFiles(Path.Combine(Root, InstanceManager.Marker, InstanceManager.RecordsDirectory))
            .ToDictionary(file => Path.GetFileName(file), File.ReadAllText);

    public void Dispose() => Directory.Delete(Root, true);

    private static void CopyDirectory(string source, string target)
    {
        Directory.CreateDirectory(target);
        foreach (string file in Directory.GetFiles(source))
        {
            File.Copy(file, Path.Combine(target, Path.GetFileName(file)));
        }
        foreach (string directory in Directory.GetDirectories(source))
        {
            CopyDirectory(directory, Path.Combine(target, Path.GetFileName(directory)));
        }
    }
}
