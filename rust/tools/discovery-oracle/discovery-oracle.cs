// Records what InstanceManager.Discover and DiscoverForDiagnosis of src/Sermofur.Infrastructure
// return on crafted folders: markers that are instances, files, empty or damaged folders, links,
// unreadable entries, foreign or unreadable owners (fake ownership), missing paths and paths
// that are not local. The scratch root is replaced by "<root>" and "\" by "/".
// Usage: dotnet run discovery-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Diagnostics;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Infrastructure;

string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-discovery-oracle-" + Guid.NewGuid().ToString("N")
);
const string Valid =
    "{\"schemaVersion\":2,\"instanceId\":\"0f8fad5b-d9cb-469f-a165-70867728950e\",\"createdAt\":\"2026-01-02T03:04:05+00:00\"}";
const UnixFileMode Rwx = UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;

// Creates <root>/.sermofur with the given entries; a name ending with "/" is a folder.
void Marker(string root, params string[] entries)
{
    string marker = Path.Combine(root, ".sermofur");
    Directory.CreateDirectory(marker);
    foreach (string entry in entries)
    {
        if (entry.EndsWith('/'))
        {
            Directory.CreateDirectory(Path.Combine(marker, entry.TrimEnd('/')));
        }
        else
        {
            File.WriteAllText(Path.Combine(marker, entry), entry == "instance.json" ? Valid : "x");
        }
    }
}

// A link to a folder: a junction on Windows (no privilege needed), a symbolic link elsewhere.
void DirectoryLink(string link, string target)
{
    if (OperatingSystem.IsWindows())
    {
        using Process process = Process.Start(
            new ProcessStartInfo("cmd.exe", ["/C", "mklink", "/J", link, target])
            {
                RedirectStandardOutput = true,
                UseShellExecute = false,
            }
        )!;
        process.WaitForExit();
    }
    else
    {
        Directory.CreateSymbolicLink(link, target);
    }
}

// Denies the listing of a folder to its owner (Windows) or every right (Unix); undone at the end.
List<Action> restore = [];
void Unlistable(string folder)
{
    if (OperatingSystem.IsWindows())
    {
        DenyListing(folder, restore);
    }
    else
    {
        File.SetUnixFileMode(folder, UnixFileMode.None);
        restore.Add(() => RestoreMode(folder));
    }
}

// Unix only: a folder that can be listed but not searched, so the attributes of its children
// cannot be read.
void Unsearchable(string folder)
{
    if (!OperatingSystem.IsWindows())
    {
        File.SetUnixFileMode(folder, UnixFileMode.UserRead | UnixFileMode.UserWrite);
        restore.Add(() => RestoreMode(folder));
    }
}

void InstanceIn(string root, string relative)
{
    Directory.CreateDirectory(Path.Combine(root, relative));
    Marker(Path.Combine(root, relative), "instance.json");
}

string sep = Path.DirectorySeparatorChar.ToString();

// Name, preparation of the root, start path relative to the root, ownership.
List<(string Name, Action<string> Prepare, string Start, string Owners)> scenarios =
[
    ("instance_at_start", r => Marker(r, "instance.json"), "", "real"),
    (
        "instance_above",
        r =>
        {
            Marker(r, "instance.json");
            Directory.CreateDirectory(Path.Combine(r, "a", "b"));
        },
        "a/b",
        "real"
    ),
    (
        "start_with_dot_dot",
        r =>
        {
            Marker(r, "instance.json");
            Directory.CreateDirectory(Path.Combine(r, "a"));
        },
        "a/..",
        "real"
    ),
    (
        "start_with_trailing_separator",
        r =>
        {
            Marker(r, "instance.json");
            Directory.CreateDirectory(Path.Combine(r, "a"));
        },
        "a/",
        "real"
    ),
    ("no_instance", _ => { }, "", "real"),
    ("start_missing", _ => { }, "missing", "real"),
    ("start_is_file", r => File.WriteAllText(Path.Combine(r, "file"), "x"), "file", "real"),
    ("marker_is_file", r => File.WriteAllText(Path.Combine(r, ".sermofur"), "x"), "", "real"),
    ("marker_empty", r => Marker(r), "", "real"),
    ("marker_unrelated_entry", r => Marker(r, "notes.txt"), "", "real"),
    ("damaged_database_only", r => Marker(r, "memory.db"), "", "real"),
    ("damaged_records_only", r => Marker(r, "records/"), "", "real"),
    ("damaged_backups_only", r => Marker(r, "backups/"), "", "real"),
    ("configuration_upper_case", r => Marker(r, "INSTANCE.JSON"), "", "real"),
    ("configuration_dotless_i", r => Marker(r, "\u0131nstance.json"), "", "real"),
    ("backups_long_s", r => Marker(r, "backup\u017F/"), "", "real"),
    ("configuration_is_folder", r => Marker(r, "instance.json/"), "", "real"),
    (
        "nearest_marker_wins",
        r =>
        {
            Marker(r, "instance.json");
            Directory.CreateDirectory(Path.Combine(r, "a"));
            Marker(Path.Combine(r, "a"));
        },
        "a",
        "real"
    ),
    (
        "marker_link",
        r =>
        {
            InstanceIn(r, "real");
            DirectoryLink(Path.Combine(r, ".sermofur"), Path.Combine(r, "real", ".sermofur"));
        },
        "",
        "real"
    ),
    (
        "start_through_link",
        r =>
        {
            InstanceIn(r, "real");
            DirectoryLink(Path.Combine(r, "link"), Path.Combine(r, "real"));
        },
        "link",
        "real"
    ),
    ("foreign_owner", r => Marker(r, "instance.json"), "", "foreign"),
    ("owner_unreadable", r => Marker(r, "instance.json"), "", "throwing"),
    ("owner_unknown", r => Marker(r, "instance.json"), "", "absent"),
    (
        "marker_unlistable",
        r =>
        {
            Marker(r, "instance.json");
            Unlistable(Path.Combine(r, ".sermofur"));
        },
        "",
        "real"
    ),
];
if (OperatingSystem.IsWindows())
{
    scenarios.Add(("unc_path", _ => { }, @"\\localhost\c$\sermofur-none", "real"));
    scenarios.Add(("device_path", _ => { }, @"\\?\C:\sermofur-none", "real"));
    scenarios.Add(("nt_path_missing", _ => { }, @"\??\C:\sermofur-none", "real"));
    scenarios.Add(("nt_path_instance", r => Marker(r, "instance.json"), @"\??\<root>", "real"));
}
else
{
    scenarios.Add(
        (
            "marker_attributes_unreadable",
            r =>
            {
                InstanceIn(r, "locked");
                Unsearchable(Path.Combine(r, "locked"));
            },
            "locked",
            "real"
        )
    );
}

