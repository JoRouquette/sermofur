//! Lock files beside shared state (`FileTurn`) and the lock of the daemon (`DaemonLock`), as the
//! .NET oracle takes them (`rust/tests/golden/daemon-files.json`, section `locks`, is the
//! truth). A lock file is exclusive across processes (share mode on Windows, `flock` on Unix,
//! so that .NET and Rust processes exclude each other), private to its owner, never written,
//! never deleted.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::get_full_path;
use sermofur_core::error::SermofurError;

/// A failure, sorted as the .NET exception it would be.
#[derive(Debug)]
pub enum FileError {
    /// An `IOException` (a lock held by another process among others).
    Io(io::Error),
    /// An `UnauthorizedAccessException`: access denied, or a folder where a file should be.
    Unauthorized(io::Error),
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        if is_unauthorized(&error) {
            Self::Unauthorized(error)
        } else {
            Self::Io(error)
        }
    }
}

/// Both become `invalid_storage_or_path`, exit 3, as the .NET command runner maps them.
impl From<FileError> for SermofurError {
    fn from(_: FileError) -> Self {
        Self::new(
            "invalid_storage_or_path",
            "Invalid path or data; run doctor.",
            3,
        )
    }
}

/// `UnauthorizedAccessException`: `ERROR_ACCESS_DENIED` on Windows; `EACCES`, `EPERM`,
/// `EBADF` and `EISDIR` (which .NET turns into `EACCES`) on Unix.
fn is_unauthorized(error: &io::Error) -> bool {
    if cfg!(windows) {
        error.raw_os_error() == Some(5)
    } else {
        matches!(error.raw_os_error(), Some(1 | 9 | 13 | 21))
    }
}

/// Opens `lock_file` exclusively, creating it; [`FileError::Io`] when another process holds
/// it. On Unix a new lock file is readable by its owner only, and one left with wider rights
/// is narrowed.
pub fn take(lock_file: &Path) -> Result<File, FileError> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    exclusive(&mut options);
    let file = options.open(lock_file)?;
    lock(&file)?;
    narrow(&file)?;
    Ok(file)
}

#[cfg(windows)]
fn exclusive(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    // FileShare.None: the share mode is the lock.
    options.share_mode(0);
}

#[cfg(unix)]
fn exclusive(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(any(windows, unix)))]
fn exclusive(_options: &mut OpenOptions) {}

/// `flock(LOCK_EX | LOCK_NB)`, as .NET takes it for `FileShare.None`; a lock refused for
/// another reason than contention is ignored, as .NET ignores it.
#[cfg(unix)]
fn lock(file: &File) -> Result<(), FileError> {
    match file.try_lock() {
        Err(fs::TryLockError::WouldBlock) => {
            Err(FileError::Io(io::Error::from(io::ErrorKind::WouldBlock)))
        }
        _ => Ok(()),
    }
}

#[cfg(not(unix))]
fn lock(_file: &File) -> Result<(), FileError> {
    Ok(())
}

/// Narrows the lock file to 0600; not ours to change is not an error.
#[cfg(unix)]
fn narrow(file: &File) -> Result<(), FileError> {
    use std::os::unix::fs::PermissionsExt;
    let mode = file.metadata()?.permissions().mode() & 0o7777;
    if mode != 0o600 {
        match file.set_permissions(fs::Permissions::from_mode(0o600)) {
            Err(error) if !is_unauthorized(&error) => return Err(FileError::Io(error)),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(not(unix))]
fn narrow(_file: &File) -> Result<(), FileError> {
    Ok(())
}

/// Waits for the turn up to `timeout`, trying again every `poll`; `None` when it did not come.
/// Every IO failure is waited for like a held lock, as .NET catches every `IOException`; only
/// an access refusal ([`FileError::Unauthorized`]) returns at once.
pub fn try_take(
    lock_file: &Path,
    timeout: Duration,
    poll: Duration,
) -> Result<Option<File>, FileError> {
    // `Path.GetFullPath`: `.` and `..` resolved before the folder is created.
    let full =
        get_full_path(lock_file).map_err(|error| FileError::Io(io::Error::other(error.message)))?;
    if let Some(folder) = full.parent() {
        create_private_folder(folder)?;
    }
    let started = Instant::now();
    loop {
        match take(&full) {
            Ok(file) => return Ok(Some(file)),
            Err(FileError::Io(_)) if started.elapsed() < timeout => thread::sleep(poll),
            Err(FileError::Io(_)) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
}

/// Creates a missing folder, open to its owner only on Unix; the folders above it get the
/// default mode, as .NET creates them. An existing folder keeps its mode.
pub fn create_private_folder(folder: &Path) -> Result<(), FileError> {
    if folder.is_dir() {
        return Ok(());
    }
    if let Some(parent) = folder.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut builder = fs::DirBuilder::new();
    private(&mut builder);
    match builder.create(folder) {
        Err(_) if folder.is_dir() => Ok(()),
        outcome => Ok(outcome?),
    }
}

#[cfg(unix)]
fn private(builder: &mut fs::DirBuilder) {
    use std::os::unix::fs::DirBuilderExt;
    builder.mode(0o700);
}

#[cfg(not(unix))]
fn private(_builder: &mut fs::DirBuilder) {}

/// `A Sermofur daemon already serves this user.` (`IpcEndpoint.AlreadyRunning`).
pub fn already_running() -> SermofurError {
    SermofurError::new(
        "daemon_already_running",
        "A Sermofur daemon already serves this user.",
        3,
    )
}

/// One daemon per user: takes the lock of the daemon for the life of the process, or
/// `daemon_already_running` (`DaemonLock.Acquire`).
pub fn acquire(paths: &DaemonPaths) -> Result<File, SermofurError> {
    create_private_folder(&paths.state_directory)?;
    match take(&paths.lock_file()) {
        Ok(file) => Ok(file),
        Err(FileError::Io(_)) => Err(already_running()),
        Err(error) => Err(error.into()),
    }
}

/// True while a process holds the lock (`DaemonLock.IsHeld`): a lock file that is not ours to
/// probe holds nothing to wait for.
pub fn is_held(lock_file: &Path) -> bool {
    // `File.Exists`: a file, links followed.
    if !lock_file.is_file() {
        return false;
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    exclusive(&mut options);
    match options.open(lock_file).map_err(FileError::from) {
        Ok(file) => lock(&file).is_err(),
        Err(FileError::Io(error)) => error.kind() != io::ErrorKind::NotFound,
        Err(FileError::Unauthorized(_)) => false,
    }
}

/// Waits until no process holds the lock, or the timeout passes; true when released
/// (`DaemonLock.WaitForReleaseAsync`, polled every 100 ms).
pub fn wait_for_release(lock_file: &Path, timeout: Duration) -> bool {
    let started = Instant::now();
    while is_held(lock_file) {
        if started.elapsed() >= timeout {
            return false;
        }
        thread::sleep(Duration::from_millis(100));
    }
    true
}
