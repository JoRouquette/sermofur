//! Where the daemon of the current user keeps its registry, its state and its endpoint, as
//! `DaemonPaths` of the .NET oracle (`rust/tests/golden/daemon-files.json` and
//! `daemon-files-unix.json` are the truth). `SERMOFUR_DAEMON_HOME` moves all three under one
//! folder, so that tests and trials never reach the real daemon of the user.
//!
//! The module lives in the core so that `doctor` finds the endpoint; `sermofur-daemon`
//! re-exports it.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::discovery::get_full_path;
use crate::error::{Result, SermofurError};
use crate::instance::invalid_storage;
use crate::local_paths::is_path_fully_qualified;

/// Environment variable that replaces every location.
pub const HOME_VARIABLE: &str = "SERMOFUR_DAEMON_HOME";

/// Longest Unix socket path, in bytes, accepted on every supported system: `sun_path` holds
/// 104 bytes on macOS and 108 on Linux, terminator included.
pub const MAX_SOCKET_PATH_BYTES: usize = 103;

/// The folders of the user profile that `Environment.GetFolderPath` gives the oracle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialFolder {
    ApplicationData,
    LocalApplicationData,
    UserProfile,
}

/// The three locations of the daemon of one user.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DaemonPaths {
    pub config_directory: PathBuf,
    pub state_directory: PathBuf,
    /// A pipe name on Windows, a socket path elsewhere.
    pub endpoint: PathBuf,
}

impl DaemonPaths {
    pub fn registry_file(&self) -> PathBuf {
        combine(&self.config_directory, "instances.json")
    }

    pub fn log_file(&self) -> PathBuf {
        combine(&self.state_directory, "daemon.log")
    }

    pub fn lock_file(&self) -> PathBuf {
        combine(&self.state_directory, "daemon.lock")
    }

    /// Folder that holds the Unix socket; `None` on Windows, where the endpoint is a pipe.
    pub fn runtime_directory(&self) -> Option<PathBuf> {
        if cfg!(windows) {
            None
        } else {
            // `Path.GetDirectoryName` collapses doubled separators.
            let parent = self.endpoint.parent()?.to_string_lossy().into_owned();
            let mut collapsed = String::with_capacity(parent.len());
            for c in parent.chars() {
                if !(c == '/' && collapsed.ends_with('/')) {
                    collapsed.push(c);
                }
            }
            Some(PathBuf::from(collapsed))
        }
    }

    /// Locations of the current user, from the environment of this process.
    pub fn for_current_user() -> Result<Self> {
        Self::resolve(&environment_text, &system_folder)
    }

    /// Resolution with replaceable environment lookups (`DaemonPaths.Resolve`), asked in the
    /// order of the oracle. A profile folder the shared files cannot live under (Windows: an
    /// APPDATA or LOCALAPPDATA that is absent, empty, not Unicode or relative; Unix: an empty or
    /// relative home that the configuration or the state needs) is refused with
    /// `invalid_storage_or_path`, where .NET would put the registry, the lock and the journal in
    /// the current folder (PARITY.md, decision of John of 2026-11-11). The endpoint alone is
    /// given by [`DaemonPaths::locate`], which refuses nothing of the kind.
    pub fn resolve(
        variable: &dyn Fn(&str) -> Option<String>,
        folder: &dyn Fn(SpecialFolder) -> String,
    ) -> Result<Self> {
        match Self::locate(variable, folder)? {
            (paths, true) => Ok(paths),
            (_, false) => Err(invalid_storage()),
        }
    }

