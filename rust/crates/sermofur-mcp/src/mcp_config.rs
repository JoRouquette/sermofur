//! The `sermofur` entry of a Claude Code project configuration (`.mcp.json`), changed in place
//! as `McpConfigFile` of the oracle (lot 8.1): only the bytes of that entry are replaced,
//! inserted or removed, so every other byte stays. `rust/tests/golden/mcp-config.json`
//! (`rust/tools/mcp-config-oracle`) is the truth.
//!
//! The layout is read token by token as `Utf8JsonReader` reads it, so that a refusal raised on
//! an early token ("not a JSON object", "\"mcpServers\" is not an object") wins over a syntax
//! error further on: strict JSON, depth 64, no comment, strings checked for control characters
//! and escapes but not for UTF-8.

use sermofur_core::error::{Result, SermofurError};
use sermofur_core::instance::invalid_storage;
use sermofur_core::json::quoted_basic_latin;

pub const FILE_NAME: &str = ".mcp.json";
pub const SERVERS_PROPERTY: &str = "mcpServers";
pub const ENTRY_NAME: &str = "sermofur";
pub const MAX_BYTES: usize = 1024 * 1024;

const BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const MAX_DEPTH: usize = 64;

/// The entry that launches the bridge, as `Entry(...).ToJsonString()` writes it.
pub fn entry_text(command: &str, arguments: &[String]) -> String {
    let args: Vec<String> = arguments.iter().map(|a| quoted_basic_latin(a)).collect();
    format!(
        "{{\"type\":\"stdio\",\"command\":{},\"args\":[{}]}}",
        quoted_basic_latin(command),
        args.join(",")
    )
}

/// Content with the entry added or updated, or `None` when it is already the same. A `None`
/// input is an absent file.
pub fn with_entry(
    content: Option<&[u8]>,
    command: &str,
    arguments: &[String],
) -> Result<Option<Vec<u8>>> {
    let entry = entry_text(command, arguments);
    let Some(content) = content else {
        return Ok(Some(
            format!("{{\n  \"{SERVERS_PROPERTY}\": {{\n    \"{ENTRY_NAME}\": {entry}\n  }}\n}}\n")
                .into_bytes(),
        ));
    };
    let layout = read(content)?;
    if let Some((start, end)) = layout.entry_value {
        return Ok(if same_entry(&content[start..end], command, arguments)? {
            None
        } else {
            Some(splice(content, start, end, &entry))
        });
    }
    if let Some((_, servers_end)) = layout.servers {
        let closing = servers_end - 1;
        let last = last_non_space(content, closing);
        let empty = content[last] == b'{';
        let insert = if empty {
            format!("\n    \"{ENTRY_NAME}\": {entry}\n  ")
        } else {
            format!(",\n    \"{ENTRY_NAME}\": {entry}")
        };
        let end = if empty { closing } else { last + 1 };
        return Ok(Some(splice(content, last + 1, end, &insert)));
    }
    let root_closing = layout.root_end - 1;
    let root_last = last_non_space(content, root_closing);
    let servers = format!("\"{SERVERS_PROPERTY}\": {{\n    \"{ENTRY_NAME}\": {entry}\n  }}");
    Ok(Some(if content[root_last] == b'{' {
        splice(
            content,
            root_last + 1,
            root_closing,
            &format!("\n  {servers}\n"),
        )
    } else {
        splice(
            content,
            root_last + 1,
            root_last + 1,
            &format!(",\n  {servers}"),
        )
    }))
}

/// Content without the entry, or `None` when there is no entry.
pub fn without_entry(content: &[u8]) -> Result<Option<Vec<u8>>> {
    let layout = read(content)?;
    let (Some(name_start), Some((_, value_end))) = (layout.entry_name, layout.entry_value) else {
        return Ok(None);
    };
    let next = next_non_space(content, value_end);
    if content[next] == b',' {
        // Not the last entry: removed with its comma, up to the next entry.
        return Ok(Some(splice(
            content,
            name_start,
            next_non_space(content, next + 1),
            "",
        )));
    }
    let previous = last_non_space(content, name_start);
    Ok(Some(if content[previous] == b',' {
        splice(content, previous, value_end, "")
    } else {
        // The only entry: the object becomes empty, "{}".
        splice(content, previous + 1, next, "")
    }))
}

/// Whether the configuration holds an entry that is an object (`CurrentEntry` not null).
pub fn has_entry(content: &[u8]) -> Result<bool> {
    let layout = read(content)?;
    Ok(layout
        .entry_value
        .is_some_and(|(start, _)| content[start] == b'{'))
}

struct Layout {
    root_end: usize,
    servers: Option<(usize, usize)>,
    entry_name: Option<usize>,
    entry_value: Option<(usize, usize)>,
}

