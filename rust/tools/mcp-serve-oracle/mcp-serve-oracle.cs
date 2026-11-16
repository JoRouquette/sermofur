// Records what the real `smf mcp serve` of the oracle (the .NET command line, already built)
// answers on stdio (lot 8.4): sessions of JSON-RPC lines sent one at a time, each with the lines
// it gets back, the `mcp_*` events of the journal and the exit code. Sessions run without a
// daemon, or beside the real daemon of the oracle (`daemon run` under a fresh
// SERMOFUR_DAEMON_HOME), on a fresh instance made by the oracle (`init`, then the setup steps).
//
// A case waits for its answers: the line, then a `ping` with the id "sync"; once the sync answer
// is in, a case that has fewer answers than messages waits 1.5 s more for each (the SDK answers
// requests side by side); a message without answer costs those 1.5 s.
// Normalized, in this order: the instance root, then the base folder of the session (raw, as a
// JSON string, as a JSON string inside a JSON string) become <root> and <base>; each GUID
// becomes <gN> by order of first appearance in the session (the ids printed by the setup steps
// first); each instant yyyy-MM-ddTHH:mm:ss[.fffffff] becomes <date>; the escaped line breaks of
// the platform (\r\n, at both levels of escaping) become \n.
//
// Usage: dotnet run mcp-serve-oracle.cs -- OUTPUT.json [ORACLE]
// ORACLE: the oracle program; default SMF_ORACLE, then rust/target/oracle/Sermofur.Cli.exe
// (Windows) or ~/sermofur-oracle-bin/Sermofur.Cli (elsewhere).
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
using System.Diagnostics;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.RegularExpressions;

string output = args[0];
string oracle =
    args.Length > 1 ? args[1]
    : Environment.GetEnvironmentVariable("SMF_ORACLE") is { Length: > 0 } variable ? variable
    : OperatingSystem.IsWindows()
        ? Path.GetFullPath(Path.Combine("..", "..", "target", "oracle", "Sermofur.Cli.exe"))
    : Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), "sermofur-oracle-bin", "Sermofur.Cli");

const string Unknown = "0f8fad5b-d9cb-469f-a165-70867728950e";
string Init(string name = "Claude Code Test") =>
    """{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"""
    + JsonSerializer.Serialize(name)
    + ""","version":"1.0"}}}""";
string Request(object id, string method, string? parameters = null) =>
    """{"jsonrpc":"2.0","id":""" + JsonSerializer.Serialize(id) + ""","method":""" + JsonSerializer.Serialize(method)
    + (parameters is null ? "}" : ""","params":""" + parameters + "}");
string Call(object id, string tool, string arguments = "{}") =>
    Request(id, "tools/call", """{"name":""" + JsonSerializer.Serialize(tool) + ""","arguments":""" + arguments + "}");
string Initialize(object id, string parameters) => Request(id, "initialize", parameters);
string Level(object id, string level) => Request(id, "logging/setLevel", """{"level":""" + level + "}");
string unknownN = Unknown.Replace("-", "");
string unknownUpper = Unknown.ToUpperInvariant();
const string Full = """ "capabilities":{},"clientInfo":{"name":"A","version":"1"}""";

