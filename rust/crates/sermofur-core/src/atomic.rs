//! Atomic file write, as `AtomicFile.Write` of the .NET 0.4.3 oracle does it
//! (`rust/tests/golden/atomic-file.json` is the truth).
//!
//! The content goes to a temporary file of a random name, created new in the folder of the
//! target, then renamed over the target. Only the target is looked at for links, not the
//! folders above it.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;
use uuid::Uuid;

use crate::error::SermofurError;
use crate::instance::normalize;

/// Links followed at most, past which a chain is refused as too long.
const MAXIMUM_LINKS: usize = 10;

/// What a write does when a link stands where its target should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPolicy {
    /// A link in place of the target is refused (the state of sermofur).
    Refuse,
    /// The chain of links is followed and its final target is replaced (a host configuration).
    FollowToTarget,
}

/// Everything a write decides beyond the path and the content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteOptions {
    /// Mode of a file that does not exist yet, on Unix. A file that exists keeps its own mode.
    pub new_file_mode: Option<u32>,
    /// What a link in place of the target means.
    pub policy: LinkPolicy,
    /// Whether the content reaches the disk before the rename.
    pub durable: bool,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            new_file_mode: None,
            policy: LinkPolicy::Refuse,
            durable: true,
        }
    }
}

/// Why a write failed. The IO error is kept: callers sort it (sharing violation, access denied).
#[derive(Debug, Error)]
pub enum WriteError {
    /// `unsafe_path`, exit 4: the target was refused and left untouched.
    #[error("{0}")]
    Refused(SermofurError),
    /// An IO failure of the system.
    #[error("{0}")]
    Io(#[from] io::Error),
}

/// `Io` becomes `invalid_storage_or_path`, exit 3, as the .NET command runner maps it.
impl From<WriteError> for SermofurError {
    fn from(error: WriteError) -> Self {
        match error {
            WriteError::Refused(e) => e,
            WriteError::Io(_) => Self::new(
                "invalid_storage_or_path",
                "Invalid path or data; run doctor.",
                3,
            ),
        }
    }
}

type Result<T> = std::result::Result<T, WriteError>;

fn refused(file: &Path, why: &str) -> WriteError {
    WriteError::Refused(SermofurError::new(
        "unsafe_path",
        format!("{} was left untouched: {why}.", file.display()),
        4,
    ))
}

fn ends_with_separator(file: &Path) -> bool {
    let text = file.to_string_lossy();
    text.ends_with('/') || (cfg!(windows) && text.ends_with('\\'))
}

/// Replaces `file` with `content` and returns the file actually replaced, which differs from
/// `file` when a link was followed.
pub fn write(file: &Path, content: &[u8], options: &WriteOptions) -> Result<PathBuf> {
    let full = normalize(&std::path::absolute(file)?);
    // .NET keeps the trailing separator in the path it shows; the checks use the normalized one.
    let mut shown = full.clone().into_os_string();
    if ends_with_separator(file) {
        shown.push(std::path::MAIN_SEPARATOR_STR);
    }
    let shown = PathBuf::from(shown);
    let target = match options.policy {
        LinkPolicy::FollowToTarget => final_target(&full, &shown)?,
        LinkPolicy::Refuse => unlinked(&full, &shown)?,
    };
    if ends_with_separator(file) {
        // .NET keeps the separator: the folder of the target is then `...\a.txt`, which it
        // creates, and the move onto it fails.
        fs::create_dir_all(&target)?;
        return Err(io::Error::other("the path ends with a separator").into());
    }
    let folder = target
        .parent()
        .ok_or_else(|| io::Error::other("no folder"))?;
    fs::create_dir_all(folder)?;
    let mode = kept_mode(&target, options);
    let mut name = OsString::from(".");
    name.push(
        target
            .file_name()
            .ok_or_else(|| io::Error::other("no file name"))?,
    );
    let random = Uuid::new_v4().simple().to_string();
    name.push(format!(".{}.{}.tmp", &random[..8], &random[8..11]));
    let temporary = folder.join(name);
    let outcome = fill_and_move(&temporary, &target, content, mode, options.durable);
    if outcome.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    outcome?;
    Ok(target)
}

/// Mode the file ends with, on Unix: the one of the target, else the one asked for.
#[cfg(unix)]
fn kept_mode(target: &Path, options: &WriteOptions) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    match fs::metadata(target) {
        Ok(m) if !m.is_dir() => Some(m.permissions().mode() & 0o7777),
        _ => options.new_file_mode,
    }
}

#[cfg(not(unix))]
fn kept_mode(_: &Path, _: &WriteOptions) -> Option<u32> {
    None
}

fn fill_and_move(
    temporary: &Path,
    target: &Path,
    content: &[u8],
    mode: Option<u32>,
    durable: bool,
) -> io::Result<()> {
    let mut open = fs::OpenOptions::new();
    open.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        if let Some(mode) = mode {
            // Born with the final mode (the umask can only narrow it): a private content is
            // never readable by others, not even before the rename.
            open.mode(mode);
        }
    }
    let mut stream = open.open(temporary)?;
    stream.write_all(content)?;
    if durable {
        stream.sync_all()?;
    }
    drop(stream);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Some(mode) = mode {
            // Undoes the umask, so the file ends with exactly the mode it had.
            fs::set_permissions(temporary, fs::Permissions::from_mode(mode))?;
        }
    }
    #[cfg(not(unix))]
    let _ = mode;
    fs::rename(temporary, target)
}

