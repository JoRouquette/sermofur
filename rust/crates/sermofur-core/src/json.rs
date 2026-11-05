//! JSON written the way the .NET `RecordJson` writes it.
//!
//! `serde_json` already indents with two spaces, like `System.Text.Json` with `WriteIndented`.
//! What differs is the escaping of strings: the .NET encoder (`JavaScriptEncoder` over
//! `UnicodeRanges.All`) keeps accented letters readable but escapes `<`, `>`, `&`, `'`, `+`,
//! the backtick and the double quote as `\uXXXX`, every control character, every scalar that
//! its Unicode tables do not allow, and writes characters above U+FFFF as a surrogate pair.
//! This module reproduces that rule; the table of allowed scalars is generated from the .NET
//! runtime (`rust/tools/escape-oracle`) and checked against it by `tests/escape.rs`.

use std::fmt::Write as _;
use std::io;

use serde::Serialize;
use serde_json::ser::{CharEscape, Formatter, PrettyFormatter, Serializer};

use crate::NEW_LINE;
use crate::dotnet_escape_table::UNICODE_ESCAPED_RANGES;

/// Serializes a value as indented JSON, with the string escaping of the .NET code.
///
/// Field names come from the `#[serde(rename_all = "camelCase")]` of the serialized types,
/// and enums are written in `snake_case`, as the .NET options do. Lines are separated by the
/// line break of the platform ([`NEW_LINE`]), because `System.Text.Json` writes
/// `Environment.NewLine` when `WriteIndented` is on.
pub fn to_string<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<String> {
    let mut buffer: Vec<u8> = Vec::new();
    let mut serializer = Serializer::with_formatter(&mut buffer, DotNetFormatter::new());
    value.serialize(&mut serializer)?;
    // The formatter only writes valid UTF-8; the lossy conversion never changes anything.
    let text = String::from_utf8_lossy(&buffer);
    // `serde_json` separates lines with `\n`. A line feed inside a JSON string is always
    // escaped (`\n` as two characters), so every raw `\n` here is a line separator.
    Ok(text.replace('\n', NEW_LINE))
}

/// Appends a scalar value to `out`, escaped as the .NET encoder does.
pub(crate) fn push_escaped(out: &mut String, c: char) {
    let scalar = u32::from(c);
    match scalar {
        0x08 => out.push_str("\\b"),
        0x09 => out.push_str("\\t"),
        0x0A => out.push_str("\\n"),
        0x0C => out.push_str("\\f"),
        0x0D => out.push_str("\\r"),
        0x5C => out.push_str("\\\\"),
        _ if scalar > 0xFFFF => {
            // Above the Basic Multilingual Plane: a surrogate pair, one `\uXXXX` per unit.
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                let _ = write!(out, "\\u{:04X}", unit);
            }
        }
        _ if is_escaped(scalar) => {
            let _ = write!(out, "\\u{scalar:04X}");
        }
        _ => out.push(c),
    }
}

