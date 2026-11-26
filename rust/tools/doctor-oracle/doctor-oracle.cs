// Records the reports of the real InstanceDoctor of src/Sermofur.Infrastructure (lot 6.2) where
// the command line cannot reach: without the probes of the CLI (daemon and mcp reported as not
// delivered), and with an injected IFileOwnership, as the tests do: an entry of another account
// (ForeignOwnership of HardeningTests) and an owner that cannot be read. Each case runs on a new
// instance made by InstanceManager.Initialize; the report is written by RecordJson, as the CLI
// prints it, with the root replaced by "<root>" and line breaks as "\n".
// Usage: dotnet run doctor-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Microsoft.Data.Sqlite;
using Sermofur.Application;
using Sermofur.Infrastructure;

JsonArray cases = [];

void Case(string name, IFileOwnership? ownership)
{
    string root = Path.Combine(
        Path.GetTempPath(),
        "sermofur-doctor-oracle-" + Guid.NewGuid().ToString("N")
    );
    Directory.CreateDirectory(root);
    new InstanceManager().Initialize(root);
    DoctorReport report = new InstanceDoctor(ownership).Inspect(root);
    SqliteConnection.ClearAllPools();
    Directory.Delete(root, recursive: true);
    cases.Add(
        new JsonObject
        {
            ["name"] = name,
            ["report"] = RecordJson.Write(report with { Root = "<root>" }).Replace("\r\n", "\n"),
        }
    );
}

Case("without_probes", null);
Case("foreign_owner", new ForeignOwnership());
Case("unreadable_owner", new UnreadableOwnership());

File.WriteAllText(
    args[0],
    cases.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ) + "\n"
);

sealed class ForeignOwnership : IFileOwnership
{
    public EntryStatus? Inspect(string path) =>
        Directory.Exists(path) ? new EntryStatus(false, true, false)
        : File.Exists(path) ? new EntryStatus(true, false, false)
        : null;
}

sealed class UnreadableOwnership : IFileOwnership
{
    public EntryStatus? Inspect(string path) => throw new IOException("owner unreadable");
}
