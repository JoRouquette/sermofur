//! `sermofur_core::collation::index_of` against the `IndexOf` answers of the .NET oracle
//! (`rust/tests/golden/collation-probes.json`, produced with the table by
//! `rust/tools/collation-oracle`): every probe must give the same UTF-16 offset, or no match,
//! except the context-dependent cases of [`CONTEXT_DEVIATIONS`] (named deviation of `PARITY.md`,
//! decision of John of 2026-11-06, option B restricted). A listed case that becomes equal fails
//! too.

use serde_json::Value;
use sermofur_core::collation::index_of;

/// Probes `(text, term)`, as scalar values in hexadecimal, where `IndexOf` depends on the
/// neighbours of a character, which the per-scalar table does not model.
const CONTEXT_DEVIATIONS: &[(&str, &str)] = &[
    // B: New Tai Lue preposed vowels reorder with the consonant that follows.
    ("19DE 19DE", "19B6 199C"),
    // D: a combining letter with a primary weight belongs to the grapheme
    // of its base; no match starts on it.
    ("0078 036A 0079", "0048"),
    ("0078 0F9D 0079", "0F9C 0FB7"),
    ("0078 0FB9 0079", "0F90 0FB5"),
    ("0078 1A59 0079", "1A26"),
    ("0078 1ACE 0079", "0054"),
    ("0078 1DD5 0079", "0041 004F"),
    ("0078 1DD9 0079", "0044"),
    ("0078 1DE1 0079", "0274"),
    ("0078 1DEA 0079", "018F"),
    ("0078 1DF3 0079", "004F"),
    ("0078 2DE6 0079", "040C"),
    ("0078 2DEE 0079", "0422"),
    ("0078 2DF6 0079", "0410"),
    ("0078 A677 0079", "040E"),
    ("0078 10376 0079", "10350"),
    ("0078 11A3B 0079", "11A2A"),
    ("0078 11A8A 0079", "11A5C"),
    ("0078 11A92 0079", "11A7D"),
    ("0078 1E008 0079", "2C08"),
    ("0078 1E010 0079", "2C10"),
    ("0078 1E018 0079", "2C18"),
    ("0078 1E023 0079", "2C23"),
    // D: a prepended mark (Prepend) joins the grapheme that follows; no match starts after it.
    ("0600 0061 0062", "0061 0062"),
    ("0890 0061 0062", "0061 0062"),
    ("110BD 0061 0062", "0061 0062"),
    ("0078 0601 0061 0062", "0061 0062"),
    // D: a spacing mark (SpacingMark) belongs to the grapheme of its base.
    ("0078 0915 093F 0079", "0915"),
    ("0078 0915 093F 0079", "093F"),
    // D: Hangul jamo sequences (L V T) form one grapheme with a syllable.
    ("0078 1100 1161 11A8 0079", "1100"),
    ("0078 1100 1161 11A8 0079", "1161"),
    ("0078 1100 1161 11A8 0079", "11A8"),
    ("0078 1100 1161 11A8 0079", "1100 1161"),
    ("0078 1100 1161 11A8 0079", "1161 11A8"),
    ("0078 1100 1161 11A8 0079", "AC00"),
    ("AC00 11A8", "11A8"),
    ("AC00 11A8", "AC00"),
    ("1100 1100 1161", "1100 1161"),
    ("1100 1100 1161", "1100"),
    ("1100 1100 1161", "AC00"),
    // D: regional indicators pair into one grapheme.
    ("1F1EB 1F1F7 1F1E9 1F1EA", "1F1F7 1F1E9"),
    ("1F1EB 1F1F7 1F1E9 1F1EA", "1F1EB"),
    ("1F1EB 1F1F7 1F1E9 1F1EA", "1F1F7"),
    // D: emoji joined by ZWJ, or followed by a skin tone, form one grapheme.
    ("0078 1F469 200D 1F4BB 0079", "1F4BB"),
    ("0078 1F469 200D 1F4BB 0079", "1F469"),
    ("0078 1F469 200D 1F4BB 0079", "200D 1F4BB"),
    ("1F468 200D 1F469 200D 1F467", "1F469"),
    ("1F468 200D 1F469 200D 1F467", "1F467"),
    ("0078 1F44D 1F3FD 0079", "1F44D"),
    ("0078 1F44D 1F3FD 0079", "1F3FD"),
];

fn hex(text: &str) -> String {
    text.chars()
        .map(|c| format!("{:04X}", u32::from(c)))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn index_of_answers_like_the_dotnet_compare_info() {
    let path = format!(
        "{}/../../tests/golden/collation-probes.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("golden file")).expect("JSON");
    let probes = golden["probes"].as_array().expect("probes");
    let mut mismatches = Vec::new();
    for probe in probes {
        let text = probe[0].as_str().expect("text");
        let term = probe[1].as_str().expect("term");
        let expected = probe[2].as_i64().expect("index");
        let actual = index_of(text, term).map_or(-1, |i| i64::try_from(i).expect("offset"));
        let known = CONTEXT_DEVIATIONS.contains(&(hex(text).as_str(), hex(term).as_str()));
        if known && actual == expected {
            mismatches.push(format!(
                "IndexOf({}, {}): listed in CONTEXT_DEVIATIONS but equal",
                hex(text),
                hex(term)
            ));
        } else if !known && actual != expected {
            mismatches.push(format!(
                "IndexOf({}, {}): expected {expected}, got {actual}",
                hex(text),
                hex(term)
            ));
        }
    }
    eprintln!("collation: {} probes compared", probes.len());
    assert!(
        mismatches.is_empty(),
        "{} mismatches:\n{}",
        mismatches.len(),
        mismatches.join("\n")
    );
}
