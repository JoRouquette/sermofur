//! Reading `.sermofur/instance.json`, as `InstanceManager.ReadConfiguration` of the .NET 0.4.3
//! oracle does (`rust/tests/golden/instance-config.json` is the truth).
//!
//! `serde_json` alone does not reproduce `System.Text.Json` (lone surrogate escapes, depth), so
//! a small reader follows the .NET rules, listed on [`parse_configuration`].

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

use crate::error::{Result, SermofurError};
use crate::format::{DateTimeOffset, parse_json_text};

/// Current format of `instance.json`.
pub const SCHEMA_VERSION: i32 = 2;
/// Format of the 0.1 CLI: read only by migrate, doctor and root.
pub const LEGACY_SCHEMA_VERSION: i32 = 1;
/// Upper bound of `instance.json`: beyond it the instance is damaged.
pub const MAX_CONFIGURATION_BYTES: u64 = 64 * 1024;
/// Name of the folder that marks an instance.
pub const MARKER: &str = ".sermofur";
/// Name of the configuration file.
pub const CONFIGURATION_FILE: &str = "instance.json";

/// Content of `instance.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceConfiguration {
    pub schema_version: i32,
    pub instance_id: Uuid,
    pub created_at: DateTimeOffset,
}

/// `unsafe_path`, exit 4: `LocalPaths.LinkNotSupported`.
fn link_not_supported() -> SermofurError {
    SermofurError::new(
        "unsafe_path",
        "Links and junctions are not supported in this version.",
        4,
    )
}

/// `invalid_storage_or_path`, exit 3: `CommandRunner` maps JSON and IO failures to it.
fn invalid_storage() -> SermofurError {
    SermofurError::new(
        "invalid_storage_or_path",
        "Invalid path or data; run doctor.",
        3,
    )
}

fn invalid_instance(message: &str) -> SermofurError {
    SermofurError::new("invalid_instance", message, 3)
}

/// Refuses a link or a junction on `path` or on any ancestor (`LocalPaths.RejectLinks`).
/// An entry that does not exist is skipped.
pub fn reject_links(path: &Path) -> Result<()> {
    let full = normalize(&std::path::absolute(path).map_err(|_| invalid_storage())?);
    for entry in full.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(entry)
            && is_link(&metadata)
        {
            return Err(link_not_supported());
        }
    }
    Ok(())
}

