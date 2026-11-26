// Records how the .NET daemon frames its messages: Framing.Encode, Framing.ReadAsync and
// Framing.Decode of src/Sermofur.Daemon, on every message shape and on forged frames, plus the
// string escaping of its JSON options (UnsafeRelaxedJsonEscaping) over every scalar value.
// Usage: dotnet run framing-oracle.cs -- OUTPUT.json [ESCAPE-TABLE.rs]
#:property PublishAot=false
#:property JsonSerializerIsReflectionEnabledByDefault=true
#:project ../../../src/Sermofur.Daemon/Sermofur.Daemon.csproj
using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;
using System.Text.Json.Nodes;
using Sermofur.Daemon;
using Sermofur.Domain;

JsonSerializerOptions frameOptions = new()
{
    DefaultIgnoreCondition = System.Text.Json.Serialization.JsonIgnoreCondition.WhenWritingNull,
    Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
};

// Strings of the golden: a JSON string when well formed, else the UTF-16 units; a long run of
// one character is written as {repeat, count}.
JsonNode? Text(string? value)
{
    if (value is null)
    {
        return null;
    }
    if (value.Length > 64 && value.All(c => c == value[0]))
    {
        return new JsonObject { ["repeat"] = value[0].ToString(), ["count"] = value.Length };
    }
    try
    {
        new UTF8Encoding(false, true).GetByteCount(value);
        return JsonValue.Create(value);
    }
    catch (EncoderFallbackException)
    {
        return new JsonObject
        {
            ["utf16"] = new JsonArray([.. value.Select(c => (JsonNode)(int)c)]),
        };
    }
}

JsonObject Describe(IpcMessage m)
{
    JsonObject o = new();
    void Put(string name, JsonNode? value)
    {
        if (value is not null)
        {
            o[name] = value;
        }
    }
    // Kind is written even when null, so that a decoded null kind would show.
    o["kind"] = Text(m.Kind);
    Put("protocol", m.Protocol);
    Put("toolVersion", Text(m.ToolVersion));
    Put("cwd", Text(m.Cwd));
    Put("pid", m.Pid);
    Put("id", m.Id);
    if (m.Argv is not null)
    {
        o["argv"] = new JsonArray([.. m.Argv.Select(Text)]);
    }
    Put("exitCode", m.ExitCode);
    Put("stdout", Text(m.Stdout));
    Put("stderr", Text(m.Stderr));
    Put("code", Text(m.Code));
    Put("message", Text(m.Message));
    Put("startedAt", Text(m.StartedAt));
    Put("instancesOpen", m.InstancesOpen);
    Put("clients", m.Clients);
    Put("registry", Text(m.Registry));
    return o;
}

JsonObject Failure(Exception exception) =>
    exception is SermofurException s
        ? new JsonObject
        {
            ["code"] = s.Code,
            ["message"] = s.Message,
            ["exitCode"] = s.ExitCode,
        }
        : new JsonObject { ["unhandled"] = exception.GetType().FullName };

JsonObject Bytes(byte[] bytes) =>
    bytes.Length <= 4096
        ? new JsonObject { ["base64"] = Convert.ToBase64String(bytes) }
        : new JsonObject
        {
            ["length"] = bytes.Length,
            ["sha256"] = Convert.ToHexString(SHA256.HashData(bytes)),
        };

// ---------------------------------------------------------------- encode
JsonArray encodes = [];
void Encode(string name, IpcMessage message, int? limit = null)
{
    JsonObject entry = new()
    {
        ["name"] = name,
        ["message"] = Describe(message),
        ["limit"] = limit ?? Framing.MaxBodyBytes,
    };
    try
    {
        byte[] frame = limit is null
            ? Framing.Encode(message)
            : Framing.Encode(message, limit.Value);
        entry["frame"] = Bytes(frame);
    }
    catch (Exception exception)
    {
        entry["error"] = Failure(exception);
    }
    encodes.Add(entry);
}

