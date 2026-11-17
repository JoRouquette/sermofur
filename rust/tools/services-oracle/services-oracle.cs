// Records the texts of the .NET service managers (lot 7.7) on the real classes of
// src/Sermofur.Daemon/Services: the systemd unit, the launchd plist and the XML of the scheduled
// task for crafted definitions (or the code of their refusal), WindowsScheduledTask.QuoteArgument
// (internal, by reflection), ServiceManagers.Same, ServiceDefinition.CarriedEnvironment with its
// dropped and narrowed names, the bytes InstalledDefinition.Write puts in service.json, and what
// InstalledDefinition.Read makes of 23 crafted service.json files. Nothing is installed and no
// process is started. Usage: dotnet run services-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Daemon/Sermofur.Daemon.csproj
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Daemon.Services;
using Sermofur.Domain;

string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-services-oracle-" + Guid.NewGuid().ToString("N")
);
DaemonPaths paths = new DaemonPaths(
    scratch,
    Path.Combine(scratch, "state"),
    Path.Combine(scratch, "s")
);

ServiceDefinition Def(
    string executable,
    string[] arguments,
    string version,
    params string[][] environment
)
{
    Dictionary<string, string> variables = [];
    foreach (string[] pair in environment)
    {
        variables[pair[0]] = pair[1];
    }
    return new ServiceDefinition(executable, arguments, version, variables);
}

JsonArray Pairs(IEnumerable<KeyValuePair<string, string>> pairs) =>
    [.. pairs.Select(pair => (JsonNode)new JsonArray(pair.Key, pair.Value))];

JsonObject Describe(ServiceDefinition definition) =>
    new JsonObject
    {
        ["executable"] = definition.Executable,
        ["arguments"] = new JsonArray([.. definition.Arguments.Select(a => (JsonNode?)a)]),
        ["version"] = definition.Version,
        ["environment"] = Pairs(definition.Environment),
    };

string? Code(Action action)
{
    try
    {
        action();
        return null;
    }
    catch (SermofurException exception)
    {
        return exception.Code;
    }
}

ServiceDefinition[] definitions =
[
    Def(
        "/opt/tools/smf",
        ["daemon", "run", "a b", "100%", "q\"uote"],
        "0.4.0",
        ["PATH", "/usr/bin:/bin"],
        ["DOTNET_ROOT", "/opt/dotnet & co"]
    ),
    Def("/home/a$b/smf", ["daemon", "run"], "0.4.0", ["DOTNET_ROOT", "/opt/$dotnet"]),
    Def("/home/a$b/smf", ["daemon", "run"], "0.4.0", ["PATH", "/bin\nExecStartPre=/x"]),
    Def(@"C:\Users\Jérôme\.dotnet\tools\smf.exe", ["daemon", "run", "--supervise"], "0.4.0"),
    Def(
        @"C:\a b\smf.exe",
        ["daemon", "run", "", "x\\\"y", "tab\there", "<&>'", "end\\"],
        "1.0.0-rc.1+a<b>",
        ["SERMOFUR_DAEMON_HOME", "/h'o\"me"],
        ["Z", "é\u0001%"],
        ["A", "1"]
    ),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["PATH", "/a\0b"]),
    Def("/opt/s\rmf", ["daemon", "run"], "0.4.0"),
    Def("/opt/smf", ["daemon", "run"], "0.4.0"),
];
string[] users = [@"PC\jérôme", @"PC\a&b<c>"];
JsonArray described = [];
int index = 0;
foreach (ServiceDefinition definition in definitions)
{
    string? unit = null;
    string? unitError = Code(() => unit = SystemdUserService.Unit(definition));
    InstalledDefinition.Write(paths, definition);
    JsonObject entry = Describe(definition);
    entry["unit"] = unit;
    entry["unitError"] = unitError;
    entry["plist"] = LaunchAgentService.Plist(definition, "/logs");
    entry["taskXml"] = WindowsScheduledTask.TaskXml(definition, users[index++ % users.Length]);
    entry["serviceJson"] = Convert.ToBase64String(
        File.ReadAllBytes(InstalledDefinition.File(paths))
    );
    described.Add(entry);
}
InstalledDefinition.Delete(paths);

