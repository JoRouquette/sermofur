//! The JSON-RPC 2.0 messages of `smf mcp serve`, read and written as the stdio transport of the
//! `ModelContextProtocol.Core` 2.2.0 SDK of the oracle does (decision A3: no SDK, no new crate).
//! Every rule here was measured on the oracle; `rust/tests/golden/mcp-serve.json` holds the
//! measures (`rust/tools/mcp-serve-oracle`).
//!
//! Reading a line (`JsonRpcMessage.Converter`, then `StreamServerTransport`):
//! - an object whose last `jsonrpc` is the string `2.0`; `id` (a string, or an integer that fits
//!   64 bits) and `method` (a string) checked at each occurrence, the last one kept; with an id
//!   and a method it is a request, with a method only a notification, with an id and `error`
//!   (an object with an `int` code and a string message) or `result` a response;
//! - anything else is refused: the SDK answers -32700 when it can read an id at the top level of
//!   the line, scanning it from the start (the first `id`, a string or a 64-bit integer, before
//!   any syntax error), and nothing otherwise.
//!
//! Reading the `params` of a handler (`JsonSerializer.Deserialize` with the options of the
//! SDK): property names without ASCII case, each occurrence checked, unknown
//! properties ignored, a required property present (null accepted for a reference); and, at the
//! top level only, two names equal without case refused (the `JsonObject` of the params). A
//! refusal is -32603, "An error occurred.".
//!
//! Writing: compact, with the default encoder of .NET (`JavaScriptEncoder.Default`, Basic Latin
//! only), fields in the order of the SDK.

use std::str::FromStr;

use sermofur_core::json::quoted_basic_latin;
use sermofur_core::stj::{self, Node};

/// `RequestId`: a string or a 64-bit integer.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RequestId {
    Text(String),
    Number(i64),
}

impl RequestId {
    /// The id as the SDK writes it back.
    pub fn to_json(&self) -> String {
        match self {
            Self::Text(text) => quoted_basic_latin(text),
            Self::Number(number) => number.to_string(),
        }
    }

    fn read(node: &Node) -> Option<Self> {
        match node {
            Node::Text {
                text,
                lone_surrogate: false,
            } => Some(Self::Text(text.clone())),
            Node::Number(raw) => integer(raw).map(Self::Number),
            _ => None,
        }
    }
}

/// One line of the client, as the transport classifies it.
#[derive(Debug, PartialEq)]
pub enum Message {
    Request {
        id: RequestId,
        method: String,
        params: Option<Node>,
    },
    Notification {
        method: String,
        params: Option<Node>,
    },
    /// A response or an error of the client: no request of this server waits for one.
    Response,
    /// Refused by the converter; answered with -32700 when the id could be read.
    Invalid(Option<RequestId>),
}

/// Classifies one line; a blank line is `None` (the transport skips it).
pub fn read(line: &str) -> Option<Message> {
    if line.trim().is_empty() {
        return None;
    }
    Some(
        stj::parse(line)
            .ok()
            .and_then(classify)
            .unwrap_or_else(|| Message::Invalid(extract_id(line))),
    )
}

fn classify(node: Node) -> Option<Message> {
    let Node::Object(properties) = node else {
        return None;
    };
    let (mut version, mut id, mut method, mut params) = (None, None, None, None);
    let (mut result, mut error) = (false, false);
    for (name, value) in properties {
        if name.lone_surrogate {
            continue;
        }
        match name.text.as_str() {
            "jsonrpc" => version = Some(text(&value)?),
            "id" => id = Some(RequestId::read(&value)?),
            "method" => method = Some(text(&value)?),
            "params" => params = Some(value),
            "result" => result = true,
            "error" => {
                if !fits(&value, Shape::Object(ERROR_DETAIL)) || value == Node::Null {
                    return None;
                }
                error = true;
            }
            _ => {}
        }
    }
    if version.as_deref() != Some("2.0") {
        return None;
    }
    match (id, method) {
        (Some(id), Some(method)) => Some(Message::Request { id, method, params }),
        (None, Some(method)) => Some(Message::Notification { method, params }),
        (Some(_), None) if error || result => Some(Message::Response),
        _ => None,
    }
}

fn text(node: &Node) -> Option<String> {
    match node {
        Node::Text {
            text,
            lone_surrogate: false,
        } => Some(text.clone()),
        _ => None,
    }
}

