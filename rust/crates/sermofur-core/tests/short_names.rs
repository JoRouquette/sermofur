//! `expand_short_names` on Windows, as `PathHelper.TryExpandShortFileName`: the temporary folder
//! of a profile whose name is longer than 8.3 is usually given in its short form (`JONATH~1.ROU`).
#![cfg(windows)]

use std::path::{Path, PathBuf};

use sermofur_core::discovery::expand_short_names;

/// The long form of an existing folder, from `canonicalize` without its `\\?\` prefix.
fn long_form(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).expect("resolves");
    let text = canonical.to_str().expect("UTF-8").to_owned();
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

#[test]
fn short_names_expand_like_dotnet() {
    let temp = std::env::temp_dir();
    let temp = temp
        .to_str()
        .expect("UTF-8")
        .trim_end_matches('\\')
        .to_owned();
    if !temp.contains('~') {
        eprintln!("no short name in {temp}: test skipped");
        return;
    }
    let long = long_form(Path::new(&temp));
    assert!(!long.contains('~'), "the profile has a long name: {long}");
    // The whole path exists.
    assert_eq!(expand_short_names(Path::new(&temp)), PathBuf::from(&long));
    // Only a part exists: the rest is appended unchanged.
    let partial = format!(r"{temp}\missing~x\child");
    assert_eq!(
        expand_short_names(Path::new(&partial)),
        PathBuf::from(format!(r"{long}\missing~x\child"))
    );
    // A `\\.\` device path keeps its prefix.
    assert_eq!(
        expand_short_names(Path::new(&format!(r"\\.\{temp}"))),
        PathBuf::from(format!(r"\\.\{long}"))
    );
    // Nothing exists below the root: the path stays as it is.
    let none = r"C:\NOPE~1\x";
    assert_eq!(expand_short_names(Path::new(none)), PathBuf::from(none));
}
