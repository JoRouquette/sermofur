//! Reading `.sermofur/instance.json`, as `InstanceManager.ReadConfiguration` of the .NET 0.4.3
//! oracle does (`rust/tests/golden/instance-config.json` is the truth).
//!
//! `serde_json` alone does not reproduce `System.Text.Json` (lone surrogate escapes, depth), so
//! the reader of [`crate::stj`] follows the .NET rules.

use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

use uuid::Uuid;

use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::stj;

/// Current format of `instance.json`.
pub const SCHEMA_VERSION: i32 = 2;
/// Format of the 0.1 CLI: read only by migrate, doctor and root.
pub const LEGACY_SCHEMA_VERSION: i32 = 1;
/// Upper bound of `instance.json`: beyond it the instance is damaged.
pub const MAX_CONFIGURATION_BYTES: u64 = 64 * 1024;
/// Name of the folder that marks an instance.
pub const MARKER: &str = ".sermofur";
/// Name of the configuration file.
pub const CONFIGURATION_FILE: &str = "instance.json";

/// Content of `instance.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstanceConfiguration {
    pub schema_version: i32,
    pub instance_id: Uuid,
    pub created_at: DateTimeOffset,
}

/// `unsafe_path`, exit 4: `LocalPaths.LinkNotSupported`.
pub(crate) fn link_not_supported() -> SermofurError {
    SermofurError::new(
        "unsafe_path",
        "Links and junctions are not supported in this version.",
        4,
    )
}

/// `invalid_storage_or_path`, exit 3: `CommandRunner` maps JSON and IO failures to it.
pub(crate) fn invalid_storage() -> SermofurError {
    SermofurError::new(
        "invalid_storage_or_path",
        "Invalid path or data; run doctor.",
        3,
    )
}

fn invalid_instance(message: &str) -> SermofurError {
    SermofurError::new("invalid_instance", message, 3)
}

/// Refuses a link or a junction on `path` or on any ancestor (`LocalPaths.RejectLinks`).
/// An entry that does not exist is skipped.
pub fn reject_links(path: &Path) -> Result<()> {
    let full = normalize(&std::path::absolute(path).map_err(|_| invalid_storage())?);
    for entry in full.ancestors() {
        if let Ok(metadata) = fs::symlink_metadata(entry)
            && is_link(&metadata)
        {
            return Err(link_not_supported());
        }
    }
    Ok(())
}

/// `Path.GetFullPath` removes `.` and `..` lexically, without looking at the disk; `..` at the
/// root stays at the root. `std::path::absolute` keeps `..` on Unix, so it is removed here.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.components().next_back(), Some(Component::Normal(_))) {
                    out.pop();
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(windows)]
pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_symlink()
}