/// `Utf8JsonReader.TryGetInt32` or `TryGetInt64` on a number token: an integer literal, without
/// fraction or exponent, in range.
pub(crate) fn integer<T: FromStr>(raw: &str) -> Option<T> {
    if raw.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
        return None;
    }
    raw.parse().ok()
}

/// The first `id` at the top level of a line the converter refused: a string or a 64-bit
/// integer met before any syntax error; `None` otherwise.
fn extract_id(line: &str) -> Option<RequestId> {
    let mut scan = Scan {
        text: line,
        bytes: line.as_bytes(),
        at: 0,
    };
    scan.blank();
    scan.eat(b'{')?;
    scan.blank();
    if scan.peek()? == b'}' {
        return None;
    }
    loop {
        let name = scan.string()?;
        scan.blank();
        scan.eat(b':')?;
        scan.blank();
        if name == "id" {
            return if scan.peek()? == b'"' {
                scan.string().map(RequestId::Text)
            } else {
                integer(scan.number()?).map(RequestId::Number)
            };
        }
        scan.skip(1)?;
        scan.blank();
        scan.eat(b',')?;
        scan.blank();
    }
}

/// A forward reader of JSON tokens with the rules of `Utf8JsonReader`.
struct Scan<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        (self.peek()? == byte).then(|| self.at += 1)
    }

    fn blank(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    /// A string token, unescaped; a lone surrogate (which `GetString` refuses) is `None`.
    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut units: Vec<u16> = Vec::new();
        loop {
            let byte = self.peek()?;
            match byte {
                b'"' => {
                    self.at += 1;
                    return String::from_utf16(&units).ok();
                }
                b'\\' => {
                    self.at += 1;
                    let escaped = self.peek()?;
                    self.at += 1;
                    let unit = match escaped {
                        b'"' => 0x22,
                        b'\\' => 0x5C,
                        b'/' => 0x2F,
                        b'b' => 0x08,
                        b'f' => 0x0C,
                        b'n' => 0x0A,
                        b'r' => 0x0D,
                        b't' => 0x09,
                        b'u' => {
                            let digits =
                                std::str::from_utf8(self.bytes.get(self.at..self.at + 4)?).ok()?;
                            if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
                                return None;
                            }
                            self.at += 4;
                            u16::from_str_radix(digits, 16).ok()?
                        }
                        _ => return None,
                    };
                    units.push(unit);
                }
                0..=0x1F => return None,
                _ => {
                    // One whole scalar of the line: `at` is on a boundary, after an ASCII byte.
                    let scalar = self.text.get(self.at..)?.chars().next()?;
                    self.at += scalar.len_utf8();
                    units.extend(scalar.encode_utf16(&mut [0; 2]).iter());
                }
            }
        }
    }

    /// A number token (RFC 8259), which must end at a delimiter or at the end of the line.
    fn number(&mut self) -> Option<&'a str> {
        let start = self.at;
        let digits = |scan: &mut Self| {
            let from = scan.at;
            while scan.peek().is_some_and(|b| b.is_ascii_digit()) {
                scan.at += 1;
            }
            scan.at > from
        };
        let _ = self.eat(b'-');
        if self.eat(b'0').is_none() && !digits(self) {
            return None;
        }
        if self.eat(b'.').is_some() && !digits(self) {
            return None;
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if !digits(self) {
                return None;
            }
        }
        self.delimited()?;
        self.text.get(start..self.at)
    }

    fn delimited(&self) -> Option<()> {
        match self.peek() {
            None | Some(b' ' | b'\t' | b'\r' | b'\n' | b',' | b'}' | b']') => Some(()),
            Some(_) => None,
        }
    }

    /// Skips one value; `depth` counts the containers around it (64 at most).
    fn skip(&mut self, depth: usize) -> Option<()> {
        match self.peek()? {
            b'"' => self.string().map(drop),
            b'{' | b'[' if depth >= 64 => None,
            open @ (b'{' | b'[') => {
                let close = if open == b'{' { b'}' } else { b']' };
                self.at += 1;
                self.blank();
                if self.eat(close).is_some() {
                    return Some(());
                }
                loop {
                    if open == b'{' {
                        self.string()?;
                        self.blank();
                        self.eat(b':')?;
                        self.blank();
                    }
                    self.skip(depth + 1)?;
                    self.blank();
                    if self.eat(close).is_some() {
                        return Some(());
                    }
                    self.eat(b',')?;
                    self.blank();
                }
            }
            b't' | b'f' | b'n' => {
                let word: &[u8] = match self.peek()? {
                    b't' => b"true",
                    b'f' => b"false",
                    _ => b"null",
                };
                (self.bytes.get(self.at..self.at + word.len())? == word).then_some(())?;
                self.at += word.len();
                self.delimited()
            }
            _ => self.number().map(drop),
        }
    }
}