    /// The locations as .NET computes them, and whether their folders are usable for the shared
    /// files (see [`DaemonPaths::resolve`]); `doctor` reads the endpoint from it.
    pub fn locate(
        variable: &dyn Fn(&str) -> Option<String>,
        folder: &dyn Fn(SpecialFolder) -> String,
    ) -> Result<(Self, bool)> {
        if let Some(home) = non_empty(variable(HOME_VARIABLE)) {
            let root = full_path_as_typed(&home)?;
            let endpoint = if cfg!(windows) {
                PathBuf::from(format!(
                    "sermofur-home-{}",
                    short_hash(&root.to_string_lossy())
                ))
            } else {
                combine(&combine(&root, "run"), "daemon.sock")
            };
            return Ok((
                checked(Self {
                    config_directory: combine(&root, "config"),
                    state_directory: combine(&root, "state"),
                    endpoint,
                })?,
                true,
            ));
        }
        if cfg!(windows) {
            let config = folder(SpecialFolder::ApplicationData);
            let state = folder(SpecialFolder::LocalApplicationData);
            let usable = is_path_fully_qualified(&config) && is_path_fully_qualified(&state);
            return Ok((
                Self {
                    config_directory: combine(Path::new(&config), "Sermofur"),
                    state_directory: combine(Path::new(&state), "Sermofur"),
                    endpoint: PathBuf::from(format!("sermofur-{}", short_hash(&current_sid()?))),
                },
                usable,
            ));
        }
        let user_home = PathBuf::from(folder(SpecialFolder::UserProfile));
        let home_usable = user_home.is_absolute();
        let private = format!("sermofur-{}", user_id());
        let temporary =
            PathBuf::from(non_empty(variable("TMPDIR")).unwrap_or_else(|| "/tmp".to_owned()));
        if cfg!(target_os = "macos") {
            let library = combine(&user_home, "Library");
            return Ok((
                checked(Self {
                    config_directory: combine(
                        &combine(&library, "Application Support"),
                        "Sermofur",
                    ),
                    state_directory: combine(&combine(&library, "Logs"), "Sermofur"),
                    endpoint: combine(&combine(&temporary, &private), "daemon.sock"),
                })?,
                home_usable,
            ));
        }
        let config = non_empty(variable("XDG_CONFIG_HOME"));
        let state = non_empty(variable("XDG_STATE_HOME"));
        let usable = home_usable || (config.is_some() && state.is_some());
        let config = config.map_or_else(|| combine(&user_home, ".config"), PathBuf::from);
        let state = state.map_or_else(
            || combine(&combine(&user_home, ".local"), "state"),
            PathBuf::from,
        );
        let socket_folder = match non_empty(variable("XDG_RUNTIME_DIR")) {
            Some(runtime) => combine(Path::new(&runtime), "sermofur"),
            None => combine(&temporary, &private),
        };
        Ok((
            checked(Self {
                config_directory: combine(&config, "sermofur"),
                state_directory: combine(&state, "sermofur"),
                endpoint: combine(&socket_folder, "daemon.sock"),
            })?,
            usable,
        ))
    }
}

/// `Encoding.UTF8.GetByteCount(endpoint) > 103`: `invalid_path`, exit 3 (Unix only).
fn checked(paths: DaemonPaths) -> Result<DaemonPaths> {
    let endpoint = paths.endpoint.to_string_lossy();
    if !cfg!(windows) && endpoint.len() > MAX_SOCKET_PATH_BYTES {
        return Err(SermofurError::new(
            "invalid_path",
            format!(
                "The daemon socket path is longer than {MAX_SOCKET_PATH_BYTES} bytes: {endpoint}. Set {HOME_VARIABLE} or TMPDIR to a shorter folder."
            ),
            3,
        ));
    }
    Ok(paths)
}

/// `Path.Combine(first, second)` for a relative `second`: a separator is added unless `first`
/// is empty or already ends with one. `C:` gives `C:\Sermofur`, where `Path::join` would give
/// `C:Sermofur`.
pub fn combine(first: &Path, second: &str) -> PathBuf {
    let mut out = OsString::from(first.as_os_str());
    if !out.is_empty() && !ends_with_separator(first) {
        out.push(std::path::MAIN_SEPARATOR_STR);
    }
    out.push(second);
    PathBuf::from(out)
}

fn ends_with_separator(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    bytes.last() == Some(&b'/') || (cfg!(windows) && bytes.last() == Some(&b'\\'))
}

/// `Path.GetFullPath`, which keeps a final separator (the hash of the home depends on it).
fn full_path_as_typed(home: &str) -> Result<PathBuf> {
    let full = get_full_path(Path::new(home))?;
    if ends_with_separator(Path::new(home)) && !ends_with_separator(&full) {
        return Ok(combine(&full, ""));
    }
    Ok(full)
}

/// `string.IsNullOrWhiteSpace` read as absent (`DaemonPaths.NonEmpty`).
fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|text| !text.chars().all(char::is_whitespace))
}

/// The first 16 hexadecimal digits, in lower case, of the SHA-256 of a text (`DaemonPaths.Hash`).
pub fn short_hash(text: &str) -> String {
    crate::store::sha256_upper(text.as_bytes())[..16].to_lowercase()
}

