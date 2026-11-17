// Records what McpConfigFile of src/Sermofur.Mcp does with crafted .mcp.json contents: WithEntry
// (for several entries), WithoutEntry and CurrentEntry, byte for byte, refusals and exceptions
// included. Usage: dotnet run mcp-config-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Mcp/Sermofur.Mcp.csproj
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Mcp;

string Hex(byte[] bytes) => Convert.ToHexString(bytes);

JsonNode Outcome(Func<object?> run)
{
    try
    {
        object? result = run();
        return result switch
        {
            null => new JsonObject { ["null"] = true },
            byte[] bytes when bytes.Length > 4096 => new JsonObject
            {
                ["sha256"] = Hex(System.Security.Cryptography.SHA256.HashData(bytes)),
                ["length"] = bytes.Length,
            },
            byte[] bytes => new JsonObject { ["bytes"] = Hex(bytes) },
            bool flag => new JsonObject { ["object"] = flag },
            _ => throw new InvalidOperationException("unexpected"),
        };
    }
    catch (SermofurException refusal)
    {
        return new JsonObject
        {
            ["code"] = refusal.Code,
            ["message"] = refusal.Message,
            ["exit"] = refusal.ExitCode,
        };
    }
    catch (Exception exception)
    {
        return new JsonObject { ["exception"] = exception.GetType().Name };
    }
}

(string Command, string[] Arguments)[] entries =
[
    ("smf", ["mcp", "serve"]),
    ("/opt/smf", ["mcp", "serve"]),
    ("C:\\Program Files\\été <&> 'q' \"d\" +`\\smf.exe 😀", ["mcp", "serve", "\u00e9\u2028\u001f"]),
    ("smf", []),
];

const string Others = """
    {
      "mcpServers": {
        "github": {
          "type": "http",   "url": "https://example.com/mcp"
        },
        "files": {"command": "npx", "args": ["-y", "pkg"]}
      },
      "extra": [1, 2]
    }
    """;

List<(string Name, byte[] Content)> cases = [];
void Text(string name, string text) => cases.Add((name, Encoding.UTF8.GetBytes(text)));
void Raw(string name, byte[] bytes) => cases.Add((name, bytes));
byte[] Bom(string text) => [0xEF, 0xBB, 0xBF, .. Encoding.UTF8.GetBytes(text)];
byte[] Utf8(string text) => Encoding.UTF8.GetBytes(text);