fn invalid(reason: &str) -> SermofurError {
    SermofurError::new(
        "invalid_mcp_config",
        format!("The MCP configuration cannot be changed ({reason}); it is left untouched."),
        3,
    )
}

fn malformed<T>(_: Syntax) -> Result<T> {
    Err(invalid("malformed JSON"))
}

fn read(content: &[u8]) -> Result<Layout> {
    let offset = if content.starts_with(BOM) {
        BOM.len()
    } else {
        0
    };
    if content.len() > MAX_BYTES {
        return Err(invalid("larger than 1 MiB"));
    }
    let mut reader = Reader::new(&content[offset..]);
    let first = reader.read().or_else(malformed)?;
    if first.map(|t| t.kind) != Some(Kind::StartObject) {
        return Err(invalid("not a JSON object"));
    }
    let mut servers = None;
    let mut entry_name = None;
    let mut entry_value = None;
    let mut last = Kind::StartObject;
    while let Some(name) = reader.read().or_else(malformed)? {
        last = name.kind;
        if name.kind != Kind::PropertyName {
            break;
        }
        let is_servers = reader.name_equals(name, SERVERS_PROPERTY);
        let value = reader
            .read()
            .or_else(malformed)?
            .ok_or(Syntax)
            .or_else(malformed)?;
        if !is_servers {
            reader.skip(value).or_else(malformed)?;
            continue;
        }
        if value.kind != Kind::StartObject {
            return Err(invalid(&format!("\"{SERVERS_PROPERTY}\" is not an object")));
        }
        let servers_start = offset + value.start;
        while let Some(inner) = reader.read().or_else(malformed)? {
            if inner.kind != Kind::PropertyName {
                break;
            }
            let is_entry = reader.name_equals(inner, ENTRY_NAME);
            let name_start = offset + inner.start;
            let value = reader
                .read()
                .or_else(malformed)?
                .ok_or(Syntax)
                .or_else(malformed)?;
            let value_start = offset + value.start;
            reader.skip(value).or_else(malformed)?;
            if is_entry {
                entry_name = Some(name_start);
                entry_value = Some((value_start, offset + reader.consumed));
            }
        }
        servers = Some((servers_start, offset + reader.consumed));
    }
    if last != Kind::EndObject {
        return Err(invalid("malformed JSON"));
    }
    let root_end = offset + reader.consumed;
    if reader.read().or_else(malformed)?.is_some() {
        return Err(invalid("content after the JSON object"));
    }
    Ok(Layout {
        root_end,
        servers,
        entry_name,
        entry_value,
    })
}

/// `JsonNode.DeepEquals(existing, entry)`. A duplicate property of an existing object entry is
/// the `ArgumentException` of `JsonObject`, and invalid UTF-8 in one of its names, or in a
/// string value reached by the comparison, the `InvalidOperationException` of `GetString`
/// (measured): the CLI answers both with `invalid_storage_or_path`.
fn same_entry(value: &[u8], command: &str, arguments: &[String]) -> Result<bool> {
    let mut reader = Reader::new(value);
    let node = reader
        .read()
        .ok()
        .flatten()
        .and_then(|first| reader.node(first).ok());
    let Some(Value::Object(properties)) = node else {
        return Ok(false);
    };
    let mut seen = std::collections::HashSet::new();
    for (name, _) in &properties {
        if std::str::from_utf8(name).is_err() || !seen.insert(name.as_slice()) {
            return Err(invalid_storage());
        }
    }
    if properties.len() != 3 {
        return Ok(false);
    }
    let text = |v: &Value, expected: &str| -> Result<bool> {
        match v {
            Value::Text(t, _) if std::str::from_utf8(t).is_err() => Err(invalid_storage()),
            Value::Text(t, lone) => Ok(!lone && t == expected.as_bytes()),
            _ => Ok(false),
        }
    };
    for (name, value) in &properties {
        let equal = match name.as_slice() {
            b"type" => text(value, "stdio")?,
            b"command" => text(value, command)?,
            b"args" => match value {
                Value::Array(items) if items.len() == arguments.len() => {
                    let mut all = true;
                    for (item, argument) in items.iter().zip(arguments) {
                        if !text(item, argument)? {
                            all = false;
                            break;
                        }
                    }
                    all
                }
                _ => false,
            },
            _ => false,
        };
        if !equal {
            return Ok(false);
        }
    }
    Ok(true)
}