/// What a property of the SDK types accepts.
#[derive(Clone, Copy)]
pub(crate) enum Shape {
    /// `string`: a string or null.
    Text,
    /// `bool?`: a boolean or null.
    Flag,
    /// `int`: an integer literal that fits 32 bits.
    Int32,
    /// `LoggingLevel` through `JsonStringEnumConverter` (see [`level`]).
    Level,
    /// `RequestId`.
    Id,
    /// `object` or `JsonElement`: anything.
    Any,
    /// A class (or a dictionary, a `JsonObject`, a lenient converter: no property listed): an
    /// object or null.
    Object(&'static [Property]),
    /// `IList<T>`: an array or null.
    List(&'static Shape),
}

pub(crate) struct Property {
    pub name: &'static str,
    pub shape: Shape,
    pub required: bool,
}

const fn optional(name: &'static str, shape: Shape) -> Property {
    Property {
        name,
        shape,
        required: false,
    }
}

const fn required(name: &'static str, shape: Shape) -> Property {
    Property {
        name,
        shape,
        required: true,
    }
}

const ANY_OBJECT: Shape = Shape::Object(&[]);
const ERROR_DETAIL: &[Property] = &[
    required("code", Shape::Int32),
    required("message", Shape::Text),
    optional("data", Shape::Any),
];
const ICON: &[Property] = &[
    required("src", Shape::Text),
    optional("mimeType", Shape::Text),
    optional("sizes", Shape::List(&Shape::Text)),
    optional("theme", Shape::Text),
];
const IMPLEMENTATION: &[Property] = &[
    required("name", Shape::Text),
    optional("title", Shape::Text),
    required("version", Shape::Text),
    optional("description", Shape::Text),
    optional("icons", Shape::List(&Shape::Object(ICON))),
    optional("websiteUrl", Shape::Text),
];
const CLIENT_CAPABILITIES: &[Property] = &[
    optional("experimental", ANY_OBJECT),
    optional(
        "roots",
        Shape::Object(&[optional("listChanged", Shape::Flag)]),
    ),
    optional(
        "sampling",
        Shape::Object(&[
            optional("context", ANY_OBJECT),
            optional("tools", ANY_OBJECT),
        ]),
    ),
    // A converter of its own, measured lenient: `{"form":5}` passes.
    optional("elicitation", ANY_OBJECT),
    optional("extensions", ANY_OBJECT),
];

/// `InitializeRequestParams`.
pub(crate) const INITIALIZE: &[Property] = &[
    required("protocolVersion", Shape::Text),
    required("capabilities", Shape::Object(CLIENT_CAPABILITIES)),
    required("clientInfo", Shape::Object(IMPLEMENTATION)),
    optional("_meta", ANY_OBJECT),
    // `InputResponse` has a converter of its own, measured lenient: `{"a":5}` passes.
    optional("inputResponses", ANY_OBJECT),
    optional("requestState", Shape::Text),
];
/// `SetLevelRequestParams`.
pub(crate) const SET_LEVEL: &[Property] = &[
    required("level", Shape::Level),
    optional("_meta", ANY_OBJECT),
    optional("inputResponses", ANY_OBJECT),
    optional("requestState", Shape::Text),
];
/// `CallToolRequestParams`.
pub(crate) const CALL_TOOL: &[Property] = &[
    required("name", Shape::Text),
    optional("arguments", ANY_OBJECT),
    optional("_meta", ANY_OBJECT),
    optional("inputResponses", ANY_OBJECT),
    optional("requestState", Shape::Text),
];
/// `ListToolsRequestParams`.
pub(crate) const LIST_TOOLS: &[Property] = &[
    optional("cursor", Shape::Text),
    optional("_meta", ANY_OBJECT),
    optional("inputResponses", ANY_OBJECT),
    optional("requestState", Shape::Text),
];
/// `CancelledNotificationParams`.
pub(crate) const CANCELLED: &[Property] = &[
    required("requestId", Shape::Id),
    optional("reason", Shape::Text),
    optional("_meta", ANY_OBJECT),
];

/// True when the `params` of a request deserialize into the type of `properties`: absent or
/// null is no params at all; at the top level, two names equal without case are refused.
pub(crate) fn params_fit(params: Option<&Node>, properties: &'static [Property]) -> bool {
    match params {
        None | Some(Node::Null) => true,
        Some(Node::Object(entries)) => {
            let distinct = entries.iter().enumerate().all(|(index, (name, _))| {
                entries[..index]
                    .iter()
                    .all(|(other, _)| !same_name(&other.text, &name.text))
            });
            distinct && fits(params.unwrap_or(&Node::Null), Shape::Object(properties))
        }
        Some(_) => false,
    }
}

fn fits(node: &Node, shape: Shape) -> bool {
    match (shape, node) {
        (Shape::Any, _) => true,
        (Shape::Int32 | Shape::Level | Shape::Id, Node::Null) => false,
        (_, Node::Null) => true,
        (Shape::Text, Node::Text { lone_surrogate, .. }) => !lone_surrogate,
        (Shape::Flag, Node::Bool(_)) => true,
        (Shape::Int32 | Shape::Level, Node::Number(raw)) => integer::<i32>(raw).is_some(),
        (
            Shape::Level,
            Node::Text {
                text,
                lone_surrogate: false,
            },
        ) => level(text),
        (Shape::Id, node) => RequestId::read(node).is_some(),
        (Shape::List(item), Node::Array(items)) => items.iter().all(|value| fits(value, *item)),
        (Shape::Object(properties), Node::Object(entries)) => {
            let each = entries.iter().all(|(name, value)| {
                properties
                    .iter()
                    .find(|property| same_name(&name.text, property.name))
                    .is_none_or(|property| fits(value, property.shape))
            });
            each && properties
                .iter()
                .filter(|property| property.required)
                .all(|property| {
                    entries
                        .iter()
                        .any(|(name, _)| same_name(&name.text, property.name))
                })
        }
        _ => false,
    }
}

/// The value of a property of an object, matched without case, last occurrence kept.
pub(crate) fn property<'a>(node: Option<&'a Node>, name: &str) -> Option<&'a Node> {
    match node? {
        Node::Object(entries) => entries
            .iter()
            .rev()
            .find(|(key, _)| same_name(&key.text, name))
            .map(|(_, value)| value),
        _ => None,
    }
}

