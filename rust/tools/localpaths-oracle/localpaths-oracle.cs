// Records what LocalPaths of src/Sermofur.Infrastructure answers: Contains, NormalizeMapping,
// ResolveMapping, RelativizeMapping and CanonicalCase, on a crafted folder (case, NFC and NFD
// names, an 8.3 short name, an ambiguous pair, a folder that cannot be listed). The scratch root
// is replaced by "<root>" and "\" by "/" in results and messages.
// Usage: dotnet run localpaths-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Infrastructure;

bool windows = OperatingSystem.IsWindows();

// The name holds an "i" and an "s", so that variants with "ı" and "ſ" exist.
string guid = Guid.NewGuid().ToString("N");
string root = Path.Combine(
    Path.GetFullPath(Path.GetTempPath()),
    "sermofur-localpaths-oracle-is-" + guid
);
string Under(string relative) => Path.Combine(root, relative);
Directory.CreateDirectory(Under(Path.Combine("Alpha", "Beta")));
File.WriteAllText(Under(Path.Combine("Alpha", "file.txt")), "x");
Directory.CreateDirectory(Under("caf\u00e9"));
Directory.CreateDirectory(Under("nfd-e\u0301"));
Directory.CreateDirectory(Under("\u212Aelvin"));
Directory.CreateDirectory(Under("Long Folder Name"));
Directory.CreateDirectory(Under("ſtate"));
Directory.CreateDirectory(Under("𐐨x"));

// A dangling link, typed in another case by the CanonicalCase rows.
File.CreateSymbolicLink(Under("Dangle"), Under("nowhere"));
Directory.CreateDirectory(Under(Path.Combine("Locked", "inner")));
if (windows)
{
    // Two names that differ only by normalization: both match DUP-É without case.
    Directory.CreateDirectory(Under("dup-\u00e9"));
    Directory.CreateDirectory(Under("dup-e\u0301"));
}
List<Action> restore = [];
if (windows)
{
    DenyListing(Under("Locked"), restore);
}
else
{
    // Unix: searchable but not listable (mode 0300).
    File.SetUnixFileMode(Under("Locked"), UnixFileMode.UserWrite | UnixFileMode.UserExecute);
    restore.Add(() => RestoreMode(Under("Locked")));
}

// The 8.3 alias of "Long Folder Name", when the volume generates short names.
string shortName = windows ? ShortName(Under("Long Folder Name")) : "Long Folder Name";

string temp = Path.GetDirectoryName(root)!;
string Show(string text) =>
    text.Replace(root, "<root>")
        .Replace(temp, "<temp>")
        .Replace(guid, "<guid>")
        .Replace(guid.ToUpperInvariant(), "<GUID>")
        .Replace('\\', '/');

JsonObject Call(string function, JsonArray arguments, Func<object> call)
{
    JsonObject row = new() { ["function"] = function, ["arguments"] = arguments };
    try
    {
        object result = call();
        row["result"] = result is bool flag ? flag : Show((string)result);
    }
    catch (SermofurException exception)
    {
        row["code"] = exception.Code;
        row["message"] = Show(exception.Message);
        row["exitCode"] = exception.ExitCode;
    }
    catch (Exception exception)
    {
        row["exception"] = exception.GetType().FullName;
    }
    return row;
}

string sep = Path.DirectorySeparatorChar.ToString();
JsonArray rows = [];

// Contains(parent, child): paths given under <root>.
List<(string Parent, string Child)> pairs =
[
    ("", ""),
    ("", "a"),
    ("a", "ab"),
    ("a" + sep, "a" + sep + "b"),
    ("A", "a" + sep + "b"),
    ("a", ""),
    ("a" + sep + "b", "a"),
    ("is", "ıs" + sep + "x"),
    ("S", "ſ" + sep + "x"),
    ("𐐀", "𐐨" + sep + "x"),
];
foreach ((string parent, string child) in pairs)
{
    string p = parent.Length == 0 ? root : Under(parent);
    string c = child.Length == 0 ? root : Under(child);
    rows.Add(Call("Contains", [Show(p), Show(c)], () => LocalPaths.Contains(p, c)));
}
string volume = Path.GetPathRoot(root)!;
rows.Add(
    Call("Contains", ["<volume>", Show(Under("x"))], () => LocalPaths.Contains(volume, Under("x")))
);