string specials =
    "<>&'+`\"\\/ é 中文 \u00a0\u00ad\u0085\u007f\u2028\u2029\ufeff\ufffd\uffff\u0378\ue000 \U0001F600 \U0010FFFF";
string controls = new([.. Enumerable.Range(0, 32).Select(i => (char)i)]);
Encode("status_only", new IpcMessage { Kind = MessageKind.Status });
foreach (
    string kind in new[]
    {
        MessageKind.Hello,
        MessageKind.Welcome,
        MessageKind.Run,
        MessageKind.Result,
        MessageKind.NotServing,
        MessageKind.Status,
        MessageKind.Shutdown,
        MessageKind.Stopping,
        MessageKind.Error,
    }
)
{
    Encode("kind_" + kind, new IpcMessage { Kind = kind });
}
Encode(
    "hello",
    new IpcMessage
    {
        Kind = MessageKind.Hello,
        Protocol = IpcMessage.CurrentProtocol,
        ToolVersion = "0.4.3",
        Cwd = @"C:\Users\x\proj é",
        Pid = 4242,
    }
);
Encode(
    "every_field",
    new IpcMessage
    {
        Kind = "result",
        Protocol = 1,
        ToolVersion = "v",
        Cwd = "/tmp/a b",
        Pid = 1,
        Id = 7,
        Argv = ["claim", "add", "é -- \"quoted\"", ""],
        ExitCode = 0,
        Stdout = "{\n  \"a\": 1\n}\n",
        Stderr = "",
        Code = "c",
        Message = "m",
        StartedAt = "2026-11-09T10:00:00.0000000+01:00",
        InstancesOpen = 2,
        Clients = 3,
        Registry = @"C:\state\instances.json",
    }
);
Encode("failure_with_id", IpcMessage.Failure("daemon_busy", "Busy.", 9));
Encode("failure_without_id", IpcMessage.Failure("protocol_error", "Malformed message."));
Encode(
    "number_edges_max",
    new IpcMessage
    {
        Kind = "n",
        Protocol = int.MaxValue,
        Pid = int.MaxValue,
        Id = long.MaxValue,
        ExitCode = int.MaxValue,
        InstancesOpen = int.MaxValue,
        Clients = int.MaxValue,
    }
);
Encode(
    "number_edges_min",
    new IpcMessage
    {
        Kind = "n",
        Protocol = int.MinValue,
        Pid = int.MinValue,
        Id = long.MinValue,
        ExitCode = int.MinValue,
        InstancesOpen = int.MinValue,
        Clients = int.MinValue,
    }
);
Encode(
    "number_zero",
    new IpcMessage
    {
        Kind = "n",
        Protocol = 0,
        Pid = 0,
        Id = 0,
        ExitCode = -1,
    }
);
Encode(
    "empty_strings",
    new IpcMessage
    {
        Kind = "x",
        Cwd = "",
        Stdout = "",
        Argv = [],
    }
);
Encode("argv_empty_items", new IpcMessage { Kind = "run", Argv = ["", "", ""] });
Encode("argv_null_item", new IpcMessage { Kind = "run", Argv = ["a", null!, "b"] });
Encode(
    "specials",
    new IpcMessage
    {
        Kind = "run",
        Argv = [specials],
        Stdout = specials,
    }
);
Encode(
    "controls",
    new IpcMessage
    {
        Kind = "run",
        Stdout = controls,
        Argv = [controls],
    }
);