/// A request id read from a value of the params (`notifications/cancelled`).
pub(crate) fn request_id(node: Option<&Node>) -> Option<RequestId> {
    node.and_then(RequestId::read)
}

/// A string value, `None` for null or any other kind.
pub(crate) fn string(node: Option<&Node>) -> Option<String> {
    node.and_then(text)
}

/// Property names of the SDK without case, ASCII only: measured, `clıentInfo` (dotless i) and
/// `ſampling` (long s) do not match.
fn same_name(text: &str, name: &str) -> bool {
    text.eq_ignore_ascii_case(name)
}

const LEVELS: [&str; 8] = [
    "debug",
    "info",
    "notice",
    "warning",
    "error",
    "critical",
    "alert",
    "emergency",
];

/// A level in a string, as measured on the converter of the SDK: an integer of 32 bits (sign
/// allowed, blanks around), or names of [`LEVELS`] separated by commas, blanks around each,
/// case kept, only the last one may be empty.
fn level(text: &str) -> bool {
    let trimmed = text.trim();
    let numeric = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    if !numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()) {
        return trimmed.parse::<i32>().is_ok();
    }
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let last = parts.len() - 1;
    !parts[0].is_empty()
        && parts.iter().enumerate().all(|(index, part)| {
            LEVELS.contains(part) || (part.is_empty() && index == last && index > 0)
        })
}

/// Writes a value compactly, as `JsonNode.ToJsonString()` and the writer of the SDK do: names
/// and strings through the default encoder, numbers as read.
pub fn write(out: &mut String, node: &Node) {
    match node {
        Node::Null => out.push_str("null"),
        Node::Bool(value) => out.push_str(if *value { "true" } else { "false" }),
        Node::Number(raw) => out.push_str(raw),
        Node::Text { text, .. } => out.push_str(&quoted_basic_latin(text)),
        Node::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write(out, item);
            }
            out.push(']');
        }
        Node::Object(entries) => {
            out.push('{');
            for (index, (name, value)) in entries.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&quoted_basic_latin(&name.text));
                out.push(':');
                write(out, value);
            }
            out.push('}');
        }
    }
}

/// A value written compactly.
pub fn to_text(node: &Node) -> String {
    let mut out = String::new();
    write(&mut out, node);
    out
}

