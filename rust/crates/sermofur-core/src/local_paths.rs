//! Local paths of an instance, as `LocalPaths` of the .NET oracle (`rust/tests/golden/
//! localpaths*.json` are the truth): containment, mappings stored relative to the root with `/`,
//! and the name of each entry as the disk spells it (`CanonicalCase`).
//!
//! Paths are compared as .NET compares them, on UTF-16 units: `LocalPaths.Comparison` and
//! `Path.GetRelativePath` each have their rule (see [`crate::casing`]).

use std::fs;
use std::path::{MAIN_SEPARATOR, Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::casing::{invariant_upper, ordinal_ignore_case_equal};
use crate::discovery::{full_path, validate_directory};
use crate::error::{Result, SermofurError};

/// Separator of a stored mapping, whatever the operating system.
pub const MAPPING_SEPARATOR: char = '/';

fn is_separator(unit: u16) -> bool {
    unit == u16::from(b'/') || (cfg!(windows) && unit == u16::from(b'\\'))
}

/// The UTF-16 units of a path, as a .NET string holds them.
/// On Windows the exact units, a lone surrogate included; elsewhere .NET decodes the bytes of
/// the path, so invalid UTF-8 becomes U+FFFD there too.
fn units(path: &Path) -> Vec<u16> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().encode_utf16().collect()
    }
}

/// Equality under `LocalPaths.Comparison`: `OrdinalIgnoreCase` on Windows, `Ordinal`
/// elsewhere.
fn equal(a: &[u16], b: &[u16]) -> bool {
    if cfg!(windows) {
        ordinal_ignore_case_equal(a, b)
    } else {
        a == b
    }
}

/// `string.Equals(a, b, LocalPaths.Comparison)` on two texts.
pub(crate) fn same_text(a: &str, b: &str) -> bool {
    equal(
        &a.encode_utf16().collect::<Vec<u16>>(),
        &b.encode_utf16().collect::<Vec<u16>>(),
    )
}

/// `PathInternal.IsCaseSensitive` is false on Windows and on macOS: `Path.GetRelativePath`
/// then ignores case, with `OrdinalIgnoreCase` for the roots and `char.ToUpperInvariant` for
/// the common part.
const PATHS_IGNORE_CASE: bool = cfg!(any(windows, target_os = "macos"));

/// `PathInternal.GetRootLength` for the paths this module meets: `X:\`, a UNC or device prefix,
/// or `/`.
fn root_length(path: &[u16]) -> usize {
    let text = String::from_utf16_lossy(path);
    let p = Path::new(&text);
    let mut components = p.components();
    match components.next() {
        Some(std::path::Component::Prefix(prefix)) => {
            let length = prefix.as_os_str().to_string_lossy().encode_utf16().count();
            let unc = matches!(
                prefix.kind(),
                std::path::Prefix::UNC(..) | std::path::Prefix::VerbatimUNC(..)
            );
            if !unc && path.get(length).is_some_and(|u| is_separator(*u)) {
                length + 1
            } else {
                length
            }
        }
        Some(std::path::Component::RootDir) => 1,
        _ => 0,
    }
}

/// `Path.TrimEndingDirectorySeparator`: one final separator removed, unless it is the root.
fn trim_ending(path: &[u16]) -> &[u16] {
    if path.len() > root_length(path) && path.last().is_some_and(|u| is_separator(*u)) {
        &path[..path.len() - 1]
    } else {
        path
    }
}

/// `Path.IsPathFullyQualified`.
pub fn is_path_fully_qualified(path: &str) -> bool {
    let units: Vec<u16> = path.encode_utf16().collect();
    if !cfg!(windows) {
        return units.first() == Some(&u16::from(b'/'));
    }
    let separator = |unit: u16| unit == u16::from(b'\\') || unit == u16::from(b'/');
    match units.as_slice() {
        [first, second, ..] if separator(*first) => {
            *second == u16::from(b'?') || separator(*second)
        }
        [drive, colon, third, ..] => {
            u8::try_from(*drive).is_ok_and(|d| d.is_ascii_alphabetic())
                && *colon == u16::from(b':')
                && separator(*third)
        }
        _ => false,
    }
}

/// `Path.TrimEndingDirectorySeparator` on a text.
pub fn trim_ending_separator(path: &str) -> String {
    let units: Vec<u16> = path.encode_utf16().collect();
    String::from_utf16_lossy(trim_ending(&units))
}

/// Same root, with the case rule of the operating system, final separators trimmed
/// (`InstanceRegistry.Same`).
pub fn same_root(a: &str, b: &str) -> bool {
    let a: Vec<u16> = a.encode_utf16().collect();
    let b: Vec<u16> = b.encode_utf16().collect();
    equal(trim_ending(&a), trim_ending(&b))
}

