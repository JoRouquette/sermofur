//! The search of a term in a text, case and accents ignored, as `CompareInfo.IndexOf` of the
//! invariant culture with `IgnoreCase | IgnoreNonSpace` does it for the excerpt of `recall`
//! (decision of John of 2026-11-06, option B restricted).
//!
//! Each scalar value weighs a list of primary weights: none (an ignorable), one, or several (an
//! expansion: `ß` weighs `s s`). The table is `rust/tests/golden/collation-table.txt`, generated
//! by `rust/tools/collation-oracle` from the sort keys of the .NET oracle and checked against its
//! `IndexOf` answers (`tests/collation_golden.rs`). A match starts and ends on whole scalars:
//! never inside an expansion, and never on a leading ignorable.
//!
//! Not modelled: weights that depend on the neighbours (contractions such as `и` + U+0306, an
//! Arabic letter + hamza, Indic two-part vowels; reordering of the Thai, Lao, Tai Viet and New
//! Tai Lue preposed vowels). They are a named deviation of `PARITY.md`.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The generated table: scalar, then the names of its weights, in hexadecimal.
const TABLE: &str = include_str!("../../../tests/golden/collation-table.txt");

/// The lines of the table that hold an entry.
fn entries() -> impl Iterator<Item = &'static str> {
    TABLE
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// The weights of the scalars listed in the table; any other scalar weighs itself. A malformed
/// line or a scalar listed twice is a bug of the generated data: it panics.
fn table() -> &'static HashMap<u32, Vec<u32>> {
    static PARSED: OnceLock<HashMap<u32, Vec<u32>>> = OnceLock::new();
    PARSED.get_or_init(|| {
        let mut parsed = HashMap::new();
        for line in entries() {
            let fields: Vec<u32> = line
                .split(' ')
                .map(|field| {
                    u32::from_str_radix(field, 16)
                        .unwrap_or_else(|_| panic!("malformed collation table line {line:?}"))
                })
                .collect();
            let previous = parsed.insert(fields[0], fields[1..].to_vec());
            assert!(previous.is_none(), "collation table lists {line:?} twice");
        }
        parsed
    })
}

/// The weights of `text`, flattened, and for each scalar its UTF-16 offset and the range of its
/// weights.
fn weigh(text: &str) -> (Vec<u32>, Vec<(usize, usize, usize)>) {
    let table = table();
    let mut weights = Vec::with_capacity(text.len());
    let mut scalars = Vec::with_capacity(text.len());
    let mut offset = 0;
    for c in text.chars() {
        let start = weights.len();
        match table.get(&u32::from(c)) {
            Some(listed) => weights.extend_from_slice(listed),
            None => weights.push(u32::from(c)),
        }
        scalars.push((offset, start, weights.len()));
        offset += c.len_utf16();
    }
    (weights, scalars)
}

/// The UTF-16 offset of the first occurrence of `term` in `text`, as `IndexOf` with
/// `IgnoreCase | IgnoreNonSpace`; a term that weighs nothing is found at 0.
pub fn index_of(text: &str, term: &str) -> Option<usize> {
    let (wanted, _) = weigh(term);
    if wanted.is_empty() {
        return Some(0);
    }
    let (weights, scalars) = weigh(text);
    'start: for (index, &(offset, start, end)) in scalars.iter().enumerate() {
        if start == end {
            continue;
        }
        let mut matched = 0;
        for &(_, start, end) in &scalars[index..] {
            if matched == wanted.len() {
                break;
            }
            let part = &weights[start..end];
            if wanted.get(matched..matched + part.len()) != Some(part) {
                continue 'start;
            }
            matched += part.len();
        }
        if matched == wanted.len() {
            return Some(offset);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{entries, index_of, table};

    /// Scalars of the table that weigh nothing, or that weigh several weights.
    fn listed(ignorable: bool) -> Vec<char> {
        let mut found: Vec<char> = table()
            .iter()
            .filter(|(_, w)| w.is_empty() == ignorable && (ignorable || w.len() > 1))
            .filter_map(|(scalar, _)| char::from_u32(*scalar))
            .collect();
        found.sort_unstable();
        found
    }

    /// Text drawn from ignorables, expansions, ASCII and the supplementary planes, or any text.
    fn text() -> impl Strategy<Value = String> {
        let symbol = prop_oneof![
            prop::sample::select(listed(true)),
            prop::sample::select(listed(false)),
            proptest::char::range(' ', '~'),
            proptest::char::range('\u{10000}', '\u{10FFFF}'),
        ];
        prop_oneof![
            prop::collection::vec(symbol, 0..24).prop_map(|c| c.into_iter().collect()),
            any::<String>(),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn index_of_lands_on_a_scalar(text in text(), term in text()) {
            let starts: Vec<usize> = text
                .chars()
                .scan(0, |offset, c| {
                    let start = *offset;
                    *offset += c.len_utf16();
                    Some(start)
                })
                .chain(std::iter::once(text.encode_utf16().count()))
                .collect();
            if let Some(found) = index_of(&text, &term) {
                prop_assert!(starts.contains(&found));
            }
        }

        #[test]
        fn a_term_without_weight_is_found_at_the_start(
            text in text(),
            term in prop::collection::vec(prop::sample::select(listed(true)), 0..6),
        ) {
            let term: String = term.into_iter().collect();
            prop_assert_eq!(index_of(&text, &term), Some(0));
        }

        #[test]
        fn printable_ascii_is_searched_without_case(
            text in "[ -~]{0,40}",
            term in "[ -~]{1,4}",
        ) {
            let naive = text.to_ascii_lowercase().find(&term.to_ascii_lowercase());
            prop_assert_eq!(index_of(&text, &term), naive);
        }
    }

    /// Every line of the table is one entry, and their number is the one the oracle wrote.
    #[test]
    fn the_table_holds_every_line_the_oracle_wrote() {
        let probes: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/golden/collation-probes.json"))
                .expect("JSON");
        let written = usize::try_from(probes["entries"].as_u64().expect("entries")).expect("size");
        assert_eq!(entries().count(), table().len());
        assert_eq!(table().len(), written);
    }
}
