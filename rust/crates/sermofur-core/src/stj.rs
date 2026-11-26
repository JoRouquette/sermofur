//! A JSON reader with the rules of `System.Text.Json` as `RecordJson` and the daemon frames
//! (`Framing.Decode`) read (the oracle is the truth, through
//! `rust/tests/golden/instance-config.json`, `records.json` and `framing.json`). `serde_json`
//! alone does not reproduce them (lone surrogate escapes, depth, duplicates):
//! - strict JSON (RFC 8259): no comment, trailing comma, single quote or trailing content;
//!   whitespace is space, tab, CR, LF; numbers without leading zero; raw control characters
//!   in strings refused; escapes `\" \\ \/ \b \f \n \r \t \uXXXX`;
//! - nesting deeper than 64 is refused (a top-level object is depth 1);
//! - a lone surrogate escape reads as U+FFFD and is flagged: it is an error only where a
//!   string or a name is read, never in a skipped value;
//! - every property is kept in order, so that a reader converts a duplicate at once (a bad
//!   earlier one fails) and the last good one wins.
//!
//! Every syntax or conversion error is the `JsonException` of .NET, which the CLI answers with
//! `invalid_storage_or_path`.
//!
//! The reader keeps the whole document as a tree: the caller bounds the size of the text it
//! reads (64 KiB for `instance.json`). [`parse_flat`] builds only the kept scalar values of a
//! top-level object, one at a time, for the 16 MiB answers of the daemon.

use uuid::Uuid;

use crate::error::{Result, SermofurError};
use crate::format::{DateTimeOffset, parse_json_text};
use crate::instance::invalid_storage;

/// Maximum depth of `System.Text.Json` (`JsonReaderOptions.MaxDepth` default).
const MAX_DEPTH: usize = 64;

/// A property name, and whether it held a lone surrogate escape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Name {
    pub text: String,
    pub lone_surrogate: bool,
}

/// A JSON value; a number keeps its raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Null,
    Bool(bool),
    Number(String),
    Text { text: String, lone_surrogate: bool },
    Array(Vec<Node>),
    Object(Vec<(Name, Node)>),
}

/// Parses a whole document: one value, then only whitespace.
pub fn parse(text: &str) -> Result<Node> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        pos: 0,
        lone_surrogate: false,
    };
    let node = reader.value(0)?;
    reader.end()?;
    Ok(node)
}

/// A property value of [`parse_flat`]: a scalar is built, a container is only checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flat {
    Scalar(Node),
    /// `texts`: every item is `null` or a string without a lone surrogate escape.
    Array {
        texts: bool,
    },
    Object,
}

/// Parses a document that must be an object or `null` (`false`) with the rules of [`parse`],
/// and hands each property whose name `keep` accepts to `visit`, in order, one at a time: a
/// scalar is built, an array or an object is checked (syntax, depth, strings) and skipped. The
/// value of any other name is checked without being built. A name with a lone surrogate escape
/// is refused, as [`object`] refuses it. The memory used stays near the size of the largest
/// kept scalar, whatever the shape and the number of properties.
pub fn parse_flat(
    text: &str,
    keep: &dyn Fn(&str) -> bool,
    visit: &mut dyn FnMut(&str, Flat) -> Result<()>,
) -> Result<bool> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        pos: 0,
        lone_surrogate: false,
    };
    reader.skip_space();
    if reader.peek() == Some(b'n') {
        reader.literal(b"null")?;
        reader.end()?;
        return Ok(false);
    }
    reader.expect(b'{')?;
    reader.skip_space();
    if reader.peek() == Some(b'}') {
        reader.pos += 1;
    } else {
        loop {
            reader.skip_space();
            if reader.peek() != Some(b'"') {
                return Err(invalid_storage());
            }
            let name = reader.string()?;
            if reader.lone_surrogate {
                // Every outcome of `parse` then `object` is this same error.
                return Err(invalid_storage());
            }
            reader.skip_space();
            reader.expect(b':')?;
            reader.skip_space();
            let kept = keep(&name);
            let value = match reader.peek() {
                Some(b'[') => Some(Flat::Array {
                    texts: reader.skip_container(1, b']', false)?,
                }),
                Some(b'{') => {
                    reader.skip_container(1, b'}', true)?;
                    Some(Flat::Object)
                }
                _ if kept => Some(Flat::Scalar(reader.value(1)?)),
                _ => {
                    reader.skip_scalar()?;
                    None
                }
            };
            if let Some(value) = value.filter(|_| kept) {
                visit(&name, value)?;
            }
            reader.skip_space();
            match reader.next() {
                Some(b',') => {}
                Some(b'}') => break,
                _ => return Err(invalid_storage()),
            }
        }
    }
    reader.end()?;
    Ok(true)
}

