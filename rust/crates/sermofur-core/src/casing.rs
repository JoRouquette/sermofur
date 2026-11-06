//! The two case foldings of .NET that paths meet, observed on the oracle
//! (`rust/tests/golden/localpaths.json`):
//!
//! - `StringComparison.OrdinalIgnoreCase` (`OrdinalCasing`): the simple upper case of each code
//!   point, surrogate pairs included, except `ı` (U+0131) and `ſ` (U+017F), which stay;
//! - `char.ToUpperInvariant`, one UTF-16 unit at a time: the simple upper case of the unit,
//!   except `ı`, which stays; `ſ` becomes `S`; a surrogate stays.

/// The simple upper case of a code point (`UnicodeData.txt`, field 12). The standard library
/// gives the full mapping; it differs only where the full mapping has several characters: then
/// the simple mapping is the char itself, except for the Greek letters with ypogegrammeni.
fn simple_upper(c: char) -> char {
    let mut mapped = c.to_uppercase();
    if let (Some(single), None) = (mapped.next(), mapped.next()) {
        return single;
    }
    let code = u32::from(c);
    let simple = match code {
        0x1F80..=0x1F87 | 0x1F90..=0x1F97 | 0x1FA0..=0x1FA7 => code + 8,
        0x1FB3 | 0x1FC3 | 0x1FF3 => code + 9,
        _ => code,
    };
    char::from_u32(simple).unwrap_or(c)
}

/// The upper case of a code point under `OrdinalIgnoreCase`.
fn ordinal_upper(c: char) -> char {
    if c == '\u{131}' || c == '\u{17F}' {
        c
    } else {
        simple_upper(c)
    }
}

/// UTF-16 units folded as `OrdinalIgnoreCase` compares them; a lone surrogate stays. Simple
/// mappings keep the plane, so the length does not change.
pub(crate) fn ordinal_fold(units: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(units.len());
    for decoded in char::decode_utf16(units.iter().copied()) {
        match decoded {
            Ok(c) => {
                let mut buffer = [0u16; 2];
                out.extend_from_slice(ordinal_upper(c).encode_utf16(&mut buffer));
            }
            Err(lone) => out.push(lone.unpaired_surrogate()),
        }
    }
    out
}

/// Equality of two UTF-16 strings under `OrdinalIgnoreCase`.
pub(crate) fn ordinal_ignore_case_equal(a: &[u16], b: &[u16]) -> bool {
    a.len() == b.len() && ordinal_fold(a) == ordinal_fold(b)
}

/// `char.ToUpperInvariant` of one UTF-16 unit.
pub(crate) fn invariant_upper(unit: u16) -> u16 {
    match char::from_u32(u32::from(unit)) {
        Some('\u{131}') | None => unit,
        Some(c) => u16::try_from(u32::from(simple_upper(c))).unwrap_or(unit),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(text: &str) -> Vec<u16> {
        text.encode_utf16().collect()
    }

    #[test]
    fn ordinal_keeps_dotless_i_and_long_s_and_folds_pairs() {
        assert!(!ordinal_ignore_case_equal(&units("\u{131}s"), &units("is")));
        assert!(!ordinal_ignore_case_equal(&units("\u{17F}"), &units("S")));
        assert!(ordinal_ignore_case_equal(
            &units("\u{10428}x"),
            &units("\u{10400}X")
        ));
        assert!(ordinal_ignore_case_equal(
            &units("caf\u{e9}"),
            &units("CAF\u{c9}")
        ));
        assert!(ordinal_ignore_case_equal(
            &units("\u{1F80}"),
            &units("\u{1F88}")
        ));
        assert!(!ordinal_ignore_case_equal(&units("\u{df}"), &units("SS")));
    }

    #[test]
    fn invariant_keeps_dotless_i_folds_long_s_and_leaves_surrogates() {
        assert_eq!(invariant_upper(0x131), 0x131);
        assert_eq!(invariant_upper(0x17F), u16::from(b'S'));
        assert_eq!(invariant_upper(u16::from(b'a')), u16::from(b'A'));
        assert_eq!(invariant_upper(0xD801), 0xD801);
        assert_eq!(invariant_upper(0x1FB3), 0x1FBC);
    }
}