Text("others", Others);
Text("others_crlf", Others.Replace("\n", "\r\n"));
Text("empty_object", "{}");
Text("empty_object_spaced", "{ \n\t }");
Text("servers_empty", "{ \"mcpServers\": {} }");
Text("servers_empty_spaced", "{\n  \"mcpServers\": {\n  }\n}\n");
Text("other_only", "{\"other\": true}");
Raw("bom_one_server", Bom("{\"mcpServers\": {\"a\": {}}}"));
Raw("bom_empty", Bom("{}"));
Raw("bom_only", [0xEF, 0xBB, 0xBF]);
Text("not_json", "not json");
Text("array", "[]");
Text("servers_array", "{\"mcpServers\": []}");
Text("servers_null", "{\"mcpServers\": null}");
Text("trailing", "{\"a\": 1} trailing");
Text("trailing_space", "{\"a\": 1}  \r\n\t");
Text("comment", "{\"a\": 1 // comment\n}");
Text("block_comment", "{/* c */\"a\": 1}");
Text("trailing_comma", "{\"a\": 1,}");
Text("servers_trailing_comma", "{\"mcpServers\": {\"a\": 1,}}");
Text("empty", "");
Text("spaces", "   ");
Text("unclosed", "{\"mcpServers\": {");
Text("unclosed_root", "{\"a\": 1");
Text("single_quote", "{'a': 1}");
Text("leading_zero", "{\"a\": 01}");
Text("numbers", "{\"a\": [-0, 1.5e+3, 2E-2, 0.0]}");
Text("bad_number", "{\"a\": 1.}");
Text("control_char", "{\"a\": \"x\u0001y\"}");
Text("bad_escape", "{\"a\": \"\\q\"}");
Text("bad_hex", "{\"a\": \"\\u12G4\"}");
Text(
    "only_entry",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"command\": \"smf\", \"args\": [\"mcp\", \"serve\"]}}}"
);
Text(
    "entry_same_reordered",
    "{\"mcpServers\": {\"sermofur\": {\"args\": [\"mcp\", \"serve\"], \"command\": \"smf\", \"type\": \"stdio\"}}}"
);
Text(
    "entry_extra_key",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"command\": \"smf\", \"args\": [\"mcp\", \"serve\"], \"env\": {}}}}"
);
Text(
    "entry_escaped_value",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"\\u0073tdio\", \"command\": \"smf\", \"args\": [\"mcp\", \"serve\"]}}}"
);
Text("entry_number", "{\"mcpServers\": {\"sermofur\": 1}}");
Text("entry_array", "{\"mcpServers\": {\"sermofur\": [1, {\"a\": 2}]}}");
Text("entry_null", "{\"mcpServers\": {\"sermofur\": null}}");
Text("entry_first", "{\"mcpServers\": {\"sermofur\": {}, \"b\": {}, \"c\": {}}}");
Text("entry_middle", "{\"mcpServers\": {\"a\": {}, \"sermofur\": {}, \"c\": {}}}");
Text("entry_last", "{\"mcpServers\": {\"a\": {},\n    \"sermofur\": {}\n  }}");
Text("entry_only_spaced", "{\n  \"mcpServers\": {\n    \"sermofur\": {}\n  }\n}\n");
Text("entry_escaped_name", "{\"mcpServers\": {\"\\u0073ermofur\": {}}}");
Text("servers_escaped_name", "{\"mcp\\u0053ervers\": {\"a\": 1}}");
Text("entry_twice", "{\"mcpServers\": {\"sermofur\": 1, \"a\": 2, \"sermofur\": 3}}");
Text("servers_twice", "{\"mcpServers\": {\"sermofur\": 1}, \"x\": 0, \"mcpServers\": {\"a\": 2}}");
Text(
    "servers_twice_second_entry",
    "{\"mcpServers\": {\"a\": 1}, \"mcpServers\": {\"sermofur\": 2}}"
);
Text(
    "entry_duplicate_key",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"type\": \"stdio\", \"command\": \"smf\", \"args\": [\"mcp\", \"serve\"]}}}"
);
Text("entry_lone_surrogate", "{\"mcpServers\": {\"sermofur\": {\"command\": \"\\ud800\"}}}");
Text("name_lone_surrogate", "{\"mcpServers\": {\"\\ud800\": 1}}");
Text("other_lone_surrogate", "{\"mcpServers\": {\"a\": \"\\udc00\"}}");
Raw("invalid_utf8_other", [.. Utf8("{\"mcpServers\": {\"a\": \""), 0xFF, .. Utf8("\"}}")]);
Raw(
    "invalid_utf8_entry",
    [.. Utf8("{\"mcpServers\": {\"sermofur\": {\"command\": \""), 0xC3, .. Utf8("\"}}}")]
);
Raw("invalid_utf8_name", [.. Utf8("{\"mcpServers\": {\""), 0xFE, .. Utf8("\": 1}}")]);
Text("unicode_others", "{\"mcpServers\": {\"été\": {\"command\": \"😀\"}}, \"z\": \"\u2028\"}");
Text("depth_64", "{\"a\": " + new string('[', 62) + new string(']', 62) + "}");
Text("depth_65", "{\"a\": " + new string('[', 63) + new string(']', 63) + "}");
Text(
    "entry_depth_64",
    "{\"mcpServers\": {\"sermofur\": " + new string('[', 61) + new string(']', 61) + "}}"
);
Text("depth_65_real", "{\"a\": " + new string('[', 64) + new string(']', 64) + "}");
Text(
    "entry_depth_65",
    "{\"mcpServers\": {\"sermofur\": " + new string('[', 63) + new string(']', 63) + "}}"
);
Text("servers_array_broken", "{\"mcpServers\": [}");
Text("array_broken", "[1,}");
Text("skipped_broken", "{\"a\": [}");
Text("root_string", "\"x\"");
Text("servers_number_slash", "{\"mcpServers\": 1/}");
Text("servers_true_slash", "{\"mcpServers\": true/* c */}");
Text("servers_number_letter", "{\"mcpServers\": 1x}");
Text("servers_true_letter", "{\"mcpServers\": truex}");
Text("root_null_letter", "nullx");
Text("root_true_letter", "truex");
Text(
    "entry3_lone_command",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"command\": \"\ud800\", \"args\": []}}}"
);
Text(
    "entry3_lone_name",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"\ud800\": \"smf\", \"args\": []}}}"
);
Text(
    "entry3_lone_type_first",
    "{\"mcpServers\": {\"sermofur\": {\"type\": \"\udc00\", \"command\": \"smf\", \"args\": [\"mcp\", \"serve\"]}}}"
);
Raw(
    "entry3_invalid_utf8_command",
    [
        .. Utf8("{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \"command\": \""),
        0xC3,
        .. Utf8("\", \"args\": []}}}"),
    ]
);
Raw(
    "entry3_invalid_utf8_name",
    [
        .. Utf8("{\"mcpServers\": {\"sermofur\": {\"type\": \"stdio\", \""),
        0xFF,
        .. Utf8("\": \"smf\", \"args\": []}}}"),
    ]
);
Text("tabs_layout", "{\t\"mcpServers\"\t:\t{\t\"a\"\t:\t1\t}\t}");
Text("no_space_layout", "{\"mcpServers\":{\"a\":1},\"b\":2}");
Raw("too_large", Utf8("{\"a\": \"" + new string('x', 1024 * 1024) + "\"}"));
Raw("exactly_limit", Utf8("{\"a\":\"" + new string('x', 1024 * 1024 - 8) + "\"}"));

