// Records how src/Sermofur.Infrastructure/FileSourceReader reads a source and how
// SourcePassages.Split of src/Sermofur.Application cuts its text (lot 5.1):
// - "whitespace": every UTF-16 unit for which char.IsWhiteSpace is true (string.IsNullOrWhiteSpace);
// - "utf8": the strict decoder of the reader (its private StrictUtf8) on crafted byte sequences;
// - "snapshots": FileSourceReader.Snapshot on crafted bytes (bound, hash, BOM, decoding, NUL,
//   blank text), with the refusal SourceService.RefuseUnindexable makes of it;
// - "splits": SourcePassages.Split on crafted and pseudo-random texts;
// - "reads": FileSourceReader.Read on crafted folders (missing, folder, links, junctions, mapping
//   outside the root, unreadable, locked, FIFO), each with the setup the Rust test replays.
// Bytes and texts are written as segments [piece, count], repeated then joined. A passage keeps
// its text when it is at most 300 UTF-16 units long; it always has its length in UTF-16 units
// and the SHA-256 of its UTF-8 bytes. The result of Read depends on the system: record on
// Windows and on Linux.
// Usage: dotnet run source-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Diagnostics;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Application;
using Sermofur.Domain;
using Sermofur.Infrastructure;

bool windows = OperatingSystem.IsWindows();
const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Static;
MethodInfo snapshotMethod = typeof(FileSourceReader).GetMethod("Snapshot", Hidden)!;
MethodInfo refuseMethod = typeof(SourceService).GetMethod("RefuseUnindexable", Hidden)!;
UTF8Encoding strict = (UTF8Encoding)
    typeof(FileSourceReader).GetField("StrictUtf8", Hidden)!.GetValue(null)!;

string Hex(byte[] bytes) => Convert.ToHexStringLower(bytes);

byte[] Bytes(JsonArray segments)
{
    List<byte> bytes = [];
    foreach (JsonNode? segment in segments)
    {
        byte[] piece = Convert.FromHexString((string)segment![0]!);
        for (int i = 0; i < (int)segment[1]!; i++)
        {
            bytes.AddRange(piece);
        }
    }
    return [.. bytes];
}

string Text(JsonArray segments)
{
    StringBuilder text = new();
    foreach (JsonNode? segment in segments)
    {
        text.Insert(text.Length, (string)segment![0]!, (int)segment[1]!);
    }
    return text.ToString();
}

JsonArray ByteSegments(params (string Text, int Count)[] parts) =>
    [.. parts.Select(part => (JsonNode)new JsonArray(part.Text, part.Count))];

JsonArray Passages(IReadOnlyList<SearchDocument> passages)
{
    JsonArray rows = [];
    foreach (SearchDocument passage in passages)
    {
        JsonObject row = new()
        {
            ["passage"] = passage.Passage,
            ["units"] = passage.Text.Length,
            ["sha256"] = Hex(SHA256.HashData(Encoding.UTF8.GetBytes(passage.Text))),
        };
        if (passage.Text.Length <= 300)
        {
            row["text"] = passage.Text;
        }
        rows.Add(row);
    }
    return rows;
}

JsonObject Describe(SourceSnapshot snapshot)
{
    JsonObject row = new()
    {
        ["status"] = snapshot.Status.ToString(),
        ["hash"] = snapshot.Hash,
        ["size"] = snapshot.Size,
        ["rejection"] = snapshot.Rejection?.ToString(),
        ["passages"] = Passages(snapshot.Passages),
    };
    try
    {
        refuseMethod.Invoke(null, [snapshot]);
        row["refusal"] = null;
    }
    catch (TargetInvocationException exception)
        when (exception.InnerException is SermofurException refusal)
    {
        row["refusal"] = new JsonObject
        {
            ["code"] = refusal.Code,
            ["message"] = refusal.Message,
            ["exitCode"] = refusal.ExitCode,
        };
    }
    return row;
}

// --- whitespace -----------------------------------------------------------------------------
JsonArray whitespace = [];
for (int unit = 0; unit <= 0xFFFF; unit++)
{
    if (char.IsWhiteSpace((char)unit))
    {
        whitespace.Add(unit);
    }
}