/// A variable of the environment, a value that is not Unicode converted with U+FFFD in place of
/// each invalid part. Under Unix .NET does the same (measured on `SERMOFUR_DAEMON_HOME`). Under
/// Windows .NET keeps the raw UTF-16, lone surrogate included, in the folders it combines; the
/// port cannot, so those folders differ, while the pipe hash, taken on the UTF-8 of the text,
/// agrees (PARITY.md).
pub fn environment_text(name: &str) -> Option<String> {
    std::env::var_os(name).map(|value| value.to_string_lossy().into_owned())
}

/// The folders of `Environment.GetFolderPath` for the current user.
// ponytail: Windows reads APPDATA and LOCALAPPDATA, where .NET asks SHGetKnownFolderPath (a
// Win32 call outside the derogation); they differ only if the variables were changed. On Unix
// an empty HOME gives what `std::env::home_dir` gives, where .NET reads the passwd entry.
pub fn system_folder(folder: SpecialFolder) -> String {
    // A value that is not Unicode is no folder: empty, then refused by `resolve`.
    let unicode = |name: &str| std::env::var(name).unwrap_or_default();
    match folder {
        SpecialFolder::ApplicationData => unicode("APPDATA"),
        SpecialFolder::LocalApplicationData => unicode("LOCALAPPDATA"),
        SpecialFolder::UserProfile => environment_text("HOME")
            .filter(|home| !home.is_empty())
            .or_else(|| std::env::home_dir().map(|home| home.to_string_lossy().into_owned()))
            .unwrap_or_default(),
    }
}

#[cfg(windows)]
fn current_sid() -> Result<String> {
    crate::ownership::current_user_sid().map_err(|_| invalid_storage())
}

#[cfg(not(windows))]
fn current_sid() -> Result<String> {
    Err(invalid_storage())
}

#[cfg(unix)]
fn user_id() -> u32 {
    crate::ownership::effective_user_id()
}

#[cfg(not(unix))]
fn user_id() -> u32 {
    0
}

#[cfg(test)]
mod tests {
    use super::DaemonPaths;
    use std::path::PathBuf;

    fn refused(paths: crate::error::Result<DaemonPaths>) {
        let error = paths.expect_err("an unusable profile folder");
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("invalid_storage_or_path", 3)
        );
    }

    /// An empty or relative profile folder would put the shared files in the current folder:
    /// refused under Windows (PARITY.md, decision of John of 2026-11-11); the endpoint, which
    /// `doctor` reads, is still the one of the SID.
    #[cfg(windows)]
    #[test]
    fn a_profile_folder_that_is_not_qualified_is_refused() {
        use super::short_hash;
        let sid = crate::ownership::current_user_sid().expect("the SID");
        for value in ["", ".", "C:", r"Roaming\x"] {
            refused(DaemonPaths::resolve(&|_| None, &|_| value.to_owned()));
            let (paths, usable) =
                DaemonPaths::locate(&|_| None, &|_| value.to_owned()).expect("the endpoint");
            assert!(!usable);
            assert_eq!(
                paths.endpoint,
                PathBuf::from(format!("sermofur-{}", short_hash(&sid)))
            );
        }
        let paths = DaemonPaths::resolve(&|_| None, &|_| r"C:\Roaming".to_owned()).expect("paths");
        assert_eq!(
            paths.config_directory,
            PathBuf::from(r"C:\Roaming\Sermofur")
        );
    }

    /// An empty or relative home, when the configuration or the state needs it, is refused
    /// under Unix; with both XDG folders set, it is not needed. The endpoint stays.
    #[cfg(unix)]
    #[test]
    fn a_home_that_is_not_absolute_is_refused() {
        for home in ["", "relative"] {
            refused(DaemonPaths::resolve(&|_| None, &|_| home.to_owned()));
            let (paths, usable) =
                DaemonPaths::locate(&|_| None, &|_| home.to_owned()).expect("the endpoint");
            assert!(!usable);
            let uid = crate::ownership::effective_user_id();
            assert_eq!(
                paths.endpoint,
                PathBuf::from(format!("/tmp/sermofur-{uid}/daemon.sock"))
            );
        }
        let xdg = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some("/x/config".to_owned()),
            "XDG_STATE_HOME" => Some("/x/state".to_owned()),
            _ => None,
        };
        if cfg!(target_os = "linux") {
            assert!(DaemonPaths::resolve(&xdg, &|_| String::new()).is_ok());
        }
    }
}