/// The target of the link at `path`; `None` when `path` is not a link (or does not exist).
/// Any other failure is an IO error, not a "no link".
fn link_target(path: &Path) -> Result<Option<PathBuf>> {
    // Only a symlink or a junction is a link: a cloud placeholder or an app-exec link is a
    // reparse point that `read_link` refuses, and .NET sees no link there.
    match fs::symlink_metadata(path) {
        Ok(m) if m.file_type().is_symlink() => {}
        Ok(_) => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    match fs::read_link(path) {
        Ok(link) => Ok(Some(link)),
        Err(e) => {
            let not_a_link = match e.kind() {
                io::ErrorKind::NotFound => true,
                // EINVAL on Unix.
                io::ErrorKind::InvalidInput => cfg!(unix),
                // ERROR_NOT_A_REPARSE_POINT on Windows.
                _ => cfg!(windows) && e.raw_os_error() == Some(4390),
            };
            if not_a_link { Ok(None) } else { Err(e.into()) }
        }
    }
}

/// The path itself, as long as no link stands there.
fn unlinked(full: &Path, shown: &Path) -> Result<PathBuf> {
    if link_target(full)?.is_some() {
        return Err(refused(
            shown,
            "it is a link, and the write would replace the link itself",
        ));
    }
    if full.is_dir() {
        return Err(refused(shown, "it is a folder"));
    }
    Ok(full.to_path_buf())
}

/// Comparison of `LocalPaths.Comparison`: `OrdinalIgnoreCase` on Windows (simple uppercase
/// of each char), ordinal elsewhere.
fn key(path: &Path) -> String {
    let text = path.to_string_lossy();
    if cfg!(windows) {
        text.chars()
            .map(|c| {
                let mut upper = c.to_uppercase();
                match (upper.next(), upper.next()) {
                    (Some(u), None) => u,
                    _ => c,
                }
            })
            .collect()
    } else {
        text.into_owned()
    }
}

/// The form .NET `FileInfo.LinkTarget` gives to a verbatim target read on Windows: `\\?\X:\a`
/// becomes `X:\a`, `\\?\UNC\s\h\a` becomes `\\s\h\a`; any other form (`Volume{GUID}`,
/// `GLOBALROOT`) stays as it is.
fn plain(path: PathBuf) -> PathBuf {
    let Some(rest) = path.to_str().and_then(|t| t.strip_prefix(r"\\?\")) else {
        return path;
    };
    if let Some(unc) = rest.strip_prefix(r"UNC\") {
        return PathBuf::from(format!(r"\\{unc}"));
    }
    let b = rest.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return PathBuf::from(rest);
    }
    path
}

/// End of the chain of links that starts at `full`: the file a write there should reach.
fn final_target(full: &Path, shown: &Path) -> Result<PathBuf> {
    let mut current = full.to_path_buf();
    let mut seen = HashSet::from([key(&current)]);
    for followed in 0..=MAXIMUM_LINKS {
        let Some(link) = link_target(&current)? else {
            return end_of_chain(shown, current, followed);
        };
        let link = if cfg!(windows) { plain(link) } else { link };
        // The target of a link may be written relative to the folder holding the link.
        let folder = current
            .parent()
            .ok_or_else(|| io::Error::other("no folder"))?;
        current = normalize(&folder.join(link));
        if !seen.insert(key(&current)) {
            return Err(refused(
                shown,
                &format!("its links come back to {}, in a circle", current.display()),
            ));
        }
    }
    Err(refused(
        shown,
        &format!("its links go through more than {MAXIMUM_LINKS} steps"),
    ))
}

fn end_of_chain(full: &Path, current: PathBuf, followed: usize) -> Result<PathBuf> {
    if current.is_dir() {
        return Err(refused(
            full,
            &if followed == 0 {
                "it is a folder".to_owned()
            } else {
                format!("its links end on the folder {}", current.display())
            },
        ));
    }
    if followed > 0 && !current.exists() {
        return Err(refused(
            full,
            &format!(
                "its links end on {}, which does not exist",
                current.display()
            ),
        ));
    }
    Ok(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_file_and_a_missing_path_are_not_links() {
        let dir = std::env::temp_dir().join(format!("sermofur-atomic-unit-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("f");
        fs::write(&file, "x").expect("file");
        assert!(link_target(&file).expect("file").is_none());
        assert!(link_target(&dir).expect("folder").is_none());
        assert!(link_target(&dir.join("nope")).expect("missing").is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_is_a_link() {
        let dir = std::env::temp_dir().join(format!("sermofur-atomic-unit-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        std::os::unix::fs::symlink(dir.join("nowhere"), dir.join("l")).expect("symlink");
        assert!(link_target(&dir.join("l")).expect("link").is_some());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn verbatim_targets_take_the_form_dotnet_gives() {
        let p = |s: &str| plain(PathBuf::from(s));
        assert_eq!(p(r"\\?\C:\a\b"), PathBuf::from(r"C:\a\b"));
        assert_eq!(p(r"\\?\UNC\srv\share\a"), PathBuf::from(r"\\srv\share\a"));
        assert_eq!(
            p(r"\\?\Volume{1234}\a"),
            PathBuf::from(r"\\?\Volume{1234}\a")
        );
        assert_eq!(
            p(r"\\?\GLOBALROOT\Device\x"),
            PathBuf::from(r"\\?\GLOBALROOT\Device\x")
        );
        assert_eq!(p(r"C:\a"), PathBuf::from(r"C:\a"));
    }
}