// --- utf8 -----------------------------------------------------------------------------------
List<byte[]> probes = [];
for (int b = 0; b <= 0xFF; b++)
{
    probes.Add([(byte)b]);
}
byte[] seconds = [0x00, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xFF];
for (int lead = 0xC0; lead <= 0xFF; lead++)
{
    foreach (byte second in seconds)
    {
        byte l = (byte)lead;
        probes.Add([l, second]);
        probes.Add([l, second, 0x80]);
        probes.Add([l, second, 0xBF]);
        probes.Add([l, second, 0xC0]);
        probes.Add([l, second, 0x80, 0x80]);
        probes.Add([l, second, 0xBF, 0xBF]);
        probes.Add([l, second, 0x80, 0xC0]);
        probes.Add([0x61, l, second, 0x80, 0x80, 0x62]);
    }
}
string[] named =
[
    "c080", // overlong NUL
    "c1bf",
    "e08080",
    "e09fbf", // overlong U+07FF
    "e0a080", // first 3-byte
    "f0808080",
    "f08fbfbf", // overlong U+FFFF
    "f0908080", // first 4-byte
    "eda080", // U+D800
    "edafbf", // U+DBFF
    "edb080", // U+DC00
    "edbfbf", // U+DFFF
    "ed9fbf", // U+D7FF
    "ee8080", // U+E000
    "eda0bdedb880", // CESU-8 pair for U+1F600
    "f09f9880", // U+1F600
    "f48fbfbf", // U+10FFFF
    "f4908080", // U+110000
    "f7bfbfbf",
    "f888808080", // 5-byte form
    "fc8480808080", // 6-byte form
    "efbfbe", // U+FFFE
    "efbfbf", // U+FFFF
    "efb790", // U+FDD0
    "efbbbf", // BOM alone
    "efbbbfefbbbf", // two BOMs
    "efbb", // truncated BOM
    "fffe6100", // UTF-16 LE BOM
    "feff0061", // UTF-16 BE BOM
    "fffe000061000000", // UTF-32 LE BOM
    "0000feff00000061", // UTF-32 BE BOM
    "c3", // truncated
    "e282", // truncated
    "f09f98", // truncated
    "61c362", // truncated inside
    "80", // lone continuation
    "80bf",
    "c3a9c3a9",
    "e282ac", // euro
    "00",
    "610062",
    "0d0a",
    "c285", // U+0085
    "e280a8", // U+2028
];
foreach (string hex in named)
{
    probes.Add(Convert.FromHexString(hex));
}
JsonArray utf8 = [];
foreach (byte[] probe in probes)
{
    string? text;
    try
    {
        text = strict.GetString(probe);
    }
    catch (DecoderFallbackException)
    {
        text = null;
    }
    utf8.Add(new JsonObject { ["hex"] = Hex(probe), ["text"] = text });
}

