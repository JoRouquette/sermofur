//! Reading a declared source file, as `FileSourceReader` of the .NET oracle reads it, and
//! cutting its text into passages, as `SourcePassages.Split` (`rust/tests/golden/sources.json`
//! on Windows and `sources-unix.json` on Linux are the truth, from `rust/tools/source-oracle`).
//!
//! One bounded read (1 MiB plus one byte), the SHA-256 of exactly the bytes read, one UTF-8 BOM
//! removed, strict UTF-8 (the .NET decoder and `std::str::from_utf8` refuse the same sequences:
//! overlong forms, encoded surrogates, beyond U+10FFFF, truncated), a NUL or invalid UTF-8 is
//! binary, a blank text is empty. A missing, unreadable or invalid file is never an error: the
//! snapshot says so; [`refuse_unindexable`] turns it into the refusal of `source add`.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::error::{Result, SermofurError};
use crate::instance::reject_links;
use crate::local_paths::normalize_mapping;
use crate::ownership::FileOwnership;
use crate::records::{SourceRejection, SourceStatus};
use crate::search::SearchDocument;
use crate::store::sha256_upper;

/// Maximum size of an indexed source: 1 MiB (`SourceLimits.MaxBytes`).
pub const MAX_BYTES: usize = 1024 * 1024;

/// Maximum length of a passage, in UTF-16 units (`SourceLimits.MaxPassageLength`).
pub const MAX_PASSAGE_LENGTH: usize = 2_000;

const BOM: &[u8] = b"\xEF\xBB\xBF";

/// What a reading of a source file found; passages only when indexable (`SourceSnapshot`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceSnapshot {
    pub status: SourceStatus,
    /// Lower-case hexadecimal SHA-256 of the bytes read; empty when nothing was hashed.
    pub hash: String,
    pub size: i64,
    pub passages: Vec<SearchDocument>,
    pub rejection: Option<SourceRejection>,
}

fn empty(status: SourceStatus) -> SourceSnapshot {
    SourceSnapshot {
        status,
        hash: String::new(),
        size: 0,
        passages: Vec::new(),
        rejection: None,
    }
}

fn rejected(rejection: SourceRejection, hash: String, size: usize) -> SourceSnapshot {
    SourceSnapshot {
        status: SourceStatus::Rejected,
        hash,
        size: i64::try_from(size).unwrap_or(i64::MAX),
        passages: Vec::new(),
        rejection: Some(rejection),
    }
}

