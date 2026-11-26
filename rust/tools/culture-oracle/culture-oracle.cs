// Measures how the current culture changes the two timestamps of the daemon (lot 7.2, review):
// InstanceRegistry.Register writes now.ToString("yyyy-MM-dd'T'HH:mm:sszzz") and DaemonLog.Write
// now().ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz"), both without a format provider. Records the
// culture the process gets from its environment, every culture whose TimeSeparator is not ":",
// then the UTF-8 bytes of both formats for a fixed instant under a list of cultures, through the
// real classes (a registry entry and a journal line) and through ToString directly.
// Usage: dotnet run culture-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Daemon/Sermofur.Daemon.csproj
using System.Globalization;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Infrastructure;

CultureInfo start = CultureInfo.CurrentCulture;
JsonObject environment = new JsonObject
{
    ["current_culture"] = start.Name,
    ["current_ui_culture"] = CultureInfo.CurrentUICulture.Name,
    ["LANG"] = Environment.GetEnvironmentVariable("LANG"),
    ["LC_ALL"] = Environment.GetEnvironmentVariable("LC_ALL"),
    ["LC_TIME"] = Environment.GetEnvironmentVariable("LC_TIME"),
    ["DOTNET_SYSTEM_GLOBALIZATION_INVARIANT"] = Environment.GetEnvironmentVariable(
        "DOTNET_SYSTEM_GLOBALIZATION_INVARIANT"
    ),
    ["invariant_mode"] = CultureInfo.GetCultureInfo("en-US").Name == "",
};

JsonArray separators = [];
foreach (CultureInfo culture in CultureInfo.GetCultures(CultureTypes.AllCultures))
{
    string separator = culture.DateTimeFormat.TimeSeparator;
    if (separator != ":")
    {
        separators.Add(new JsonObject { ["culture"] = culture.Name, ["separator"] = separator });
    }
}

DateTimeOffset instant = new DateTimeOffset(2026, 11, 9, 10, 11, 12, 345, TimeSpan.FromMinutes(90));
string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-culture-oracle-" + Guid.NewGuid().ToString("N")
);
Directory.CreateDirectory(scratch);
string root = Path.Combine(scratch, "instance");
Directory.CreateDirectory(root);
new InstanceManager().Initialize(root);

JsonObject Bytes(string text) =>
    new JsonObject
    {
        ["text"] = text,
        ["utf8"] = Convert.ToHexString(Encoding.UTF8.GetBytes(text)),
    };

JsonArray cultures = [];
List<string> names =
[
    "",
    "en-US",
    "fr-FR",
    "th-TH",
    "fi-FI",
    "fa-IR",
    "ar-SA",
    "ja-JP-u-ca-japanese",
    "he-IL",
];
names.AddRange(
    separators
        .Select(entry => entry!["culture"]!.GetValue<string>())
        .Where(name => name != "")
        .Take(5)
);
foreach (string name in names.Distinct())
{
    JsonObject entry = new JsonObject { ["culture"] = name };
    try
    {
        CultureInfo culture = CultureInfo.GetCultureInfo(name);
        CultureInfo.CurrentCulture = culture;
        entry["calendar"] = culture.Calendar.GetType().Name;
        entry["time_separator"] = culture.DateTimeFormat.TimeSeparator;
        entry["seconds_format"] = Bytes(instant.ToString("yyyy-MM-dd'T'HH:mm:sszzz"));
        entry["log_format"] = Bytes(instant.ToString("yyyy-MM-dd'T'HH:mm:ss.fffzzz"));
        string home = Path.Combine(scratch, "home-" + cultures.Count);
        InstanceRegistry registry = new InstanceRegistry(Path.Combine(home, "instances.json"));
        entry["registered_at"] = Bytes(registry.Register(root, instant).RegisteredAt);
        string log = Path.Combine(home, "daemon.log");
        DaemonLog journal = new DaemonLog(log, () => instant);
        journal.Write("probe");
        journal.Flush(TimeSpan.FromSeconds(10));
        entry["journal_line"] = Bytes(File.ReadAllText(log));
    }
    catch (Exception exception)
    {
        entry["error"] = exception.GetType().Name + ": " + exception.Message;
    }
    finally
    {
        CultureInfo.CurrentCulture = start;
    }
    cultures.Add(entry);
}

Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
Directory.Delete(scratch, recursive: true);
File.WriteAllText(
    args[0],
    new JsonObject
    {
        ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
        ["environment"] = environment,
        ["instant"] = "2026-11-09T10:11:12.345+01:30",
        ["cultures"] = cultures,
        ["time_separators"] = separators,
    }
        .ToJsonString(
            new JsonSerializerOptions
            {
                WriteIndented = true,
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            }
        )
        .Replace("\r\n", "\n") + "\n"
);