List<string> protocol =
[
    // Initialization and negotiation of the version.
    Init(),
    """{"jsonrpc":"2.0","method":"notifications/initialized"}""",
    Initialize(1, $$$"""{"protocolVersion":"1999-01-01",{{{Full}}}}"""),
    Initialize(2, $$$"""{"protocolVersion":"2024-11-05",{{{Full}}}}"""),
    Initialize(3, $$$"""{"protocolVersion":"2025-03-26",{{{Full}}}}"""),
    Initialize(4, $$$"""{"protocolVersion":"2025-11-25",{{{Full}}}}"""),
    Initialize(5, $$$"""{"protocolVersion":null,{{{Full}}}}"""),
    Initialize(6, $$$"""{"protocolVersion":"",{{{Full}}}}"""),
    Request(7, "initialize"),
    Initialize(8, "null"),
    Initialize(9, "{}"),
    Initialize(10, "[]"),
    Initialize(11, "\"x\""),
    Initialize(12, $$$"""{{{{Full}}}}"""),
    Initialize(13, """{"protocolVersion":"2025-06-18","clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(14, """{"protocolVersion":"2025-06-18","capabilities":{}}"""),
    Initialize(15, """{"protocolVersion":5,"capabilities":{},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(16, """{"protocolVersion":"2025-06-18","capabilities":null,"clientInfo":null}"""),
    Initialize(17, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"version":"1"}}"""),
    Initialize(18, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":null,"version":"1"}}"""),
    Initialize(19, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":5}}"""),
    Initialize(20, """{"protocolVersion":"2025-06-18","capabilities":{"sampling":5},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(21, """{"protocolVersion":"2025-06-18","capabilities":{"x":5,"tasks":5},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(22, """{"protocolVersion":"2025-06-18","capabilities":{"roots":{"listChanged":"yes"}},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(23, """{"protocolVersion":"2025-06-18","capabilities":{"experimental":{"a":[1]},"extensions":{"b":5},"elicitation":{"form":5}},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(24, """{"protocolVersion":"2025-06-18","capabilities":{"experimental":5},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(25, """{"protocolVersion":"2025-06-18","capabilities":{"elicitation":5},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(26, """{"protocolVersion":"2025-06-18","capabilities":{"sampling":{"context":{},"tools":5}},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(27, """{"protocolVersion":"2025-06-18","capabilities":[],"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(28, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":"1","title":5}}"""),
    Initialize(29, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":"1","icons":[{"src":"s","sizes":["1"]}]}}"""),
    Initialize(30, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":"1","icons":[{}]}}"""),
    Initialize(31, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":"1","icons":[{"src":"s","sizes":[5]}]}}"""),
    Initialize(32, """{"ProtocolVersion":"2024-11-05","CAPABILITIES":{},"clientInfo":{"Name":"A","version":"1"}}"""),
    Initialize(33, """{"protocolVersion":"2025-06-18","capabilities":{"roots":{},"Roots":{}},"clientInfo":{"name":"A","name":"B","version":"1"}}"""),
    Initialize(34, """{"protocolVersion":"2025-06-18","protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"A","version":"1"}}"""),
    Initialize(35, """{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"A","version":"1"},"protocolversion":"2024-11-05"}"""),
    Initialize(36, $$$"""{"protocolVersion":"2025-06-18",{{{Full}}},"_meta":5}"""),
    Initialize(37, $$$"""{"protocolVersion":"2025-06-18",{{{Full}}},"inputResponses":{"a":5},"_meta":{}}"""),
    Initialize(38, $$$"""{"protocolVersion":"2025-06-18",{{{Full}}},"inputResponses":5}"""),
    Initialize(39, $$$"""{"protocolVersion":"2025-06-18",{{{Full}}},"requestState":5}"""),
    Initialize(40, """{"protocolVersion":"2025-06-18","capabilities":{},"clıentInfo":{"name":"A","version":"1"}}"""),
    Initialize("40b", """{"protocolVersion":"2025-06-18","capabilities":{"ſampling":5},"clientInfo":{"name":"A","version":"1"}}"""),
    // Methods.
    Request(41, "ping"),
    Request(42, "ping", "[]"),
    Request(43, "ping", "5"),
    Request(44, "ping", "null"),
    Request(45, "ping", """{"_meta":5}"""),
    Request(46, "foo/bar"),
    Request(47, ""),
    Request(48, "PING"),
    Request(49, "resources/list"),
    Request(50, "prompts/list"),
    Request(51, "completion/complete", "{}"),
    Request(52, "notifications/initialized"),
    Request(53, "notifications/cancelled", """{"requestId":1}"""),
    Request(54, "tools/list"),
    Request(55, "tools/list", "null"),
    Request(56, "tools/list", """{"cursor":"abc"}"""),
    Request(57, "tools/list", """{"cursor":5}"""),
    Request(58, "tools/list", "[]"),
    Request(59, "tools/list", """{"_meta":null,"cursor":null}"""),
    Request(60, "tools/list", """{"zz":1,"ZZ":2}"""),
    Request(61, "logging/setLevel"),
    Request(62, "logging/setLevel", "{}"),
    Request(63, "logging/setLevel", """{"Level":"debug","level":"info"}"""),
    Request(64, "logging/setLevel", """{"level":"debug","level":"info"}"""),
    Level(65, "\"debug\""), Level(66, "\"emergency\""), Level(67, "\"Debug\""), Level(68, "\"DEBUG\""),
    Level(69, "\" debug\""), Level(70, "\"info \""), Level(71, "\"debug, info\""), Level(72, "\"debug,\""),
    Level(73, "\",\""), Level(74, "\"debug,,info\""), Level(75, "\"\""), Level(76, "\" \""),
    Level(77, "\"0\""), Level(78, "\"+1\""), Level(79, "\" 1 \""), Level(80, "\"-5\""), Level(81, "\"99\""),
    Level(82, "\"0x1\""), Level(83, "\"1,debug\""), Level(84, "\"debug,7\""), Level(85, "\"2147483648\""),
    Level(86, "\"debug\\u0000\""), Level(87, "0"), Level(88, "99"), Level(89, "-1"), Level(90, "1.5"),
    Level(91, "2147483648"), Level(92, "true"), Level(93, "null"),
    // Envelopes.
    "not json",
    "{}",
    "[]",
    """[{"jsonrpc":"2.0","id":94,"method":"ping"}]""",
    """{"jsonrpc":"1.0","id":95,"method":"ping"}""",
    """{"jsonrpc":2,"id":96,"method":"ping"}""",
    """{"JSONRPC":"2.0","id":97,"method":"ping"}""",
    """{"id":98,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":99}""",
    """{"jsonrpc":"2.0","ID":100,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":101,"method":"ping","extra":1}""",
    """{"jsonrpc":"2.0","id":102,"method":"ping","jsonrpc":"2.0"}""",
    """{"jsonrpc":"2.0","id":103,"method":"ping","jsonrpc":"1.0"}""",
    """{"jsonrpc":"2.0","id":104,"method":5}""",
    """{"jsonrpc":"2.0","id":105,"method":null}""",
    """{"jsonrpc":"2.0","id":106,"method":"ping","method":"tools/list"}""",
    """{"jsonrpc":"2.0","id":107,"method":"ping","id":108}""",
    """{"jsonrpc":"2.0","id":1.5,"method":"ping","id":109}""",
    """{"jsonrpc":"2.0","id":110,"method":"ping","id":1.5}""",
    """{"jsonrpc":"2.0","id":"111","id":112,"method":5}""",
    """{"jsonrpc":"2.0","id":113,"method":"ping\ud800"}""",
    """{"jsonrpc":"2.0","id":"a\ud800","method":"ping"}""",
    """{"jsonrpc":"2.0","id":114,"method":"ping""" + "\"",
    """{"jsonrpc":"2.0","id":115 , "method":"ping"} x""",
    """{"jsonrpc":"2.0","id":5x}""",
    """{"params":{"id":116},"jsonrpc":"1.0","method":"x"}""",
    """{"jsonrpc":"2.0","id":117,"result":{}}""",
    """{"jsonrpc":"2.0","id":118,"result":5}""",
    """{"jsonrpc":"2.0","id":119,"result":null}""",
    """{"jsonrpc":"2.0","id":120,"error":{"code":1,"message":"m"}}""",
    """{"jsonrpc":"2.0","id":121,"error":5}""",
    """{"jsonrpc":"2.0","id":122,"error":null}""",
    """{"jsonrpc":"2.0","id":123,"error":{"code":1}}""",
    """{"jsonrpc":"2.0","id":124,"error":{"code":1.5,"message":"m"}}""",
    """{"jsonrpc":"2.0","method":"ping"}""",
    """{"jsonrpc":"2.0","method":"tools/call","params":{"name":"sermofur_status"}}""",
    """{"jsonrpc":"2.0","method":"notifications/unknown"}""",
    """{"jsonrpc":"2.0","method":"notifications/cancelled","params":5}""",
    """{"jsonrpc":"2.0","method":5}""",
    """{"jsonrpc":"2.0","id":"x\u00e9 <'&+`\"\\>","method":"ping"}""",
    """{"jsonrpc":"2.0","id":"","method":"ping"}""",
    """{"jsonrpc":"2.0","id":99999999999999,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":-1,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":-0,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":1.0,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":1e2,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":9223372036854775807,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":9223372036854775808,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":null,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":true,"method":"ping"}""",
    """{"jsonrpc":"2.0","id":{},"method":"ping"}""",
    """  {"jsonrpc":"2.0","id":125,"method":"ping"}  """,
    // Tool calls: names, params, validation (no daemon: a valid call is daemon_unavailable).
    Request(126, "tools/call"),
    Request(127, "tools/call", "{}"),
    Request(128, "tools/call", """{"name":5}"""),
    Request(129, "tools/call", """{"name":null}"""),
    Request(130, "tools/call", """{"name":"nope"}"""),
    Request(131, "tools/call", """{"Name":"x"}"""),
    Request(132, "tools/call", """{"name":"x","Name":"y"}"""),
    Request(133, "tools/call", """{"name":"sermofur_recall","name":5}"""),
    Request(134, "tools/call", """{"name":"x","_meta":[]}"""),
    Request(135, "tools/call", """{"name":"sermofur_status","arguments":5}"""),
    Request(136, "tools/call", """{"name":"sermofur_status","arguments":[]}"""),
    Request(137, "tools/call", """{"name":"sermofur_status","arguments":null}"""),
    Request(138, "tools/call", """{"name":"sermofur_status","arguments":{},"_meta":{"progressToken":"p"}}"""),
    Call(139, "sermofur_status"),
    Call(140, "sermofur_status", """{"path":"x"}"""),
    Call(141, "sermofur_status", """{"a":1,"b":2}"""),
    Call(142, "sermofur_context"),
    Call(143, "sermofur_recall"),
    Call(144, "sermofur_recall", """{"question":null}"""),
    Call(145, "sermofur_recall", """{"question":""}"""),
    Call(146, "sermofur_recall", """{"question":"a\u0000b"}"""),
    Call(147, "sermofur_recall", """{"question":"a\ud800b"}"""),
    Call(148, "sermofur_recall", """{"question":"@16385*x@"}"""),
    Call(149, "sermofur_recall", """{"question":"@16384*x@"}"""),
    Call(150, "sermofur_recall", """{"question":"@16384*é@"}"""),
    Call(151, "sermofur_recall", """{"question":"@8193*😀@"}"""),
    Call(152, "sermofur_recall", """{"question":"--x"}"""),
    Call(153, "sermofur_recall", """{"question":5}"""),
    Call(154, "sermofur_recall", """{"question":"ok","limit":0}"""),
    Call(155, "sermofur_recall", """{"question":"ok","limit":4}"""),
    Call(156, "sermofur_recall", """{"question":"ok","limit":2.0}"""),
    Call(157, "sermofur_recall", """{"question":"ok","limit":-0}"""),
    Call(158, "sermofur_recall", """{"question":"ok","limit":"2"}"""),
    Call(159, "sermofur_recall", """{"question":"ok","limit":3}"""),
    Call(160, "sermofur_recall", """{"question":"","question":"ok"}"""),
    Call(161, "sermofur_recall", """{"question":"a","question":"b","zz":1}"""),
    Call(162, "sermofur_recall", """{"zz":1,"question":"","yy":2}"""),
    Call(163, "sermofur_recall", """{"Question":"a"}"""),
    Call(164, "sermofur_challenge"),
    Call(165, "sermofur_challenge", $$$"""{"claimId":"{{{Unknown}}}","text":"t"}"""),
    Call(166, "sermofur_challenge", $$$"""{"claimId":" {{{Unknown}}} "}"""),
    Call(167, "sermofur_challenge", $$$"""{"claimId":"{{{unknownUpper}}}"}"""),
    Call(168, "sermofur_challenge", $$$"""{"claimId":"{{{unknownN}}}"}"""),
    Call(169, "sermofur_challenge", $$$"""{"claimId":"{{{{Unknown}}}}"}"""),
    Call(170, "sermofur_challenge", """{"claimId":"+0x0fad5b-d9cb-469f-a165-70867728950e"}"""),
    Call(171, "sermofur_challenge", """{"text":"--x"}"""),
    Call(172, "sermofur_challenge", """{"text":null,"claimId":null}"""),
    Call(173, "sermofur_claim", """{"text":"x","origin":"user"}"""),
    Call(174, "sermofur_claim", """{"text":"x","actor":"someone"}"""),
    Call(175, "sermofur_claim", """{"text":"x","category":"decisions"}"""),
    Call(176, "sermofur_claim", """{"text":"x","category":"Semantic"}"""),
    Call(177, "sermofur_claim", """{"text":"x","volatility":"volatile","category":"procedural","key":"--k"}"""),
    Call(178, "sermofur_claim", """{"text":"-- starts with dashes"}"""),
    Call(179, "sermofur_evidence", """{"claimId":"x","kind":"source_code","reference":"r","lineage":"l"}"""),
    Call(180, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"proof","reference":"r","lineage":"l"}"""),
    Call(181, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"execution","reference":"--r","lineage":"--l"}"""),
    Call(182, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"execution","reference":"r","lineage":"l","contradicts":"true"}"""),
    Call(183, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"execution","reference":"r","lineage":"l","contradicts":1}"""),
    Call(184, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"execution","reference":"r","lineage":"l","sourceId":"s"}"""),
    Call(185, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"execution","reference":"r"}"""),
    Call(186, "sermofur_record_retex", """{"event":"--e","impact":"i","next":"n"}"""),
    Call(187, "sermofur_record_retex", """{"event":"e","impact":"i"}"""),
    Call(188, "sermofur_record_retex", """{"event":"e","impact":"i","next":"n","key":""}"""),
    Call(189, "sermofur_feedback", $$$"""{"targetId":"{{{Unknown}}}","verdict":"great"}"""),
    Call(190, "sermofur_feedback", $$$"""{"targetId":"{{{Unknown}}}","verdict":"wrong","comment":"--c"}"""),
    Call(191, "sermofur_feedback", """{"verdict":"wrong"}"""),
    Call("é", "sermofur_feedback", $$$"""{"targetId":"{{{Unknown}}}","verdict":"helpful","comment":"<b> & 'c'"}"""),
];

List<string> framing =
[
    "\uFEFF" + Request(1, "ping"),
    Request(2, "ping") + "\r" + Request(3, "ping"),
    Request(4, "ping") + "\r",
    "",
    "   ",
    "\t" + Request(5, "ping"),
    Request(6, "ping") + "\r\n" + Request(7, "ping"),
];

List<string> unserved = [Init(), Call(1, "sermofur_status"), Call(2, "sermofur_recall", """{"question":"q"}""")];

List<string> tools =
[
    Init(),
    Call(1, "sermofur_status"),
    Call(2, "sermofur_context"),
    Call(3, "sermofur_recall", """{"question":"cache"}"""),
    Call(4, "sermofur_recall", """{"question":"cache","limit":1}"""),
    Call(5, "sermofur_recall", """{"question":"nothing matches this"}"""),
    Call(6, "sermofur_challenge", """{"claimId":"@0@"}"""),
    Call(7, "sermofur_challenge", """{"claimId":" @0@ "}"""),
    Call(8, "sermofur_challenge", """{"text":"cache invalidation is easy"}"""),
    Call(9, "sermofur_challenge", $$$"""{"claimId":"{{{Unknown}}}"}"""),
    Call(10, "sermofur_claim", """{"text":"-- starts with dashes","key":"k1"}"""),
    Call(11, "sermofur_claim", """{"text":"-- starts with dashes","key":"k1"}"""),
    Call(12, "sermofur_claim", """{"text":"-h"}"""),
    Call(13, "sermofur_claim", """{"text":"Billing <paginates> & it's é 😀 \u2028","category":"procedural","volatility":"stable"}"""),
    Call(14, "sermofur_claim", """{"text":"-- starts with dashes","key":"k1","category":"episodic"}"""),
    Call(15, "sermofur_evidence", """{"claimId":"@0@","kind":"source_code","reference":"--src/a.cs","lineage":"repo"}"""),
    Call(16, "sermofur_evidence", """{"claimId":"@0@","kind":"execution","reference":"run","lineage":"ci","contradicts":true,"key":"e1"}"""),
    Call(17, "sermofur_evidence", """{"claimId":"@0@","kind":"execution","reference":"run","lineage":"ci","contradicts":false,"key":"e1"}"""),
    Call(18, "sermofur_evidence", $$$"""{"claimId":"@0@","kind":"execution","reference":"run","lineage":"ci","sourceId":"{{{Unknown}}}"}"""),
    Call(19, "sermofur_evidence", $$$"""{"claimId":"{{{Unknown}}}","kind":"llm_assertion","reference":"r","lineage":"l"}"""),
    Call(20, "sermofur_record_retex", """{"event":"e","impact":"i","next":"n"}"""),
    Call(21, "sermofur_record_retex", """{"event":"e","impact":"i","next":"n","key":"r1"}"""),
    Call(22, "sermofur_feedback", """{"targetId":"@1@","verdict":"wrong","comment":"it retries five times"}"""),
    Call(23, "sermofur_feedback", """{"targetId":"@1@","verdict":"helpful"}"""),
    Call(24, "sermofur_feedback", $$$"""{"targetId":"{{{Unknown}}}","verdict":"not_applicable"}"""),
    Call(25, "sermofur_recall", """{"question":"queues retry"}"""),
    Call(26, "sermofur_status"),
    Init("Other Host"),
    Call(27, "sermofur_claim", """{"text":"written by another host"}"""),
    Request(28, "initialize"),
    Call(29, "sermofur_claim", """{"text":"written by an unnamed host"}"""),
    Call(30, "sermofur_status", """{"zz":1}"""),
];

List<string> sibling =
[
    Init(),
    Call(1, "sermofur_challenge", """{"claimId":"@6@"}"""),
    Call(2, "sermofur_feedback", """{"targetId":"@6@","verdict":"wrong"}"""),
    Call(3, "sermofur_evidence", """{"claimId":"@6@","kind":"llm_assertion","reference":"r","lineage":"l"}"""),
    Call(4, "sermofur_recall", """{"question":"secret client"}"""),
    Call(5, "sermofur_status", """{"path":"x"}"""),
    Call(6, "sermofur_recall", """{"question":"x","scope":"client-b"}"""),
    Call(7, "sermofur_context"),
];

JsonObject[] setup =
[
    Step(null, "claim", "add", "cache invalidation is hard", "--origin", "user", "--json"),
    Step(null, "claim", "add", "queues retry three times", "--origin", "user", "--json"),
    Mkdir("client-a"),
    Mkdir("client-b"),
    Step(null, "scope", "add", "client-a", "client", "workspace", "client-a", "--json"),
    Step(null, "scope", "add", "client-b", "client", "workspace", "client-b", "--json"),
    Step("client-b", "claim", "add", "secret of client b", "--origin", "user", "--json"),
];

JsonArray sessions =
[
    Session("protocol", "root", daemon: false, registered: false, null, [], protocol),
    Session("framing", "root", daemon: false, registered: false, null, [], framing),
    Session("not_registered", "root", daemon: true, registered: false, null, [], unserved),
    Session("outside_an_instance", "outside", daemon: true, registered: false, null, [], unserved),
    Session("tools", "root", daemon: true, registered: true, null, setup, tools),
    Session("sibling_scope", "root/client-a", daemon: true, registered: true, null, setup, sibling),
    Session("project_directory", "outside", daemon: true, registered: true, "root", [], unserved),
    Session("relative_project_directory", "outside", daemon: true, registered: true, "relative", [], unserved),
];

JsonObject golden = new JsonObject
{
    ["tool"] = "rust/tools/mcp-serve-oracle",
    ["sessions"] = sessions,
};
File.WriteAllText(
    output,
    golden.ToJsonString(
        new JsonSerializerOptions
        {
            WriteIndented = true,
            Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
        }
    ) + "\n"
);
return 0;

JsonObject Step(string? folder, params string[] words) =>
    new JsonObject { ["cwd"] = folder, ["run"] = new JsonArray([.. words.Select(w => JsonValue.Create(w))]) };

JsonObject Mkdir(string folder) => new JsonObject { ["mkdir"] = folder };

JsonObject Session(
    string name,
    string folder,
    bool daemon,
    bool registered,
    string? projectDirectory,
    JsonObject[] steps,
    List<string> sends
)
{
    string tag = Guid.NewGuid().ToString("N")[..8];
    string @base = Path.Combine(Path.GetTempPath(), "smf-mcp-oracle-" + tag);
    string root = Path.Combine(@base, "proj é");
    string home = Path.Combine(@base, "home");
    Directory.CreateDirectory(root);
    Directory.CreateDirectory(Path.Combine(@base, "outside"));
    Process? daemonProcess = null;
    try
    {
        Run(root, home, ["init"]);
        List<string> ids = [];
        foreach (JsonObject step in steps)
        {
            if (step["mkdir"] is { } made)
            {
                Directory.CreateDirectory(Path.Combine(root, made.GetValue<string>()));
                ids.Add("");
                continue;
            }
            string cwd = step["cwd"] is { } sub ? Path.Combine(root, sub.GetValue<string>()) : root;
            string printed = Run(cwd, home, [.. step["run"]!.AsArray().Select(w => w!.GetValue<string>())]);
            ids.Add(JsonNode.Parse(printed)?["id"]?.GetValue<string>() ?? "");
        }
        if (daemon)
        {
            daemonProcess = Start(root, home, ["daemon", "run"], redirectInput: false);
            string? first = daemonProcess.StandardOutput.ReadLine();
            if (first is null || !first.Contains("listening on"))
            {
                throw new InvalidOperationException($"daemon: {first}");
            }
        }
        if (registered)
        {
            Run(root, home, ["daemon", "register"]);
        }
        string working = folder switch
        {
            "root" => root,
            "outside" => Path.Combine(@base, "outside"),
            _ => Path.Combine(root, folder["root/".Length..]),
        };
        Process serve = Start(
            working,
            daemon ? home : Path.Combine(@base, "absent"),
            ["mcp", "serve"],
            redirectInput: true,
            projectDirectory switch
            {
                "root" => root,
                "relative" => "proj é",
                _ => null,
            }
        );
        StringBuilder errors = new StringBuilder();
        serve.ErrorDataReceived += (_, line) => { lock (errors) { errors.Append(line.Data); } };
        serve.BeginErrorReadLine();
        System.Collections.Concurrent.BlockingCollection<string> lines = [];
        Thread reader = new Thread(() =>
        {
            while (serve.StandardOutput.ReadLine() is { } line)
            {
                lines.Add(line);
            }
            lines.CompleteAdding();
        });
        reader.Start();
        Stream input = serve.StandardInput.BaseStream;
        Normalizer names = new Normalizer(root, @base);
        foreach (string id in ids.Where(id => id.Length > 0))
        {
            names.Normalize(id);
        }
        JsonArray cases = [];
        foreach (string template in sends)
        {
            string send = template;
            for (int index = 0; index < ids.Count; index++)
            {
                send = send.Replace($"@{index}@", ids[index]);
            }
            send = Patterns.Repeat().Replace(send, match =>
                string.Concat(Enumerable.Repeat(match.Groups[2].Value, int.Parse(match.Groups[1].Value))));
            Write(input, send);
            Write(input, """{"jsonrpc":"2.0","id":"sync","method":"ping"}""");
            List<string> answers = [];
            bool synced = false;
            // A line may carry several messages (a lone CR ends a line for the reader of the SDK).
            int messages = Math.Max(
                1,
                Regex.Split(send, "\r\n|\r|\n").Count(part => part.Trim().Length > 0)
            );
            while (!synced || answers.Count < messages)
            {
                if (!lines.TryTake(out string? line, synced ? 1500 : 60_000))
                {
                    if (!synced)
                    {
                        throw new TimeoutException($"{name}: no answer to the sync ping after {send}");
                    }
                    break;
                }
                if (line == """{"result":{},"id":"sync","jsonrpc":"2.0"}""")
                {
                    synced = true;
                    continue;
                }
                answers.Add(names.Normalize(line));
            }
            cases.Add(new JsonObject
            {
                ["send"] = template,
                ["answers"] = new JsonArray([.. answers.Select(a => JsonValue.Create(a))]),
            });
        }
        input.Close();
        if (!serve.WaitForExit(20_000))
        {
            serve.Kill(true);
            throw new TimeoutException($"{name}: the server did not stop at the end of stdin");
        }
        reader.Join();
        string logFile = Path.Combine(home, "state", "daemon.log");
        JsonArray events = [];
        if (daemon && File.Exists(logFile))
        {
            foreach (string entry in File.ReadAllLines(logFile))
            {
                JsonNode? line = JsonNode.Parse(entry);
                string evt = line?["event"]?.GetValue<string>() ?? "";
                if (evt.StartsWith("mcp_", StringComparison.Ordinal))
                {
                    events.Add(line?["code"] is { } code ? $"{evt} {code.GetValue<string>()}" : evt);
                }
            }
        }
        string all = string.Join("\n", cases.Select(c => c!.ToJsonString()));
        if (all.Contains(tag, StringComparison.Ordinal))
        {
            throw new InvalidOperationException($"{name}: a path of this machine is left after normalization");
        }
        Console.Error.WriteLine($"{name}: {cases.Count} cases");
        return new JsonObject
        {
            ["name"] = name,
            ["folder"] = folder,
            ["daemon"] = daemon,
            ["registered"] = registered,
            ["projectDirectory"] = projectDirectory,
            ["setup"] = new JsonArray([.. steps.Select(s => s.DeepClone())]),
            ["cases"] = cases,
            ["exit"] = serve.ExitCode,
            ["stderr"] = errors.ToString(),
            ["log"] = events,
        };
    }
    finally
    {
        if (daemonProcess is not null)
        {
            try { Run(root, home, ["daemon", "stop"]); } catch (Exception) { }
            if (!daemonProcess.WaitForExit(10_000))
            {
                daemonProcess.Kill(true);
            }
        }
        for (int attempt = 0; attempt < 20 && Directory.Exists(@base); attempt++)
        {
            try { Directory.Delete(@base, recursive: true); }
            catch (IOException) { Thread.Sleep(250); }
            catch (UnauthorizedAccessException) { Thread.Sleep(250); }
        }
    }
}

static void Write(Stream input, string line)
{
    byte[] bytes = Encoding.UTF8.GetBytes(line + "\n");
    input.Write(bytes);
    input.Flush();
}

Process Start(string cwd, string home, string[] words, bool redirectInput, string? projectDirectory = null)
{
    ProcessStartInfo start = new ProcessStartInfo(oracle)
    {
        WorkingDirectory = cwd,
        RedirectStandardOutput = true,
        RedirectStandardError = true,
        RedirectStandardInput = redirectInput,
        StandardOutputEncoding = new UTF8Encoding(false),
        StandardErrorEncoding = new UTF8Encoding(false),
        UseShellExecute = false,
    };
    foreach (string word in words)
    {
        start.ArgumentList.Add(word);
    }
    start.Environment["SERMOFUR_DAEMON_HOME"] = home;
    start.Environment.Remove("SERMOFUR_NO_DAEMON");
    start.Environment.Remove("CLAUDE_PROJECT_DIR");
    if (projectDirectory is not null)
    {
        start.Environment["CLAUDE_PROJECT_DIR"] = projectDirectory;
    }
    return Process.Start(start)!;
}

string Run(string cwd, string home, string[] words)
{
    Process process = Start(cwd, home, words, redirectInput: false);
    Task<string> error = process.StandardError.ReadToEndAsync();
    string text = process.StandardOutput.ReadToEnd();
    process.WaitForExit();
    if (process.ExitCode != 0)
    {
        throw new InvalidOperationException($"{string.Join(' ', words)}: {process.ExitCode} {error.Result}");
    }
    return text;
}

/// <summary>`@N*TEXT@` in a sent line: TEXT repeated N times.</summary>
static partial class Patterns
{
    [GeneratedRegex(@"@(\d+)\*([^@]*)@")]
    internal static partial Regex Repeat();
}

/// <summary>The placeholders of one session (see the header).</summary>
sealed partial class Normalizer(string root, string @base)
{
    private readonly Dictionary<string, string> guids = [];

    public string Normalize(string line)
    {
        string text = line;
        foreach ((string path, string placeholder) in new[] { (root, "<root>"), (@base, "<base>") })
        {
            string once = JsonSerializer.Serialize(path)[1..^1];
            string twice = JsonSerializer.Serialize(once)[1..^1];
            text = text.Replace(twice, placeholder).Replace(once, placeholder).Replace(path, placeholder);
        }
        text = Guid().Replace(text, match =>
        {
            string key = match.Value.ToLowerInvariant();
            if (!guids.TryGetValue(key, out string? name))
            {
                name = $"<g{guids.Count + 1}>";
                guids[key] = name;
            }
            return name;
        });
        text = Instant().Replace(text, "<date>");
        return text.Replace(@"\\r\\n", @"\\n").Replace(@"\r\n", @"\n");
    }

    [GeneratedRegex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")]
    private static partial Regex Guid();

    [GeneratedRegex(@"\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,7})?")]
    private static partial Regex Instant();
}
