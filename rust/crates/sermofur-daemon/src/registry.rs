//! The instances the user entrusted to the daemon, as `InstanceRegistry` of the .NET oracle
//! (`rust/tests/golden/daemon-files.json`, sections `registry_*`, is the truth): a small JSON
//! file in the user configuration folder, replaced atomically under a lock file, never
//! rewritten when it cannot be read.

use std::fs::{self, File};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use sermofur_core::NEW_LINE;
use sermofur_core::atomic::{self, WriteError, WriteOptions};
use sermofur_core::discovery::{discover, get_full_path};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::{DateTimeOffset, local_offset_of_clock};
use sermofur_core::instance::{CONFIGURATION_FILE, MARKER, read_configuration};
use sermofur_core::json::quoted_basic_latin;
use sermofur_core::local_paths::{is_path_fully_qualified, same_root, trim_ending_separator};
use sermofur_core::ownership::{FileOwnership, SystemOwnership};
use sermofur_core::stj::{self, Node};

use crate::file_turn::{FileError, try_take};

/// Largest registry file read.
pub const MAX_BYTES: u64 = 64 * 1024;
/// Most instances registered.
pub const MAX_ENTRIES: usize = 256;
/// How long a change waits for another process that changes the registry.
pub const DEFAULT_LOCK_TIMEOUT: Duration = Duration::from_secs(10);

const FORMAT_VERSION: i32 = 1;
const SAVE_ATTEMPTS: u32 = 10;
const LOCK_POLL: Duration = Duration::from_millis(20);
const SAVE_RETRY_DELAY: Duration = Duration::from_millis(50);

/// One instance the daemon may serve; both fields may be `null` in a file written by hand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegisteredInstance {
    pub root: Option<String>,
    pub registered_at: Option<String>,
}

/// An entry of the registry with whether its instance is still where it was registered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegistryEntry {
    pub root: String,
    pub registered_at: Option<String>,
    pub missing: bool,
}

/// What tells a changed registry file from the one already read: its date of last write and
/// its length; `None` for a registry that does not exist (`RegistryStamp.Absent`).
pub type RegistryStamp = Option<(SystemTime, u64)>;

/// The registry file of one user.
pub struct InstanceRegistry {
    file: PathBuf,
    owners: Box<dyn FileOwnership + Send + Sync>,
    lock_timeout: Duration,
}

impl InstanceRegistry {
    pub fn new(file: &Path) -> Self {
        Self::with(file, Box::new(SystemOwnership), DEFAULT_LOCK_TIMEOUT)
    }