// A .NET string can hold a lone surrogate; a Rust String cannot.
Encode("lone_high_surrogate_in_stdout", new IpcMessage { Kind = "run", Stdout = "a\uD800b" });
Encode("lone_low_surrogate_in_argv", new IpcMessage { Kind = "run", Argv = ["a\uDC00b"] });
Encode("empty_kind", new IpcMessage { Kind = "" });
string padding = new('a', Framing.MaxBodyBytes - "{\"kind\":\"\"}".Length);
Encode("at_limit", new IpcMessage { Kind = padding });
Encode("one_over_limit", new IpcMessage { Kind = padding + "a" });
string accents = new('é', (Framing.MaxBodyBytes - "{\"kind\":\"\"}".Length) / 2);
Encode("two_byte_chars_at_limit", new IpcMessage { Kind = accents });
Encode("two_byte_chars_over_limit", new IpcMessage { Kind = accents + "é" });
Encode("escapes_count_in_limit", new IpcMessage { Kind = new string('\n', 131_068) });
Encode("small_limit_refused", new IpcMessage { Kind = MessageKind.Status }, 16);
Encode("small_limit_exact", new IpcMessage { Kind = MessageKind.Status }, 17);
string response = new(
    'r',
    Framing.MaxResponseBytes - "{\"kind\":\"result\",\"stdout\":\"\"}".Length
);
Encode(
    "response_at_limit",
    new IpcMessage { Kind = MessageKind.Result, Stdout = response },
    Framing.MaxResponseBytes
);
Encode(
    "response_over_limit",
    new IpcMessage { Kind = MessageKind.Result, Stdout = response + "r" },
    Framing.MaxResponseBytes
);
Encode(
    "response_over_request_limit",
    new IpcMessage { Kind = MessageKind.Result, Stdout = new string('r', Framing.MaxBodyBytes) }
);

// ---------------------------------------------------------------- decode
// Input streams are lists of segments: {base64} or {repeat: byte, count}.
JsonArray decodes = [];
async Task Read(
    string name,
    List<(byte[]? Bytes, byte Repeat, int Count)> segments,
    int? limit = null
)
{
    List<byte> all = [];
    JsonArray described = [];
    foreach ((byte[]? bytes, byte repeat, int count) in segments)
    {
        if (bytes is not null)
        {
            all.AddRange(bytes);
            described.Add(new JsonObject { ["base64"] = Convert.ToBase64String(bytes) });
        }
        else
        {
            all.AddRange(Enumerable.Repeat(repeat, count));
            described.Add(new JsonObject { ["repeat"] = (int)repeat, ["count"] = count });
        }
    }
    JsonArray outcomes = [];
    using MemoryStream stream = new([.. all]);
    for (int i = 0; i < 8; i++)
    {
        try
        {
            IpcMessage? message = limit is null
                ? await Framing.ReadAsync(stream, CancellationToken.None)
                : await Framing.ReadAsync(stream, limit.Value, CancellationToken.None);
            if (message is null)
            {
                outcomes.Add(new JsonObject { ["end"] = true });
                break;
            }
            outcomes.Add(new JsonObject { ["message"] = Describe(message) });
        }
        catch (Exception exception)
        {
            outcomes.Add(new JsonObject { ["error"] = Failure(exception) });
            break;
        }
    }
    decodes.Add(
        new JsonObject
        {
            ["name"] = name,
            ["limit"] = limit ?? Framing.MaxBodyBytes,
            ["input"] = described,
            ["consumed"] = stream.Position,
            ["outcomes"] = outcomes,
        }
    );
}

(byte[]?, byte, int) Raw(params byte[] bytes) => (bytes, 0, 0);
(byte[]?, byte, int) Fill(byte value, int count) => (null, value, count);
byte[] Header(uint length)
{
    byte[] header = new byte[4];
    BinaryPrimitives.WriteUInt32LittleEndian(header, length);
    return header;
}
byte[] Framed(byte[] body) => [.. Header((uint)body.Length), .. body];
byte[] Utf8(string text) => Encoding.UTF8.GetBytes(text);