MethodInfo quoteArgument = typeof(WindowsScheduledTask).GetMethod(
    "QuoteArgument",
    BindingFlags.NonPublic | BindingFlags.Static
)!;
string[] quoted =
[
    "",
    "a",
    "a b",
    @"C:\a b\",
    "q\"uote",
    "a\\\\\"b",
    @"\\",
    "tab\there",
    @"x\",
    "\"",
    "a\\b c",
    "--supervise",
];
JsonArray quotes =
[
    .. quoted.Select(text =>
        (JsonNode)new JsonArray(text, (string)quoteArgument.Invoke(null, [text])!)
    ),
];

ServiceDefinition baseline = Def(
    "/opt/smf",
    ["daemon", "run"],
    "0.4.0",
    ["PATH", "/a"],
    ["DOTNET_ROOT", "/d"]
);
ServiceDefinition?[] others =
[
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["PATH", "/b"], ["DOTNET_ROOT", "/d"]),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["PATH", "/a"], ["DOTNET_ROOT", "/e"]),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["DOTNET_ROOT", "/d"]),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["Path", "/z"], ["DOTNET_ROOT", "/d"]),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["DOTNET_ROOT", "/d"], ["X", "1"]),
    Def("/opt/smf", ["daemon", "run"], "0.4.1", ["PATH", "/a"], ["DOTNET_ROOT", "/d"]),
    Def("/opt/smf2", ["daemon", "run"], "0.4.0", ["PATH", "/a"], ["DOTNET_ROOT", "/d"]),
    Def(
        "/opt/smf",
        ["daemon", "run", "--supervise"],
        "0.4.0",
        ["PATH", "/a"],
        ["DOTNET_ROOT", "/d"]
    ),
    Def("/opt/smf", ["daemon", "run"], "0.4.0", ["DOTNET_ROOT", "/D"]),
];
JsonArray same = [];
foreach (ServiceDefinition? other in others)
{
    same.Add(
        new JsonObject
        {
            ["left"] = Describe(baseline),
            ["right"] = Describe(other!),
            ["same"] = ServiceManagers.Same(baseline, other!),
        }
    );
    same.Add(
        new JsonObject
        {
            ["left"] = Describe(other!),
            ["right"] = Describe(baseline),
            ["same"] = ServiceManagers.Same(other, baseline),
        }
    );
}

string[][][] environments =
[
    [
        ["PATH", "/usr/bin:rel:/bin"],
        ["DOTNET_ROOT", "/d"],
    ],
    [
        ["PATH", @"C:\a;rel;D:\b;;"],
        ["DOTNET_ROOT", @"C:\dotnet"],
    ],
    [
        ["PATH", "rel:other"],
        ["DOTNET_ROOT", "relative"],
        ["SERMOFUR_DAEMON_HOME", "/home"],
    ],
    [
        ["SERMOFUR_DAEMON_HOME", @"C:\home"],
        ["path", "/x"],
        ["UNKNOWN", "/y"],
        ["PATH", ""],
    ],
    [
        ["PATH", "::"],
        ["DOTNET_ROOT", ""],
        ["HOME", "/root"],
    ],
    [
        ["PATH", @"\\server\share;C:rel;\rooted"],
    ],
];
JsonArray carried = [];
foreach (string[][] environment in environments)
{
    List<string> dropped = [];
    List<string> narrowed = [];
    IReadOnlyDictionary<string, string> kept = Def("/opt/smf", ["daemon", "run"], "1", environment)
        .CarriedEnvironment(dropped.Add, narrowed.Add);
    carried.Add(
        new JsonObject
        {
            ["environment"] = new JsonArray([
                .. environment.Select(pair => (JsonNode)new JsonArray(pair[0], pair[1])),
            ]),
            ["carried"] = Pairs(kept),
            ["dropped"] = new JsonArray([.. dropped.Select(name => (JsonNode)name)]),
            ["narrowed"] = new JsonArray([.. narrowed.Select(name => (JsonNode)name)]),
        }
    );
}