/// Whether `child` is `parent` or lies under it, lexically (`LocalPaths.Contains`).
pub fn contains(parent: &Path, child: &Path) -> bool {
    let parent_units = units(parent);
    let child_units = units(child);
    let parent = trim_ending(&parent_units);
    let child = trim_ending(&child_units);
    if equal(parent, child) {
        return true;
    }
    // A volume root (C:\ or /) keeps its trailing separator: do not double it.
    let mut prefix = parent.to_vec();
    if !prefix.last().is_some_and(|u| is_separator(*u)) {
        prefix.extend(std::path::MAIN_SEPARATOR_STR.encode_utf16());
    }
    child.len() >= prefix.len() && equal(&child[..prefix.len()], &prefix)
}

/// `Path.IsPathRooted`: a separator first, or on Windows a drive letter and a colon.
fn is_rooted(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.first() == Some(&b'/')
        || (cfg!(windows)
            && (bytes.first() == Some(&b'\\')
                || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')))
}

/// Lexical normalization of a mapping: relative, without drive, contained in the root; `/` and
/// `\` both separate (`LocalPaths.NormalizeMapping`). The folder need not exist.
pub fn normalize_mapping(root: &Path, relative: &str) -> Result<PathBuf> {
    let portable = relative.replace('\\', "/");
    if is_rooted(&portable) || portable.contains(':') {
        return Err(SermofurError::new(
            "unsafe_path",
            "Relative mapping required.",
            4,
        ));
    }
    let full = full_path(&root.join(&portable))?;
    if !contains(root, &full) {
        return Err(SermofurError::new(
            "scope_boundary",
            "Mapping outside the instance.",
            4,
        ));
    }
    Ok(full)
}

/// A normalized mapping then checked on disk: an existing local folder without link
/// (`LocalPaths.ResolveMapping`).
pub fn resolve_mapping(root: &Path, relative: &str) -> Result<PathBuf> {
    validate_directory(&normalize_mapping(root, relative)?)
}

/// `PathInternal.GetCommonPathLength`.
fn common_path_length(first: &[u16], second: &[u16]) -> usize {
    let mut common = first
        .iter()
        .zip(second)
        .take_while(|(a, b)| {
            a == b || (PATHS_IGNORE_CASE && invariant_upper(**a) == invariant_upper(**b))
        })
        .count();
    if common == 0 {
        return 0;
    }
    if common == first.len() && (common == second.len() || is_separator(second[common])) {
        return common;
    }
    if common == second.len() && is_separator(first[common]) {
        return common;
    }
    while common > 0 && !is_separator(first[common - 1]) {
        common -= 1;
    }
    common
}

/// `Path.GetRelativePath(relativeTo, path)` of .NET.
fn relative_path(relative_to: &Path, path: &Path) -> Result<String> {
    let relative_to = units(&full_path(relative_to)?);
    let path = units(&full_path(path)?);
    let separator = u16::try_from(u32::from(MAIN_SEPARATOR)).unwrap_or(u16::from(b'/'));
    let (root_a, root_b) = (root_length(&relative_to), root_length(&path));
    let roots_equal = if PATHS_IGNORE_CASE {
        ordinal_ignore_case_equal(&relative_to[..root_a], &path[..root_b])
    } else {
        relative_to[..root_a] == path[..root_b]
    };
    if root_a != root_b || !roots_equal {
        return Ok(String::from_utf16_lossy(&path));
    }
    let mut common = common_path_length(&relative_to, &path);
    if common == 0 {
        return Ok(String::from_utf16_lossy(&path));
    }
    let relative_to_length =
        relative_to.len() - usize::from(relative_to.last().is_some_and(|u| is_separator(*u)));
    let ends_in_separator = path.last().is_some_and(|u| is_separator(*u));
    let path_length = path.len() - usize::from(ends_in_separator);
    if relative_to_length == path_length && common >= relative_to_length {
        return Ok(".".to_owned());
    }
    let mut out: Vec<u16> = Vec::new();
    let dots: Vec<u16> = "..".encode_utf16().collect();
    if common < relative_to_length {
        out.extend(&dots);
        for unit in relative_to
            .get(common + 1..relative_to_length)
            .unwrap_or_default()
        {
            if is_separator(*unit) {
                out.push(separator);
                out.extend(&dots);
            }
        }
    } else if path.get(common).is_some_and(|u| is_separator(*u)) {
        common += 1;
    }
    let difference = (path_length + usize::from(ends_in_separator)).saturating_sub(common);
    if difference > 0 {
        if !out.is_empty() {
            out.push(separator);
        }
        out.extend(&path[common..common + difference]);
    }
    Ok(String::from_utf16_lossy(&out))
}

/// The mapping of `absolute` relative to `root`, with `/` as separator
/// (`LocalPaths.RelativizeMapping`).
pub fn relativize_mapping(root: &Path, absolute: &Path) -> Result<String> {
    Ok(relative_path(root, absolute)?.replace(MAIN_SEPARATOR, "/"))
}

/// The entries of `directory` equal to `name` without case and normalization (`Listed`);
/// `None` when the folder cannot be listed.
fn listed(directory: &Path, name: &str) -> Option<Vec<String>> {
    let folded: Vec<u16> = name.nfc().collect::<String>().encode_utf16().collect();
    // The pattern of .NET only filters: an ASCII name without wildcard is matched first as is,
    // without case; any other name is compared after normalization only.
    let pattern = !name.chars().any(|c| c == '*' || c == '?' || !c.is_ascii());
    let pattern_units: Vec<u16> = name.encode_utf16().collect();
    let mut found = Vec::new();
    for entry in fs::read_dir(directory).ok()? {
        let entry = entry.ok()?;
        // A name that is not Unicode cannot equal a Unicode name.
        let Ok(candidate) = entry.file_name().into_string() else {
            continue;
        };
        let raw: Vec<u16> = candidate.encode_utf16().collect();
        if pattern && !ordinal_ignore_case_equal(&raw, &pattern_units) {
            continue;
        }
        let normalized: Vec<u16> = candidate.nfc().collect::<String>().encode_utf16().collect();
        if ordinal_ignore_case_equal(&normalized, &folded) {
            found.push(candidate);
        }
    }
    Some(found)
}

/// A stored relative path with each existing entry named as the disk lists it
/// (`LocalPaths.CanonicalCase`). Only an entry that the file system opens under the given name
/// but lists under another one is renamed; a missing entry keeps its name.
pub fn canonical_case(root: &Path, relative: &str) -> Result<String> {
    let mut current = root.to_path_buf();
    let mut parts: Vec<String> = Vec::new();
    for part in relative.split(MAPPING_SEPARATOR).filter(|p| !p.is_empty()) {
        let candidate = current.join(part);
        let mut name = part.to_owned();
        // `File.Exists || Directory.Exists`: true for a dangling link too, as both read the
        // entry itself when its target is missing.
        if part != "." && fs::symlink_metadata(&candidate).is_ok() {
            // A folder that can be crossed but not listed cannot tell the name on disk.
            let found = listed(&current, part).ok_or_else(|| {
                SermofurError::new(
                    "invalid_path",
                    "A folder of the path cannot be listed; its names on disk cannot be checked.",
                    1,
                )
            })?;
            if !found.iter().any(|f| f == part) {
                // An alias (a Windows 8.3 short name) or an ambiguous name has no stored form.
                if found.len() != 1 {
                    return Err(SermofurError::new(
                        "unsafe_path",
                        "Path reached through an alias (short name) or an ambiguous name.",
                        4,
                    ));
                }
                name.clone_from(&found[0]);
            }
        }
        current = current.join(&name);
        parts.push(name);
    }
    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;

    /// Units that stress the index arithmetic: separators, a colon, dots, case pairs, the two
    /// letters that .NET folds apart, and lone or paired surrogates.
    fn unit() -> impl Strategy<Value = u16> {
        prop::sample::select(vec![
            u16::from(b'a'),
            u16::from(b'A'),
            0xE9,
            u16::from(b'/'),
            u16::from(b'\\'),
            u16::from(b':'),
            u16::from(b'.'),
            u16::from(b'~'),
            0x131,
            0x17F,
            0xD801,
            0xDC28,
        ])
    }

    proptest! {
        #[test]
        fn index_arithmetic_never_panics(a in vec(unit(), 0..24), b in vec(unit(), 0..24)) {
            let _ = trim_ending(&a);
            let _ = root_length(&a);
            let _ = common_path_length(&a, &b);
            let pa = PathBuf::from(String::from_utf16_lossy(&a));
            let pb = PathBuf::from(String::from_utf16_lossy(&b));
            let _ = contains(&pa, &pb);
            let base = std::env::temp_dir();
            let _ = relative_path(&base.join(&pa), &base.join(&pb));
        }

        #[test]
        fn a_path_contains_itself_and_is_dot_from_itself(name in "[a-zA-Z0-9_~-]{1,12}") {
            let path = std::env::temp_dir().join(&name);
            prop_assert!(contains(&path, &path));
            prop_assert!(contains(&path, &path.join("child")));
            prop_assert_eq!(relative_path(&path, &path).expect("full"), ".");
            prop_assert_eq!(
                relativize_mapping(&path, &path.join("a").join("b")).expect("full"),
                "a/b"
            );
        }
    }

    /// `Path.GetRelativePath` ignores case on Windows and macOS only.
    #[test]
    fn relative_path_ignores_case_where_dotnet_does() {
        let base = std::env::temp_dir();
        let expected = if PATHS_IGNORE_CASE { "c" } else { "../aB/c" };
        let found = relative_path(&base.join("Ab"), &base.join("aB").join("c")).expect("full");
        assert_eq!(found.replace(std::path::MAIN_SEPARATOR, "/"), expected);
    }

    /// A lone surrogate is a unit of its own, not U+FFFD, as in a .NET string.
    #[cfg(windows)]
    #[test]
    fn a_lone_surrogate_is_not_the_replacement_character() {
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let root = PathBuf::from(OsString::from_wide(&[
            u16::from(b'C'),
            u16::from(b':'),
            0x5C,
            0xD800,
        ]));
        let other = PathBuf::from("C:\\\u{FFFD}\\x");
        assert!(!contains(&root, &other));
    }
}