// --- snapshots ------------------------------------------------------------------------------
const int MiB = 1024 * 1024;
string Utf8Hex(string text) => Hex(Encoding.UTF8.GetBytes(text));
List<(string Name, JsonArray Segments)> snapshotCases =
[
    ("empty", ByteSegments()),
    ("text", ByteSegments((Utf8Hex("bonjour\n\nle monde"), 1))),
    ("bom_text", ByteSegments(("efbbbf", 1), (Utf8Hex("été"), 1))),
    ("bom_only", ByteSegments(("efbbbf", 1))),
    ("two_boms", ByteSegments(("efbbbf", 2), (Utf8Hex("x"), 1))),
    ("bom_then_blank", ByteSegments(("efbbbf", 1), (Utf8Hex(" \n\t "), 1))),
    ("blank", ByteSegments((Utf8Hex("  \n "), 1))),
    ("blank_unicode", ByteSegments((Utf8Hex("\u00a0\u2028\u3000\u0085\r\n"), 1))),
    ("zero_width_space", ByteSegments((Utf8Hex("\u200b"), 1))),
    ("nul_inside", ByteSegments(("410042", 1))),
    ("nul_only", ByteSegments(("00", 1))),
    ("nul_after_bom", ByteSegments(("efbbbf00", 1))),
    ("latin1", ByteSegments(("636166e9", 1))),
    ("utf16le_bom", ByteSegments(("fffe", 1), ("6100", 3))),
    ("utf16be_bom", ByteSegments(("feff", 1), ("0061", 3))),
    ("utf32le_bom", ByteSegments(("fffe0000", 1), ("61000000", 3))),
    ("utf32be_bom", ByteSegments(("0000feff", 1), ("00000061", 3))),
    ("overlong", ByteSegments((Utf8Hex("a"), 1), ("c0af", 1))),
    ("surrogate", ByteSegments((Utf8Hex("a"), 1), ("eda080", 1))),
    ("truncated_end", ByteSegments((Utf8Hex("abc"), 1), ("e282", 1))),
    ("invalid_and_nul", ByteSegments(("00ff", 1))),
    ("crlf", ByteSegments((Utf8Hex("un\r\n\r\ndeux\r\ntrois\r\n"), 1))),
    ("cr_only", ByteSegments((Utf8Hex("un\r\rdeux\rtrois"), 1))),
    ("emoji", ByteSegments((Utf8Hex("mot 😀 "), 900))),
    ("exact_bound", ByteSegments(("61", MiB))),
    ("bound_plus_one", ByteSegments(("61", MiB + 1))),
    ("bound_with_bom", ByteSegments(("efbbbf", 1), ("61", MiB - 3))),
    ("bound_plus_bom", ByteSegments(("efbbbf", 1), ("61", MiB))),
    ("bound_lines", ByteSegments((Utf8Hex("ligne de texte\n"), MiB / 15))),
    ("bound_invalid_last", ByteSegments(("61", MiB - 1), ("ff", 1))),
    ("bound_paragraphs", ByteSegments((Utf8Hex(new string('é', 999) + "\n\n"), 524))),
    ("large", ByteSegments(("61", 3 * MiB))),
];
JsonArray snapshots = [];
foreach ((string name, JsonArray segments) in snapshotCases)
{
    SourceSnapshot snapshot = (SourceSnapshot)snapshotMethod.Invoke(null, [Bytes(segments)])!;
    JsonObject row = new() { ["name"] = name, ["bytes"] = segments.DeepClone() };
    foreach (KeyValuePair<string, JsonNode?> field in Describe(snapshot))
    {
        row[field.Key] = field.Value?.DeepClone();
    }
    snapshots.Add(row);
}

// --- splits ---------------------------------------------------------------------------------
JsonArray T(params (string Text, int Count)[] parts) => ByteSegments(parts);
List<JsonArray> splitCases =
[
    T(),
    T(("a", 1)),
    T(("\n", 5)),
    T((" ", 3)),
    T(("a\n\nb", 1)),
    T(("a\n\n\nb", 1)),
    T(("a\n\n\n\nb", 1)),
    T(("\n\na\n\n", 1)),
    T(("a\r\n\r\nb", 1)),
    T(("a\r\n\nb", 1)),
    T(("a\n\r\nb", 1)),
    T(("a\r\rb", 1)),
    T(("a\r\r\n\nb", 1)),
    T(("  a  \n\n  \n\n b ", 1)),
    T(("a\n \nb", 1)),
    T(("\ta\t\n\n\t", 1)),
    T(("a\u2028\u2028b", 1)),
    T(("a", 2000)),
    T(("a", 2001)),
    T(("a", 4000)),
    T(("a", 4001)),
    T(("a", 1999), ("😀", 1)),
    T(("a", 1998), ("😀", 1)),
    T(("a", 1999), ("😀", 2)),
    T(("😀", 1000)),
    T(("😀", 1001)),
    T(("a", 999), ("\n\n", 1), ("b", 999)),
    T(("a", 999), ("\n\n", 1), ("b", 1000)),
    T(("a", 998), ("\n\n", 1), ("b", 1000)),
    T(("a", 1000), ("\n", 1), ("b", 999)),
    T(("a", 1000), ("\n", 1), ("b", 1000)),
    T(("a", 1500), ("\n", 1), ("b", 1500)),
    T(("a", 2500), ("\n", 1), ("b", 10)),
    T(("a", 1500), ("\n\n", 1), ("b", 2500)),
    T(("a", 1500), ("\n\n", 1), (" ", 2500)),
    T(("ligne\n", 500)),
    T(("ligne\r\n", 500)),
    T(("ligne\r", 500)),
    T(("mot ", 600), ("\n", 1), ("\n", 1), ("x", 3)),
    T(("mot 😀 ", 900), ("\n\ncourt\n\n", 1), ("mot 😀 ", 900)),
    T(("a", 1999), ("\n\n", 1), ("b", 1)),
    T(("\n", 1), ("a", 2001), ("\n", 1)),
    T(("a", 2001), ("\n \n", 1), ("b", 3)),
];