/// True if the scalar is in one of the generated ranges that the .NET encoder escapes.
fn is_escaped(scalar: u32) -> bool {
    UNICODE_ESCAPED_RANGES
        .binary_search_by(|&(start, end)| {
            if end < scalar {
                std::cmp::Ordering::Less
            } else if start > scalar {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// `serde_json`'s pretty formatter, with the string escaping of the .NET encoder.
struct DotNetFormatter {
    inner: PrettyFormatter<'static>,
}

impl DotNetFormatter {
    fn new() -> Self {
        Self {
            inner: PrettyFormatter::with_indent(b"  "),
        }
    }
}

impl Formatter for DotNetFormatter {
    // Structure (brackets, commas, indentation): delegated to the pretty formatter.
    fn begin_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.begin_array(writer)
    }

    fn end_array<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.end_array(writer)
    }

    fn begin_array_value<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.inner.begin_array_value(writer, first)
    }

    fn end_array_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.end_array_value(writer)
    }

    fn begin_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.begin_object(writer)
    }

    fn end_object<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.end_object(writer)
    }

    fn begin_object_key<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        first: bool,
    ) -> io::Result<()> {
        self.inner.begin_object_key(writer, first)
    }

    fn end_object_key<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.end_object_key(writer)
    }

    fn begin_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.begin_object_value(writer)
    }

    fn end_object_value<W: ?Sized + io::Write>(&mut self, writer: &mut W) -> io::Result<()> {
        self.inner.end_object_value(writer)
    }

    // Strings: `serde_json` hands over the runs of characters it does not escape itself...
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut escaped = String::with_capacity(fragment.len());
        for c in fragment.chars() {
            push_escaped(&mut escaped, c);
        }
        writer.write_all(escaped.as_bytes())
    }

    // ...and the characters it escapes itself (quote, backslash, controls), which the .NET
    // encoder escapes differently: they go through the same rule.
    fn write_char_escape<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        char_escape: CharEscape,
    ) -> io::Result<()> {
        let c = match char_escape {
            CharEscape::Quote => '"',
            CharEscape::ReverseSolidus => '\\',
            CharEscape::Solidus => '/',
            CharEscape::Backspace => '\u{8}',
            CharEscape::FormFeed => '\u{c}',
            CharEscape::LineFeed => '\n',
            CharEscape::CarriageReturn => '\r',
            CharEscape::Tab => '\t',
            CharEscape::AsciiControl(byte) => char::from(byte),
        };
        let mut escaped = String::new();
        push_escaped(&mut escaped, c);
        writer.write_all(escaped.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde::Serialize;

    use super::to_string;
    use crate::NEW_LINE;

    #[derive(Serialize)]
    struct Version<'a> {
        version: &'a str,
    }

    #[derive(Serialize)]
    struct Empty {
        items: Vec<u32>,
        nested: Nested,
    }

    #[derive(Serialize)]
    struct Nested {}

    #[test]
    fn objects_are_indented_with_two_spaces() {
        let json = to_string(&Version {
            version: "0.0.0-dev",
        })
        .expect("serializes");
        let expected = "{\n  \"version\": \"0.0.0-dev\"\n}".replace('\n', NEW_LINE);
        assert_eq!(json, expected);
    }

    #[test]
    fn empty_array_and_object_stay_on_one_line() {
        let json = to_string(&Empty {
            items: vec![],
            nested: Nested {},
        })
        .expect("serializes");
        let expected = "{\n  \"items\": [],\n  \"nested\": {}\n}".replace('\n', NEW_LINE);
        assert_eq!(json, expected);
    }

    #[test]
    fn accents_stay_readable_and_html_characters_are_escaped() {
        let json = to_string("é <b> & 'a' + `c` \"q\"").expect("serializes");
        assert_eq!(
            json,
            "\"é \\u003Cb\\u003E \\u0026 \\u0027a\\u0027 \\u002B \\u0060c\\u0060 \\u0022q\\u0022\""
        );
    }

    #[test]
    fn short_escapes_and_controls() {
        let json = to_string("a\nb\tc\u{1}\u{7f}\\").expect("serializes");
        assert_eq!(json, "\"a\\nb\\tc\\u0001\\u007F\\\\\"");
    }

    #[test]
    fn characters_above_the_basic_plane_become_surrogate_pairs() {
        let json = to_string("😀").expect("serializes");
        assert_eq!(json, "\"\\uD83D\\uDE00\"");
    }

    /// Writes `text` as JSON, reads it back with `serde_json`, and checks that it is the same
    /// text and that none of the characters that the .NET encoder escapes stays as it is.
    fn round_trip(text: &str) -> Result<(), TestCaseError> {
        let json = to_string(text).expect("a string always serializes");
        let back: String = serde_json::from_str(&json).expect("the output is valid JSON");
        prop_assert_eq!(back, text);
        prop_assert!(!json.contains(['<', '>', '&', '\'', '+', '`']));
        Ok(())
    }

    #[test]
    fn fixed_strings_round_trip() {
        for text in [
            "",
            "😀".repeat(16_384).as_str(),
            "\u{0}\u{1f}\u{7f}\u{85}\u{a0}",
        ] {
            round_trip(text).expect("round trip");
        }
    }

    proptest! {
        // Port of the idea of the arbitrary-string property: any text, long runs of controls
        // included, comes back unchanged from the writer.
        #[test]
        fn any_string_round_trips(text in "\\PC{0,200}|[\\x00-\\x1f]{0,8}") {
            round_trip(&text)?;
        }
    }
}