/// Reads `relative` (stored form, `/` separators) below `root` (`FileSourceReader.Read`).
/// A mapping outside the root, rooted, with a colon, or through a link is rejected as
/// `unsafe_path`; a folder or an absent file is missing; a special file is binary, never opened.
/// Errors only where the oracle throws: a path that `Path.GetFullPath` refuses (a NUL) gives
/// `invalid_storage_or_path`, as the command line maps its `ArgumentException`.
pub fn read(root: &Path, relative: &str, owners: &dyn FileOwnership) -> Result<SourceSnapshot> {
    let full = match normalize_mapping(root, relative)
        .and_then(|full| reject_links(&full).map(|()| full))
    {
        Ok(full) => full,
        Err(error) if error.code == "unsafe_path" || error.code == "scope_boundary" => {
            return Ok(rejected(SourceRejection::UnsafePath, String::new(), 0));
        }
        Err(error) => return Err(error),
    };
    let status = match owners.inspect(&full) {
        Ok(status) => status,
        Err(_) => return Ok(empty(SourceStatus::Unreadable)),
    };
    let Some(status) = status.filter(|status| !status.is_directory) else {
        return Ok(empty(SourceStatus::Missing));
    };
    if !status.is_regular_file {
        // FIFO, device or socket: opening it could block or never end.
        return Ok(rejected(SourceRejection::Binary, String::new(), 0));
    }
    match read_bounded(&full) {
        Ok(bytes) => Ok(snapshot(&bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(empty(SourceStatus::Missing)),
        Err(_) => Ok(empty(SourceStatus::Unreadable)),
    }
}

/// At most [`MAX_BYTES`] plus one byte. The default sharing of `File::open` on Windows (read,
/// write, delete) is the `FileShare` of the oracle. On Unix, `FileStream` takes a shared `flock`
/// and fails only when it would block: a file locked exclusively is unreadable there too.
fn read_bounded(path: &Path) -> io::Result<Vec<u8>> {
    let file = File::open(path)?;
    #[cfg(unix)]
    if let Err(std::fs::TryLockError::WouldBlock) = file.try_lock_shared() {
        return Err(io::Error::from(io::ErrorKind::WouldBlock));
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// The snapshot of bytes read (`FileSourceReader.Snapshot`).
pub fn snapshot(bytes: &[u8]) -> SourceSnapshot {
    if bytes.len() > MAX_BYTES {
        return rejected(SourceRejection::TooLarge, String::new(), bytes.len());
    }
    // The hash covers exactly the bytes read, those that are indexed.
    let hash = sha256_upper(bytes).to_ascii_lowercase();
    let content = bytes.strip_prefix(BOM).unwrap_or(bytes);
    let Ok(text) = std::str::from_utf8(content) else {
        return rejected(SourceRejection::Binary, hash, bytes.len());
    };
    if text.contains('\0') {
        return rejected(SourceRejection::Binary, hash, bytes.len());
    }
    if is_blank(text) {
        return rejected(SourceRejection::Empty, hash, bytes.len());
    }
    SourceSnapshot {
        status: SourceStatus::Indexed,
        hash,
        size: i64::try_from(bytes.len()).unwrap_or(i64::MAX),
        passages: split(text),
        rejection: None,
    }
}

/// The refusal of `source add` for a snapshot that cannot be indexed
/// (`SourceService.RefuseUnindexable`).
pub fn refuse_unindexable(snapshot: &SourceSnapshot) -> Result<()> {
    let refusal = match (snapshot.status, snapshot.rejection) {
        (SourceStatus::Indexed, _) => return Ok(()),
        (SourceStatus::Missing, _) => {
            SermofurError::new("invalid_path", "Source file missing or not a file.", 1)
        }
        (SourceStatus::Unreadable, _) => SermofurError::new(
            "invalid_path",
            "Source file cannot be read; check permissions.",
            1,
        ),
        (SourceStatus::Rejected, Some(SourceRejection::UnsafePath)) => SermofurError::new(
            "unsafe_path",
            "Links, junctions and network paths cannot be sources.",
            4,
        ),
        (SourceStatus::Rejected, rejection) => SermofurError::new(
            "source_rejected",
            match rejection {
                Some(SourceRejection::TooLarge) => "Source larger than 1 MiB.",
                Some(SourceRejection::Binary) => {
                    "Source is not UTF-8 text (binary, NUL byte or special file)."
                }
                Some(SourceRejection::Empty) => "Source is empty.",
                _ => "Source cannot be indexed.",
            },
            1,
        ),
    };
    Err(refusal)
}

/// `string.IsNullOrWhiteSpace`: `char.IsWhiteSpace` and `char::is_whitespace` hold for the same
/// code points (checked against the oracle's table); no supplementary character is white space.
fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

fn units(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Joins `piece` to `current` with `separator` (lengths in UTF-16 units).
fn append(current: &mut (String, usize), separator: &str, piece: &str, length: usize) {
    if current.1 > 0 {
        current.0.push_str(separator);
        current.1 += separator.len();
    }
    current.0.push_str(piece);
    current.1 += length;
}

/// Cuts a text into passages of at most [`MAX_PASSAGE_LENGTH`] UTF-16 units
/// (`SourcePassages.Split`): `\r\n` becomes `\n`; paragraphs (separated by `\n\n`) are grouped
/// while they fit; a longer paragraph is cut on line ends, then on the bound, never inside a
/// surrogate pair. Blank pieces are dropped; passages are numbered from 0.
pub fn split(text: &str) -> Vec<SearchDocument> {
    let normalized = text.replace("\r\n", "\n");
    let mut pieces = Vec::new();
    for paragraph in normalized.split("\n\n").filter(|p| !p.is_empty()) {
        cut(paragraph.trim_matches('\n'), &mut pieces);
    }
    let mut passages: Vec<SearchDocument> = Vec::new();
    let mut current = (String::new(), 0);
    for piece in pieces.iter().filter(|piece| !is_blank(piece)) {
        let length = units(piece);
        if current.1 > 0 && current.1 + 2 + length > MAX_PASSAGE_LENGTH {
            push(&mut passages, std::mem::take(&mut current).0);
        }
        append(&mut current, "\n\n", piece, length);
    }
    if current.1 > 0 {
        push(&mut passages, current.0);
    }
    passages
}

fn push(passages: &mut Vec<SearchDocument>, text: String) {
    let number = i32::try_from(passages.len()).unwrap_or(i32::MAX);
    passages.push((number, text));
}

/// `SourcePassages.Cut`: a paragraph within the bound as is, else its lines, chunked, grouped
/// while they fit.
fn cut(paragraph: &str, pieces: &mut Vec<String>) {
    if units(paragraph) <= MAX_PASSAGE_LENGTH {
        pieces.push(paragraph.to_owned());
        return;
    }
    let mut current = (String::new(), 0);
    for line in paragraph.split('\n') {
        for part in chunk(line) {
            let length = units(part);
            if current.1 > 0 && current.1 + 1 + length > MAX_PASSAGE_LENGTH {
                pieces.push(std::mem::take(&mut current).0);
            }
            append(&mut current, "\n", part, length);
        }
    }
    if current.1 > 0 {
        pieces.push(current.0);
    }
}

/// `SourcePassages.Chunk`: slices of at most [`MAX_PASSAGE_LENGTH`] units, one unit shorter when
/// the bound falls inside a surrogate pair; the text is valid UTF-8, so a pair is one `char`.
fn chunk(line: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let (mut start, mut length) = (0, 0);
    for (index, c) in line.char_indices() {
        if length + c.len_utf16() > MAX_PASSAGE_LENGTH {
            parts.push(&line[start..index]);
            (start, length) = (index, 0);
        }
        length += c.len_utf16();
    }
    if start < line.len() {
        parts.push(&line[start..]);
    }
    parts
}