/// Reads `<root>/.sermofur/instance.json`. A format 1 instance is refused with
/// `migration_required` unless `allow_legacy`. `local_offset_minutes` serves a `createdAt`
/// without offset (see [`parse_json_text`]).
pub fn read_configuration(
    root: &Path,
    allow_legacy: bool,
    local_offset_minutes: impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let file = root.join(MARKER).join(CONFIGURATION_FILE);
    // Every ancestor of the file includes the ones of the marker folder.
    reject_links(&file)?;
    // `File.Exists` is false for a directory: the file is missing.
    if !fs::metadata(&file).is_ok_and(|m| m.is_file()) {
        return Err(invalid_instance("Instance configuration missing."));
    }
    // One bounded read: a file that grows after the check is still refused beyond the cap.
    let mut bytes = Vec::new();
    fs::File::open(&file)
        .and_then(|f| f.take(MAX_CONFIGURATION_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|_| invalid_storage())?;
    if bytes.len() as u64 > MAX_CONFIGURATION_BYTES {
        return Err(invalid_instance(
            "Damaged Sermofur instance: instance.json exceeds 64 KiB; restore it from a backup.",
        ));
    }
    interpret(&bytes, allow_legacy, local_offset_minutes)
}

/// Everything after the read: decoding, parsing, identity and version checks.
fn interpret(
    bytes: &[u8],
    allow_legacy: bool,
    local_offset_minutes: impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let configuration = parse_configuration(&decode(bytes), &local_offset_minutes)?;
    if configuration.instance_id.is_nil() {
        return Err(SermofurError::new(
            "unsupported_schema",
            "Invalid instance identity.",
            3,
        ));
    }
    match configuration.schema_version {
        LEGACY_SCHEMA_VERSION if allow_legacy => Ok(configuration),
        LEGACY_SCHEMA_VERSION => Err(SermofurError::new(
            "migration_required",
            "Instance in format 1; back it up if you wish, then run smf migrate.",
            3,
        )),
        SCHEMA_VERSION => Ok(configuration),
        _ => Err(SermofurError::new(
            "unsupported_schema",
            "Unsupported instance identity or version.",
            3,
        )),
    }
}

/// `File.ReadAllText`: the byte order mark picks UTF-8, UTF-16 or UTF-32 (the BOM is dropped);
/// without one the text is UTF-8. Invalid sequences become U+FFFD.
fn decode(bytes: &[u8]) -> String {
    // StreamReader flushes an incomplete last unit as one U+FFFD.
    let units16 = |data: &[u8], little: bool| -> String {
        let (chunks, rest) = data.as_chunks::<2>();
        let units: Vec<u16> = chunks
            .iter()
            .map(|c| {
                if little {
                    u16::from_le_bytes(*c)
                } else {
                    u16::from_be_bytes(*c)
                }
            })
            .collect();
        let mut text = String::from_utf16_lossy(&units);
        if !rest.is_empty() {
            text.push('\u{FFFD}');
        }
        text
    };
    let units32 = |data: &[u8], little: bool| -> String {
        let (chunks, rest) = data.as_chunks::<4>();
        let mut text: String = chunks
            .iter()
            .map(|c| {
                let v = if little {
                    u32::from_le_bytes(*c)
                } else {
                    u32::from_be_bytes(*c)
                };
                char::from_u32(v).unwrap_or('\u{FFFD}')
            })
            .collect();
        if !rest.is_empty() {
            text.push('\u{FFFD}');
        }
        text
    };
    match bytes {
        [0xFE, 0xFF, rest @ ..] => units16(rest, false),
        [0xFF, 0xFE, 0, 0, rest @ ..] => units32(rest, true),
        [0xFF, 0xFE, rest @ ..] => units16(rest, true),
        [0xEF, 0xBB, 0xBF, rest @ ..] => String::from_utf8_lossy(rest).into_owned(),
        [0, 0, 0xFE, 0xFF, rest @ ..] => units32(rest, false),
        _ => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// Deserializes `InstanceConfiguration` like `JsonSerializer.Deserialize` with `RecordJson`
/// options, on the reader of [`crate::stj`]: missing properties take the defaults (0, empty
/// GUID, `0001-01-01T00:00:00+00:00`); `schemaVersion` is a JSON integer in `i32` without `.`
/// or exponent; `instanceId` is a GUID in format D; `createdAt` goes through
/// [`parse_json_text`]; `null` is refused for all three; a top-level `null` is
/// `invalid_record`, any other non-object is a JSON error.
fn parse_configuration(
    text: &str,
    local_offset_minutes: &impl Fn(i64) -> i16,
) -> Result<InstanceConfiguration> {
    let node = stj::parse(text)?;
    let mut configuration = InstanceConfiguration {
        schema_version: 0,
        instance_id: Uuid::nil(),
        created_at: DateTimeOffset::new(0, 0).ok_or_else(invalid_storage)?,
    };
    for (name, value) in stj::document(&node)? {
        match name {
            "schemaVersion" => configuration.schema_version = stj::integer(value)?,
            "instanceId" => configuration.instance_id = stj::guid(value)?,
            "createdAt" => configuration.created_at = stj::date(value, local_offset_minutes)?,
            _ => {}
        }
    }
    Ok(configuration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    const VALID: &str = "{\n  \"schemaVersion\": 2,\n  \"instanceId\": \"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\",\n  \"createdAt\": \"2026-11-06T11:00:00.1234567+00:00\"\n}";

    /// The bytes of `text` behind the byte order mark number `bom` (0 to 5) and in its encoding.
    fn encode(text: &str, bom: usize) -> Vec<u8> {
        let units16 = |little: bool| -> Vec<u8> {
            text.encode_utf16()
                .flat_map(|u| {
                    if little {
                        u.to_le_bytes()
                    } else {
                        u.to_be_bytes()
                    }
                })
                .collect()
        };
        let units32 = |little: bool| -> Vec<u8> {
            text.chars()
                .flat_map(|c| {
                    if little {
                        u32::from(c).to_le_bytes()
                    } else {
                        u32::from(c).to_be_bytes()
                    }
                })
                .collect()
        };
        let (prefix, body): (&[u8], Vec<u8>) = match bom {
            1 => (&[0xEF, 0xBB, 0xBF], text.as_bytes().to_vec()),
            2 => (&[0xFE, 0xFF], units16(false)),
            3 => (&[0xFF, 0xFE], units16(true)),
            4 => (&[0xFF, 0xFE, 0, 0], units32(true)),
            5 => (&[0, 0, 0xFE, 0xFF], units32(false)),
            _ => (&[], text.as_bytes().to_vec()),
        };
        [prefix, &body].concat()
    }

    /// Whatever the input, no panic; a success has a real identity and a readable version.
    fn check(bytes: &[u8]) {
        for legacy in [false, true] {
            if let Ok(c) = interpret(bytes, legacy, |_| 60) {
                assert!(!c.instance_id.is_nil());
                assert!(c.schema_version == 1 || c.schema_version == 2);
            }
        }
    }

    const FRAGMENTS: &[&str] = &[
        "{",
        "}",
        "[",
        "]",
        ",",
        ":",
        " ",
        "\n",
        "\"a\"",
        "\\u00",
        "\\ud800",
        "\\udc00",
        "\\",
        "\\\"",
        "\"",
        "1",
        "-",
        "0",
        "1e5",
        "-0",
        ".5",
        "null",
        "true",
        "\"schemaVersion\"",
        "\"instanceId\"",
        "\"createdAt\"",
        "\"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\"",
        "2",
        "1",
        "\"2026-11-06T11:00:00Z\"",
        "\u{feff}",
        "\u{0}",
    ];

    proptest! {
        #[test]
        fn mutated_valid_documents_never_panic(
            ops in proptest::collection::vec((0u8..3, any::<usize>(), any::<u8>()), 0..6),
            bom in 0usize..6,
        ) {
            let mut bytes = VALID.as_bytes().to_vec();
            for (kind, index, byte) in ops {
                let at = index % (bytes.len() + 1);
                match kind {
                    0 if at < bytes.len() => bytes[at] = byte,
                    1 => bytes.insert(at, byte),
                    _ => bytes.truncate(at),
                }
            }
            check(&encode(&String::from_utf8_lossy(&bytes), bom));
        }

        #[test]
        fn truncated_encodings_never_panic(bom in 0usize..6, cut in 0usize..400) {
            let mut bytes = encode(VALID, bom);
            bytes.truncate(cut.min(bytes.len()));
            check(&bytes);
        }

        #[test]
        fn open_brackets_never_panic(open in 0usize..200, close in 0usize..200, bom in 0usize..6) {
            let text = format!(
                "{{\"schemaVersion\":2,\"instanceId\":\"6f1c2b3a-4d5e-4f60-8a7b-9c0d1e2f3a4b\",\"x\":{}{}}}",
                "[".repeat(open),
                "]".repeat(close)
            );
            check(&encode(&text, bom));
        }

        #[test]
        fn assembled_fragments_never_panic(
            parts in proptest::collection::vec(0..FRAGMENTS.len(), 0..14),
            wrap in any::<bool>(),
            bom in 0usize..6,
        ) {
            let body: String = parts.into_iter().map(|i| FRAGMENTS[i]).collect();
            let text = if wrap { format!("{{{body}}}") } else { body };
            check(&encode(&text, bom));
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..=512)) {
            check(&bytes);
        }
    }

    #[test]
    fn the_valid_document_is_read_in_every_encoding() {
        for bom in 0..6 {
            let c = interpret(&encode(VALID, bom), false, |_| 60).expect("valid");
            assert_eq!(c.schema_version, 2, "bom {bom}");
        }
    }

    #[test]
    fn dot_segments_are_removed_lexically() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalize(Path::new("/a/../..")), PathBuf::from("/"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_cancelled_by_dot_dot_is_not_seen() {
        let base = std::fs::canonicalize(std::env::temp_dir())
            .expect("temp")
            .join(format!("sermofur-dotdot-{}", Uuid::new_v4()));
        std::fs::create_dir_all(base.join("target")).expect("dirs");
        std::os::unix::fs::symlink(base.join("target"), base.join("link")).expect("link");
        // GetFullPath turns `link/..` into the base itself, which has no link.
        let through = reject_links(&base.join("link").join(".."));
        let direct = reject_links(&base.join("link"));
        let _ = std::fs::remove_dir_all(&base);
        assert!(through.is_ok());
        assert_eq!(direct.expect_err("link").code, "unsafe_path");
    }
}
