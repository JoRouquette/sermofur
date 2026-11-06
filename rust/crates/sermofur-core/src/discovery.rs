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
/// `invalid_storage_or_path` (`CommandRunner.Run`), and so does the port.
fn full_path(path: &Path) -> Result<PathBuf> {
    if cfg!(windows) && path.to_string_lossy().starts_with(r"\??\") {
        return Err(invalid_storage());
    }
    std::path::absolute(path)
        .map(|full| normalize(&full))
        .map_err(|_| invalid_storage())
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
