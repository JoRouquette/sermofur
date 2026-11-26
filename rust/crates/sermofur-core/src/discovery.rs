//! Discovery of the instance that holds a folder, as `InstanceManager.Discover` and
//! `LocalPaths.ValidateDirectory` of the .NET oracle (`rust/tests/golden/discovery*.json` are
//! the truth). The search fails closed: the nearest `.sermofur` entry decides, and an entry that
//! is not a readable instance of the current user stops it.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, SermofurError};
use crate::instance::{
    CONFIGURATION_FILE, MARKER, invalid_storage, is_link, link_not_supported, normalize,
};
use crate::ownership::FileOwnership;

/// Entries that only Sermofur creates inside `.sermofur`.
const SERMOFUR_ARTIFACTS: [&str; 4] = [CONFIGURATION_FILE, "memory.db", "records", "backups"];

/// What the `.sermofur` entry of a folder is (`MarkerState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarkerState {
    Absent,
    Foreign,
    Damaged,
    Unreadable,
    ForeignOwner,
    Instance,
}

/// The full form of a path, as `Path.GetFullPath`: absolute, `.` and `..` removed lexically.
/// Where .NET throws `ArgumentException` (an empty path; on Windows an NT path `\??\X:\...`,
/// kept as is by `GetFullPath`, then refused by `DriveInfo`), the CLI answers
/// `invalid_storage_or_path` (`CommandRunner.Run`), and so does the port. A NUL is refused
/// everywhere (`PathInternal`); on Unix `std::path::absolute` would keep it.
pub fn full_path(path: &Path) -> Result<PathBuf> {
    if is_nt_path(path) {
        return Err(invalid_storage());
    }
    get_full_path(path)
}