string Show(string root, string text) => text.Replace(root, "<root>").Replace('\\', '/');

JsonObject Run(Func<string> call, string root)
{
    try
    {
        return new JsonObject { ["root"] = Show(root, call()) };
    }
    catch (SermofurException exception)
    {
        return new JsonObject
        {
            ["code"] = exception.Code,
            ["message"] = Show(root, exception.Message),
            ["exitCode"] = exception.ExitCode,
        };
    }
    catch (Exception exception)
    {
        // Not a Sermofur refusal: the CLI maps it later; the type is what the port must match.
        return new JsonObject { ["exception"] = exception.GetType().FullName };
    }
}

JsonArray rows = [];
foreach ((string name, Action<string> prepare, string start, string owners) in scenarios)
{
    string root = Path.Combine(scratch, name);
    Directory.CreateDirectory(root);
    prepare(root);
    IFileOwnership ownership = owners switch
    {
        "foreign" => new FakeOwnership(Throws: false, Absent: false),
        "throwing" => new FakeOwnership(Throws: true, Absent: false),
        "absent" => new FakeOwnership(Throws: false, Absent: true),
        _ => new FileOwnership(),
    };
    InstanceManager manager = new InstanceManager(ownership);
    string path =
        start.StartsWith('\\') ? start.Replace("<root>", root)
        : start.Length == 0 ? root
        : root + sep + start.Replace('/', Path.DirectorySeparatorChar);
    rows.Add(
        new JsonObject
        {
            ["name"] = name,
            ["start"] = start,
            ["owners"] = owners,
            ["discover"] = Run(() => manager.Discover(path), root),
            ["diagnosis"] = Run(() => manager.DiscoverForDiagnosis(path), root),
        }
    );
}

foreach (Action undo in restore)
{
    undo();
}

// Links and junctions must not be followed by the recursive delete.
foreach (
    string entry in Directory
        .EnumerateFileSystemEntries(scratch, "*", SearchOption.AllDirectories)
        .Where(entry => new DirectoryInfo(entry).LinkTarget is not null)
        .ToList()
)
{
    Directory.Delete(entry);
}
Directory.Delete(scratch, recursive: true);

JsonObject result = new()
{
    ["generator"] = "rust/tools/discovery-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
    ["scenarios"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} scenarios");

static void RestoreMode(string folder)
{
    if (!OperatingSystem.IsWindows())
    {
        File.SetUnixFileMode(folder, Rwx);
    }
}

static void DenyListing(string folder, List<Action> restore)
{
    if (!OperatingSystem.IsWindows())
    {
        return;
    }
    DirectoryInfo info = new DirectoryInfo(folder);
    FileSystemAccessRule deny = new FileSystemAccessRule(
        WindowsIdentity.GetCurrent().User!,
        FileSystemRights.ListDirectory,
        AccessControlType.Deny
    );
    DirectorySecurity security = info.GetAccessControl();
    security.AddAccessRule(deny);
    info.SetAccessControl(security);
    restore.Add(() =>
    {
        if (!OperatingSystem.IsWindows())
        {
            return;
        }
        DirectorySecurity current = info.GetAccessControl();
        current.RemoveAccessRule(deny);
        info.SetAccessControl(current);
    });
}

/// <summary>Ownership of a test: every entry is foreign, its owner unreadable, or absent.</summary>
sealed record FakeOwnership(bool Throws, bool Absent) : IFileOwnership
{
    public EntryStatus? Inspect(string path) =>
        Throws ? throw new IOException("owner unreadable")
        : Absent ? null
        : new EntryStatus(false, true, false);
}