// NormalizeMapping and ResolveMapping(root, relative).
string[] mappings =
[
    "Alpha/Beta",
    "Alpha\\Beta",
    "alpha/beta",
    "/abs",
    "\\abs",
    "C:x",
    "a:b",
    "../x",
    "a/../..",
    "a/../b",
    "",
    ".",
    "a/./b/",
    "a/b ",
    "a/b.",
    "LONGFO~1/y",
    "missing",
    "Alpha/file.txt",
    "../sermofur-localpaths-oracle-ıs-" + guid + "/x",
    "../sermofur-localpaths-oracle-iſ-" + guid + "/x",
];
foreach (string mapping in mappings)
{
    rows.Add(
        Call(
            "NormalizeMapping",
            ["<root>", Show(mapping)],
            () => LocalPaths.NormalizeMapping(root, mapping)
        )
    );
    rows.Add(
        Call(
            "ResolveMapping",
            ["<root>", Show(mapping)],
            () => LocalPaths.ResolveMapping(root, mapping)
        )
    );
}

// RelativizeMapping(root, absolute): absolute paths given under <root>, or outside it.
List<string> absolutes =
[
    Under(Path.Combine("a", "b")),
    root,
    root + sep,
    Under(Path.Combine("A", "b")),
    Path.GetDirectoryName(root)!,
    Path.Combine(Path.GetDirectoryName(root)!, "sibling", "x"),
    Under(Path.Combine("a", "..", "b")),
    Under("a" + sep + sep + "b"),
    Path.Combine(temp, "sermofur-localpaths-oracle-ıs-" + guid, "x"),
    Path.Combine(temp, "sermofur-localpaths-oracle-iſ-" + guid, "x"),
    Path.Combine(temp, "SERMOFUR-LOCALPATHS-ORACLE-IS-" + guid.ToUpperInvariant(), "x"),
];
if (windows)
{
    absolutes.Add(@"Q:\elsewhere\x");
}
foreach (string absolute in absolutes)
{
    rows.Add(
        Call(
            "RelativizeMapping",
            ["<root>", Show(absolute)],
            () => LocalPaths.RelativizeMapping(root, absolute)
        )
    );
}

// CanonicalCase(root, relative).
List<string> relatives =
[
    "Alpha/Beta",
    "alpha/beta",
    "ALPHA/BETA/missing/x",
    "Alpha/FILE.TXT",
    "caf\u00e9",
    "CAF\u00c9",
    "cafe\u0301",
    "nfd-\u00e9",
    "kelvin",
    "\u212Aelvin",
    "locked",
    "locked/inner",
    "Alpha/../alpha",
    "./alpha",
    "",
    "alpha//beta",
    "a*b",
    "missing/x",
    "STATE",
    "ſtate",
    "𐐀x",
    "𐐨x",
    "dangle",
];
if (windows)
{
    relatives.Add("DUP-\u00c9");
    relatives.Add(shortName + "/x");
}
foreach (string relative in relatives)
{
    rows.Add(
        Call("CanonicalCase", ["<root>", relative], () => LocalPaths.CanonicalCase(root, relative))
    );
}

foreach (Action undo in restore)
{
    undo();
}
Directory.Delete(root, recursive: true);

JsonObject result = new()
{
    ["generator"] = "rust/tools/localpaths-oracle",
    ["runtime"] = RuntimeInformation.FrameworkDescription,
    ["os"] = windows ? "windows" : "unix",
    ["shortName"] = shortName,
    ["calls"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} calls, short name {shortName}");

static void RestoreMode(string folder)
{
    if (!OperatingSystem.IsWindows())
    {
        File.SetUnixFileMode(
            folder,
            UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute
        );
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

static string ShortName(string path)
{
    StringBuilder buffer = new StringBuilder(1024);
    uint length = Native.GetShortPathName(path, buffer, (uint)buffer.Capacity);
    return length == 0 ? Path.GetFileName(path) : Path.GetFileName(buffer.ToString());
}

static class Native
{
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern uint GetShortPathName(string longPath, StringBuilder shortPath, uint size);
}