/// Whether `path` is an NT path `\??\...` (Windows only).
fn is_nt_path(path: &Path) -> bool {
    cfg!(windows) && path.to_string_lossy().starts_with(r"\??\")
}

/// Whether `path` is extended for .NET (`PathInternal.IsExtended`: `\\?\` or `\??\`, backslashes
/// only; Windows only).
fn is_extended(path: &Path) -> bool {
    is_nt_path(path) || (cfg!(windows) && path.to_string_lossy().starts_with(r"\\?\"))
}

/// `Path.GetFullPath` itself: as [`full_path`], except that an NT path `\??\...`, extended for
/// .NET, is returned as is, where the discovery refuses it. An extended path `\\?\...` is returned
/// as is too, `.` and `..` included, and the discovery then refuses it as a network path
/// (`unsafe_path`). The command line takes the context folder (`--path`) and the file of
/// `source add` through it; an NT file is then outside the instance (`scope_boundary`), as for
/// .NET.
pub fn get_full_path(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(invalid_storage());
    }
    if is_extended(path) {
        return Ok(path.to_path_buf());
    }
    let full = std::path::absolute(path)
        .map(|full| normalize(&full))
        .map_err(|_| invalid_storage())?;
    // An extended path (`\\?\`) is returned as is by `GetFullPath`; any other one has its short
    // 8.3 names expanded (`PathHelper.Normalize`).
    if full.to_string_lossy().starts_with(r"\\?\") {
        return Ok(full);
    }
    Ok(expand_short_names(&full))
}

/// The long form of a full path whose text holds a `~`, as `PathHelper.TryExpandShortFileName`
/// of .NET on Windows: `GetLongPathNameW` on the longest part of the path that exists, the rest
/// appended unchanged; the path itself when nothing can be expanded. `Path.GetFullPath` and
/// `Environment.CurrentDirectory` both apply it. Nothing changes elsewhere than on Windows.
pub fn expand_short_names(full: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        expand_windows(full).unwrap_or_else(|| full.to_path_buf())
    }
    #[cfg(not(windows))]
    {
        full.to_path_buf()
    }
}

#[cfg(windows)]
fn expand_windows(full: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::{Component, Prefix};

    const SEPARATOR: u16 = b'\\' as u16;
    let text: Vec<u16> = full.as_os_str().encode_wide().collect();
    if !text.contains(&(b'~' as u16)) || text.len() < 3 {
        return None;
    }
    // `PathInternal.GetRootLength`: the prefix, and the separator after a drive or a device.
    let Some(Component::Prefix(prefix)) = full.components().next() else {
        return None;
    };
    let prefix_length = prefix.as_os_str().encode_wide().count();
    let unc = matches!(prefix.kind(), Prefix::UNC(..) | Prefix::VerbatimUNC(..));
    let root_length = if !unc && text.get(prefix_length) == Some(&SEPARATOR) {
        prefix_length + 1
    } else {
        prefix_length
    };
    let device = text.starts_with(&[SEPARATOR, SEPARATOR, u16::from(b'?'), SEPARATOR])
        || text.starts_with(&[SEPARATOR, SEPARATOR, u16::from(b'.'), SEPARATOR]);
    let dos_unc = !device && text.starts_with(&[SEPARATOR, SEPARATOR]);
    // The extended prefix allows growth over MAX_PATH; it is removed at the end.
    let (mut input, root_difference): (Vec<u16>, usize) = if device {
        let mut input = text.clone();
        input[2] = u16::from(b'?');
        (input, 0)
    } else if dos_unc {
        let mut input: Vec<u16> = r"\\?\UNC\".encode_utf16().collect();
        input.extend_from_slice(&text[2..]);
        (input, 6)
    } else {
        let mut input: Vec<u16> = r"\\?\".encode_utf16().collect();
        input.extend_from_slice(&text);
        (input, 4)
    };
    let root_length = root_length + root_difference;
    let length = input.len();
    let mut found = length - 1;
    let mut cut = false;
    let mut output = None;
    loop {
        let probe = if cut { &input[..found] } else { &input[..] };
        match crate::windows::long_path_name(probe) {
            Ok(mut long) => {
                if cut {
                    long.extend_from_slice(&input[found..]);
                }
                output = Some(long);
                break;
            }
            // ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND: look further up the path.
            Err(2 | 3) => {
                found = found.checked_sub(1)?;
                while found > root_length && input[found] != SEPARATOR {
                    found -= 1;
                }
                if found <= root_length {
                    break;
                }
                cut = true;
            }
            Err(_) => break,
        }
    }
    let mut result = output.unwrap_or_else(|| std::mem::take(&mut input));
    if text.get(2) == Some(&u16::from(b'.')) && device {
        result[2] = u16::from(b'.');
    }
    if dos_unc {
        // `\\?\UNC\` back to `\\`: the character before the server becomes a separator.
        result[6] = SEPARATOR;
    }
    Some(PathBuf::from(OsString::from_wide(
        result.get(root_difference..)?,
    )))
}

/// Refuses a network share or a mapped network drive with the message of the caller
/// (`LocalPaths.RequireLocalDisk`): an instance lives on a local disk only.
pub fn require_local_disk(path: &Path, message: &str) -> Result<()> {
    let full = full_path(path)?;
    let text = full.to_string_lossy();
    if text.starts_with(r"\\") || is_network_drive(&text) {
        return Err(SermofurError::new("unsafe_path", message, 4));
    }
    Ok(())
}

/// A drive letter path whose volume is a network drive (`DriveInfo.DriveType`); never on Unix.
#[cfg(windows)]
fn is_network_drive(full: &str) -> bool {
    let bytes = full.as_bytes();
    if bytes.len() < 3 || !bytes[0].is_ascii_alphabetic() || &bytes[1..3] != br":\" {
        return false;
    }
    crate::windows::is_network_drive(Path::new(&full[..3]))
}

#[cfg(not(windows))]
fn is_network_drive(_full: &str) -> bool {
    false
}

/// The full form of an existing local folder without any link on its path
/// (`LocalPaths.ValidateDirectory`).
pub fn validate_directory(path: &Path) -> Result<PathBuf> {
    let full = full_path(path)?;
    require_local_disk(&full, "An instance requires a local disk.")?;
    if !fs::metadata(&full).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(SermofurError::new(
            "invalid_path",
            "The context directory must exist.",
            1,
        ));
    }
    crate::instance::reject_links(&full)?;
    Ok(full)
}

/// The root of the instance that holds `path`.
pub fn discover(path: &Path, owners: &dyn FileOwnership) -> Result<PathBuf> {
    locate(path, owners, false)
}

/// Same search as [`discover`], but the folder of an invalid `.sermofur` entry is returned so
/// that doctor can report it instead of failing before its report.
pub fn discover_for_diagnosis(path: &Path, owners: &dyn FileOwnership) -> Result<PathBuf> {
    locate(path, owners, true)
}

/// The case of an invalid `.sermofur` entry in `root`, as doctor names it, or `None` for a valid
/// or absent one (`DescribeInvalidMarker`). A link is still refused.
pub fn describe_invalid_marker(
    root: &Path,
    owners: &dyn FileOwnership,
) -> Result<Option<&'static str>> {
    Ok(match inspect_marker(root, owners)? {
        MarkerState::ForeignOwner => Some("foreign_owner"),
        MarkerState::Foreign => Some("foreign"),
        MarkerState::Damaged => Some("damaged"),
        MarkerState::Unreadable => Some("unreadable"),
        MarkerState::Absent | MarkerState::Instance => None,
    })
}