fn splice(content: &[u8], start: usize, end: usize, insert: &str) -> Vec<u8> {
    let mut result = Vec::with_capacity(content.len() - (end - start) + insert.len());
    result.extend_from_slice(&content[..start]);
    result.extend_from_slice(insert.as_bytes());
    result.extend_from_slice(&content[end..]);
    result
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// The last non-space index before `before`; the callers always have a bracket before it.
fn last_non_space(content: &[u8], before: usize) -> usize {
    (0..before)
        .rev()
        .find(|&i| !is_space(content[i]))
        .unwrap_or(0)
}

fn next_non_space(content: &[u8], from: usize) -> usize {
    let index = (from..content.len())
        .find(|&i| !is_space(content[i]))
        .unwrap_or(content.len());
    index.min(content.len() - 1)
}

/// A syntax error of the reader (`JsonException`).
#[derive(Debug, Clone, Copy)]
struct Syntax;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    StartObject,
    EndObject,
    StartArray,
    EndArray,
    PropertyName,
    Text,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Token {
    kind: Kind,
    start: usize,
    end: usize,
}

/// A value of the entry: strings unescaped, with their lone surrogate flag.
enum Value {
    Text(Vec<u8>, bool),
    Other,
    Array(Vec<Value>),
    Object(Vec<(Vec<u8>, Value)>),
}

/// What the reader expects next.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Value,
    NameOrEnd,
    Name,
    ValueOrEnd,
    CommaOrEnd,
    Done,
}

/// A streaming reader with the token rules of `Utf8JsonReader` (final block, default options).
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// `BytesConsumed`: the end of the last token read.
    consumed: usize,
    stack: Vec<u8>,
    expect: Expect,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            consumed: 0,
            stack: Vec::new(),
            expect: Expect::Value,
        }
    }

    fn skip_space(&mut self) {
        while self.bytes.get(self.pos).copied().is_some_and(is_space) {
            self.pos += 1;
        }
    }

    /// The next token, or `None` at the end of a complete document.
    fn read(&mut self) -> std::result::Result<Option<Token>, Syntax> {
        self.skip_space();
        let Some(&b) = self.bytes.get(self.pos) else {
            return if self.expect == Expect::Done {
                Ok(None)
            } else {
                Err(Syntax)
            };
        };
        let start = self.pos;
        let token = match self.expect {
            Expect::Done => return Err(Syntax),
            Expect::CommaOrEnd => {
                if b == b',' {
                    self.pos += 1;
                    self.expect = if self.stack.last() == Some(&b'{') {
                        Expect::Name
                    } else {
                        Expect::Value
                    };
                    return self.read();
                }
                self.close(b)?
            }
            Expect::NameOrEnd if b == b'}' => self.close(b)?,
            Expect::NameOrEnd | Expect::Name => {
                if b != b'"' {
                    return Err(Syntax);
                }
                self.string()?;
                let end = self.pos;
                self.skip_space();
                if self.bytes.get(self.pos) != Some(&b':') {
                    return Err(Syntax);
                }
                self.pos += 1;
                self.expect = Expect::Value;
                self.consumed = self.pos;
                return Ok(Some(Token {
                    kind: Kind::PropertyName,
                    start,
                    end,
                }));
            }
            Expect::ValueOrEnd if b == b']' => self.close(b)?,
            Expect::Value | Expect::ValueOrEnd => self.value(b)?,
        };
        self.consumed = self.pos;
        Ok(Some(Token {
            kind: token,
            start,
            end: self.pos,
        }))
    }

    fn close(&mut self, b: u8) -> std::result::Result<Kind, Syntax> {
        let kind = match (self.stack.last(), b) {
            (Some(b'{'), b'}') => Kind::EndObject,
            (Some(b'['), b']') => Kind::EndArray,
            _ => return Err(Syntax),
        };
        self.pos += 1;
        self.stack.pop();
        self.after_value();
        Ok(kind)
    }

    fn after_value(&mut self) {
        self.expect = if self.stack.is_empty() {
            Expect::Done
        } else {
            Expect::CommaOrEnd
        };
    }

    fn value(&mut self, b: u8) -> std::result::Result<Kind, Syntax> {
        let kind = match b {
            b'{' | b'[' => {
                if self.stack.len() >= MAX_DEPTH {
                    return Err(Syntax);
                }
                self.pos += 1;
                self.stack.push(b);
                self.expect = if b == b'{' {
                    Expect::NameOrEnd
                } else {
                    Expect::ValueOrEnd
                };
                return Ok(if b == b'{' {
                    Kind::StartObject
                } else {
                    Kind::StartArray
                });
            }
            b'"' => {
                self.string()?;
                Kind::Text
            }
            b't' => self.literal(b"true")?,
            b'f' => self.literal(b"false")?,
            b'n' => self.literal(b"null")?,
            b'-' | b'0'..=b'9' => self.number()?,
            _ => return Err(Syntax),
        };
        self.after_value();
        Ok(kind)
    }

    /// A number ends on a delimiter (`JsonConstants.Delimiters`, `/` included), or the reader
    /// refuses it at once.
    fn delimited(&self) -> std::result::Result<Kind, Syntax> {
        match self.bytes.get(self.pos) {
            None => Ok(Kind::Other),
            Some(&b) if is_space(b) || matches!(b, b',' | b']' | b'}' | b'/') => Ok(Kind::Other),
            Some(_) => Err(Syntax),
        }
    }

    fn literal(&mut self, word: &[u8]) -> std::result::Result<Kind, Syntax> {
        if !self.bytes[self.pos..].starts_with(word) {
            return Err(Syntax);
        }
        // `ConsumeLiteral` checks the word only: the next byte is refused by the next token.
        self.pos += word.len();
        Ok(Kind::Other)
    }

    fn digits(&mut self) -> usize {
        let from = self.pos;
        while self.bytes.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        self.pos - from
    }

    fn number(&mut self) -> std::result::Result<Kind, Syntax> {
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        match self.bytes.get(self.pos) {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(Syntax),
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(Syntax);
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(Syntax);
            }
        }
        self.delimited()
    }

    /// Skips a string whose opening quote is at `pos`, checking control characters and escapes.
    fn string(&mut self) -> std::result::Result<(), Syntax> {
        self.pos += 1;
        loop {
            let b = *self.bytes.get(self.pos).ok_or(Syntax)?;
            self.pos += 1;
            match b {
                b'"' => return Ok(()),
                0..=0x1F => return Err(Syntax),
                b'\\' => {
                    let e = *self.bytes.get(self.pos).ok_or(Syntax)?;
                    self.pos += 1;
                    match e {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => {}
                        b'u' => {
                            let hex = self.bytes.get(self.pos..self.pos + 4).ok_or(Syntax)?;
                            if !hex.iter().all(u8::is_ascii_hexdigit) {
                                return Err(Syntax);
                            }
                            self.pos += 4;
                        }
                        _ => return Err(Syntax),
                    }
                }
                _ => {}
            }
        }
    }

    /// `ValueTextEquals` on a name or string token.
    fn name_equals(&self, token: Token, expected: &str) -> bool {
        let (text, lone) = unescape(&self.bytes[token.start + 1..token.end - 1]);
        !lone && text == expected.as_bytes()
    }

    /// Skips the value a token starts (`Utf8JsonReader.Skip`).
    fn skip(&mut self, token: Token) -> std::result::Result<(), Syntax> {
        if !matches!(token.kind, Kind::StartObject | Kind::StartArray) {
            return Ok(());
        }
        let depth = self.stack.len();
        while self.stack.len() >= depth {
            self.read()?.ok_or(Syntax)?;
        }
        Ok(())
    }

    /// The value a token starts, as a tree.
    fn node(&mut self, token: Token) -> std::result::Result<Value, Syntax> {
        Ok(match token.kind {
            Kind::Text => {
                let (text, lone) = unescape(&self.bytes[token.start + 1..token.end - 1]);
                Value::Text(text, lone)
            }
            Kind::StartArray => {
                let mut items = Vec::new();
                loop {
                    let next = self.read()?.ok_or(Syntax)?;
                    if next.kind == Kind::EndArray {
                        break Value::Array(items);
                    }
                    items.push(self.node(next)?);
                }
            }
            Kind::StartObject => {
                let mut properties = Vec::new();
                loop {
                    let name = self.read()?.ok_or(Syntax)?;
                    if name.kind == Kind::EndObject {
                        break Value::Object(properties);
                    }
                    let key = unescape(&self.bytes[name.start + 1..name.end - 1]).0;
                    let value = self.read()?.ok_or(Syntax)?;
                    properties.push((key, self.node(value)?));
                }
            }
            _ => Value::Other,
        })
    }
}