/// `RecordJson.Read` of a top-level `null`.
pub(crate) fn invalid_record() -> SermofurError {
    SermofurError::new("invalid_record", "Invalid persisted object.", 3)
}

/// The properties of an object that a record is read from: a name with a lone surrogate
/// escape is an error there. `null` is `None`; any other value is an error.
pub fn object(node: &Node) -> Result<Option<Vec<(&str, &Node)>>> {
    match node {
        Node::Object(properties) => properties
            .iter()
            .map(|(name, value)| {
                if name.lone_surrogate {
                    Err(invalid_storage())
                } else {
                    Ok((name.text.as_str(), value))
                }
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Node::Null => Ok(None),
        _ => Err(invalid_storage()),
    }
}

/// The properties of a whole document read as a record: a top-level `null` is
/// `invalid_record`.
pub(crate) fn document(node: &Node) -> Result<Vec<(&str, &Node)>> {
    object(node)?.ok_or_else(invalid_record)
}

/// An integer as `Utf8Parser` reads it: optional `-`, digits only, in range of `T`.
pub fn integer<T: std::str::FromStr>(node: &Node) -> Result<T> {
    let Node::Number(raw) = node else {
        return Err(invalid_storage());
    };
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid_storage());
    }
    raw.parse().map_err(|_| invalid_storage())
}

/// A string: `null` is `None`; a lone surrogate escape or another kind of value is an error.
pub fn text(node: &Node) -> Result<Option<String>> {
    match node {
        Node::Null => Ok(None),
        Node::Text {
            text,
            lone_surrogate: false,
        } => Ok(Some(text.clone())),
        _ => Err(invalid_storage()),
    }
}

/// The text of a string that must not be `null` (a GUID, a date, an enumeration name).
pub(crate) fn required_text(node: &Node) -> Result<String> {
    text(node)?.ok_or_else(invalid_storage)
}

/// A GUID in format D: 8-4-4-4-12 hexadecimal digits, any case, nothing around.
pub(crate) fn guid(node: &Node) -> Result<Uuid> {
    let raw = required_text(node)?;
    let b = raw.as_bytes();
    if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
        return Err(invalid_storage());
    }
    Uuid::parse_str(&raw).map_err(|_| invalid_storage())
}

/// A `Guid?`: `null` is `None`.
pub(crate) fn optional_guid(node: &Node) -> Result<Option<Uuid>> {
    if *node == Node::Null {
        Ok(None)
    } else {
        guid(node).map(Some)
    }
}

/// A `DateTimeOffset` in the formats of `System.Text.Json`; `local_offset_minutes` serves a
/// text without offset.
pub(crate) fn date(
    node: &Node,
    local_offset_minutes: &dyn Fn(i64) -> i16,
) -> Result<DateTimeOffset> {
    let raw = required_text(node)?;
    parse_json_text(&raw, local_offset_minutes).ok_or_else(invalid_storage)
}