/// `Path.GetFullPath` removes `.` and `..` lexically, without looking at the disk; `..` at the
/// root stays at the root. `std::path::absolute` keeps `..` on Unix, so it is removed here.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(windows)]
fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Reads `<root>/.sermofur/instance.json`. A format 1 instance is refused with
/// `migration_required` unless `allow_legacy`. `local_offset_minutes` serves a `createdAt`
/// without offset (see [`parse_json_text`]).
pub fn read_configuration(
    root: &Path,
    allow_legacy: bool,
    local_offset_minutes: impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let file = root.join(MARKER).join(CONFIGURATION_FILE);
    // Every ancestor of the file includes the ones of the marker folder.
    reject_links(&file)?;
    // `File.Exists` is false for a directory: the file is missing.
    if !fs::metadata(&file).is_ok_and(|m| m.is_file()) {
        return Err(invalid_instance("Instance configuration missing."));
    }
    // One bounded read: a file that grows after the check is still refused beyond the cap.
    let mut bytes = Vec::new();
    fs::File::open(&file)
        .and_then(|f| f.take(MAX_CONFIGURATION_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| invalid_storage())?;
    if bytes.len() as u64 > MAX_CONFIGURATION_BYTES {
        return Err(invalid_instance(
            "Damaged Sermofur instance: instance.json exceeds 64 KiB; restore it from a backup.",
        ));
    }
    interpret(&bytes, allow_legacy, local_offset_minutes)
}

/// Everything after the read: decoding, parsing, identity and version checks.
fn interpret(
    bytes: &[u8],
    allow_legacy: bool,
    local_offset_minutes: impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let configuration = parse_configuration(&decode(bytes), &local_offset_minutes)?;
    if configuration.instance_id.is_nil() {
        return Err(SermofurError::new(
            "unsupported_schema",
            "Invalid instance identity.",
            3,
        ));
    }
    match configuration.schema_version {
        LEGACY_SCHEMA_VERSION if allow_legacy => Ok(configuration),
        LEGACY_SCHEMA_VERSION => Err(SermofurError::new(
            "migration_required",
            "Instance in format 1; back it up if you wish, then run smf migrate.",
            3,
        )),
        SCHEMA_VERSION => Ok(configuration),
        _ => Err(SermofurError::new(
            "unsupported_schema",
            "Unsupported instance identity or version.",
            3,
        )),
    }
}

/// `File.ReadAllText`: the byte order mark picks UTF-8, UTF-16 or UTF-32 (the BOM is dropped);
/// without one the text is UTF-8. Invalid sequences become U+FFFD.
fn decode(bytes: &[u8]) -> String {
    // StreamReader flushes an incomplete last unit as one U+FFFD.
    let units16 = |data: &[u8], little: bool| -> String {
        let (chunks, rest) = data.as_chunks::<2>();
        let units: Vec<u16> = chunks
            .iter()
            .map(|c| {
                if little {
                    u16::from_le_bytes(*c)
                } else {
                    u16::from_be_bytes(*c)
                }
            })
            .collect();
        let mut text = String::from_utf16_lossy(&units);
        if !rest.is_empty() {
            text.push('\u{FFFD}');
        }
        text
    };
    let units32 = |data: &[u8], little: bool| -> String {
        let (chunks, rest) = data.as_chunks::<4>();
        let mut text: String = chunks
            .iter()
            .map(|c| {
                let v = if little {
                    u32::from_le_bytes(*c)
                } else {
                    u32::from_be_bytes(*c)
                };
                char::from_u32(v).unwrap_or('\u{FFFD}')
            })
            .collect();
        if !rest.is_empty() {
            text.push('\u{FFFD}');
        }
        text
    };
    match bytes {
        [0xFE, 0xFF, rest @ ..] => units16(rest, false),
        [0xFF, 0xFE, 0, 0, rest @ ..] => units32(rest, true),
        [0xFF, 0xFE, rest @ ..] => units16(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0, 0, 0xFE, 0xFF, rest @ ..] => units32(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A JSON value, reduced to what the configuration needs.
enum Value {
    Null,
    /// Raw text of a number.
    Number(String),
    Text(String),
    /// A boolean, an array or an object.
    Other,
}

/// Maximum depth of `System.Text.Json` (`JsonReaderOptions.MaxDepth` default).
const MAX_DEPTH: usize = 64;

/// Deserializes `InstanceConfiguration` like `JsonSerializer.Deserialize` with `RecordJson`
/// options. Rules reproduced:
/// - strict JSON (RFC 8259): no comment, trailing comma, single quote or trailing content;
///   whitespace is space, tab, CR, LF; numbers without leading zero; raw control characters
///   in strings refused; escapes `\" \\ \/ \b \f \n \r \t \uXXXX`;
/// - nesting deeper than 64 is refused (the top-level object is depth 1);
/// - a lone surrogate escape is accepted in a skipped value (it reads as U+FFFD, which never
///   matches a name, a GUID or a date);
/// - property names are matched case-sensitively after unescaping; unknown ones are skipped;
///   a duplicate is converted at once, so a bad earlier one fails and the last good one wins;
/// - missing properties take the defaults (0, empty GUID, `0001-01-01T00:00:00+00:00`);
/// - `schemaVersion` is a JSON integer in `i32` without `.` or exponent; `instanceId` is a
///   GUID in format D; `createdAt` goes through [`parse_json_text`]; `null` is refused for all
///   three; a top-level `null` is `invalid_record`, any other non-object is a JSON error.
fn parse_configuration(
    text: &str,
    local_offset_minutes: &impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let mut reader = Reader {
        bytes: text.as_bytes(),
        pos: 0,
        lone_surrogate: false,
    };
    reader.skip_space();
    let mut configuration = InstanceConfiguration {
        schema_version: 0,
        instance_id: Uuid::nil(),
        created_at: DateTimeOffset::new(0, 0).ok_or_else(invalid_storage)?,
    };
    match reader.peek() {
        Some(b'n') => {
            reader.literal(b"null")?;
            reader.end()?;
            return Err(SermofurError::new(
                "invalid_record",
                "Invalid persisted object.",
                3,
            ));
        }
        Some(b'{') => {}
        _ => return Err(invalid_storage()),
    }
    reader.pos += 1;
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
            // STJ reads the text of a name of the object it fills: a lone surrogate escape
            // there is an error (a name inside a skipped value is never read).
            if reader.lone_surrogate {
                return Err(invalid_storage());
            }
            reader.skip_space();
            reader.expect(b':')?;
            let value = reader.value(1)?;
            match (name.as_str(), value) {
                ("schemaVersion", Value::Number(raw)) => {
                    configuration.schema_version = parse_i32(&raw)?;
                }
                ("instanceId", Value::Text(raw)) => {
                    configuration.instance_id = parse_guid(&raw)?;
                }
                ("createdAt", Value::Text(raw)) => {
                    configuration.created_at =
                        parse_json_text(&raw, local_offset_minutes).ok_or_else(invalid_storage)?;
                }
                ("schemaVersion" | "instanceId" | "createdAt", _) => {
                    return Err(invalid_storage());
                }
                _ => {}
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
    Ok(configuration)
}

/// `Int32` as `Utf8Parser` reads it: optional `-`, digits only, in range.
fn parse_i32(raw: &str) -> Result<i32> {
    let digits = raw.strip_prefix('-').unwrap_or(raw);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid_storage());
    }
    raw.parse().map_err(|_| invalid_storage())
}

/// A GUID in format D: 8-4-4-4-12 hexadecimal digits, any case, nothing around.
fn parse_guid(raw: &str) -> Result<Uuid> {
    let b = raw.as_bytes();
    if b.len() != 36 || [8, 13, 18, 23].iter().any(|&i| b[i] != b'-') {
        return Err(invalid_storage());
    }
    Uuid::parse_str(raw).map_err(|_| invalid_storage())
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
    fn value(&mut self, depth: usize) -> Result<Value> {
        self.skip_space();
        match self.peek().ok_or_else(invalid_storage)? {
            b'"' => self.string().map(Value::Text),
            b'n' => self.literal(b"null").map(|()| Value::Null),
            b't' => self.literal(b"true").map(|()| Value::Other),
            b'f' => self.literal(b"false").map(|()| Value::Other),
            b'-' | b'0'..=b'9' => self.number().map(Value::Number),
            b'[' => {
                self.container(depth, b']', false)?;
                Ok(Value::Other)
            }
            b'{' => {
                self.container(depth, b'}', true)?;
                Ok(Value::Other)
            }
            _ => Err(invalid_storage()),
        }
    }

    /// Skips an array or an object whose opening bracket is at `pos`.
    fn container(&mut self, depth: usize, close: u8, object: bool) -> Result<()> {
        if depth + 1 > MAX_DEPTH {
            return Err(invalid_storage());
        }
        self.pos += 1;
        self.skip_space();
        if self.peek() == Some(close) {
            self.pos += 1;
            return Ok(());
        }
        loop {
            if object {
                self.skip_space();
                if self.peek() != Some(b'"') {
                    return Err(invalid_storage());
                }
                self.string()?;
                self.skip_space();
                self.expect(b':')?;
            }
            self.value(depth + 1)?;
            self.skip_space();
            match self.next() {
                Some(b',') => {}
                Some(b) if b == close => return Ok(()),
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
        Ok(String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned())
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
        self.pos += 1;
        self.lone_surrogate = false;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let b = self.next().ok_or_else(invalid_storage)?;
            match b {
                b'"' => return Ok(String::from_utf8_lossy(&out).into_owned()),
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
                    let mut buffer = [0u8; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                _ => out.push(b),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const VALID: &str = "{\n  \"schemaVersion\": 2,\n  \"instanceId\": \"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\",\n  \"createdAt\": \"2026-11-06T11:00:00.1234567+00:00\"\n}";

    /// The bytes of `text` behind the byte order mark number `bom` (0 to 5) and in its encoding.
    fn encode(text: &str, bom: usize) -> Vec<u8> {
        let units16 = |little: bool| -> Vec<u8> {
            text.encode_utf16()
                .flat_map(|u| {
                    if little {
                        u.to_le_bytes()
                    } else {
                        u.to_be_bytes()
                    }
                })
                .collect()
        };
        let units32 = |little: bool| -> Vec<u8> {
            text.chars()
                .flat_map(|c| {
                    if little {
                        u32::from(c).to_le_bytes()
                    } else {
                        u32::from(c).to_be_bytes()
                    }
                })
                .collect()
        };
        let (prefix, body): (&[u8], Vec<u8>) = match bom {
            1 => (&[0xEF, 0xBB, 0xBF], text.as_bytes().to_vec()),
            2 => (&[0xFE, 0xFF], units16(false)),
            3 => (&[0xFF, 0xFE], units16(true)),
            4 => (&[0xFF, 0xFE, 0, 0], units32(true)),
            5 => (&[0, 0, 0xFE, 0xFF], units32(false)),
            _ => (&[], text.as_bytes().to_vec()),
        };
        [prefix, &body].concat()
    }

    /// Whatever the input, no panic; a success has a real identity and a readable version.
    fn check(bytes: &[u8]) {
        for legacy in [false, true] {
            if let Ok(c) = interpret(bytes, legacy, |_| 60) {
                assert!(!c.instance_id.is_nil());
                assert!(c.schema_version == 1 || c.schema_version == 2);
            }
        }
    }

    const FRAGMENTS: &[&str] = &[
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        " ",
        "\n",
        "\"a\"",
        "\\u00",
        "\\ud800",
        "\\udc00",
        "\\",
        "\\\"",
        "\"",
        "1",
        "-",
        "0",
        "1e5",
        "-0",
        ".5",
        "null",
        "true",
        "\"schemaVersion\"",
        "\"instanceId\"",
        "\"createdAt\"",
        "\"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\"",
        "2",
        "1",
        "\"2026-11-06T11:00:00Z\"",
        "\u{feff}",
        "\u{0}",
    ];

    proptest! {
        #[test]
        fn mutated_valid_documents_never_panic(
            ops in proptest::collection::vec((0u8..3, any::<usize>(), any::<u8>()), 0..6),
            bom in 0usize..6,
        ) {
            let mut bytes = VALID.as_bytes().to_vec();
            for (kind, index, byte) in ops {
                let at = index % (bytes.len() + 1);
                match kind {
                    0 if at < bytes.len() => bytes[at] = byte,
                    1 => bytes.insert(at, byte),
                    _ => bytes.truncate(at),
                }
            }
            check(&encode(&String::from_utf8_lossy(&bytes), bom));
        }

        #[test]
        fn truncated_encodings_never_panic(bom in 0usize..6, cut in 0usize..400) {
            let mut bytes = encode(VALID, bom);
            bytes.truncate(cut.min(bytes.len()));
            check(&bytes);
        }

        #[test]
        fn open_brackets_never_panic(open in 0usize..200, close in 0usize..200, bom in 0usize..6) {
            let text = format!(
                "{{\"schemaVersion\":2,\"instanceId\":\"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\",\"x\":{}{}}}",
                "[".repeat(open),
                "]".repeat(close)
            );
            check(&encode(&text, bom));
        }

        #[test]
        fn assembled_fragments_never_panic(
            parts in proptest::collection::vec(0..FRAGMENTS.len(), 0..14),
            wrap in any::<bool>(),
            bom in 0usize..6,
        ) {
            let body: String = parts.into_iter().map(|i| FRAGMENTS[i]).collect();
            let text = if wrap { format!("{{{body}}}") } else { body };
            check(&encode(&text, bom));
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..=512)) {
            check(&bytes);
        }
    }

    #[test]
    fn the_valid_document_is_read_in_every_encoding() {
        for bom in 0..6 {
            let c = interpret(&encode(VALID, bom), false, |_| 60).expect("valid");
            assert_eq!(c.schema_version, 2, "bom {bom}");
        }
    }

    #[test]
    fn dot_segments_are_removed_lexically() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalize(Path::new("/a/../..")), PathBuf::from("/"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_cancelled_by_dot_dot_is_not_seen() {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("temp")
            .join(format!("sermofur-dotdot-{}", Uuid::new_v4()));
        std::fs::create_dir_all(base.join("target")).expect("dirs");
        std::os::unix::fs::symlink(base.join("target"), base.join("link")).expect("link");
        // GetFullPath turns `link/..` into the base itself, which has no link.
        let through = reject_links(&base.join("link").join(".."));
        let direct = reject_links(&base.join("link"));
        let _ = std::fs::remove_dir_all(&base);
        assert!(through.is_ok());
        assert_eq!(direct.expect_err("link").code, "unsafe_path");
    }
}
