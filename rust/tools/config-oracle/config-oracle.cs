// Records how the .NET oracle reads instance.json: InstanceManager.ReadConfiguration of
// src/Sermofur.Infrastructure, called on crafted files, with the error mapping of
// src/Sermofur.Cli/CommandRunner.cs (lines 104-126). Usage:
// dotnet run config-oracle.cs -- OUTPUT.json
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Infrastructure/Sermofur.Infrastructure.csproj
using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Domain;
using Sermofur.Infrastructure;

const string Id = "6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b";

static string Config(string body) => "{" + body + "}";

string Valid(
    int schema = 2,
    string id = Id,
    string created = "2026-11-06T11:00:00.1234567+00:00"
) =>
    Config(
        $"\n  \"schemaVersion\": {schema},\n  \"instanceId\": \"{id}\",\n  \"createdAt\": \"{created}\"\n"
    );

byte[] Utf8(string text) => new UTF8Encoding(false).GetBytes(text);

byte[] WithPreamble(Encoding encoding, string text) =>
    [.. encoding.GetPreamble(), .. encoding.GetBytes(text)];

string Nested(int depth) =>
    Config(
        $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"extra\": "
            + new string('[', depth - 1)
            + new string(']', depth - 1)
    );

string Padded(int total)
{
    string text = Valid();
    return text + new string(' ', total - Encoding.UTF8.GetByteCount(text));
}

