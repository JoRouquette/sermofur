// Records what CodexConfigFile of src/Sermofur.Mcp does with crafted Codex config.toml contents:
// WithBlock (for several launch commands), WithoutBlock and Declares, character for character,
// refusals and exceptions included. Usage: dotnet run codex-config-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Mcp/Sermofur.Mcp.csproj
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Mcp;

JsonNode Outcome(Func<object?> run)
{
    try
    {
        return run() switch
        {
            null => new JsonObject { ["null"] = true },
            string text => new JsonObject { ["text"] = text },
            bool flag => new JsonObject { ["declares"] = flag },
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
    ("C:\\it's\\smf.exe", ["mcp", "serve"]),
    ("C:\\Users\\me\\.dotnet\\tools\\smf.exe", ["mcp", "serve"]),
    ("a\nb\"c\\ été 😀", ["x'y", "", "\u2028"]),
    ("smf", []),
];

const string Existing = """
    # personal Codex configuration
    model = "gpt-5"   # comment kept

    [profiles.work]
    approval_policy = "on-request"

    [mcp_servers.github]
    command = "npx"
    args = ["-y", "@modelcontextprotocol/server-github"]
    notes = '''
    [mcp_servers.sermofur]
    not a header, inside a multi-line string
    '''

    [mcp_servers.github.env]
    TOKEN = "x"

    [[hooks]]
    name = "a"
    """;

const string Ours = "[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n";

List<(string Name, string Content)> cases = [];
void Case(string name, string content) => cases.Add((name, content));
string Crlf(string text) => text.Replace("\n", "\r\n");

// CodexConfigFileTests.
Case("existing", Existing + "\n");
Case("existing_no_final", Existing);
Case("existing_crlf", Crlf(Existing + "\n"));
Case("existing_installed", Existing + "\n\n" + Ours);
Case("existing_installed_crlf", Crlf(Existing + "\n\n" + Ours));
Case("middle", "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'old'\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n");
Case("middle_approval", "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\ndefault_tools_approval_mode = \"writes\"\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n");
Case("args_multiline", "[mcp_servers.sermofur]\nargs = [\n  'x',\n]\n");
Case("model_crlf", "model = \"x\"\r\n");
Case("unsafe_inline_table", "[mcp_servers]\nsermofur = { command = \"smf\" }\n");
Case("unsafe_dotted_key", "mcp_servers.sermofur.command = \"smf\"\n");
Case("unsafe_root_inline", "mcp_servers = { sermofur = { command = \"smf\" } }\n");
Case("unsafe_unclosed", "notes = \"\"\"\nnever closed\n");
Case("unsafe_twice", "[mcp_servers.sermofur]\ncommand = 'a'\n[mcp_servers.sermofur]\ncommand = 'b'\n");
Case("array_values", "matrix = [\n  [\"a\", \"b\"],\n  [\"c\"]\n]\n");
foreach (string nl in new[] { "\n", "\r\n" })
{
    string suffix = nl == "\n" ? "lf" : "crlf";
    Case($"comments_above_{suffix}", $"[a]{nl}x = 1{nl}{nl}[mcp_servers.sermofur]{nl}# ours{nl}command = 'smf'{nl}args = ['mcp', 'serve']{nl}{nl}# docs server{nl}# second line{nl}[mcp_servers.docs]{nl}command = 'd'{nl}");
}
Case("no_final_newline", "a = 1");
// Edges.
Case("empty", "");
Case("lf_only", "\n");
Case("crlf_only", "\r\n");
Case("two_lf", "\n\n");
Case("spaces", "   ");
Case("bom_text", "\uFEFFa = 1\n");
Case("bom_block", "\uFEFF" + Ours);
Case("bom_only", "\uFEFF");
Case("ours", Ours);
Case("ours_crlf", Crlf(Ours));
Case("ours_after_blank", "\n" + Ours);
Case("ours_after_blank_crlf", "\r\n" + Crlf(Ours));
Case("ours_no_final", Ours.TrimEnd('\n'));
Case("header_only", "[mcp_servers.sermofur]");
Case("header_only_lf", "[mcp_servers.sermofur]\n");
Case("header_only_crlf", "[mcp_servers.sermofur]\r\n");
Case("ours_different", "[mcp_servers.sermofur]\ncommand = 'other'\nargs = ['x']\n");
Case("ours_args_first", "[mcp_servers.sermofur]\nargs = ['mcp', 'serve']\ncommand = 'smf'\n");
Case("ours_only_args", "[mcp_servers.sermofur]\nargs = ['mcp', 'serve']\n");
Case("ours_command_twice", "[mcp_servers.sermofur]\ncommand = 'a'\ncommand = 'b'\nargs = []\n");
Case("ours_comment_header", "[mcp_servers.sermofur]  # ours\ncommand = 'smf'\n");
Case("ours_spaced_header", "  [ mcp_servers . sermofur ]  \ncommand = 'smf'\n");
Case("ours_quoted_header", "[\"mcp_servers\".'sermofur']\ncommand = 'smf'\nargs = ['mcp', 'serve']\n");
Case("ours_unicode_spaces", "[\u00a0mcp_servers\u3000.sermofur\u2028]\u2009# c\ncommand = 'smf'\n");
Case("ours_array_header", "[[mcp_servers.sermofur]]\ncommand = 'smf'\n");
Case("ours_half_array", "[[mcp_servers.sermofur]\ncommand = 'smf'\n");
Case("ours_bad_tail", "[mcp_servers.sermofur] x\ncommand = 'smf'\n");
Case("ours_extra_bracket", "[mcp_servers.sermofur]]]\n");
Case("ours_mixed_newlines", "a = 1\nb = 2\r\n[mcp_servers.sermofur]\ncommand = 'old'\r\n");
Case("ours_double_cr", "[mcp_servers.sermofur]\r\r\ncommand = 'old'\r\r\n");
Case("ours_cr_in_line", "x = 1\r2\n[mcp_servers.sermofur]\ncommand = 'x'\r\r\nargs = []\n");
Case("ours_sub_only", "[mcp_servers.sermofur.env]\nK = 1\n");
Case("ours_sub_before", "[mcp_servers.sermofur.env]\nK = 1\n\n[mcp_servers.sermofur]\ncommand = 'smf'\n");
Case("ours_sub_and_next", "[mcp_servers.sermofur]\ncommand = 'old'\n[mcp_servers.sermofur.env]\nK = 1\n# about b\n\n# about b too\n[b]\n");
Case("ours_command_in_sub", "[mcp_servers.sermofur]\n[mcp_servers.sermofur.env]\ncommand = 'x'\nargs = [\n");
Case("ours_comment_last", "[mcp_servers.sermofur]\ncommand = 'smf'\n# trailing\n");
Case("ours_comment_then_table", "[mcp_servers.sermofur]\ncommand = 'smf'\n\n# c1\n\n# c2\n[z]\n");
Case("ours_indented_comment", "[mcp_servers.sermofur]\ncommand = 'smf'\n  # c\n[z]\n");
Case("neighbour_prefix", "[mcp_servers.sermofurx]\ncommand = 'x'\n");
Case("neighbour_other", "[mcp_servers.other]\nsermofur = 1\n");
Case("servers_key_sermofur", "[mcp_servers]\n\"sermofur\" = {}\n");
Case("servers_key_other", "[mcp_servers]\nother = 'sermofur'\n");
Case("root_quoted_key", "\"mcp_servers\".sermofur = 1\n");
Case("root_comment_mentions", "mcp_servers = 1 # sermofur\n");
Case("root_value_mentions", "x = 'mcp_servers.sermofur'\n");
Case("root_eq_first", "= mcp_servers.sermofur\n");
Case("header_in_basic_multiline", "s = \"\"\"\n[mcp_servers.sermofur]\n\"\"\"\n");
Case("header_after_close", "s = '''a''' # x\n[mcp_servers.sermofur]\ncommand = 'smf'\n");
Case("quad_quotes", "s = ''''\n[mcp_servers.sermofur]\n'''\n");
Case("escaped_quote", "s = \"a\\\"\"\"\"\n[mcp_servers.sermofur]\n\"\"\"\n");
Case("hash_in_string", "s = \"#\"\"\"\n[x]\n");
Case("unclosed_literal", "s = '''\n[mcp_servers.sermofur]\n");
Case("unicode_text", "été = '😀'\n[mcp_servers.sermofur]\ncommand = 'é\\\\'\n");
Case("escaped_backslash_unicode", "s = \"\\é\"\"\"\nx\n\"\"\"\n");
Case("table_then_ours_no_blank", "[a]\nx = 1\n[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n");
// A launch value in a multi-line string (lot 8.4): WithLaunchLines replaces its first line and
// leaves the rest; each output is recorded again as a case, so Declares on it is measured too.
foreach ((string name, string content) in new[]
{
    ("multiline_command", "[mcp_servers.sermofur]\ncommand=\"\"\"\n\"\"\""),
    ("multiline_command_literal", "[mcp_servers.sermofur]\ncommand = '''\nsmf\n'''\nargs = ['mcp', 'serve']\n"),
    ("multiline_args", "[mcp_servers.sermofur]\ncommand = 'smf'\nargs = [\"\"\"\nmcp\"\"\", 'serve']\n"),
    ("multiline_args_string", "[mcp_servers.sermofur]\ncommand = 'smf'\nargs = '''\nmcp serve\n'''\n"),
})
{
    Case(name, content);
    try
    {
        Case(name + "_installed", CodexConfigFile.WithBlock(content, entries[0].Command, entries[0].Arguments) ?? "");
    }
    catch (SermofurException)
    {
        // Refused: the case above records it.
    }
}
// Shapes of the FsCheck properties.
string Tables(IEnumerable<string> names, string nl, bool comments) =>
    string.Concat(names.Select(name =>
        (comments ? $"# server {name}{nl}" : "")
        + $"[mcp_servers.{name}]{nl}command = '{name}'{nl}"
        + $"[mcp_servers.{name}.env]{nl}K = '{name}'  # kept{nl}{nl}"));
Random random = new Random(20261116);
string Name() => new string([.. Enumerable.Range(0, random.Next(1, 6)).Select(_ => "abcXYZ019"[random.Next(9)])]);
for (int index = 0; index < 32; index++)
{
    string nl = index % 2 == 0 ? "\r\n" : "\n";
    string[] names = [.. Enumerable.Range(0, random.Next(0, 4)).Select(_ => Name()).Distinct()];
    string content = Tables(names, nl, index % 4 < 2);
    if (index % 8 >= 4 && content.Length > 0)
    {
        content = content.TrimEnd('\r', '\n');
    }
    Case($"generated_{index}", content);
    string after = Tables([.. names.Reverse(), "last"], nl, true);
    Case($"generated_middle_{index}", content + $"[mcp_servers.sermofur]{nl}command = 'smf'{nl}args = ['mcp', 'serve']{nl}{nl}[mcp_servers.sermofur.env]{nl}K = 'v'{nl}{nl}" + after);
}

JsonArray results = [];
foreach ((string name, string content) in cases)
{
    JsonArray withBlock = [];
    foreach ((string command, string[] arguments) in entries)
    {
        withBlock.Add(Outcome(() => CodexConfigFile.WithBlock(content, command, arguments)));
    }
    results.Add(new JsonObject
    {
        ["name"] = name,
        ["content"] = content,
        ["withBlock"] = withBlock,
        ["withoutBlock"] = Outcome(() => CodexConfigFile.WithoutBlock(content)),
        ["declares"] = Outcome(() => CodexConfigFile.Declares(content)),
    });
}
JsonArray absent = [];
foreach ((string command, string[] arguments) in entries)
{
    absent.Add(Outcome(() => CodexConfigFile.WithBlock(null, command, arguments)));
}
JsonObject document = new JsonObject
{
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["entries"] = new JsonArray([.. entries.Select(e => (JsonNode)new JsonObject
    {
        ["command"] = e.Command,
        ["args"] = new JsonArray([.. e.Arguments.Select(a => (JsonNode)JsonValue.Create(a))]),
    })]),
    ["absent"] = absent,
    ["cases"] = results,
};
File.WriteAllText(args[0], document.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n");
Console.WriteLine($"{results.Count} cases");
