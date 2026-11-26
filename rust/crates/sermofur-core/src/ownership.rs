//! Type and owner of a file system entry, as `FileOwnership` of the .NET tool reads them
//! (ADR 0014). An entry of another account is never trusted: its content could be planted.

use std::io;
use std::path::Path;

/// Type and owner of an entry, as `EntryStatus` of the .NET tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EntryStatus {
    pub is_regular_file: bool,
    pub is_directory: bool,
    pub is_owned_by_current_user: bool,
}

/// Reads the owner and type of an entry; replaceable in tests, as `IFileOwnership`.
pub trait FileOwnership {
    /// Status of `path`, or `None` when it does not exist. Every caller of the .NET tool maps an
    /// error to "unreadable", so the kind of the error carries no meaning.
    fn inspect(&self, path: &Path) -> io::Result<Option<EntryStatus>>;
}

/// The implementation of the operating system.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemOwnership;

impl FileOwnership for SystemOwnership {
    fn inspect(&self, path: &Path) -> io::Result<Option<EntryStatus>> {
        inspect(path)
    }
}

/// Binary form of the BUILTIN\Administrators SID, S-1-5-32-544.
#[cfg_attr(not(windows), allow(dead_code))]
const ADMINISTRATORS: [u8; 16] = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 32, 2, 0, 0];

#[cfg_attr(not(windows), allow(dead_code))]
const SE_GROUP_ENABLED: u32 = 0x4;
#[cfg_attr(not(windows), allow(dead_code))]
const SE_GROUP_USE_FOR_DENY_ONLY: u32 = 0x10;
#[cfg_attr(not(windows), allow(dead_code))]
const SE_GROUP_LOGON_ID: u32 = 0xC000_0000;

/// Whether the current user owns an entry, as `InspectWindows`: the owner is the user, or the
/// Administrators group when the user belongs to it (folders created from an elevated session).
/// Membership is `WindowsIdentity.Groups` (enabled, not a logon SID, not deny-only) or a
/// `DenyOnlySid` claim (deny-only, not enabled, not a logon SID): the group of a session without
/// elevation is deny-only.
#[cfg_attr(not(windows), allow(dead_code))]
fn owned_by(owner: Option<&[u8]>, user: &[u8], groups: &[(Vec<u8>, u32)]) -> bool {
    let Some(owner) = owner else {
        return false;
    };
    if owner == user {
        return true;
    }
    let mask = SE_GROUP_ENABLED | SE_GROUP_LOGON_ID | SE_GROUP_USE_FOR_DENY_ONLY;
    owner == ADMINISTRATORS
        && groups.iter().any(|(sid, attributes)| {
            let kind = attributes & mask;
            *sid == ADMINISTRATORS
                && (kind == SE_GROUP_ENABLED || kind == SE_GROUP_USE_FOR_DENY_ONLY)
        })
}

/// Windows: the type comes from the attributes of the entry itself (`FileSystemInfo`, which does
/// not follow a link); the owner comes from `GetNamedSecurityInfoW` on the full path. Any failure
/// to read the attributes means "absent", as `File.Exists` and `DirectoryInfo.Exists`.
#[cfg(windows)]
fn inspect(path: &Path) -> io::Result<Option<EntryStatus>> {
    use std::os::windows::fs::MetadataExt;

    const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return Ok(None);
    };
    let owner = crate::windows::owner_sid(&std::path::absolute(path)?)?;
    let token = crate::windows::process_token()?;
    let directory = metadata.file_attributes() & FILE_ATTRIBUTE_DIRECTORY != 0;
    Ok(Some(EntryStatus {
        is_regular_file: !directory,
        is_directory: directory,
        is_owned_by_current_user: owned_by(owner.as_deref(), &token.user, &token.groups),
    }))
}

/// Linux and macOS: the type and the owner of the entry itself, read without following a link
/// (`statx` with `AT_SYMLINK_NOFOLLOW` on Linux, `lstat` on macOS in the .NET tool), against the
/// effective user. `ENOENT` and `ENOTDIR` mean "absent"; any other failure is an error. On musl
/// the .NET tool cannot load `statx` and fails closed, so the port does too.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn inspect(path: &Path) -> io::Result<Option<EntryStatus>> {
    use std::os::unix::fs::MetadataExt;

    const NO_SUCH_ENTRY: i32 = 2;
    const NOT_A_DIRECTORY: i32 = 20;
    const TYPE_MASK: u32 = 0xF000;
    const REGULAR_FILE: u32 = 0x8000;
    const DIRECTORY: u32 = 0x4000;
    if cfg!(target_env = "musl") {
        return Err(io::Error::other(
            "File owner unavailable on this C library (glibc 2.28+ required).",
        ));
    }
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if matches!(error.raw_os_error(), Some(NO_SUCH_ENTRY | NOT_A_DIRECTORY)) => {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    let kind = metadata.mode() & TYPE_MASK;
    Ok(Some(EntryStatus {
        is_regular_file: kind == REGULAR_FILE,
        is_directory: kind == DIRECTORY,
        is_owned_by_current_user: metadata.uid() == rustix::process::geteuid().as_raw(),
    }))
}