/// A `DateTimeOffset?`: `null` is `None`.
pub(crate) fn optional_date(
    node: &Node,
    local_offset_minutes: &dyn Fn(i64) -> i16,
) -> Result<Option<DateTimeOffset>> {
    if *node == Node::Null {
        Ok(None)
    } else {
        date(node, local_offset_minutes).map(Some)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    /// Set by `string` when it met a lone surrogate escape.
    lone_surrogate: bool,
}

impl Reader<'_> {
    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let b = self.peek();
        self.pos += usize::from(b.is_some());
        b
    }

    fn skip_space(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<()> {
        (self.next() == Some(byte))
            .then_some(())
            .ok_or_else(invalid_storage)
    }

    fn literal(&mut self, word: &[u8]) -> Result<()> {
        if self.bytes[self.pos..].starts_with(word) {
            self.pos += word.len();
            Ok(())
        } else {
            Err(invalid_storage())
        }
    }

    /// Only whitespace may follow the value.
    fn end(&mut self) -> Result<()> {
        self.skip_space();
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(invalid_storage())
        }
    }

    /// Reads a value; `depth` is the depth of the container that holds it.
    fn value(&mut self, depth: usize) -> Result<Node> {
        self.skip_space();
        match self.peek().ok_or_else(invalid_storage)? {
            b'"' => {
                let text = self.string()?;
                Ok(Node::Text {
                    text,
                    lone_surrogate: self.lone_surrogate,
                })
            }
            b'n' => self.literal(b"null").map(|()| Node::Null),
            b't' => self.literal(b"true").map(|()| Node::Bool(true)),
            b'f' => self.literal(b"false").map(|()| Node::Bool(false)),
            b'-' | b'0'..=b'9' => self.number().map(Node::Number),
            b'[' => self.container(depth, b']', false),
            b'{' => self.container(depth, b'}', true),
            _ => Err(invalid_storage()),
        }
    }

    /// Reads an array or an object whose opening bracket is at `pos`.
    fn container(&mut self, depth: usize, close: u8, object: bool) -> Result<Node> {
        if depth + 1 > MAX_DEPTH {
            return Err(invalid_storage());
        }
        self.pos += 1;
        let mut items = Vec::new();
        let mut properties = Vec::new();
        self.skip_space();
        if self.peek() == Some(close) {
            self.pos += 1;
        } else {
            loop {
                let mut name = None;
                if object {
                    self.skip_space();
                    if self.peek() != Some(b'"') {
                        return Err(invalid_storage());
                    }
                    let text = self.string()?;
                    name = Some(Name {
                        text,
                        lone_surrogate: self.lone_surrogate,
                    });
                    self.skip_space();
                    self.expect(b':')?;
                }
                let value = self.value(depth + 1)?;
                match name {
                    Some(name) => properties.push((name, value)),
                    None => items.push(value),
                }
                self.skip_space();
                match self.next() {
                    Some(b',') => {}
                    Some(b) if b == close => break,
                    _ => return Err(invalid_storage()),
                }
            }
        }
        Ok(if object {
            Node::Object(properties)
        } else {
            Node::Array(items)
        })
    }

    /// Checks a string, a number or a literal at `pos`, as [`Reader::value`] reads it, without
    /// building it.
    fn skip_scalar(&mut self) -> Result<()> {
        match self.peek().ok_or_else(invalid_storage)? {
            b'"' => self.scan_string(None),
            b'n' => self.literal(b"null"),
            b't' => self.literal(b"true"),
            b'f' => self.literal(b"false"),
            b'-' | b'0'..=b'9' => self.skip_number(),
            _ => Err(invalid_storage()),
        }
    }

    /// Checks an array or an object whose opening bracket is at `pos`, as [`Reader::container`]
    /// reads it, without building anything; true when every item is `null` or a string
    /// without a lone surrogate escape.
    fn skip_container(&mut self, depth: usize, close: u8, object: bool) -> Result<bool> {
        if depth + 1 > MAX_DEPTH {
            return Err(invalid_storage());
        }
        self.pos += 1;
        let mut texts = true;
        self.skip_space();
        if self.peek() == Some(close) {
            self.pos += 1;
            return Ok(texts);
        }
        loop {
            if object {
                self.skip_space();
                if self.peek() != Some(b'"') {
                    return Err(invalid_storage());
                }
                self.scan_string(None)?;
                self.skip_space();
                self.expect(b':')?;
            }
            self.skip_space();
            texts &= match self.peek().ok_or_else(invalid_storage)? {
                b'"' => {
                    self.scan_string(None)?;
                    !self.lone_surrogate
                }
                b'n' => self.literal(b"null").map(|()| true)?,
                b't' => self.literal(b"true").map(|()| false)?,
                b'f' => self.literal(b"false").map(|()| false)?,
                b'-' | b'0'..=b'9' => self.skip_number().map(|()| false)?,
                b'[' => self.skip_container(depth + 1, b']', false).map(|_| false)?,
                b'{' => self.skip_container(depth + 1, b'}', true).map(|_| false)?,
                _ => return Err(invalid_storage()),
            };
            self.skip_space();
            match self.next() {
                Some(b',') => {}
                Some(b) if b == close => return Ok(texts),
                _ => return Err(invalid_storage()),
            }
        }
    }

    fn digits(&mut self) -> usize {
        let from = self.pos;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.pos += 1;
        }
        self.pos - from
    }

    fn number(&mut self) -> Result<String> {
        let start = self.pos;
        self.skip_number()?;
        Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
    }

    fn skip_number(&mut self) -> Result<()> {
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.digits();
            }
            _ => return Err(invalid_storage()),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.digits() == 0 {
                return Err(invalid_storage());
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if self.digits() == 0 {
                return Err(invalid_storage());
            }
        }
        // A digit after a leading zero ("02") is caught by the caller as an unexpected token.
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32> {
        let chunk = self
            .bytes
            .get(self.pos..self.pos + 4)
            .ok_or_else(invalid_storage)?;
        self.pos += 4;
        chunk.iter().try_fold(0u32, |acc, b| {
            char::from(*b)
                .to_digit(16)
                .map(|d| acc * 16 + d)
                .ok_or_else(invalid_storage)
        })
    }

    /// Reads a string whose opening quote is at `pos`.
    fn string(&mut self) -> Result<String> {
        let mut out: Vec<u8> = Vec::new();
        self.scan_string(Some(&mut out))?;
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    /// Checks a string whose opening quote is at `pos`, and decodes it into `out` if given.
    fn scan_string(&mut self, mut out: Option<&mut Vec<u8>>) -> Result<()> {
        self.pos += 1;
        self.lone_surrogate = false;
        loop {
            let b = self.next().ok_or_else(invalid_storage)?;
            match b {
                b'"' => return Ok(()),
                0..=0x1F => return Err(invalid_storage()),
                b'\\' => {
                    let c = match self.next().ok_or_else(invalid_storage)? {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let first = self.hex4()?;
                            let mut code = first;
                            if (0xD800..0xDC00).contains(&first)
                                && self.bytes[self.pos..].starts_with(b"\\u")
                            {
                                let saved = self.pos;
                                self.pos += 2;
                                let second = self.hex4()?;
                                if (0xDC00..0xE000).contains(&second) {
                                    code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                                } else {
                                    self.pos = saved;
                                }
                            }
                            char::from_u32(code).unwrap_or_else(|| {
                                self.lone_surrogate = true;
                                '\u{FFFD}'
                            })
                        }
                        _ => return Err(invalid_storage()),
                    };
                    if let Some(out) = out.as_deref_mut() {
                        let mut buffer = [0u8; 4];
                        out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                    }
                }
                _ => {
                    if let Some(out) = out.as_deref_mut() {
                        out.push(b);
                    }
                }
            }
        }
    }
}