List<(string Name, byte[]? Content)> cases =
[
    ("written_by_init", Utf8(Valid())),
    ("utf8_bom", WithPreamble(new UTF8Encoding(true), Valid())),
    ("utf16le_bom", WithPreamble(new UnicodeEncoding(false, true), Valid())),
    ("utf16be_bom", WithPreamble(new UnicodeEncoding(true, true), Valid())),
    ("crlf", Utf8(Valid().Replace("\n", "\r\n"))),
    (
        "compact",
        Utf8(
            $"{{\"schemaVersion\":2,\"instanceId\":\"{Id}\",\"createdAt\":\"2026-11-06T11:00:00Z\"}}"
        )
    ),
    (
        "reordered",
        Utf8(
            $"{{\"createdAt\":\"2026-11-06T11:00:00Z\",\"instanceId\":\"{Id}\",\"schemaVersion\":2}}"
        )
    ),
    (
        "extra_property",
        Utf8(
            Config(
                $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"createdAt\": \"2026-11-06T11:00:00Z\", \"laya\": {{\"a\": [1, 2.5, null, true]}}"
            )
        )
    ),
    (
        "pascal_case_names",
        Utf8(
            $"{{\"SchemaVersion\":2,\"InstanceId\":\"{Id}\",\"CreatedAt\":\"2026-11-06T11:00:00Z\"}}"
        )
    ),
    (
        "missing_schema",
        Utf8(Config($"\"instanceId\": \"{Id}\", \"createdAt\": \"2026-11-06T11:00:00Z\""))
    ),
    ("missing_id", Utf8(Config("\"schemaVersion\": 2, \"createdAt\": \"2026-11-06T11:00:00Z\""))),
    ("missing_created", Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\""))),
    ("empty_object", Utf8("{}")),
    ("schema_1", Utf8(Valid(1))),
    ("schema_3", Utf8(Valid(3))),
    ("schema_0", Utf8(Valid(0))),
    ("schema_negative", Utf8(Valid(-1))),
    ("schema_float", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": 2.0"))),
    ("schema_exponent", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": 2e0"))),
    ("schema_string", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": \"2\""))),
    (
        "schema_too_large",
        Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": 99999999999"))
    ),
    ("schema_null", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": null"))),
    ("schema_leading_zero", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": 02"))),
    ("id_empty_guid", Utf8(Valid(id: "00000000-0000-0000-0000-000000000000"))),
    ("id_uppercase", Utf8(Valid(id: Id.ToUpperInvariant()))),
    ("id_format_n", Utf8(Valid(id: Id.Replace("-", "")))),
    ("id_format_b", Utf8(Valid(id: "{" + Id + "}"))),
    ("id_format_p", Utf8(Valid(id: "(" + Id + ")"))),
    ("id_invalid", Utf8(Valid(id: "not-a-guid"))),
    ("id_null", Utf8(Valid().Replace($"\"{Id}\"", "null"))),
    ("id_number", Utf8(Valid().Replace($"\"{Id}\"", "42"))),
    ("id_spaces", Utf8(Valid(id: " " + Id + " "))),
    ("created_zulu", Utf8(Valid(created: "2026-11-06T11:00:00Z"))),
    ("created_offset", Utf8(Valid(created: "2026-11-06T12:00:00+01:00"))),
    ("created_no_offset", Utf8(Valid(created: "2026-11-06T11:00:00"))),
    ("created_invalid", Utf8(Valid(created: "yesterday"))),
    ("created_null", Utf8(Valid().Replace("\"2026-11-06T11:00:00.1234567+00:00\"", "null"))),
    ("created_number", Utf8(Valid().Replace("\"2026-11-06T11:00:00.1234567+00:00\"", "0"))),
    (
        "duplicate_schema_1_then_2",
        Utf8(Config($"\"schemaVersion\": 1, \"schemaVersion\": 2, \"instanceId\": \"{Id}\""))
    ),
    (
        "duplicate_schema_2_then_1",
        Utf8(Config($"\"schemaVersion\": 2, \"schemaVersion\": 1, \"instanceId\": \"{Id}\""))
    ),
    (
        "duplicate_extra",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 1, \"x\": 2"))
    ),
    ("top_level_null", Utf8("null")),
    ("top_level_array", Utf8("[]")),
    ("top_level_string", Utf8("\"x\"")),
    ("empty_file", Utf8("")),
    ("whitespace_only", Utf8("  \n ")),
    ("comment", Utf8("// note\n" + Valid())),
    ("block_comment", Utf8(Valid().Replace("{", "{ /* c */"))),
    ("trailing_comma", Utf8(Valid().Replace("\n}", ",\n}"))),
    ("trailing_content", Utf8(Valid() + " x")),
    ("two_objects", Utf8(Valid() + Valid())),
    ("single_quotes", Utf8($"{{'schemaVersion': 2, 'instanceId': '{Id}'}}")),
    ("unquoted_name", Utf8($"{{schemaVersion: 2, \"instanceId\": \"{Id}\"}}")),
    ("depth_64", Utf8(Nested(64))),
    ("depth_65", Utf8(Nested(65))),
    (
        "invalid_utf8_in_extra",
        [
            .. Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"")),
            0xC3,
            0x28,
            .. Utf8("\"}"),
        ]
    ),
    (
        "lone_surrogate_escape_in_extra",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"\\ud800\""))
    ),
    (
        "control_char_in_string",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"a\tb\""))
    ),
    ("escaped_name", Utf8($"{{\"schema\\u0056ersion\":2,\"instanceId\":\"{Id}\"}}")),
    ("size_65536", Utf8(Padded(65536))),
    ("size_65537", Utf8(Padded(65537))),
    ("missing_file", null),
    ("instance_json_is_directory", null),
    ("utf32le_bom", WithPreamble(new UTF32Encoding(false, true), Valid())),
    ("utf32be_bom", WithPreamble(new UTF32Encoding(true, true), Valid())),
    ("utf16le_odd_tail", [.. WithPreamble(new UnicodeEncoding(false, true), Valid()), 0x20]),
    ("utf16be_odd_tail", [.. WithPreamble(new UnicodeEncoding(true, true), Valid()), 0x20]),
    ("utf32le_tail_1", [.. WithPreamble(new UTF32Encoding(false, true), Valid()), 0x20]),
    (
        "utf32le_tail_3",
        [.. WithPreamble(new UTF32Encoding(false, true), Valid()), 0x20, 0x00, 0x00]
    ),
    ("utf8_bom_only", [0xEF, 0xBB, 0xBF]),
    ("utf16le_bom_only", [0xFF, 0xFE]),
    ("double_utf8_bom", [0xEF, 0xBB, 0xBF, .. WithPreamble(new UTF8Encoding(true), Valid())]),
    ("utf16_without_bom", new UnicodeEncoding(false, false).GetBytes(Valid())),
    (
        "utf16le_lone_high_unit_in_extra",
        [
            .. WithPreamble(
                new UnicodeEncoding(false, true),
                Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"")
            ),
            0x00,
            0xD8,
            .. new UnicodeEncoding(false, false).GetBytes("\"}"),
        ]
    ),
    (
        "nul_byte_inside_extra",
        [
            .. Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"a")),
            0x00,
            .. Utf8("\"}"),
        ]
    ),
    ("nul_byte_after_object", [.. Utf8(Valid()), 0x00]),
    (
        "surrogate_in_root_name",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\\ud800\": 1"))
    ),
    (
        "surrogate_in_nested_name",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"a\": {{\"\\udc00\": 1}}"))
    ),
    (
        "surrogate_pair_bad_low",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"\\ud800\\u0041\""))
    ),
    (
        "short_hex_escape",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": \"\\ud800\\u12\""))
    ),
    ("escaped_digit_in_id", Utf8(Valid(id: "\\u0036" + Id[1..]))),
    ("escaped_z_in_created", Utf8(Valid(created: "2026-11-06T11:00:00\\u005a"))),
    (
        "schema_name_fully_escaped",
        Utf8($"{{\"\\u0073\\u0063\\u0068\\u0065\\u006d\\u0061Version\":2,\"instanceId\":\"{Id}\"}}")
    ),
    (
        "skipped_number_1dot",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 1."))
    ),
    (
        "skipped_number_dot5",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": .5"))
    ),
    (
        "skipped_number_plus",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": +1"))
    ),
    (
        "skipped_number_minus_zero",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": -0"))
    ),
    (
        "skipped_number_leading_zero",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 01"))
    ),
    (
        "skipped_number_bare_exponent",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 1e"))
    ),
    (
        "skipped_number_huge",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 1e400"))
    ),
    (
        "skipped_number_long",
        Utf8(
            Config(
                $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": 123456789012345678901234567890"
            )
        )
    ),
    ("skipped_nan", Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": NaN"))),
    ("schema_minus_zero", Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": -0"))),
    (
        "schema_int_min",
        Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": -2147483648"))
    ),
    (
        "schema_int_max_plus_1",
        Utf8(Valid().Replace("\"schemaVersion\": 2", "\"schemaVersion\": 2147483648"))
    ),
    (
        "depth_objects_64",
        Utf8(
            Config(
                $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": "
                    + string.Concat(Enumerable.Repeat("{\"a\":", 62))
                    + "1"
                    + new string('}', 62)
            )
        )
    ),
    (
        "depth_objects_65",
        Utf8(
            Config(
                $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"x\": "
                    + string.Concat(Enumerable.Repeat("{\"a\":", 63))
                    + "1"
                    + new string('}', 63)
            )
        )
    ),
    ("many_open_brackets", Utf8(new string('[', 60000))),
    ("null_then_garbage", Utf8("null x")),
    ("true_alone", Utf8("true")),
    ("double_comma", Utf8(Config($"\"schemaVersion\": 2,, \"instanceId\": \"{Id}\""))),
    ("missing_colon", Utf8(Config($"\"schemaVersion\" 2, \"instanceId\": \"{Id}\""))),
    ("empty_key", Utf8(Config($"\"\": 1, \"schemaVersion\": 2, \"instanceId\": \"{Id}\""))),
    (
        "duplicate_id_good_then_bad",
        Utf8(Config($"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"instanceId\": \"bad\""))
    ),
    (
        "duplicate_created_bad_then_good",
        Utf8(
            Config(
                $"\"schemaVersion\": 2, \"instanceId\": \"{Id}\", \"createdAt\": \"bad\", \"createdAt\": \"2026-11-06T11:00:00Z\""
            )
        )
    ),
    (
        "duplicate_schema_null_then_2",
        Utf8(Config($"\"schemaVersion\": null, \"schemaVersion\": 2, \"instanceId\": \"{Id}\""))
    ),
    ("form_feed_whitespace", Utf8(Valid().Replace("\n", "\f"))),
    ("nbsp_whitespace", Utf8(Valid().Replace("\n", "\u00A0"))),
    ("bom_char_in_middle", Utf8(Valid().Replace("\n", "\uFEFF"))),
    ("tab_before_colon", Utf8(Valid().Replace("\":", "\"\t:"))),
    ("size_65536_utf16", WithPreamble(new UnicodeEncoding(false, true), Padded(32767))),
];