// Bodies, each in one well-formed frame.
Dictionary<string, byte[]> bodies = new()
{
    ["not_json"] = Utf8("not json"),
    ["empty_object"] = Utf8("{}"),
    ["empty_kind"] = Utf8("{\"kind\":\"\"}"),
    ["numeric_kind"] = Utf8("{\"kind\":3}"),
    ["array"] = Utf8("[]"),
    ["invalid_utf8"] = [.. Utf8("{\"kind\":\""), 0xC3, 0x28, .. Utf8("\"}")],
    ["unknown_fields"] = Utf8("{\"kind\":\"status\",\"future\":{\"x\":1}}"),
    ["null"] = Utf8("null"),
    ["string_document"] = Utf8("\"status\""),
    ["number_document"] = Utf8("3"),
    ["true_document"] = Utf8("true"),
    ["null_kind"] = Utf8("{\"kind\":null}"),
    ["whitespace_around"] = Utf8(" \t\r\n{ \"kind\" : \"status\" } \r\n\t "),
    ["form_feed"] = Utf8("\f{\"kind\":\"status\"}"),
    ["bom"] = [0xEF, 0xBB, 0xBF, .. Utf8("{\"kind\":\"status\"}")],
    ["trailing_content"] = Utf8("{\"kind\":\"status\"} x"),
    ["two_documents"] = Utf8("{\"kind\":\"a\"}{\"kind\":\"b\"}"),
    ["trailing_nul"] = [.. Utf8("{\"kind\":\"status\"}"), 0],
    ["trailing_comma"] = Utf8("{\"kind\":\"status\",}"),
    ["comment"] = Utf8("{\"kind\":\"status\"/* c */}"),
    ["line_comment"] = Utf8("{\"kind\":\"status\"}// c"),
    ["single_quotes"] = Utf8("{'kind':'status'}"),
    ["unquoted_name"] = Utf8("{kind:\"status\"}"),
    ["unknown_kind"] = Utf8("{\"kind\":\"bogus\"}"),
    ["wrong_protocol"] = Utf8("{\"kind\":\"hello\",\"protocol\":2,\"toolVersion\":\"9.9.9\"}"),
    ["duplicate_kind_last_wins"] = Utf8("{\"kind\":\"a\",\"kind\":\"b\"}"),
    ["duplicate_kind_then_null"] = Utf8("{\"kind\":\"a\",\"kind\":null}"),
    ["duplicate_kind_then_number"] = Utf8("{\"kind\":\"a\",\"kind\":3}"),
    ["duplicate_kind_number_first"] = Utf8("{\"kind\":3,\"kind\":\"a\"}"),
    ["duplicate_id"] = Utf8("{\"kind\":\"a\",\"id\":1,\"id\":2}"),
    ["duplicate_argv"] = Utf8("{\"kind\":\"a\",\"argv\":[\"x\"],\"argv\":null}"),
    ["kind_case_upper"] = Utf8("{\"Kind\":\"status\"}"),
    ["kind_case_all"] = Utf8("{\"KIND\":\"status\"}"),
    ["kind_both_cases"] = Utf8("{\"Kind\":\"x\",\"kind\":\"status\"}"),
    ["escaped_name"] = Utf8("{\"\\u006bind\":\"status\"}"),
    ["every_field"] = Utf8(
        "{\"kind\":\"result\",\"protocol\":1,\"toolVersion\":\"v\",\"cwd\":\"/a\",\"pid\":1,\"id\":7,\"argv\":[\"a\",\"\"],\"exitCode\":0,\"stdout\":\"o\",\"stderr\":\"e\",\"code\":\"c\",\"message\":\"m\",\"startedAt\":\"s\",\"instancesOpen\":2,\"clients\":3,\"registry\":\"r\"}"
    ),
    ["fields_reordered"] = Utf8("{\"id\":5,\"argv\":[],\"kind\":\"run\"}"),
    ["explicit_nulls"] = Utf8(
        "{\"kind\":\"x\",\"protocol\":null,\"toolVersion\":null,\"pid\":null,\"id\":null,\"argv\":null,\"registry\":null}"
    ),
    ["int_max"] = Utf8("{\"kind\":\"x\",\"protocol\":2147483647,\"pid\":-2147483648}"),
    ["int_overflow"] = Utf8("{\"kind\":\"x\",\"protocol\":2147483648}"),
    ["int_underflow"] = Utf8("{\"kind\":\"x\",\"pid\":-2147483649}"),
    ["long_max"] = Utf8("{\"kind\":\"x\",\"id\":9223372036854775807}"),
    ["long_min"] = Utf8("{\"kind\":\"x\",\"id\":-9223372036854775808}"),
    ["long_overflow"] = Utf8("{\"kind\":\"x\",\"id\":9223372036854775808}"),
    ["minus_zero"] = Utf8("{\"kind\":\"x\",\"id\":-0,\"pid\":-0}"),
    ["fraction"] = Utf8("{\"kind\":\"x\",\"id\":1.0}"),
    ["exponent"] = Utf8("{\"kind\":\"x\",\"exitCode\":1e2}"),
    ["leading_zero"] = Utf8("{\"kind\":\"x\",\"id\":01}"),
    ["number_as_string"] = Utf8("{\"kind\":\"x\",\"protocol\":\"1\"}"),
    ["bool_as_number"] = Utf8("{\"kind\":\"x\",\"clients\":true}"),
    ["string_as_number"] = Utf8("{\"kind\":\"x\",\"stdout\":1}"),
    ["argv_string"] = Utf8("{\"kind\":\"run\",\"argv\":\"x\"}"),
    ["argv_number_item"] = Utf8("{\"kind\":\"run\",\"argv\":[1]}"),
    ["argv_null_item"] = Utf8("{\"kind\":\"run\",\"argv\":[\"a\",null]}"),
    ["argv_nested"] = Utf8("{\"kind\":\"run\",\"argv\":[[\"a\"]]}"),
    ["argv_object"] = Utf8("{\"kind\":\"run\",\"argv\":{}}"),
    ["unicode_raw"] = Utf8("{\"kind\":\"run\",\"argv\":[\"é 中文 😀 \u2028 \ufeff \uffff\"]}"),
    ["unicode_escaped"] = Utf8(
        "{\"kind\":\"run\",\"argv\":[\"\\u00e9\\uD83D\\uDE00\\u0000\\/\\b\\f\\n\\r\\t\\\"\\\\\"]}"
    ),
    ["lone_high_surrogate_in_kind"] = Utf8("{\"kind\":\"\\uD800\"}"),
    ["lone_low_surrogate_in_argv"] = Utf8("{\"kind\":\"run\",\"argv\":[\"\\uDC00\"]}"),
    ["reversed_surrogates"] = Utf8("{\"kind\":\"run\",\"stdout\":\"\\uDE00\\uD83D\"}"),
    ["lone_surrogate_in_unknown_value"] = Utf8("{\"kind\":\"status\",\"x\":\"\\uD800\"}"),
    ["lone_surrogate_in_unknown_name"] = Utf8("{\"kind\":\"status\",\"\\uD800\":1}"),
    ["raw_control_in_string"] = Utf8("{\"kind\":\"a\u0001\"}"),
    ["raw_tab_in_string"] = Utf8("{\"kind\":\"a\tb\"}"),
    ["invalid_escape"] = Utf8("{\"kind\":\"\\x41\"}"),
    ["short_unicode_escape"] = Utf8("{\"kind\":\"\\u41\"}"),
    ["overlong_utf8"] = [.. Utf8("{\"kind\":\""), 0xC0, 0x80, .. Utf8("\"}")],
    ["encoded_surrogate_utf8"] = [.. Utf8("{\"kind\":\""), 0xED, 0xA0, 0x80, .. Utf8("\"}")],
    ["byte_f5"] = [.. Utf8("{\"kind\":\""), 0xF5, 0x80, 0x80, 0x80, .. Utf8("\"}")],
    ["truncated_utf8"] = [.. Utf8("{\"kind\":\""), 0xE2, 0x82, .. Utf8("\"}")],
    ["noncharacter_utf8"] = [.. Utf8("{\"kind\":\""), 0xEF, 0xBF, 0xBF, .. Utf8("\"}")],
    ["invalid_utf8_in_unknown_value"] =
    [
        .. Utf8("{\"kind\":\"status\",\"x\":\""),
        0xFF,
        .. Utf8("\"}"),
    ],
    ["big_number_unknown"] = Utf8("{\"kind\":\"status\",\"x\":1e400,\"y\":-0.0E-0}"),
    ["literals_unknown"] = Utf8("{\"kind\":\"status\",\"x\":[true,false,null]}"),
    ["bad_literal_unknown"] = Utf8("{\"kind\":\"status\",\"x\":tru}"),
    ["unclosed_object"] = Utf8("{\"kind\":\"status\""),
    ["missing_colon"] = Utf8("{\"kind\" \"status\"}"),
};
for (int depth = 62; depth <= 66; depth++)
{
    // The object is depth 1, the value of "x" opens depth 2.
    bodies[$"depth_{depth}"] = Utf8(
        "{\"kind\":\"status\",\"x\":"
            + new string('[', depth - 1)
            + new string(']', depth - 1)
            + "}"
    );
}
bodies["depth_65_objects"] = Utf8(
    "{\"kind\":\"status\",\"x\":"
        + string.Concat(Enumerable.Repeat("{\"a\":", 64))
        + "1"
        + new string('}', 64)
        + "}"
);
foreach ((string name, byte[] body) in bodies)
{
    await Read("body_" + name, [Raw(Framed(body))]);
}