// Shapes of the FsCheck property: generated servers, compact or indented.
Random random = new Random(20261116);
for (int index = 0; index < 40; index++)
{
    JsonObject servers = [];
    int count = random.Next(0, 5);
    for (int k = 0; k < count; k++)
    {
        string key = new string([
            .. Enumerable.Range(0, random.Next(1, 6)).Select(_ => (char)random.Next(0x20, 0x250)),
        ]);
        if (key != "sermofur" && !servers.ContainsKey(key))
        {
            servers[key] = new JsonObject { ["command"] = key };
        }
    }
    string content = new JsonObject { ["mcpServers"] = servers }.ToJsonString(
        new JsonSerializerOptions { WriteIndented = index % 2 == 0 }
    );
    Text($"generated_{index}", content);
}

JsonArray results = [];
foreach ((string name, byte[] content) in cases)
{
    JsonArray withEntry = [];
    foreach ((string command, string[] arguments) in entries)
    {
        withEntry.Add(
            Outcome(() => McpConfigFile.WithEntry(content, McpConfigFile.Entry(command, arguments)))
        );
    }
    results.Add(
        new JsonObject
        {
            ["name"] = name,
            ["content"] = content.Length > 4096 ? null : Hex(content),
            ["withEntry"] = withEntry,
            ["withoutEntry"] = Outcome(() => McpConfigFile.WithoutEntry(content)),
            ["currentEntry"] = Outcome(() => McpConfigFile.CurrentEntry(content) is not null),
        }
    );
}
JsonArray absent = [];
foreach ((string command, string[] arguments) in entries)
{
    absent.Add(
        Outcome(() => McpConfigFile.WithEntry(null, McpConfigFile.Entry(command, arguments)))
    );
}
JsonObject document = new JsonObject
{
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["entries"] = new JsonArray([
        .. entries.Select(e =>
            (JsonNode)
                new JsonObject
                {
                    ["command"] = e.Command,
                    ["args"] = new JsonArray([
                        .. e.Arguments.Select(a => (JsonNode)JsonValue.Create(a)),
                    ]),
                }
        ),
    ]),
    ["absent"] = absent,
    ["cases"] = results,
};
File.WriteAllText(
    args[0],
    document.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{results.Count} cases");
