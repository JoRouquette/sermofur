// Records the shared files of the .NET daemon (lot 7.2) on the real classes of src/Sermofur.Daemon:
// DaemonPaths.Resolve (internal, by reflection) on forged environments, its Hash, FileTurn and
// DaemonLock on crafted lock files, DaemonLog lines and rotation with an injected clock, and the
// InstanceRegistry: reads of forged files, the bytes its private Save writes, then a scenario of
// registrations on real instances made by InstanceManager.Initialize. The scratch folder is
// replaced by "<base>" (raw, and as the registry escapes it), the user ID by "<uid>" and the
// hash of the Windows SID by "<sid>". Usage: dotnet run daemon-files-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Daemon/Sermofur.Daemon.csproj
using System.Reflection;
using System.Security.Principal;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Domain;
using Sermofur.Infrastructure;

if (args.Length == 1 && args[0] == "--home-probe")
{
    string? value = Environment.GetEnvironmentVariable(DaemonPaths.HomeVariable);
    DaemonPaths current = DaemonPaths.ForCurrentUser();
    Console.Out.Write(
        new JsonObject
        {
            ["home_utf16"] = new JsonArray([.. value!.Select(unit => (JsonNode)(int)unit)]),
            ["config"] = current.ConfigDirectory,
            ["endpoint"] = current.Endpoint,
        }.ToJsonString()
    );
    return;
}

const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Static;
Assembly daemon = typeof(DaemonPaths).Assembly;
MethodInfo resolve = typeof(DaemonPaths).GetMethod("Resolve", Hidden)!;
MethodInfo hash = typeof(DaemonPaths).GetMethod("Hash", Hidden)!;
Type fileTurn = daemon.GetType("Sermofur.Daemon.FileTurn")!;
MethodInfo take = fileTurn.GetMethod("Take")!;
MethodInfo tryTake = fileTurn.GetMethod("TryTake")!;
MethodInfo createPrivateFolder = fileTurn.GetMethod("CreatePrivateFolder")!;
MethodInfo save = typeof(InstanceRegistry).GetMethod(
    "Save",
    BindingFlags.NonPublic | BindingFlags.Instance
)!;

string Hash(string text) => (string)hash.Invoke(null, [text])!;

string uid = OperatingSystem.IsWindows()
    ? ""
    : daemon
        .GetType("Sermofur.Daemon.UnixNative")!
        .GetMethod("EffectiveUserId")!
        .Invoke(null, null)!
        .ToString()!;
string sidHash = OperatingSystem.IsWindows() ? Hash(CurrentSid()) : "";

string Anonymous(string text)
{
    if (OperatingSystem.IsWindows())
    {
        return text.Replace("sermofur-" + sidHash, "sermofur-<sid>");
    }
    return text.Replace($"sermofur-{uid}", "sermofur-<uid>");
}

JsonObject Error(Exception exception)
{
    Exception inner = exception is TargetInvocationException { InnerException: { } wrapped }
        ? wrapped
        : exception;
    JsonObject error = new JsonObject { ["type"] = inner.GetType().Name };
    if (inner is SermofurException sermofur)
    {
        error["code"] = sermofur.Code;
        error["message"] = sermofur.Message;
        error["exit"] = sermofur.ExitCode;
    }
    return error;
}

// ---------------------------------------------------------------- DaemonPaths
JsonArray paths = [];

void Paths(string name, Dictionary<string, string?> variables, Dictionary<string, string> folders)
{
    JsonArray asked = [];
    Func<string, string?> variable = key =>
    {
        asked.Add("variable:" + key);
        return variables.GetValueOrDefault(key);
    };
    Func<Environment.SpecialFolder, string> folder = key =>
    {
        asked.Add("folder:" + key);
        return folders.GetValueOrDefault(key.ToString()) ?? "";
    };
    JsonObject entry = new JsonObject
    {
        ["name"] = name,
        ["variables"] = JsonSerializer.SerializeToNode(variables),
        ["folders"] = JsonSerializer.SerializeToNode(folders),
    };
    try
    {
        DaemonPaths resolved = (DaemonPaths)resolve.Invoke(null, [variable, folder])!;
        entry["result"] = new JsonObject
        {
            ["config"] = resolved.ConfigDirectory,
            ["state"] = resolved.StateDirectory,
            ["endpoint"] = Anonymous(resolved.Endpoint),
            ["registry"] = resolved.RegistryFile,
            ["log"] = resolved.LogFile,
            ["lock"] = resolved.LockFile,
            ["runtime"] = resolved.RuntimeDirectory is null
                ? null
                : Anonymous(resolved.RuntimeDirectory),
        };
    }
    catch (TargetInvocationException exception)
    {
        JsonObject error = Error(exception);
        if (error["message"] is JsonNode message)
        {
            error["message"] = Anonymous(message.GetValue<string>());
        }
        entry["error"] = error;
    }
    entry["asked"] = asked;
    paths.Add(entry);
}

Dictionary<string, string?> Home(string? home) => new() { ["SERMOFUR_DAEMON_HOME"] = home };
Dictionary<string, string> noFolders = [];