// Pseudo-random texts: runs of pieces with counts around the bounds.
string[] pieces =
[
    "a",
    "mot ",
    " ",
    "\n",
    "\r\n",
    "\r",
    "😀",
    "é",
    "\t",
    "\u2028",
    "\u00a0",
    "x\n\ny",
    "\n\n",
];
int[] counts = [1, 1, 2, 3, 7, 499, 500, 998, 999, 1000, 1001, 1002, 1999, 2000, 2001, 2002, 4001];
Random random = new(51);
for (int i = 0; i < 250; i++)
{
    int runs = random.Next(1, 7);
    (string, int)[] parts = new (string, int)[runs];
    for (int r = 0; r < runs; r++)
    {
        string piece = pieces[random.Next(pieces.Length)];
        int count = counts[random.Next(counts.Length)];
        if (piece.Length > 1)
        {
            count = Math.Max(1, count / piece.Length);
        }
        parts[r] = (piece, count);
    }
    splitCases.Add(T(parts));
}
JsonArray splits = [];
foreach (JsonArray segments in splitCases)
{
    splits.Add(
        new JsonObject
        {
            ["text"] = segments,
            ["passages"] = Passages(SourcePassages.Split(Text(segments))),
        }
    );
}

// --- reads ----------------------------------------------------------------------------------
string scratch = Path.Combine(
    Path.GetFullPath(Path.GetTempPath()),
    "sermofur-source-oracle-" + Guid.NewGuid().ToString("N")
);
string user = Environment.UserName;

void Run(string program, params string[] arguments)
{
    using Process process = Process.Start(
        new ProcessStartInfo(program, arguments)
        {
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            UseShellExecute = false,
        }
    )!;
    process.WaitForExit();
    if (process.ExitCode != 0)
    {
        throw new InvalidOperationException($"{program} failed");
    }
}

JsonObject Op(string op, string path, JsonNode? extra = null, string key = "target")
{
    JsonObject row = new() { ["op"] = op, ["path"] = path };
    if (extra is not null)
    {
        row[key] = extra;
    }
    return row;
}
JsonObject FileOp(string path, JsonArray bytes) => Op("file", path, bytes, "bytes");
JsonObject Utf8File(string path, string text) => FileOp(path, ByteSegments((Utf8Hex(text), 1)));

List<IDisposable> held = [];
List<Action> undo = [];

void Apply(string folder, JsonObject step)
{
    string Under(string relative) => Path.GetFullPath(Path.Combine(folder, relative));
    string path = Under((string)step["path"]!);
    switch ((string)step["op"]!)
    {
        case "file":
            File.WriteAllBytes(path, Bytes(step["bytes"]!.AsArray()));
            break;
        case "dir":
            Directory.CreateDirectory(path);
            break;
        case "fileLink":
            File.CreateSymbolicLink(path, Under((string)step["target"]!));
            break;
        case "dirLink":
            Directory.CreateSymbolicLink(path, Under((string)step["target"]!));
            break;
        case "junction":
            if (windows)
            {
                Run("cmd.exe", "/C", "mklink", "/J", path, Under((string)step["target"]!));
            }
            else
            {
                Directory.CreateSymbolicLink(path, Under((string)step["target"]!));
            }
            break;
        case "readOnly":
            File.SetAttributes(path, FileAttributes.ReadOnly);
            undo.Add(() => File.SetAttributes(path, FileAttributes.Normal));
            break;
        case "denyRead":
            if (OperatingSystem.IsWindows())
            {
                Run("icacls", path, "/deny", user + ":(RD)");
                undo.Add(() => Run("icacls", path, "/remove:d", user));
            }
            else
            {
                File.SetUnixFileMode(path, UnixFileMode.None);
                UnixFileMode owner =
                    UnixFileMode.UserRead | UnixFileMode.UserWrite | UnixFileMode.UserExecute;
                undo.Add(() =>
                {
                    if (!OperatingSystem.IsWindows())
                    {
                        File.SetUnixFileMode(path, owner);
                    }
                });
            }
            break;
        case "lock":
            held.Add(new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.None));
            break;
        case "fifo":
            Run("mkfifo", path);
            break;
        default:
            throw new InvalidOperationException((string)step["op"]!);
    }
}