    /// A registry with its own ownership check and lock timeout, for tests.
    pub fn with(
        file: &Path,
        owners: Box<dyn FileOwnership + Send + Sync>,
        lock_timeout: Duration,
    ) -> Self {
        Self {
            file: file.to_path_buf(),
            owners,
            lock_timeout,
        }
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    /// File held while a process changes the registry.
    pub fn lock_file(&self) -> PathBuf {
        let mut name = self.file.clone().into_os_string();
        name.push(".lock");
        PathBuf::from(name)
    }

    /// Registered instances; an absent file is an empty registry.
    pub fn read(&self) -> Result<Vec<RegisteredInstance>> {
        let Some(content) = self.read_content()? else {
            return Ok(Vec::new());
        };
        let (version, instances) = parse(&content).ok_or_else(|| self.invalid("malformed JSON"))?;
        let Some(instances) = instances else {
            return Err(self.invalid("unexpected content"));
        };
        if version != FORMAT_VERSION || instances.len() > MAX_ENTRIES {
            return Err(self.invalid("unexpected content"));
        }
        let mut entries = Vec::with_capacity(instances.len());
        for entry in instances {
            // ponytail: a null entry stops .NET with a NullReferenceException; the port
            // answers as the command runner answers the runtime errors it knows.
            let entry = entry.ok_or_else(invalid_storage)?;
            if !entry.root.as_deref().is_some_and(is_path_fully_qualified) {
                return Err(self.invalid("unexpected content"));
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    /// The bytes of the registry, `None` when absent.
    fn read_content(&self) -> Result<Option<Vec<u8>>> {
        let file = match File::open(&self.file) {
            Ok(file) => file,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                ) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(FileError::from(error).into()),
        };
        let length = file.metadata().map_err(FileError::from)?.len();
        if length > MAX_BYTES {
            return Err(self.invalid(&format!("larger than {MAX_BYTES} bytes")));
        }
        // Bounded even if the file grows after the check; the oracle reads it whole.
        let mut content = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(FileError::from)?;
        if content.len() as u64 > MAX_BYTES {
            return Err(self.invalid(&format!("larger than {MAX_BYTES} bytes")));
        }
        Ok(Some(content))
    }

    /// Entries with a flag for instances no longer found at their root.
    pub fn list(&self) -> Result<Vec<RegistryEntry>> {
        Ok(self
            .read()?
            .into_iter()
            .map(|entry| {
                let root = entry.root.unwrap_or_default();
                let configuration = Path::new(&root).join(MARKER).join(CONFIGURATION_FILE);
                RegistryEntry {
                    missing: !configuration.is_file(),
                    root,
                    registered_at: entry.registered_at,
                }
            })
            .collect())
    }

    /// Registers the instance found from `path`, with the checks of a direct command and their
    /// error codes; idempotent. `now` is written as `yyyy-MM-dd'T'HH:mm:sszzz`.
    pub fn register(&self, path: &Path, now: DateTimeOffset) -> Result<RegisteredInstance> {
        let root = discover(path, self.owners.as_ref())?;
        read_configuration(&root, false, local_offset_of_clock)?;
        let root = root.to_string_lossy().into_owned();
        let _turn = self.take_lock()?;
        let mut entries = self.read()?;
        if let Some(existing) = entries.iter().find(|entry| same(entry, &root)) {
            return Ok(existing.clone());
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(self.invalid(&format!("more than {MAX_ENTRIES} instances")));
        }
        let added = RegisteredInstance {
            root: Some(root),
            registered_at: Some(now.to_seconds()),
        };
        entries.push(added.clone());
        self.save(&entries)?;
        Ok(added)
    }

    /// Removes the instance registered at `path`, or the one found from it; `not_registered`
    /// when neither is in the registry.
    pub fn unregister(&self, path: &Path) -> Result<RegisteredInstance> {
        let _turn = self.take_lock()?;
        let mut entries = self.read()?;
        let full = trim_ending_separator(&get_full_path(path)?.to_string_lossy());
        let mut index = entries.iter().position(|entry| same(entry, &full));
        if index.is_none() {
            // A moved or deleted instance is removed by the exact path it was registered at.
            // .NET catches only its refusals: a runtime error (`invalid_storage_or_path`, an NT
            // path refused by `DriveInfo` among others) goes through.
            match discover(Path::new(&full), self.owners.as_ref()) {
                Ok(root) => {
                    let root = root.to_string_lossy();
                    index = entries.iter().position(|entry| same(entry, &root));
                }
                Err(error) if error.code == "invalid_storage_or_path" => return Err(error),
                Err(_) => {}
            }
        }
        let Some(index) = index else {
            return Err(SermofurError::new(
                "not_registered",
                format!("No registered instance at {full}; see: smf daemon instances"),
                1,
            ));
        };
        let removed = entries.remove(index);
        self.save(&entries)?;
        Ok(removed)
    }

    /// Date of the last write and length of the file, to reload it only when it changed.
    pub fn stamp(&self) -> RegistryStamp {
        let metadata = fs::metadata(&self.file)
            .ok()
            .filter(fs::Metadata::is_file)?;
        Some((metadata.modified().ok()?, metadata.len()))
    }

    /// One change at a time, across processes.
    fn take_lock(&self) -> Result<File> {
        try_take(&self.lock_file(), self.lock_timeout, LOCK_POLL)?.ok_or_else(|| {
            // `{lockLimit.TotalSeconds:0}`: rounded half away from zero.
            let seconds = self.lock_timeout.as_secs_f64().round();
            self.busy(&format!(
                "another smf process has been changing it for {seconds} s"
            ))
        })
    }

    /// Replaces the file under the lock. On Windows a reader of another process may hold the
    /// file for an instant: the replacement is tried again briefly, then reported as
    /// `registry_busy`, nothing changed. Any other failure is reported as it is.
    fn save(&self, entries: &[RegisteredInstance]) -> Result<()> {
        let content = to_json(entries);
        let mut attempt = 1;
        loop {
            match atomic::write(&self.file, content.as_bytes(), &WriteOptions::default()) {
                Ok(_) => return Ok(()),
                Err(WriteError::Io(error)) if self.held_by_a_reader(&error) => {
                    if attempt >= SAVE_ATTEMPTS {
                        return Err(self.busy("the file is held open by another program"));
                    }
                    attempt += 1;
                    thread::sleep(SAVE_RETRY_DELAY);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    /// A replacement refused because another process has the file open (Windows only): a
    /// sharing or lock violation, or the access denied of a rename onto an open file. A
    /// read-only target refuses the rename for good: that is not a reader.
    fn held_by_a_reader(&self, error: &io::Error) -> bool {
        if !cfg!(windows) {
            return false;
        }
        match error.raw_os_error() {
            Some(32 | 33) => true,
            Some(5) => fs::metadata(&self.file)
                .is_ok_and(|metadata| metadata.is_file() && !metadata.permissions().readonly()),
            _ => false,
        }
    }

    fn busy(&self, reason: &str) -> SermofurError {
        SermofurError::new(
            "registry_busy",
            format!(
                "The daemon registry {} is busy ({reason}); nothing changed, run the command again.",
                self.file.display()
            ),
            3,
        )
    }

    fn invalid(&self, reason: &str) -> SermofurError {
        SermofurError::new(
            "invalid_registry",
            format!(
                "The daemon registry {} cannot be used ({reason}); it is left untouched.",
                self.file.display()
            ),
            3,
        )
    }
}

fn invalid_storage() -> SermofurError {
    SermofurError::new(
        "invalid_storage_or_path",
        "Invalid path or data; run doctor.",
        3,
    )
}

/// Same root, with the case rule of the operating system (`InstanceRegistry.Same`); a `null`
/// root is never the same.
fn same(entry: &RegisteredInstance, root: &str) -> bool {
    entry
        .root
        .as_deref()
        .is_some_and(|text| same_root(text, root))
}

/// An entry list read from the file: `None` for a `null` entry.
type Instances = Option<Vec<Option<RegisteredInstance>>>;

/// `JsonSerializer.Deserialize<RegistryDocument>` with the default options: no byte order mark,
/// exact property names, the last duplicate wins after each was converted,
/// unknown properties skipped. `None` where .NET throws a `JsonException`; a `null` document
/// reads as no instance list.
fn parse(content: &[u8]) -> Option<(i32, Instances)> {
    let node = stj::parse(&lenient_text(content)?).ok()?;
    let Some(properties) = stj::object(&node).ok()? else {
        return Some((0, None));
    };
    let mut version = 0;
    let mut instances = None;
    for (name, value) in properties {
        match name {
            "version" => version = stj::integer::<i32>(value).ok()?,
            "instances" => instances = list(value)?,
            _ => {}
        }
    }
    Some((version, instances))
}

/// The text of a JSON document as `JsonSerializer.Deserialize` reads its bytes: .NET checks
/// UTF-8 only where it reads a string, so an invalid sequence becomes a lone surrogate escape,
/// refused where a string is read, skipped elsewhere as STJ skips it. Right after a backslash it
/// is a broken escape: `None`, refused even in a skipped value.
pub(crate) fn lenient_text(content: &[u8]) -> Option<String> {
    let mut text = String::with_capacity(content.len());
    for chunk in content.utf8_chunks() {
        text.push_str(chunk.valid());
        if !chunk.invalid().is_empty() {
            let backslashes = text.bytes().rev().take_while(|b| *b == b'\\').count();
            if backslashes % 2 == 1 {
                return None;
            }
            text.push_str("\\uDFFF");
        }
    }
    Some(text)
}

fn list(node: &Node) -> Option<Instances> {
    match node {
        Node::Null => Some(None),
        Node::Array(items) => items
            .iter()
            .map(entry)
            .collect::<Option<Vec<_>>>()
            .map(Some),
        _ => None,
    }
}

fn entry(node: &Node) -> Option<Option<RegisteredInstance>> {
    let Some(properties) = stj::object(node).ok()? else {
        return Some(None);
    };
    let mut entry = RegisteredInstance {
        root: None,
        registered_at: None,
    };
    for (name, value) in properties {
        match name {
            "root" => entry.root = stj::text(value).ok()?,
            "registeredAt" => entry.registered_at = stj::text(value).ok()?,
            _ => {}
        }
    }
    Some(Some(entry))
}

/// `JsonSerializer.SerializeToUtf8Bytes` with `WriteIndented`: two spaces, the line break of
/// the platform, the default encoder, `null` written.
pub fn to_json(entries: &[RegisteredInstance]) -> String {
    let text = |value: &Option<String>| {
        value
            .as_deref()
            .map_or_else(|| "null".to_owned(), quoted_basic_latin)
    };
    let mut out =
        format!("{{{NEW_LINE}  \"version\": {FORMAT_VERSION},{NEW_LINE}  \"instances\": [");
    if entries.is_empty() {
        out.push(']');
    } else {
        for (index, entry) in entries.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{NEW_LINE}    {{{NEW_LINE}      \"root\": {},{NEW_LINE}      \"registeredAt\": {}{NEW_LINE}    }}",
                text(&entry.root),
                text(&entry.registered_at)
            ));
        }
        out.push_str(&format!("{NEW_LINE}  ]"));
    }
    out.push_str(&format!("{NEW_LINE}}}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualified_paths_follow_the_rules_of_dotnet() {
        assert!(is_path_fully_qualified(if cfg!(windows) {
            r"C:\a"
        } else {
            "/a"
        }));
        assert!(!is_path_fully_qualified("relative"));
        assert!(!is_path_fully_qualified(""));
    }
}
