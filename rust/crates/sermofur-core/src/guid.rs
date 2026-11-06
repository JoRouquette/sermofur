//! `Guid.TryParse` of .NET, for identifiers typed on the command line
//! (`rust/tests/golden/guid.json`, from `rust/tools/guid-oracle`, and
//! `rust/tests/golden/cli-3-4/guid_forms.json` are the truth): blanks around trimmed, then the
//! formats N (32 digits), D (8-4-4-4-12), B (`{D}`), P (`(D)`) and X
//! (`{0x…,0x…,0x…,{0x…,…}}`), with the compatibility leniencies of the .NET parser: in D, B and
//! P the first four groups may start with `+` and/or `0x`; in X blanks are allowed anywhere,
//! numbers have any length and a 16-bit group keeps only its low bits.

pub use uuid::Uuid;

fn hex(unit: char) -> Option<u32> {
    unit.to_digit(16)
}

/// `TryParseHex(span, out uint, ref overflow)`: an optional `+`, an optional `0x`, then hex
/// digits (none is zero). Returns the value and whether more than 8 significant digits came.
fn parse_hex(text: &[char]) -> Option<(u32, bool)> {
    let mut text = text;
    if text.first() == Some(&'+') {
        text = &text[1..];
    }
    if text.len() > 1 && text[0] == '0' && (text[1] == 'x' || text[1] == 'X') {
        text = &text[2..];
    }
    let start = text.iter().take_while(|c| **c == '0').count();
    let mut value = 0u32;
    let mut digits = 0usize;
    for unit in &text[start..] {
        value = value.wrapping_mul(16).wrapping_add(hex(*unit)?);
        digits += 1;
    }
    Some((value, digits > 8))
}

/// Exactly `text.len()` hex digits, nothing else (`NumberStyles.AllowHexSpecifier`).
fn plain_hex(text: &[char]) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    text.iter().try_fold(0u64, |value, unit| {
        Some(value * 16 + u64::from(hex(*unit)?))
    })
}

fn from_fields(a: u32, b: u16, c: u16, rest: [u8; 8]) -> Uuid {
    Uuid::from_fields(a, b, c, &rest)
}

fn exact_n(text: &[char]) -> Option<Uuid> {
    if text.len() != 32 {
        return None;
    }
    let value = plain_hex(&text[..16])?;
    let low = plain_hex(&text[16..])?;
    Some(Uuid::from_u128((u128::from(value) << 64) | u128::from(low)))
}

fn exact_d(text: &[char]) -> Option<Uuid> {
    if text.len() != 36 || [8, 13, 18, 23].iter().any(|&i| text[i] != '-') {
        return None;
    }
    let group = |from: usize, to: usize| parse_hex(&text[from..to]).map(|(v, _)| v);
    let a = group(0, 8)?;
    let b = group(9, 13)?;
    let c = group(14, 18)?;
    let de = group(19, 23)?;
    let fg = group(24, 28)?;
    let last = u32::try_from(plain_hex(&text[28..36])?).ok()?;
    let [_, _, d, e] = de.to_be_bytes();
    let [_, _, f, g] = fg.to_be_bytes();
    let [h, i, j, k] = last.to_be_bytes();
    Some(from_fields(a, b as u16, c as u16, [d, e, f, g, h, i, j, k]))
}

fn wrapped_d(text: &[char], open: char, close: char) -> Option<Uuid> {
    if text.len() != 38 || text[0] != open || text[37] != close {
        return None;
    }
    exact_d(&text[1..37])
}

fn is_hex_prefix(text: &[char], index: usize) -> bool {
    text.len() > index + 1 && text[index] == '0' && matches!(text[index + 1], 'x' | 'X')
}

fn exact_x(text: &[char]) -> Option<Uuid> {
    let text: Vec<char> = text
        .iter()
        .copied()
        .filter(|c| !c.is_whitespace())
        .collect();
    if text.first() != Some(&'{') || !is_hex_prefix(&text, 1) {
        return None;
    }
    let find = |from: usize, stop: char| -> Option<usize> {
        let length = text.get(from..)?.iter().position(|c| *c == stop)?;
        (length > 0).then_some(length)
    };
    let number = |from: usize, length: usize| -> Option<u32> {
        let (value, overflow) = parse_hex(&text[from..from + length])?;
        (!overflow).then_some(value)
    };
    let mut start = 3;
    let mut length = find(start, ',')?;
    let a = number(start, length)?;
    let mut shorts = [0u16; 2];
    for short in &mut shorts {
        if !is_hex_prefix(&text, start + length + 1) {
            return None;
        }
        start += length + 3;
        length = find(start, ',')?;
        // A 16-bit group is read on 32 bits and keeps its low bits.
        let value = number(start, length)? as u16;
        *short = value;
    }
    if text.get(start + length + 1) != Some(&'{') {
        return None;
    }
    length += 1;
    let mut bytes = [0u8; 8];
    for (index, byte) in bytes.iter_mut().enumerate() {
        if !is_hex_prefix(&text, start + length + 1) {
            return None;
        }
        start += length + 3;
        length = find(start, if index < 7 { ',' } else { '}' })?;
        *byte = u8::try_from(number(start, length)?).ok()?;
    }
    if text.get(start + length + 1) != Some(&'}') || start + length + 2 != text.len() {
        return None;
    }
    Some(from_fields(a, shorts[0], shorts[1], bytes))
}