/// Any other system: the .NET tool throws `PlatformNotSupportedException`; the owner is never
/// trusted.
#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn inspect(_path: &Path) -> io::Result<Option<EntryStatus>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "File ownership is checked on Windows, Linux and macOS.",
    ))
}

/// The SID of the current user in its text form, as `WindowsIdentity.User.Value`.
#[cfg(windows)]
pub fn current_user_sid() -> io::Result<String> {
    sid_text(&crate::windows::process_token()?.user)
        .ok_or_else(|| io::Error::other("malformed SID"))
}

/// `S-<revision>-<authority>-<sub-authorities>`: the authority is a 48-bit big-endian number,
/// written in hexadecimal from 2^32 on; each sub-authority is a 32-bit little-endian number.
#[cfg_attr(not(windows), allow(dead_code))]
fn sid_text(sid: &[u8]) -> Option<String> {
    let count = usize::from(*sid.get(1)?);
    let authority = sid
        .get(2..8)?
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
    let mut text = if authority >= 1 << 32 {
        format!("S-{}-0x{authority:012X}", sid[0])
    } else {
        format!("S-{}-{authority}", sid[0])
    };
    for index in 0..count {
        let bytes = sid.get(8 + 4 * index..12 + 4 * index)?;
        let value = u32::from_le_bytes(bytes.try_into().ok()?);
        text.push_str(&format!("-{value}"));
    }
    Some(text)
}

/// The effective user of the process, as `UnixNative.EffectiveUserId`.
#[cfg(unix)]
pub fn effective_user_id() -> u32 {
    rustix::process::geteuid().as_raw()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sid_is_written_as_windows_writes_it() {
        assert_eq!(sid_text(&ADMINISTRATORS).as_deref(), Some("S-1-5-32-544"));
        assert_eq!(sid_text(&[1, 1, 0, 0, 0, 0, 0, 5]), None);
    }

    /// The SID of the account, as `whoami /user` prints it.
    #[cfg(windows)]
    #[test]
    fn the_current_sid_is_the_one_of_whoami() {
        let out = std::process::Command::new("whoami")
            .args(["/user", "/fo", "csv", "/nh"])
            .output()
            .expect("whoami");
        let text = String::from_utf8_lossy(&out.stdout);
        let expected = text
            .trim()
            .rsplit(',')
            .next()
            .expect("a SID")
            .trim_matches('"');
        assert_eq!(current_user_sid().expect("SID"), expected);
    }

    const USER: [u8; 12] = [1, 1, 0, 0, 0, 0, 0, 5, 18, 0, 0, 0];

    fn admins(attributes: u32) -> Vec<(Vec<u8>, u32)> {
        vec![(ADMINISTRATORS.to_vec(), attributes)]
    }

    #[test]
    fn the_user_owns_what_its_sid_owns() {
        assert!(owned_by(Some(&USER), &USER, &[]));
        assert!(!owned_by(Some(&ADMINISTRATORS), &USER, &[]));
        assert!(!owned_by(None, &USER, &admins(SE_GROUP_ENABLED)));
    }

    #[test]
    fn administrators_count_when_enabled_or_deny_only() {
        let owner = Some(&ADMINISTRATORS[..]);
        assert!(owned_by(owner, &USER, &admins(SE_GROUP_ENABLED | 0x3)));
        assert!(owned_by(owner, &USER, &admins(SE_GROUP_USE_FOR_DENY_ONLY)));
        // Both flags, a logon SID, or neither: no Groups entry and no DenyOnlySid claim.
        assert!(!owned_by(
            owner,
            &USER,
            &admins(SE_GROUP_ENABLED | SE_GROUP_USE_FOR_DENY_ONLY)
        ));
        assert!(!owned_by(
            owner,
            &USER,
            &admins(SE_GROUP_ENABLED | SE_GROUP_LOGON_ID)
        ));
        assert!(!owned_by(owner, &USER, &admins(0)));
        // Another group with the right attributes does not stand for Administrators.
        assert!(!owned_by(
            owner,
            &USER,
            &[(USER.to_vec(), SE_GROUP_ENABLED)]
        ));
    }
}