/// `{"result":…,"id":…,"jsonrpc":"2.0"}`.
pub fn result(id: &RequestId, result: &str) -> String {
    format!(
        "{{\"result\":{result},\"id\":{},\"jsonrpc\":\"2.0\"}}",
        id.to_json()
    )
}

/// `{"error":{"code":…,"message":…},"id":…,"jsonrpc":"2.0"}`.
pub fn error(id: &RequestId, code: i32, message: &str) -> String {
    format!(
        "{{\"error\":{{\"code\":{code},\"message\":{}}},\"id\":{},\"jsonrpc\":\"2.0\"}}",
        quoted_basic_latin(message),
        id.to_json()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(line: &str) -> Option<RequestId> {
        match read(line) {
            Some(Message::Invalid(id)) => id,
            other => panic!("{line}: {other:?}"),
        }
    }

    #[test]
    fn the_id_of_a_refused_line_is_read_from_its_start() {
        assert_eq!(
            invalid(r#"{"jsonrpc":"2.0","id":5,"method":"ping""#),
            Some(RequestId::Number(5))
        );
        assert_eq!(invalid(r#"{"jsonrpc":"2.0","id":1.5,"id":2}"#), None);
        assert_eq!(invalid(r#"{"params":{"id":3},"method":"x"}"#), None);
        assert_eq!(invalid(r#"{"jsonrpc":"2.0","id":5x}"#), None);
        assert_eq!(
            invalid(r#"{"a":[{"b":"é"},true,null,-1e3],"id":"s"}"#),
            Some(RequestId::Text("s".to_owned()))
        );
        assert_eq!(invalid(r#"{"id":"a\ud800"}"#), None);
        assert_eq!(invalid("[]"), None);
    }

    #[test]
    fn levels_follow_the_measured_converter() {
        for accepted in [
            "debug",
            " debug",
            "info ",
            "debug, info",
            "debug,",
            "0",
            "+1",
            " 1 ",
            "-5",
            "99",
        ] {
            assert!(level(accepted), "{accepted}");
        }
        for refused in [
            "",
            " ",
            ",",
            "Debug",
            "DEBUG",
            "0x1",
            "1,debug",
            "debug,,info",
            "debug,7",
            "2147483648",
            "debug\0",
        ] {
            assert!(!level(refused), "{refused}");
        }
    }

    /// Each ordinary character is read once: a refused line of 4 MiB with a long string before
    /// `id` is scanned in linear time.
    #[test]
    fn a_long_refused_line_is_scanned_once() {
        let line = format!(r#"{{"a":"{}","id":7"#, "é".repeat(2 * 1024 * 1024));
        let started = std::time::Instant::now();
        assert_eq!(invalid(&line), Some(RequestId::Number(7)));
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
    }

    #[test]
    fn names_match_without_case() {
        assert!(same_name("NAME", "name"));
        assert!(!same_name("clıentInfo", "clientInfo"));
        assert!(!same_name("ſampling", "sampling"));
        assert!(!same_name("names", "name"));
    }

    proptest::proptest! {
        #[test]
        fn any_line_never_panics(line in r"\PC{0,256}") {
            let _ = read(&line);
        }

        #[test]
        fn any_line_near_json_never_panics(
            line in r#"(\{|\}|\[|\]|:|,| |\t|"id"|"jsonrpc"|"2\.0"|"method"|"params"|"error"|"result"|-?[0-9]{1,20}(\.[0-9])?(e[+-]?[0-9])?|true|false|null|"([a-z\u{e9}\u{1F600}]|\\[nu"\\/]|\\u[0-9a-fA-F]{4})*"?|\\u[dD][89abAB][0-9a-f]{2}){0,64}"#
        ) {
            let _ = read(&line);
        }

        #[test]
        fn a_written_request_reads_back(
            number in proptest::prelude::any::<i64>(),
            text in r"\PC{0,32}",
            as_text in proptest::prelude::any::<bool>(),
            method in r"\PC{0,32}",
        ) {
            let id = if as_text { RequestId::Text(text) } else { RequestId::Number(number) };
            let line = format!(
                r#"{{"jsonrpc":"2.0","id":{},"method":{}}}"#,
                id.to_json(),
                quoted_basic_latin(&method)
            );
            proptest::prop_assert_eq!(
                read(&line),
                Some(Message::Request { id, method, params: None })
            );
        }

        #[test]
        fn any_level_never_panics(text in r"[\PC,]{0,64}") {
            let _ = level(&text);
        }
    }
}