// Frames.
await Read("empty_stream", []);
await Read("empty_frame", [Raw(0, 0, 0, 0)]);
await Read("header_one_byte", [Raw(5)]);
await Read("header_two_bytes", [Raw(5, 0)]);
await Read("header_three_bytes", [Raw(5, 0, 0)]);
await Read("body_truncated", [Raw(5, 0, 0, 0, (byte)'{')]);
await Read("body_missing", [Raw(Header(17))]);
await Read("oversized_header", [Raw(Header(Framing.MaxBodyBytes + 1))]);
await Read(
    "oversized_header_with_body",
    [Raw(Header(Framing.MaxBodyBytes + 1)), Fill((byte)'a', 64)]
);
await Read("max_header", [Raw(0xFF, 0xFF, 0xFF, 0xFF)]);
await Read("high_bit_header", [Raw(0, 0, 0, 0x80)]);
await Read("header_at_limit_without_body", [Raw(Header(Framing.MaxBodyBytes))]);
byte[] status = Utf8("{\"kind\":\"status\"}");
await Read("two_frames", [Raw(Framed(status)), Raw(Framed(Utf8("{\"kind\":\"run\",\"id\":1}")))]);
await Read("frame_then_partial_header", [Raw(Framed(status)), Raw(1, 0)]);
await Read("frame_then_empty_frame", [Raw(Framed(status)), Raw(0, 0, 0, 0)]);
await Read("frame_then_bad_body", [Raw(Framed(status)), Raw(Framed(Utf8("{}")))]);
await Read("extra_bytes_after_body", [Raw(Header(2)), Raw(Utf8("{}x"))]);
await Read("short_length_cuts_json", [Raw(Header(5)), Raw(status)]);
byte[] atLimit = Utf8("{\"kind\":\"" + padding + "\"}");
await Read(
    "body_at_limit",
    [
        Raw(Header((uint)atLimit.Length)),
        Raw(Utf8("{\"kind\":\"")),
        Fill((byte)'a', padding.Length),
        Raw(Utf8("\"}")),
    ]
);
await Read(
    "body_one_over_limit",
    [
        Raw(Header((uint)atLimit.Length + 1)),
        Raw(Utf8("{\"kind\":\"")),
        Fill((byte)'a', padding.Length + 1),
        Raw(Utf8("\"}")),
    ]
);
await Read("small_limit_refused", [Raw(Framed(status))], 16);
await Read("small_limit_exact", [Raw(Framed(status))], 17);
await Read(
    "response_limit_accepts_larger",
    [
        Raw(Header((uint)atLimit.Length + 1)),
        Raw(Utf8("{\"kind\":\"")),
        Fill((byte)'a', padding.Length + 1),
        Raw(Utf8("\"}")),
    ],
    Framing.MaxResponseBytes
);
await Read(
    "response_limit_oversized_header",
    [Raw(Header(Framing.MaxResponseBytes + 1))],
    Framing.MaxResponseBytes
);
await Read(
    "response_limit_header_at_limit_without_body",
    [Raw(Header(Framing.MaxResponseBytes))],
    Framing.MaxResponseBytes
);