/// The bytes of a checked string body, escapes decoded, and whether it held a lone surrogate
/// escape (read as U+FFFD, never equal to an entry string).
fn unescape(body: &[u8]) -> (Vec<u8>, bool) {
    let mut out = Vec::with_capacity(body.len());
    let mut lone = false;
    let mut i = 0;
    let hex = |at: usize| {
        std::str::from_utf8(&body[at..at + 4])
            .ok()
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .unwrap_or(0xFFFF_FFFF)
    };
    while i < body.len() {
        if body[i] != b'\\' {
            out.push(body[i]);
            i += 1;
            continue;
        }
        let e = body[i + 1];
        i += 2;
        let c = match e {
            b'b' => '\u{8}',
            b'f' => '\u{c}',
            b'n' => '\n',
            b'r' => '\r',
            b't' => '\t',
            b'u' => {
                let first = hex(i);
                i += 4;
                let mut code = first;
                if (0xD800..0xDC00).contains(&first) && body[i..].starts_with(b"\\u") {
                    let second = hex(i + 2);
                    if (0xDC00..0xE000).contains(&second) {
                        code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                        i += 6;
                    }
                }
                char::from_u32(code).unwrap_or_else(|| {
                    lone = true;
                    '\u{FFFD}'
                })
            }
            other => char::from(other),
        };
        let mut buffer = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
    }
    (out, lone)
}