/// `Guid.TryParse(text)`.
pub fn try_parse(text: &str) -> Option<Uuid> {
    let text: Vec<char> = text.trim().chars().collect();
    if text.len() < 32 {
        return None;
    }
    match text[0] {
        '(' => wrapped_d(&text, '(', ')'),
        '{' if text[9] == '-' => wrapped_d(&text, '{', '}'),
        '{' => exact_x(&text),
        _ if text[8] == '-' => exact_d(&text),
        _ => exact_n(&text),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{Uuid, try_parse};

    /// `Guid.ToString("X")`: `{0xaaaaaaaa,0xbbbb,0xcccc,{0xdd,…}}`.
    fn x_form(id: Uuid) -> String {
        let (a, b, c, rest) = id.as_fields();
        let bytes: Vec<String> = rest.iter().map(|byte| format!("0x{byte:02x}")).collect();
        format!("{{0x{a:08x},0x{b:04x},0x{c:04x},{{{}}}}}", bytes.join(","))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn arbitrary_text_never_panics(text in "\\PC{0,80}") {
            let _ = try_parse(&text);
        }

        // Text made of the characters of the formats, X above all: every index path of the
        // parser, near its accepted syntax.
        #[test]
        fn text_near_the_x_syntax_never_panics(
            text in "[{}0xX,a-fA-F0-9+ \t()-]{0,80}",
            tail in "\\{0[xX][0-9a-fA-F+xX, {}]{0,70}",
        ) {
            let _ = try_parse(&text);
            let _ = try_parse(&tail);
        }

        // Every format of `Guid.ToString` reads back the value it was written from.
        #[test]
        fn every_format_round_trips(value in any::<u128>()) {
            let id = Uuid::from_u128(value);
            let d = id.hyphenated().to_string();
            for form in [
                id.simple().to_string(),
                d.clone(),
                id.braced().to_string(),
                format!("({d})"),
                x_form(id),
                d.to_uppercase(),
                x_form(id).to_uppercase(),
            ] {
                prop_assert_eq!(try_parse(&form), Some(id), "{}", form);
            }
        }
    }

    #[test]
    fn the_usual_forms_read_the_same_value() {
        let d = "0f8fad5b-d9cb-469f-a165-70867728950e";
        let expected = try_parse(d).expect("D");
        for form in [
            "0f8fad5bd9cb469fa16570867728950e",
            "{0f8fad5b-d9cb-469f-a165-70867728950e}",
            "(0F8FAD5B-D9CB-469F-A165-70867728950E)",
            " 0f8fad5b-d9cb-469f-a165-70867728950e\t",
            "{0x0f8fad5b,0xd9cb,0x469f,{0xa1,0x65,0x70,0x86,0x77,0x28,0x95,0x0e}}",
            "{0xf8fad5b, 0xd9cb,0x469f,{0xa1,0x65,0x70,0x86,0x77,0x28,0x95,0xe}}",
            "0x0fad5b-d9cb-469f-a165-70867728950e",
        ] {
            let parsed = try_parse(form);
            assert!(parsed.is_some(), "{form}");
            if !form.starts_with("0x0f") {
                assert_eq!(parsed, Some(expected), "{form}");
            }
        }
        assert_eq!(expected.hyphenated().to_string(), d);
    }

    #[test]
    fn broken_forms_are_refused() {
        for form in [
            "",
            "0f8fad5b-d9cb-469f-a165-70867728950",
            "0f8fad5b-d9cb-469f-a165-70867728950g",
            "{0f8fad5b-d9cb-469f-a165-70867728950e)",
            "0f8fad5b-d9cb-469f-a165+70867728950e",
        ] {
            assert_eq!(try_parse(form), None, "{form}");
        }
    }
}