// Each case: a fresh folder, its setup, then Read(<folder>/<root>, path).
List<(string Name, JsonArray Setup, string Root, string Path)> readCases =
[
    ("text", [Op("dir", "r"), Utf8File("r/a.md", "bonjour\n\nmonde")], "r", "a.md"),
    (
        "bom",
        [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("efbbbf", 1), (Utf8Hex("été"), 1)))],
        "r",
        "a.md"
    ),
    ("empty", [Op("dir", "r"), FileOp("r/a.md", ByteSegments())], "r", "a.md"),
    ("blank", [Op("dir", "r"), Utf8File("r/a.md", "  \n ")], "r", "a.md"),
    ("nul", [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("410042", 1)))], "r", "a.md"),
    ("latin1", [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("636166e9", 1)))], "r", "a.md"),
    ("exact_bound", [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("61", MiB)))], "r", "a.md"),
    (
        "bound_plus_one",
        [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("61", MiB + 1)))],
        "r",
        "a.md"
    ),
    ("large", [Op("dir", "r"), FileOp("r/a.md", ByteSegments(("61", 3 * MiB)))], "r", "a.md"),
    ("crlf", [Op("dir", "r"), Utf8File("r/a.md", "un\r\n\r\ndeux\r\n")], "r", "a.md"),
    ("missing", [Op("dir", "r")], "r", "nope.md"),
    ("missing_folder", [Op("dir", "r")], "r", "nodir/a.md"),
    ("directory", [Op("dir", "r"), Op("dir", "r/dir")], "r", "dir"),
    ("root_itself", [Op("dir", "r")], "r", ""),
    ("dot", [Op("dir", "r")], "r", "."),
    ("outside", [Op("dir", "r"), Utf8File("outside.md", "dehors")], "r", "../outside.md"),
    ("rooted_slash", [Op("dir", "r")], "r", "/a.md"),
    ("rooted_backslash", [Op("dir", "r")], "r", "\\a.md"),
    ("drive", [Op("dir", "r")], "r", "C:/a.md"),
    ("colon", [Op("dir", "r"), Utf8File("r/a.md", "x")], "r", "a.md:stream"),
    (
        "backslash",
        [Op("dir", "r"), Op("dir", "r/sub"), Utf8File("r/sub/a.md", "x")],
        "r",
        "sub\\a.md"
    ),
    ("dotted", [Op("dir", "r"), Op("dir", "r/sub"), Utf8File("r/a.md", "x")], "r", "./sub/../a.md"),
    ("up_and_back", [Op("dir", "r"), Utf8File("r/a.md", "x")], "r", "../r/a.md"),
    ("case_variant", [Op("dir", "r"), Utf8File("r/a.md", "x")], "r", "A.MD"),
    ("trailing_dot", [Op("dir", "r"), Utf8File("r/a.md", "x")], "r", "a.md."),
    ("nul_in_path", [Op("dir", "r")], "r", "a\0.md"),
    ("read_only", [Op("dir", "r"), Utf8File("r/a.md", "x"), Op("readOnly", "r/a.md")], "r", "a.md"),
    (
        "file_link",
        [Op("dir", "r"), Utf8File("r/a.md", "x"), Op("fileLink", "r/l.md", "r/a.md")],
        "r",
        "l.md"
    ),
    ("dangling_link", [Op("dir", "r"), Op("fileLink", "r/l.md", "r/nowhere.md")], "r", "l.md"),
    (
        "dir_link",
        [Op("dir", "r"), Op("dir", "d"), Utf8File("d/f.md", "x"), Op("dirLink", "r/l", "d")],
        "r",
        "l/f.md"
    ),
    (
        "junction",
        [Op("dir", "r"), Op("dir", "d"), Utf8File("d/f.md", "x"), Op("junction", "r/j", "d")],
        "r",
        "j/f.md"
    ),
    (
        "root_through_junction",
        [Op("dir", "real"), Utf8File("real/a.md", "x"), Op("junction", "j", "real")],
        "j",
        "a.md"
    ),
    ("deny_read", [Op("dir", "r"), Utf8File("r/a.md", "x"), Op("denyRead", "r/a.md")], "r", "a.md"),
    ("through_file", [Op("dir", "r"), Utf8File("r/a.md", "x")], "r", "a.md/x"),
    // A handle without sharing: FileShare.None, an exclusive flock on Unix.
    ("locked", [Op("dir", "r"), Utf8File("r/a.md", "x"), Op("lock", "r/a.md")], "r", "a.md"),
];
if (!windows)
{
    readCases.Add(("fifo", [Op("dir", "r"), Op("fifo", "r/pipe")], "r", "pipe"));
    readCases.Add(
        (
            "folder_not_searchable",
            [Op("dir", "r"), Op("dir", "r/s"), Utf8File("r/s/a.md", "x"), Op("denyRead", "r/s")],
            "r",
            "s/a.md"
        )
    );
}