JsonArray rows = [];
string scratch = Path.Combine(
    Path.GetTempPath(),
    "sermofur-config-oracle-" + Guid.NewGuid().ToString("N")
);
foreach ((string name, byte[]? content) in cases)
{
    string root = Path.Combine(scratch, name);
    Directory.CreateDirectory(Path.Combine(root, ".sermofur"));
    if (name == "instance_json_is_directory")
    {
        Directory.CreateDirectory(Path.Combine(root, ".sermofur", "instance.json"));
    }
    else if (content is not null)
    {
        File.WriteAllBytes(Path.Combine(root, ".sermofur", "instance.json"), content);
    }
    JsonObject row = new()
    {
        ["name"] = name,
        ["content"] = content is null ? null : Convert.ToBase64String(content),
        ["strict"] = Outcome(root, allowLegacy: false),
        ["legacy"] = Outcome(root, allowLegacy: true),
    };
    rows.Add(row);
}
Directory.Delete(scratch, recursive: true);

JsonObject Outcome(string root, bool allowLegacy)
{
    try
    {
        InstanceConfiguration configuration = new InstanceManager().ReadConfiguration(
            root,
            allowLegacy
        );
        return new JsonObject
        {
            ["schemaVersion"] = configuration.SchemaVersion,
            ["instanceId"] = configuration.InstanceId.ToString("D"),
            ["createdAtUtcTicks"] = configuration.CreatedAt.UtcTicks.ToString(
                CultureInfo.InvariantCulture
            ),
            ["createdAtOffsetMinutes"] = (int)configuration.CreatedAt.Offset.TotalMinutes,
            ["localOffsetMinutes"] = (int)
                TimeZoneInfo.Local.GetUtcOffset(configuration.CreatedAt.UtcDateTime).TotalMinutes,
        };
    }
    catch (SermofurException exception)
    {
        return Error(exception.Code, exception.Message, exception.ExitCode);
    }
    catch (Exception exception)
        when (exception
                is JsonException
                    or IOException
                    or UnauthorizedAccessException
                    or ArgumentException
                    or InvalidOperationException
        )
    {
        // CommandRunner.cs:115-126.
        JsonObject error = Error("invalid_storage_or_path", "Invalid path or data; run doctor.", 3);
        error["exception"] = exception.GetType().Name;
        return error;
    }
}

static JsonObject Error(string code, string message, int exit) =>
    new()
    {
        ["code"] = code,
        ["message"] = message,
        ["exitCode"] = exit,
    };

JsonObject result = new()
{
    ["generator"] = "rust/tools/config-oracle",
    ["runtime"] = System.Runtime.InteropServices.RuntimeInformation.FrameworkDescription,
    ["cases"] = rows,
};
File.WriteAllText(
    args[0],
    result.ToJsonString(new JsonSerializerOptions { WriteIndented = true }) + "\n"
);
Console.WriteLine($"{rows.Count} cases");