if (OperatingSystem.IsWindows())
{
    Dictionary<string, string> profile = new()
    {
        ["ApplicationData"] = @"C:\Roaming",
        ["LocalApplicationData"] = @"C:\Local",
    };
    Paths("home", Home(@"C:\smf-home"), noFolders);
    Paths("home_trailing_separator", Home(@"C:\smf-home\"), noFolders);
    Paths("home_forward_slashes", Home("C:/smf-home/x/"), noFolders);
    Paths("home_dots", Home(@"C:\smf-home\.\a\..\b"), noFolders);
    Paths("home_doubled_separators", Home(@"C:\\smf-home\\\b"), noFolders);
    Paths("home_trailing_dot_and_space", Home(@"C:\smf-home\b. "), noFolders);
    Paths("home_drive_root", Home(@"C:\"), noFolders);
    Paths("home_non_ascii", Home(@"C:\Données\é😀"), noFolders);
    Paths("home_unc", Home(@"\\server\share\h"), noFolders);
    Paths("home_extended", Home(@"\\?\C:\smf-home\.\x"), noFolders);
    Paths("home_long", Home(@"C:\" + new string('a', 300)), noFolders);
    Paths("home_case", Home(@"C:\SMF-HOME"), noFolders);
    Paths("home_whitespace", Home(" \t "), profile);
    Paths("home_empty", Home(""), profile);
    Paths("profile", [], profile);
    Paths("profile_empty", [], noFolders);
    Paths(
        "profile_drive_only",
        [],
        new() { ["ApplicationData"] = "C:", ["LocalApplicationData"] = "D:" }
    );
    Paths(
        "profile_trailing_separator",
        new() { ["TMPDIR"] = @"C:\t", ["XDG_RUNTIME_DIR"] = @"C:\r" },
        new() { ["ApplicationData"] = @"C:\Roaming\", ["LocalApplicationData"] = @"C:\Local\" }
    );
}
else
{
    Dictionary<string, string> home = new() { ["UserProfile"] = "/home/u" };
    Dictionary<string, string?> xdg = new()
    {
        ["XDG_CONFIG_HOME"] = "/x/config",
        ["XDG_STATE_HOME"] = "/x/state",
        ["XDG_RUNTIME_DIR"] = "/run/user/1000",
        ["TMPDIR"] = "/tmp/t",
    };
    Paths("home", Home("/tmp/smf-home"), noFolders);
    Paths("home_trailing_separator", Home("/tmp/smf-home/"), noFolders);
    Paths("home_dots", Home("//tmp//smf-home/./a/../b"), noFolders);
    Paths("home_root", Home("/"), noFolders);
    Paths("home_non_ascii", Home("/tmp/Données/é😀"), noFolders);
    Paths("home_backslash", Home(@"/tmp/a\b"), noFolders);
    Paths("home_whitespace", Home(" \t "), home);
    Paths("home_empty", Home(""), home);
    // run/daemon.sock adds 16 bytes: 87 bytes of home make 103, 88 make 104.
    Paths("home_socket_103_bytes", Home("/tmp/" + new string('a', 82)), noFolders);
    Paths("home_socket_104_bytes", Home("/tmp/" + new string('a', 83)), noFolders);
    Paths("home_socket_103_bytes_utf8", Home("/tmp/" + new string('a', 80) + "é"), noFolders);
    Paths("home_socket_104_bytes_utf8", Home("/tmp/" + new string('a', 81) + "é"), noFolders);
    Paths("home_too_long", Home("/tmp/" + new string('a', 100)), noFolders);
    Paths("xdg", xdg, home);
    Paths("defaults", [], home);
    Paths("defaults_without_profile", [], noFolders);
    Paths(
        "xdg_whitespace",
        new()
        {
            ["XDG_CONFIG_HOME"] = " ",
            ["XDG_STATE_HOME"] = "",
            ["XDG_RUNTIME_DIR"] = "\t",
            ["TMPDIR"] = "  ",
        },
        home
    );
    Paths(
        "xdg_relative",
        new()
        {
            ["XDG_CONFIG_HOME"] = "cfg",
            ["XDG_STATE_HOME"] = "st/",
            ["XDG_RUNTIME_DIR"] = "run/",
        },
        home
    );
    Paths("tmpdir", new() { ["TMPDIR"] = "/tmp/t/" }, home);
    Paths("tmpdir_relative", new() { ["TMPDIR"] = "rel" }, home);
    Paths("tmpdir_too_long", new() { ["TMPDIR"] = "/" + new string('t', 120) }, home);
    // sermofur/daemon.sock adds 21 bytes: 82 bytes of runtime folder make 103, 83 make 104.
    Paths("runtime_103_bytes", new() { ["XDG_RUNTIME_DIR"] = "/" + new string('r', 81) }, home);
    Paths("runtime_104_bytes", new() { ["XDG_RUNTIME_DIR"] = "/" + new string('r', 82) }, home);
    Paths("runtime_trailing_separator", new() { ["XDG_RUNTIME_DIR"] = "/run/user/1000/" }, home);
    Paths("profile_trailing_separator", [], new() { ["UserProfile"] = "/home/u/" });
    Paths("runtime_doubled_separators", new() { ["XDG_RUNTIME_DIR"] = "/run//user/1000" }, home);
}

JsonNode? nonUnicodeHome = null;
if (!OperatingSystem.IsWindows())
{
    // A home that is not UTF-8: the bytes /tmp/smf-a<FF>b, set by the shell.
    System.Diagnostics.ProcessStartInfo start = new System.Diagnostics.ProcessStartInfo(
        "/bin/bash",
        [
            "-c",
            "SERMOFUR_DAEMON_HOME=\"$(printf '/tmp/smf-a\\377b')\" exec \"$0\" --home-probe",
            Environment.ProcessPath!,
        ]
    )
    {
        RedirectStandardOutput = true,
        UseShellExecute = false,
    };
    using System.Diagnostics.Process child = System.Diagnostics.Process.Start(start)!;
    nonUnicodeHome = JsonNode.Parse(child.StandardOutput.ReadToEnd());
    child.WaitForExit();
}

JsonArray hashes = [];
foreach (
    string text in new[]
    {
        "",
        "a",
        @"C:\smf-home",
        @"C:\smf-home\",
        "S-1-5-21-1004336348-1177238915-682003330-512",
        "Données é😀",
        "/tmp/smf-home",
    }
)
{
    hashes.Add(new JsonObject { ["text"] = text, ["hash"] = Hash(text) });
}

// ---------------------------------------------------------------- scratch folder
string temporary = LongPath.Of(Path.GetTempPath());
string scratch = Path.Combine(
    temporary,
    "sermofur-daemon-files-oracle-" + Guid.NewGuid().ToString("N")
);
Directory.CreateDirectory(scratch);
string escapedScratch = JsonEncodedText.Encode(scratch).ToString();

string Scrub(string text) => text.Replace(escapedScratch, "<base>").Replace(scratch, "<base>");

string NewFolder(string name)
{
    string folder = Path.Combine(scratch, name);
    Directory.CreateDirectory(folder);
    return folder;
}

// ---------------------------------------------------------------- FileTurn and DaemonLock
JsonArray locks = [];

void LockCase(string name, Func<string, JsonNode?> run)
{
    string folder = NewFolder("lock-" + name);
    JsonObject entry = new JsonObject { ["name"] = name };
    try
    {
        entry["result"] = run(folder);
    }
    catch (Exception exception)
    {
        entry["error"] = Error(exception);
    }
    locks.Add(entry);
}

FileStream Take(string file) => (FileStream)take.Invoke(null, [file])!;

LockCase("is_held_absent", folder => DaemonLock.IsHeld(Path.Combine(folder, "daemon.lock")));
LockCase(
    "is_held_free",
    folder =>
    {
        string file = Path.Combine(folder, "daemon.lock");
        File.WriteAllBytes(file, []);
        return DaemonLock.IsHeld(file);
    }
);
LockCase(
    "is_held_taken",
    folder =>
    {
        string file = Path.Combine(folder, "daemon.lock");
        using FileStream held = Take(file);
        return DaemonLock.IsHeld(file);
    }
);
LockCase(
    "is_held_directory",
    folder =>
    {
        string file = Path.Combine(folder, "daemon.lock");
        Directory.CreateDirectory(file);
        return DaemonLock.IsHeld(file);
    }
);
LockCase(
    "take_twice",
    folder =>
    {
        string file = Path.Combine(folder, "x.lock");
        using FileStream first = Take(file);
        using FileStream second = Take(file);
        return "taken twice";
    }
);
LockCase(
    "take_directory",
    folder =>
    {
        string file = Path.Combine(folder, "x.lock");
        Directory.CreateDirectory(file);
        using FileStream turn = Take(file);
        return "taken";
    }
);
LockCase(
    "take_missing_folder",
    folder =>
    {
        using FileStream turn = Take(Path.Combine(folder, "missing", "x.lock"));
        return "taken";
    }
);
LockCase(
    "take_keeps_content",
    folder =>
    {
        string file = Path.Combine(folder, "x.lock");
        File.WriteAllText(file, "kept");
        using (Take(file)) { }
        return File.ReadAllText(file);
    }
);
LockCase(
    "try_take_creates_folders",
    folder =>
    {
        string file = Path.Combine(folder, "a", "b", "x.lock");
        using FileStream? turn = (FileStream?)
            tryTake.Invoke(
                null,
                [file, TimeSpan.FromMilliseconds(50), TimeSpan.FromMilliseconds(10)]
            );
        return turn is not null && File.Exists(file);
    }
);
LockCase(
    "try_take_held",
    folder =>
    {
        string file = Path.Combine(folder, "x.lock");
        using FileStream held = Take(file);
        using FileStream? turn = (FileStream?)
            tryTake.Invoke(
                null,
                [file, TimeSpan.FromMilliseconds(50), TimeSpan.FromMilliseconds(10)]
            );
        return turn is null ? "null" : "taken";
    }
);
LockCase(
    "try_take_directory",
    folder =>
    {
        string file = Path.Combine(folder, "x.lock");
        Directory.CreateDirectory(file);
        using FileStream? turn = (FileStream?)
            tryTake.Invoke(
                null,
                [file, TimeSpan.FromMilliseconds(50), TimeSpan.FromMilliseconds(10)]
            );
        return turn is null ? "null" : "taken";
    }
);
LockCase(
    "try_take_folder_is_a_file",
    folder =>
    {
        File.WriteAllText(Path.Combine(folder, "f"), "");
        using FileStream? turn = (FileStream?)
            tryTake.Invoke(
                null,
                [
                    Path.Combine(folder, "f", "x.lock"),
                    TimeSpan.FromMilliseconds(50),
                    TimeSpan.FromMilliseconds(10),
                ]
            );
        return turn is null ? "null" : "taken";
    }
);
LockCase(
    "acquire",
    folder =>
    {
        DaemonPaths home = (DaemonPaths)
            resolve.Invoke(
                null,
                [
                    (Func<string, string?>)(key => key == DaemonPaths.HomeVariable ? folder : null),
                    (Func<Environment.SpecialFolder, string>)(_ => ""),
                ]
            )!;
        using FileStream held = DaemonLock.Acquire(home);
        JsonObject state = new JsonObject
        {
            ["held"] = DaemonLock.IsHeld(home.LockFile),
            ["state_folder"] = Directory.Exists(home.StateDirectory),
        };
        try
        {
            using FileStream second = DaemonLock.Acquire(home);
        }
        catch (SermofurException exception)
        {
            state["second"] = Error(exception);
        }
        return state;
    }
);
LockCase(
    "acquire_file",
    folder =>
    {
        File.WriteAllText(Path.Combine(folder, "state"), "");
        DaemonPaths home = (DaemonPaths)
            resolve.Invoke(
                null,
                [
                    (Func<string, string?>)(key => key == DaemonPaths.HomeVariable ? folder : null),
                    (Func<Environment.SpecialFolder, string>)(_ => ""),
                ]
            )!;
        using FileStream held = DaemonLock.Acquire(home);
        return "taken";
    }
);
if (!OperatingSystem.IsWindows())
{
    string Mode(string path) => Convert.ToString((int)File.GetUnixFileMode(path), 8);
    LockCase(
        "unix_modes",
        folder =>
        {
            string nested = Path.Combine(folder, "a", "b", "c");
            createPrivateFolder.Invoke(null, [nested]);
            string existing = Path.Combine(folder, "open");
            Directory.CreateDirectory(existing);
            File.SetUnixFileMode(existing, (UnixFileMode)Convert.ToInt32("755", 8));
            createPrivateFolder.Invoke(null, [existing]);
            string fresh = Path.Combine(nested, "x.lock");
            using (Take(fresh)) { }
            string wide = Path.Combine(folder, "wide.lock");
            File.WriteAllBytes(wide, []);
            File.SetUnixFileMode(wide, (UnixFileMode)Convert.ToInt32("4644", 8));
            using (Take(wide)) { }
            return new JsonObject
            {
                ["a"] = Mode(Path.Combine(folder, "a")),
                ["a/b"] = Mode(Path.Combine(folder, "a", "b")),
                ["a/b/c"] = Mode(nested),
                ["open"] = Mode(existing),
                ["new_lock"] = Mode(fresh),
                ["wide_lock"] = Mode(wide),
            };
        }
    );
}

// ---------------------------------------------------------------- DaemonLog
JsonArray logs = [];

void LogCase(
    string name,
    long maxBytes,
    DateTimeOffset[] clock,
    Action<DaemonLog> write,
    Action<string>? prepare = null
)
{
    string folder = Path.Combine(scratch, "log-" + name);
    string file = Path.Combine(folder, "state", "daemon.log");
    prepare?.Invoke(file);
    int tick = 0;
    DaemonLog log = new DaemonLog(file, () => clock[tick++ % clock.Length], maxBytes);
    write(log);
    bool flushed = log.Flush(TimeSpan.FromSeconds(30));
    JsonObject files = [];
    foreach (string part in new[] { file, file + ".1", file + ".2", file + ".3" })
    {
        if (File.Exists(part))
        {
            files[Path.GetFileName(part)] = File.ReadAllText(part);
        }
    }
    logs.Add(
        new JsonObject
        {
            ["name"] = name,
            ["max_bytes"] = maxBytes,
            ["flushed"] = flushed,
            ["lock_file"] = File.Exists(file + ".lock"),
            ["files"] = files,
        }
    );
}

DateTimeOffset At(string text) =>
    DateTimeOffset.Parse(text, System.Globalization.CultureInfo.InvariantCulture);

DateTimeOffset[] clocks =
[
    At("2026-11-09T10:11:12.3456789+01:00"),
    At("2026-11-09T10:11:12.9999999-05:30"),
    At("0001-01-01T00:00:00+00:00"),
    At("9999-12-31T23:59:59.9999999+00:00"),
    At("2026-03-29T01:59:59.0010000+14:00"),
    At("2026-10-25T02:30:00.0500000-14:00"),
];
LogCase(
    "fields",
    DaemonLog.MaxFileBytes,
    clocks,
    log =>
    {
        log.Write("started");
        log.Write("command", "ok", 4242, 17);
        log.Write("command", null, null, 0);
        log.Write("refused", "protocol_error", -1, long.MaxValue);
        log.Write("pid_only", null, int.MinValue);
        log.Write("duration_only", durationMs: long.MinValue);
    }
);
LogCase(
    "escaping",
    DaemonLog.MaxFileBytes,
    clocks,
    log =>
    {
        foreach (
            string text in new[]
            {
                "",
                "quote\" backslash\\ slash/",
                "<tag> & 'apostrophe' + plus `tick`",
                "control\u0001\b\f\n\r\t\u001F\u007F\u0080\u009F",
                "Données é ü ß ñ 中文 русский",
                "line\u2028separator\u2029paragraph",
                "emoji 😀 and \uFFFD replacement \uFEFF bom",
                "lone \uD800 high",
                "lone \uDC00 low",
                "soft\u00ADhyphen \u200Bzero width \u0300combining",
            }
        )
        {
            log.Write(text, text);
        }
    }
);
LogCase(
    "rotation",
    300,
    clocks,
    log =>
    {
        for (int line = 0; line < 25; line++)
        {
            log.Write($"line-{line:00}", "code", line);
        }
    }
);
LogCase(
    "rotation_existing_large_file",
    100,
    clocks,
    log => log.Write("after"),
    file =>
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        File.WriteAllText(file, new string('x', 150) + "\n");
        File.WriteAllText(file + ".1", "one\n");
        File.WriteAllText(file + ".2", "two\n");
        File.WriteAllText(file + ".3", "three\n");
    }
);
LogCase(
    "existing_small_file",
    1000,
    clocks,
    log => log.Write("appended"),
    file =>
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        File.WriteAllText(file, "previous line without end");
    }
);
LogCase(
    "zero_limit",
    0,
    clocks,
    log =>
    {
        log.Write("first");
        log.Write("second");
        log.Write("third");
    }
);
LogCase(
    "journal_is_a_directory",
    1000,
    clocks,
    log => log.Write("lost"),
    file => Directory.CreateDirectory(file)
);

// ---------------------------------------------------------------- InstanceRegistry: reads
JsonArray reads = [];

JsonNode Entries(IReadOnlyList<RegisteredInstance> entries) =>
    new JsonArray([
        .. entries.Select(entry => new JsonObject
        {
            ["root"] = entry.Root,
            ["registeredAt"] = entry.RegisteredAt,
        }),
    ]);

void Read(string name, byte[]? content, Action<string>? prepare = null)
{
    string folder = NewFolder("read-" + name);
    string file = Path.Combine(folder, "config", "instances.json");
    if (content is not null)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(file)!);
        File.WriteAllBytes(file, content);
    }
    prepare?.Invoke(file);
    JsonObject entry = new JsonObject
    {
        ["name"] = name,
        ["content"] = content is null ? null : Convert.ToBase64String(content),
    };
    try
    {
        entry["entries"] = Entries(new InstanceRegistry(file).Read());
    }
    catch (Exception exception)
    {
        JsonObject error = Error(exception);
        if (error["message"] is JsonNode message)
        {
            error["message"] = Scrub(message.GetValue<string>());
        }
        entry["error"] = error;
    }
    reads.Add(entry);
}

void ReadText(string name, string text) => Read(name, Encoding.UTF8.GetBytes(text));

string absolute = OperatingSystem.IsWindows() ? @"C:\\a" : "/a";
string Doc(string instances) => $"{{\"version\":1,\"instances\":[{instances}]}}";
string One(string root) => Doc($"{{\"root\":\"{root}\",\"registeredAt\":\"x\"}}");

Read("absent", null);
Read("folder_missing", null, file => { });
ReadText("empty_list", Doc(""));
ReadText("one", One(absolute));
ReadText("not_json", "not json");
ReadText("empty_file", "");
ReadText("whitespace_file", " \r\n\t");
ReadText("version_2", "{\"version\":2,\"instances\":[]}");
ReadText("version_0", "{\"version\":0,\"instances\":[]}");
ReadText("version_missing", "{\"instances\":[]}");
ReadText("instances_missing", "{\"version\":1}");
ReadText("instances_null", "{\"version\":1,\"instances\":null}");
ReadText("instances_object", "{\"version\":1,\"instances\":{}}");
ReadText("version_string", "{\"version\":\"1\",\"instances\":[]}");
ReadText("version_float", "{\"version\":1.0,\"instances\":[]}");
ReadText("version_exponent", "{\"version\":1e0,\"instances\":[]}");
ReadText("version_negative_zero", "{\"version\":-0,\"instances\":[]}");
ReadText("version_null", "{\"version\":null,\"instances\":[]}");
ReadText("version_overflow", "{\"version\":4294967297,\"instances\":[]}");
ReadText("version_true", "{\"version\":true,\"instances\":[]}");
ReadText("version_leading_zero", "{\"version\":01,\"instances\":[]}");
ReadText("null_document", "null");
ReadText("array_document", "[]");
ReadText("string_document", "\"x\"");
ReadText("names_other_case", "{\"Version\":1,\"Instances\":[]}");
ReadText("duplicate_version_last_wins", "{\"version\":2,\"version\":1,\"instances\":[]}");
ReadText("duplicate_version_bad_first", "{\"version\":\"x\",\"version\":1,\"instances\":[]}");
ReadText("duplicate_instances", $"{{\"version\":1,\"instances\":null,\"instances\":[]}}");
ReadText(
    "unknown_property",
    "{\"version\":1,\"extra\":{\"deep\":[1,2,{\"a\":null}]},\"instances\":[]}"
);
ReadText("unknown_lone_surrogate", "{\"version\":1,\"extra\":\"\\uD800\",\"instances\":[]}");
ReadText("trailing_comma", "{\"version\":1,\"instances\":[],}");
ReadText("comment", "{\"version\":1,/* c */\"instances\":[]}");
ReadText("trailing_content", Doc("") + " x");
ReadText("two_documents", Doc("") + Doc(""));
ReadText("indented_crlf", "{\r\n  \"version\": 1,\r\n  \"instances\": []\r\n}\r\n");
Read("bom", [0xEF, 0xBB, 0xBF, .. Encoding.UTF8.GetBytes(Doc(""))]);
Read("utf16_bom", [0xFF, 0xFE, .. Encoding.Unicode.GetBytes(Doc(""))]);
Read(
    "invalid_utf8_in_root",
    [
        .. Encoding.UTF8.GetBytes("{\"version\":1,\"instances\":[{\"root\":\"/a"),
        0xFF,
        .. Encoding.UTF8.GetBytes("\",\"registeredAt\":\"x\"}]}"),
    ]
);
Read(
    "invalid_utf8_ignored",
    [
        .. Encoding.UTF8.GetBytes("{\"version\":1,\"x\":\""),
        0xFF,
        .. Encoding.UTF8.GetBytes("\",\"instances\":[]}"),
    ]
);
Read(
    "invalid_utf8_after_backslash_ignored",
    [
        .. Encoding.UTF8.GetBytes("{\"version\":1,\"x\":\"\\"),
        0xFF,
        .. Encoding.UTF8.GetBytes("uDFFF\",\"instances\":[]}"),
    ]
);
ReadText("root_relative", One("relative"));
ReadText("root_empty", One(""));
ReadText("root_null", Doc("{\"root\":null,\"registeredAt\":\"x\"}"));
ReadText("root_missing", Doc("{\"registeredAt\":\"x\"}"));
ReadText("root_number", Doc("{\"root\":5,\"registeredAt\":\"x\"}"));
ReadText("root_lone_surrogate", One(absolute + "\\uD800"));
ReadText("root_escapes", One(absolute + "\\u00e9\\/\\\"\\t"));
ReadText("root_case_insensitive_name", Doc($"{{\"Root\":\"{absolute}\",\"registeredAt\":\"x\"}}"));
ReadText("registered_at_missing", Doc($"{{\"root\":\"{absolute}\"}}"));
ReadText("registered_at_null", Doc($"{{\"root\":\"{absolute}\",\"registeredAt\":null}}"));
ReadText("registered_at_number", Doc($"{{\"root\":\"{absolute}\",\"registeredAt\":1}}"));
ReadText(
    "duplicate_root_last_wins",
    Doc($"{{\"root\":\"relative\",\"root\":\"{absolute}\",\"registeredAt\":\"x\"}}")
);
ReadText(
    "duplicate_root_bad_last",
    Doc($"{{\"root\":\"{absolute}\",\"root\":\"relative\",\"registeredAt\":\"x\"}}")
);
ReadText(
    "entry_extra_property",
    Doc($"{{\"root\":\"{absolute}\",\"registeredAt\":\"x\",\"more\":[true]}}")
);
ReadText("entry_null", Doc("null"));
ReadText("entry_array", Doc("[]"));
ReadText("entry_empty_object", Doc("{}"));
ReadText("root_unc", One(@"\\\\server\\share"));
ReadText("root_drive_relative", One("C:a"));
ReadText("root_rooted_not_qualified", One("\\\\a"));
ReadText("root_forward_slashes", One("C:/a"));
ReadText("root_double_forward_slash", One("//a"));
ReadText("root_device", One(@"\\\\?\\C:\\a"));
ReadText("root_slash", One("/a"));
ReadText("root_drive_only", One("C:"));
ReadText("root_whitespace", One(" "));
ReadText(
    "depth_64",
    "{\"version\":1,\"x\":" + new string('[', 63) + new string(']', 63) + ",\"instances\":[]}"
);
ReadText(
    "depth_65",
    "{\"version\":1,\"x\":" + new string('[', 64) + new string(']', 64) + ",\"instances\":[]}"
);
ReadText(
    "entries_256",
    Doc(
        string.Join(
            ",",
            Enumerable.Repeat($"{{\"root\":\"{absolute}\",\"registeredAt\":\"x\"}}", 256)
        )
    )
);
ReadText(
    "entries_257",
    Doc(
        string.Join(
            ",",
            Enumerable.Repeat($"{{\"root\":\"{absolute}\",\"registeredAt\":\"x\"}}", 257)
        )
    )
);
ReadText("max_bytes", Doc("").PadRight(InstanceRegistry.MaxBytes));
ReadText("max_bytes_plus_one", Doc("").PadRight(InstanceRegistry.MaxBytes + 1));
Read("path_is_a_directory", null, file => Directory.CreateDirectory(file));
Read(
    "parent_is_a_file",
    null,
    file =>
    {
        Directory.CreateDirectory(Path.GetDirectoryName(Path.GetDirectoryName(file))!);
        File.WriteAllText(Path.GetDirectoryName(file)!, "");
    }
);

// ---------------------------------------------------------------- InstanceRegistry: writes
JsonArray saves = [];

void Save(string name, List<RegisteredInstance> entries)
{
    string file = Path.Combine(NewFolder("save-" + name), "config", "instances.json");
    save.Invoke(new InstanceRegistry(file), [entries]);
    saves.Add(
        new JsonObject
        {
            ["name"] = name,
            ["entries"] = Entries(entries),
            ["bytes"] = Convert.ToBase64String(File.ReadAllBytes(file)),
        }
    );
}

Save("empty", []);
Save("one", [new RegisteredInstance(@"C:\a", "2026-11-09T10:11:12+01:00")]);
Save(
    "two",
    [
        new RegisteredInstance("/a/b", "2026-11-09T10:11:12-05:30"),
        new RegisteredInstance("/c", "0001-01-01T00:00:00+00:00"),
    ]
);
Save("registered_at_null", [new RegisteredInstance("/a", null!)]);
Save("root_null", [new RegisteredInstance(null!, "x")]);
Save(
    "escaping",
    [
        new RegisteredInstance("quote\" backslash\\ slash/ <tag> & 'a' + `t`", "+01:00"),
        new RegisteredInstance("control\u0001\b\f\n\r\t\u001F\u007F\u0080\u009F", ""),
        new RegisteredInstance("Données é ü ß ñ 中文 русский", "é"),
        new RegisteredInstance("line\u2028separator\u2029paragraph \uFEFF", "x"),
        new RegisteredInstance("emoji 😀 \uFFFD", "x"),
        new RegisteredInstance("lone \uD800 high", "lone \uDC00 low"),
        new RegisteredInstance("soft\u00ADhyphen \u200B \u0300", "x"),
    ]
);

// ---------------------------------------------------------------- InstanceRegistry: scenario
JsonArray steps = [];
string registryHome = NewFolder("home");
string registryFile = Path.Combine(registryHome, "config", "instances.json");
InstanceRegistry registry = new InstanceRegistry(registryFile);
InstanceManager manager = new InstanceManager();

string Instance(string name)
{
    string root = Path.Combine(scratch, name);
    Directory.CreateDirectory(root);
    manager.Initialize(root);
    return root;
}

void Step(string operation, string? path, Func<JsonNode?> run)
{
    JsonObject entry = new JsonObject
    {
        ["op"] = operation,
        ["path"] = path is null ? null : Scrub(path),
    };
    try
    {
        JsonNode? result = run();
        entry["result"] = result is null ? null : JsonNode.Parse(Scrub(result.ToJsonString()));
    }
    catch (Exception exception)
    {
        JsonObject error = Error(exception);
        if (error["message"] is JsonNode message)
        {
            error["message"] = Scrub(message.GetValue<string>());
        }
        entry["error"] = error;
    }
    entry["file"] = File.Exists(registryFile) ? Scrub(File.ReadAllText(registryFile)) : null;
    steps.Add(entry);
}

JsonNode Registered(RegisteredInstance entry) =>
    new JsonObject { ["root"] = entry.Root, ["registeredAt"] = entry.RegisteredAt };

JsonNode Listed(IReadOnlyList<RegistryEntry> entries) =>
    new JsonArray([
        .. entries.Select(entry => new JsonObject
        {
            ["root"] = entry.Root,
            ["registeredAt"] = entry.RegisteredAt,
            ["missing"] = entry.Missing,
        }),
    ]);

string a = Instance("a");
string b = Instance("b");
string unicode = Instance("Données é+");
string moved = Instance("moved");
Directory.CreateDirectory(Path.Combine(a, "deep", "er"));
string legacy = Path.Combine(scratch, "legacy");
Directory.CreateDirectory(Path.Combine(legacy, InstanceManager.Marker));
File.Copy(
    Path.Combine(
        Path.GetDirectoryName(
            Path.GetDirectoryName(Path.GetDirectoryName(Environment.CurrentDirectory)!)!
        )!,
        "tests",
        "Sermofur.Tests",
        "Fixtures",
        "instance-v1",
        "marker",
        "instance.json"
    ),
    Path.Combine(legacy, InstanceManager.Marker, InstanceManager.ConfigurationFile)
);
string none = NewFolder("none");

Step("list", null, () => Listed(registry.List()));
Step("register", none, () => Registered(registry.Register(none, At("2026-11-09T10:11:12+01:00"))));
Step(
    "register",
    legacy,
    () => Registered(registry.Register(legacy, At("2026-11-09T10:11:12+01:00")))
);
Step(
    "register",
    Path.Combine(scratch, "absent"),
    () =>
        Registered(
            registry.Register(Path.Combine(scratch, "absent"), At("2026-11-09T10:11:12+01:00"))
        )
);
InstanceRegistry foreign = new InstanceRegistry(
    registryFile,
    new InstanceManager(new ForeignOwnership())
);
Step(
    "register_foreign_owner",
    a,
    () => Registered(foreign.Register(a, At("2026-11-09T10:11:12+01:00")))
);
Step("register", a, () => Registered(registry.Register(a, At("2026-11-09T10:11:12.999+01:00"))));
Step(
    "register",
    Path.Combine(a, "deep", "er"),
    () =>
        Registered(
            registry.Register(Path.Combine(a, "deep", "er"), At("2027-01-01T00:00:00+00:00"))
        )
);
Step(
    "register",
    a + Path.DirectorySeparatorChar,
    () =>
        Registered(
            registry.Register(a + Path.DirectorySeparatorChar, At("2027-01-01T00:00:00+00:00"))
        )
);
if (OperatingSystem.IsWindows())
{
    string upper = Path.Combine(scratch, "A");
    Step(
        "register",
        upper,
        () => Registered(registry.Register(upper, At("2027-01-01T00:00:00+00:00")))
    );
}
Step("register", b, () => Registered(registry.Register(b, At("2026-11-09T10:11:12-05:30"))));
Step(
    "register",
    unicode,
    () => Registered(registry.Register(unicode, At("0001-01-01T14:00:00+14:00")))
);
Step(
    "register",
    moved,
    () => Registered(registry.Register(moved, At("9999-12-31T09:59:59-14:00")))
);
Step("list", null, () => Listed(registry.List()));
Directory.Delete(moved, true);
Step("list", null, () => Listed(registry.List()));
Step(
    "unregister",
    Path.Combine(a, "deep"),
    () => Registered(registry.Unregister(Path.Combine(a, "deep")))
);
Step(
    "unregister",
    Path.Combine(a, "deep"),
    () => Registered(registry.Unregister(Path.Combine(a, "deep")))
);
Step(
    "unregister",
    moved + Path.DirectorySeparatorChar,
    () => Registered(registry.Unregister(moved + Path.DirectorySeparatorChar))
);
Step("unregister", moved, () => Registered(registry.Unregister(moved)));
Step("unregister", none, () => Registered(registry.Unregister(none)));
if (OperatingSystem.IsWindows())
{
    string nt = @"\??\" + a;
    Step("unregister", nt, () => Registered(registry.Unregister(nt)));
}
if (OperatingSystem.IsWindows())
{
    string upper = Path.Combine(scratch, "B");
    Step("unregister", upper, () => Registered(registry.Unregister(upper)));
}
else
{
    Step("unregister", b, () => Registered(registry.Unregister(b)));
}
Step("list", null, () => Listed(registry.List()));
Step("read", null, () => Entries(registry.Read()));
Step("unregister", unicode, () => Registered(registry.Unregister(unicode)));

// A registry full of 256 entries refuses the 257th.
File.WriteAllText(
    registryFile,
    Doc(
        string.Join(
            ",",
            Enumerable
                .Range(0, 256)
                .Select(index =>
                    $"{{\"root\":\"{JsonEncodedText.Encode(Path.Combine(scratch, $"x{index}"))}\",\"registeredAt\":\"x\"}}"
                )
        )
    )
);
Step("register", a, () => Registered(registry.Register(a, At("2026-11-09T10:11:12+01:00"))));
File.WriteAllText(registryFile, "not json");
Step("register", a, () => Registered(registry.Register(a, At("2026-11-09T10:11:12+01:00"))));
Step("unregister", a, () => Registered(registry.Unregister(a)));
Step("list", null, () => Listed(registry.List()));
File.Delete(registryFile);
using (Take(registry.LockFile))
{
    InstanceRegistry hurried = new InstanceRegistry(
        registryFile,
        lockTimeout: TimeSpan.FromMilliseconds(300)
    );
    Step(
        "register_while_locked",
        a,
        () => Registered(hurried.Register(a, At("2026-11-09T10:11:12+01:00")))
    );
    Step("unregister_while_locked", a, () => Registered(hurried.Unregister(a)));
    InstanceRegistry slower = new InstanceRegistry(
        registryFile,
        lockTimeout: TimeSpan.FromMilliseconds(1600)
    );
    Step(
        "register_while_locked_1600ms",
        a,
        () => Registered(slower.Register(a, At("2026-11-09T10:11:12+01:00")))
    );
}
Step("register", a, () => Registered(registry.Register(a, At("2026-11-09T10:11:12+01:00"))));

JsonObject output = new JsonObject
{
    ["os"] = OperatingSystem.IsWindows() ? "windows" : "unix",
    ["paths"] = paths,
    ["hashes"] = hashes,
    ["non_unicode_home"] = nonUnicodeHome,
    ["locks"] = locks,
    ["logs"] = logs,
    ["registry_reads"] = reads,
    ["registry_saves"] = saves,
    ["registry_steps"] = steps,
};

Microsoft.Data.Sqlite.SqliteConnection.ClearAllPools();
Directory.Delete(scratch, recursive: true);
File.WriteAllText(
    args[0],
    output
        .ToJsonString(
            new JsonSerializerOptions
            {
                WriteIndented = true,
                Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
            }
        )
        .Replace("\r\n", "\n") + "\n"
);

static string CurrentSid()
{
    if (!OperatingSystem.IsWindows())
    {
        return "";
    }
    using WindowsIdentity identity = WindowsIdentity.GetCurrent();
    return identity.User!.Value;
}

sealed class ForeignOwnership : IFileOwnership
{
    public EntryStatus? Inspect(string path) =>
        Directory.Exists(path) ? new EntryStatus(false, true, false)
        : File.Exists(path) ? new EntryStatus(true, false, false)
        : null;
}
