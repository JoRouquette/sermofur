//! Generators shared by the property tests of the command line (`PropertyTests.cs` in .NET).

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

/// The words that make up generated command lines: options, the separator, shortcuts, plain
/// words and the empty string. Same list as the .NET property tests.
const WORDS: [&str; 15] = [
    "--json",
    "--path",
    "--actor",
    "--text",
    "--contradicts",
    "--",
    "--x",
    "claim",
    "add",
    "value",
    "-v",
    "-h",
    "--help",
    "--version",
    "",
];

/// Any command line of up to 20 words taken from [`WORDS`].
pub fn command_line() -> impl Strategy<Value = Vec<String>> {
    vec(select(&WORDS[..]).prop_map(str::to_string), 0..=20)
}

/// A command line of up to 130 arguments, around the limit of 128, whose words are either from
/// [`WORDS`], an option-like text (`--` followed by arbitrary printable characters, accents and
/// characters above the Basic Multilingual Plane included), or any printable text.
pub fn arbitrary_line() -> impl Strategy<Value = Vec<String>> {
    let word = prop_oneof![
        select(&WORDS[..]).prop_map(str::to_string),
        "--\\PC{0,8}",
        "\\PC{0,40}",
    ];
    vec(word, 0..=130)
}
