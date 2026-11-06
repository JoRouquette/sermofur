//! The Markdown projection of a record, `.sermofur/records/<id>.md`, as `MarkdownProjection` of
//! the .NET oracle (`rust/tests/golden/store*.json`, key `projections`). The store is the
//! authority: a projection is rebuilt from it, never read back as data.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::atomic::{WriteError, WriteOptions, write as atomic_write};
use crate::error::{Result, SermofurError};
use crate::init::RECORDS_DIRECTORY;
use crate::instance::{MARKER, decode, reject_links};
use crate::records::{DotNetEnum, KnowledgeStatus, MemoryRecord, Provenance, RecordKind, write};

/// `<root>/.sermofur/records/<id>.md` (`PathFor`).
pub fn path_for(root: &Path, record: &MemoryRecord) -> PathBuf {
    root.join(MARKER)
        .join(RECORDS_DIRECTORY)
        .join(format!("{}.md", record.id.hyphenated()))
}

/// The text of the projection (`Render`): front matter in JSON written by `RecordJson` (line
/// breaks of the platform), then the content, with `\n` breaks around it.
pub fn render(record: &MemoryRecord) -> Result<String> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Metadata<'a> {
        schema_version: i32,
        #[serde(serialize_with = "guid")]
        id: uuid::Uuid,
        scope_id: &'a Option<String>,
        kind: RecordKind,
        status: KnowledgeStatus,
        revision: i32,
        provenance: &'a Option<Provenance>,
    }
    fn guid<S: serde::Serializer>(
        value: &uuid::Uuid,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&value.hyphenated().to_string())
    }
    let metadata = write(&Metadata {
        schema_version: 1,
        id: record.id,
        scope_id: &record.scope_id,
        kind: record.kind,
        status: record.status,
        revision: record.revision,
        provenance: &record.provenance,
    })?;
    let kind = RecordKind::NAMES
        .iter()
        .find(|(_, _, member)| *member == record.kind)
        .map_or("", |(name, _, _)| name);
    Ok(format!(
        "---\n{metadata}\n---\n\n# {kind} {}\n\n```json\n{}\n```\n",
        record.id.hyphenated(),
        record.content_json.as_deref().unwrap_or("")
    ))
}

/// Whether the file holds exactly the projection of `record` (`IsCurrent`). A link on the path
/// is refused (`unsafe_path`); an unreadable file is an error of reading.
fn is_current(root: &Path, record: &MemoryRecord) -> Result<std::io::Result<bool>> {
    let target = path_for(root, record);
    reject_links(&target)?;
    if !fs::metadata(&target).is_ok_and(|m| m.is_file()) {
        return Ok(Ok(false));
    }
    let expected = render(record)?;
    Ok(fs::read(&target).map(|bytes| decode(&bytes) == expected))
}

/// `IsCurrentSafe`: a failure to read is "not current"; a link is still refused.
pub fn is_current_safe(root: &Path, record: &MemoryRecord) -> Result<bool> {
    Ok(is_current(root, record)?.unwrap_or(false))
}

/// Writes the projection (`Write`): no link on the path, an atomic replacement without a flush
/// to the disk; a failure to write is forgiven when the file already holds the same text (a
/// concurrent writer published it).
pub fn write_projection(root: &Path, record: &MemoryRecord) -> Result<()> {
    let target = path_for(root, record);
    reject_links(&target)?;
    let options = WriteOptions {
        durable: false,
        ..WriteOptions::default()
    };
    match atomic_write(&target, render(record)?.as_bytes(), &options) {
        Ok(_) => Ok(()),
        Err(WriteError::Io(error)) => {
            // An error in the filter of .NET counts as "no match": the first error stands.
            if matches!(is_current(root, record), Ok(Ok(true))) {
                Ok(())
            } else {
                Err(SermofurError::new(
                    "invalid_storage_or_path",
                    format!("Projection not written: {error}"),
                    3,
                ))
            }
        }
        Err(WriteError::Refused(refusal)) => Err(refusal),
    }
}

/// `SqliteStore.Project`: any failure leaves the record saved and reports
/// `projection_pending`, exit 3.
pub fn project(root: &Path, record: &MemoryRecord) -> Result<()> {
    write_projection(root, record).map_err(|_| {
        SermofurError::new(
            "projection_pending",
            format!(
                "Object {} saved; projection to rebuild with export.",
                record.id.hyphenated()
            ),
            3,
        )
    })
}