string exe = @"C:\\smf.exe";
string run = "\"arguments\":[\"daemon\",\"run\"]";
string sound = $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\"}}";
List<byte[]> files =
[
    .. new[]
    {
        sound,
        $"{{\"executable\":\"a\",\"executable\":\"{exe}\",{run},\"version\":\"1\"}}",
        $"{{\"executable\":5,\"executable\":\"{exe}\",{run},\"version\":\"1\"}}",
        $"{{\"executable\":\"\\ud800x\",{run},\"version\":\"1\"}}",
        "\uFEFF" + sound,
        $"{{\"executable\":\"{exe}\",\"arguments\":[\"daemon\",null,\"run\"],\"version\":\"1\"}}",
        $"{{\"executable\":\"{exe}\",\"arguments\":[null,\"daemon\",\"run\"],\"version\":\"1\",\"environment\":null}}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\",\"environment\":{{\"A\":null,\"A\":\"x\"}}}}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\",\"environment\":[]}}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\",\"environment\":{{\"A\":1}}}}",
        $"{{\"Executable\":\"{exe}\",{run},\"version\":\"1\"}}",
        $"{{\"executable\":\"rel.exe\",{run},\"version\":\"\u00a0\"}}",
        "{\"executable\":\"rel\",\"arguments\":[\"--x\",\"daemon\",\"run\",\"--supervise\"],\"version\":\"v\"}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\",}}",
        sound + " ",
        "[]",
        "null",
        "",
        $"{{\"executable\":\"{exe}\",\"arguments\":\"daemon run\",\"version\":\"1\"}}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":1}}",
        $"{{\"executable\":\"{exe}\",{run},\"version\":\"1\",\"environment\":{{\"PATH\":\"/a\",\"B\":null,\"DOTNET_ROOT\":\"/d\",\"PATH\":\"/b\"}}}}",
    }.Select(Encoding.UTF8.GetBytes),
    [
        .. "{\"executable\":\"x"u8,
        0xFF,
        .. "\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"1\"}"u8,
    ],
    [
        .. "{\"executable\":\"x\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"1\",\"z\":\""u8,
        0xFF,
        .. "\"}"u8,
    ],
];
JsonArray reads = [];
Directory.CreateDirectory(paths.StateDirectory);
foreach (byte[] content in files)
{
    File.WriteAllBytes(InstalledDefinition.File(paths), content);
    ServiceDefinition? read = InstalledDefinition.Read(paths);
    JsonObject entry = new JsonObject { ["content"] = Convert.ToBase64String(content) };
    if (read is not null)
    {
        entry["read"] = new JsonObject
        {
            ["executable"] = read.Executable,
            ["arguments"] = new JsonArray([.. read.Arguments.Select(a => (JsonNode?)a)]),
            ["version"] = read.Version,
            ["environment"] = new JsonArray([
                .. read.Environment.Select(pair => (JsonNode)new JsonArray(pair.Key, pair.Value)),
            ]),
        };
    }
    else
    {
        entry["read"] = null;
    }
    reads.Add(entry);
}
Directory.Delete(scratch, true);

JsonObject golden = new JsonObject
{
    ["stopTimeoutSeconds"] = (int)DaemonLimits.Default.StopTimeout.TotalSeconds,
    ["definitions"] = described,
    ["quotes"] = quotes,
    ["same"] = same,
    ["carried"] = carried,
    ["reads"] = reads,
};
File.WriteAllText(
    args[0],
    golden.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine(
    $"{described.Count} definitions, {quotes.Count} quotes, {same.Count} comparisons, {carried.Count} environments, {reads.Count} reads"
);