// ---------------------------------------------------------------- escaping
string EscapeString(string value) => JsonSerializer.Serialize(value, frameOptions)[1..^1];
JsonArray escapeSpecials = [];
List<(int Start, int End)> ranges = [];
for (int cp = 0; cp <= 0xFFFF; cp++)
{
    if (cp is >= 0xD800 and <= 0xDFFF)
    {
        continue;
    }
    string text = ((char)cp).ToString();
    string escaped = EscapeString(text);
    if (escaped == text)
    {
        continue;
    }
    if (escaped == $"\\u{cp:X4}")
    {
        if (ranges.Count > 0 && ranges[^1].End == cp - 1)
        {
            ranges[^1] = (ranges[^1].Start, cp);
        }
        else
        {
            ranges.Add((cp, cp));
        }
    }
    else
    {
        escapeSpecials.Add(new JsonArray(cp, escaped));
    }
}
JsonArray supplementary = [];
for (int cp = 0x10000; cp <= 0x10FFFF; cp += 4099)
{
    supplementary.Add(new JsonArray(cp, EscapeString(char.ConvertFromUtf32(cp))));
}
supplementary.Add(new JsonArray(0x10FFFF, EscapeString(char.ConvertFromUtf32(0x10FFFF))));

JsonObject document = new()
{
    ["note"] =
        "Generated by rust/tools/framing-oracle from src/Sermofur.Daemon (Framing, IpcMessage). Do not edit by hand.",
    ["maxBodyBytes"] = Framing.MaxBodyBytes,
    ["maxResponseBytes"] = Framing.MaxResponseBytes,
    ["currentProtocol"] = IpcMessage.CurrentProtocol,
    ["kinds"] = new JsonArray(
        MessageKind.Hello,
        MessageKind.Welcome,
        MessageKind.Run,
        MessageKind.Result,
        MessageKind.NotServing,
        MessageKind.Status,
        MessageKind.Shutdown,
        MessageKind.Stopping,
        MessageKind.Error
    ),
    ["escape"] = new JsonObject
    {
        ["specials"] = escapeSpecials,
        ["unicodeEscapedRanges"] = new JsonArray([
            .. ranges.Select(r => (JsonNode)new JsonArray(r.Start, r.End)),
        ]),
        ["supplementarySamples"] = supplementary,
    },
    ["encode"] = encodes,
    ["decode"] = decodes,
};
JsonSerializerOptions output = new()
{
    WriteIndented = true,
    NewLine = "\n",
    Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
};
File.WriteAllText(args[0], document.ToJsonString(output) + "\n", new UTF8Encoding(false));
if (args.Length > 1)
{
    string table =
        "// Generated by rust/tools/framing-oracle from the .NET JavaScriptEncoder.UnsafeRelaxedJsonEscaping.\n"
        + "// Do not edit by hand: run the tool again after a change of .NET version.\n"
        + "//\n"
        + "// Inclusive ranges of Basic Multilingual Plane scalars that the daemon frames write as\n"
        + "// `\\uXXXX` (uppercase hexadecimal). Scalars outside these ranges, and outside the\n"
        + "// short escapes handled by the caller, are written as they are.\n\n"
        + "pub(crate) const UNICODE_ESCAPED_RANGES: &[(u32, u32)] = &[\n"
        + string.Join("\n", ranges.Select(r => $"    ({r.Start}, {r.End}),"))
        + "\n];\n";
    File.WriteAllText(args[1], table.Replace("\r\n", "\n"), new UTF8Encoding(false));
}
Console.WriteLine(
    $"encode: {encodes.Count}; decode: {decodes.Count}; escaped ranges: {ranges.Count}"
);