fn locate(path: &Path, owners: &dyn FileOwnership, for_diagnosis: bool) -> Result<PathBuf> {
    let start = validate_directory(path)?;
    let mut current = Some(start.as_path());
    while let Some(folder) = current {
        let state = inspect_marker(folder, owners)?;
        if state == MarkerState::Instance || (state != MarkerState::Absent && for_diagnosis) {
            return Ok(folder.to_path_buf());
        }
        if state != MarkerState::Absent {
            return Err(invalid_marker(state));
        }
        current = folder.parent();
    }
    Err(SermofurError::new(
        "no_instance",
        "No .sermofur instance in the parent folders.",
        2,
    ))
}

/// Classifies the `.sermofur` entry of `folder` without following links, so that a dangling
/// link is an entry and not an absence. Any link is refused, even for doctor. An entry whose
/// attributes or listing cannot be read is unreadable, never absent nor foreign.
fn inspect_marker(folder: &Path, owners: &dyn FileOwnership) -> Result<MarkerState> {
    let marker = folder.join(MARKER);
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(MarkerState::Absent);
        }
        Err(_) => return Ok(MarkerState::Unreadable),
    };
    if is_link(&metadata) {
        return Err(link_not_supported());
    }
    if !metadata.is_dir() {
        return Ok(MarkerState::Foreign);
    }
    // An entry of another account is never read (ADR 0014): its content could be planted.
    match owners.inspect(&marker) {
        Err(_) => return Ok(MarkerState::Unreadable),
        Ok(Some(status)) if !status.is_owned_by_current_user => {
            return Ok(MarkerState::ForeignOwner);
        }
        Ok(_) => {}
    }
    let Ok(listing) = fs::read_dir(&marker) else {
        return Ok(MarkerState::Unreadable);
    };
    let mut names = Vec::new();
    for entry in listing {
        let Ok(entry) = entry else {
            return Ok(MarkerState::Unreadable);
        };
        names.push(entry.file_name());
    }
    // `LocalPaths.Comparison`: OrdinalIgnoreCase on Windows, which maps no other letter to an
    // ASCII one (golden cases `ı` and `ſ`), Ordinal elsewhere.
    let present = |artifact: &str| {
        names.iter().any(|name| {
            if cfg!(windows) {
                name.to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(artifact))
            } else {
                name == artifact
            }
        })
    };
    if present(CONFIGURATION_FILE) {
        return Ok(MarkerState::Instance);
    }
    Ok(if SERMOFUR_ARTIFACTS.iter().any(|a| present(a)) {
        MarkerState::Damaged
    } else {
        MarkerState::Foreign
    })
}

fn invalid_marker(state: MarkerState) -> SermofurError {
    let (code, message, exit) = match state {
        MarkerState::ForeignOwner => (
            "foreign_owner",
            "The .sermofur entry belongs to another user account; it is not read. Use your own instance.",
            4,
        ),
        MarkerState::Damaged => (
            "invalid_instance",
            "Damaged Sermofur instance: .sermofur holds Sermofur data but no instance.json; run smf doctor and restore instance.json from a backup.",
            3,
        ),
        MarkerState::Unreadable => (
            "invalid_instance",
            "A .sermofur entry, or whether one exists, cannot be read in this folder or a parent; check permissions, then run smf doctor.",
            3,
        ),
        MarkerState::Foreign => (
            "invalid_instance",
            "A .sermofur entry that is not a Sermofur instance stops the search for an instance; rename or move it away.",
            3,
        ),
        MarkerState::Absent | MarkerState::Instance => {
            unreachable!("marker state {state:?} does not describe an invalid entry")
        }
    };
    SermofurError::new(code, message, exit)
}

#[cfg(all(test, windows))]
mod tests {
    use std::path::Path;

    /// `Path.GetFullPath` returns an extended path as typed (`PathInternal.IsExtended`, checked
    /// before any normalization in `Path.Windows.cs`); the discovery refuses both forms.
    #[test]
    fn an_extended_path_is_returned_as_typed() {
        for typed in [r"\\?\C:\a\..\b\.", r"\??\C:\a\..\b\."] {
            let full = super::get_full_path(Path::new(typed)).expect("full path");
            assert_eq!(full.as_os_str(), typed);
        }
        let full = super::full_path(Path::new(r"\\?\C:\a\..\b")).expect("full path");
        assert_eq!(full.as_os_str(), r"\\?\C:\a\..\b");
        assert_eq!(
            super::full_path(Path::new(r"\??\C:\a"))
                .expect_err("NT path")
                .code,
            "invalid_storage_or_path"
        );
    }
}
