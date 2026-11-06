//! `smf init`: creation of an instance, as `InstanceManager.Initialize` of the .NET oracle.
//!
//! The instance is built in a staging folder `.sermofur-init-<guid>` and moved into place in one
//! rename, so that a concurrent `init` either wins or finds the winner's complete instance.

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use uuid::Uuid;

use crate::discovery::{discover, validate_directory};
use crate::error::{Result, SermofurError};
use crate::format::DateTimeOffset;
use crate::instance::{
    CONFIGURATION_FILE, InstanceConfiguration, MARKER, SCHEMA_VERSION, invalid_storage,
    read_configuration, reject_links,
};
use crate::ownership::SystemOwnership;
use crate::{json, schema};

/// Name of the database file inside `.sermofur`.
pub const DATABASE_FILE: &str = "memory.db";
/// Name of the folder of the Markdown projection inside `.sermofur`.
pub const RECORDS_DIRECTORY: &str = "records";

/// `<root>/.sermofur/memory.db` (`InstanceManager.DatabasePath`).
pub fn database_path(root: &Path) -> PathBuf {
    root.join(MARKER).join(DATABASE_FILE)
}

/// Opens the database of an instance and checks its version and identity, as the constructor
/// of `SqliteStore` followed by `ValidateSchema`: no link on its path, the file must exist, and
/// it is never created here.
pub fn open_store(root: &Path, instance_id: Uuid, read_only: bool) -> Result<Connection> {
    let database = database_path(root);
    reject_links(&database)?;
    if !fs::metadata(&database).is_ok_and(|metadata| metadata.is_file()) {
        return Err(SermofurError::new(
            "missing_database",
            "Memory database missing.",
            3,
        ));
    }
    let mode = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    };
    let connection = schema::open(&database, mode)?;
    schema::validate(&connection, instance_id)?;
    Ok(connection)
}

/// The answer of `smf init`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialized {
    pub root: PathBuf,
    pub configuration: InstanceConfiguration,
    pub created: bool,
}

/// `instance.json` as `RecordJson.Write(InstanceConfiguration)` writes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigurationJson {
    schema_version: i32,
    instance_id: String,
    created_at: String,
}

/// The JSON text of a configuration, as `RecordJson` writes it.
pub fn configuration_json(configuration: &InstanceConfiguration) -> Result<String> {
    json::to_string(&ConfigurationJson {
        schema_version: configuration.schema_version,
        instance_id: configuration.instance_id.hyphenated().to_string(),
        created_at: configuration.created_at.to_json_text(),
    })
    .map_err(|_| invalid_storage())
}

/// The offset of local time, which reads a `createdAt` written without offset.
// ponytail: UTC until lot 7.2 ports the local offset (DateTimeOffset.Now); only a hand-written
// instance.json without offset is read differently.
fn local_offset(_utc_ticks: i64) -> i16 {
    0
}

/// Reads the configuration and checks the database of an existing instance.
fn existing(root: &Path) -> Result<Initialized> {
    let configuration = read_configuration(root, false, local_offset)?;
    open_store(root, configuration.instance_id, false)?;
    Ok(Initialized {
        root: root.to_path_buf(),
        configuration,
        created: false,
    })
}

/// Initializes the instance of `path`, or returns the one that already holds it.
pub fn initialize(path: &Path) -> Result<Initialized> {
    let root = validate_directory(path)?;
    match discover(&root, &SystemOwnership) {
        Ok(found) => {
            // `discover` returns `root` itself or one of its ancestors, both taken from the same
            // path, so the case-insensitive comparison of .NET reduces to equality.
            if found != root {
                return Err(SermofurError::new(
                    "nested_instance",
                    "An ancestor instance exists; nested initialization refused.",
                    4,
                ));
            }
            return existing(&root);
        }
        Err(error) if error.code == "no_instance" => {}
        Err(error) => return Err(error),
    }
    // no_instance guarantees that neither root nor any ancestor holds a .sermofur entry.
    let staging = root.join(format!(".sermofur-init-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&staging).map_err(|_| invalid_storage())?;
    let result = build(&root, &staging);
    // The `finally` of .NET: a failed deletion replaces the result, success included.
    if fs::metadata(&staging).is_ok_and(|metadata| metadata.is_dir()) {
        fs::remove_dir_all(&staging).map_err(|_| invalid_storage())?;
    }
    result
}

fn build(root: &Path, staging: &Path) -> Result<Initialized> {
    let configuration = InstanceConfiguration {
        schema_version: SCHEMA_VERSION,
        instance_id: Uuid::new_v4(),
        created_at: DateTimeOffset::now_utc(),
    };
    fs::write(
        staging.join(CONFIGURATION_FILE),
        configuration_json(&configuration)?,
    )
    .map_err(|_| invalid_storage())?;
    schema::create_database(&staging.join(DATABASE_FILE), configuration.instance_id)?;
    fs::create_dir_all(staging.join(RECORDS_DIRECTORY)).map_err(|_| invalid_storage())?;
    let marker = root.join(MARKER);
    // `Directory.Move` refuses an existing destination on every system; `rename` would replace
    // an empty folder on Unix, so the check comes first, as in .NET.
    let moved = if fs::symlink_metadata(&marker).is_ok() {
        Err(())
    } else {
        fs::rename(staging, &marker).map_err(|_| ())
    };
    if moved.is_err() {
        if fs::metadata(&marker).is_ok_and(|metadata| metadata.is_dir()) {
            // Another init won the race: its instance is the answer.
            return existing(root);
        }
        return Err(invalid_storage());
    }
    Ok(Initialized {
        root: root.to_path_buf(),
        configuration,
        created: true,
    })
}