FileSourceReader reader = new(new FileOwnership());
JsonArray reads = [];
try
{
    foreach ((string name, JsonArray setup, string rootName, string typed) in readCases)
    {
        // A numbered folder: a case name such as "nul" is a device name on Windows.
        string folder = Path.Combine(scratch, "c" + reads.Count);
        Directory.CreateDirectory(folder);
        JsonObject row = new()
        {
            ["name"] = name,
            ["setup"] = setup,
            ["root"] = rootName,
            ["path"] = typed,
        };
        try
        {
            foreach (JsonNode? step in setup)
            {
                Apply(folder, step!.AsObject());
            }
            SourceSnapshot snapshot = reader.Read(Path.Combine(folder, rootName), typed);
            foreach (KeyValuePair<string, JsonNode?> field in Describe(snapshot))
            {
                row[field.Key] = field.Value?.DeepClone();
            }
        }
        catch (SermofurException exception)
        {
            row["error"] = new JsonObject
            {
                ["code"] = exception.Code,
                ["message"] = exception.Message,
                ["exitCode"] = exception.ExitCode,
            };
        }
        catch (Exception exception) when (exception is not InvalidOperationException)
        {
            row["exception"] = exception.GetType().FullName;
        }
        finally
        {
            held.ForEach(handle => handle.Dispose());
            held.Clear();
            undo.ForEach(action => action());
            undo.Clear();
        }
        reads.Add(row);
    }
}
finally
{
    // A junction just removed can stay "access denied" for a moment on Windows: retry.
    for (int attempt = 0; Directory.Exists(scratch); attempt++)
    {
        try
        {
            Directory.Delete(scratch, recursive: true);
        }
        catch (Exception exception)
            when (attempt < 10 && exception is IOException or UnauthorizedAccessException)
        {
            Thread.Sleep(300);
        }
    }
}

JsonObject result = new()
{
    ["generator"] = "rust/tools/source-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["os"] = windows ? "windows" : "unix",
    ["whitespace"] = whitespace,
    ["utf8"] = utf8,
    ["snapshots"] = snapshots,
    ["splits"] = splits,
    ["reads"] = reads,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ) + "\n",
    new UTF8Encoding(false)
);
Console.WriteLine(
    $"{utf8.Count} probes, {snapshots.Count} snapshots, {splits.Count} splits, {reads.Count} reads"
);
