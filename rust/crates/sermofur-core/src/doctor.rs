//! `smf doctor`, as `InstanceDoctor` of the .NET oracle (`rust/tests/golden/cli-6-2` is the
//! truth): eighteen checks in a fixed order, read-only, never a repair. A failure to read the
//! instance stops the inspection with one `instance_storage` check; the capabilities `daemon`,
//! `laya`, `model` and `mcp` always close the report.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::types::ValueRef;
use serde::Serialize;

use crate::daemon_paths::{DaemonPaths, system_folder};
use crate::discovery::{describe_invalid_marker, validate_directory};
use crate::error::{Result, SermofurError};
use crate::instance::{invalid_storage, read_configuration};
use crate::local_paths::{canonical_case, normalize_mapping, relativize_mapping, same_text};
use crate::ownership::FileOwnership;
use crate::projection::is_current;
use crate::records::{
    ClaimContent, EvidenceContent, MemoryRecord, RecordKind, RetexContent, SourceContent, read,
};
use crate::schema::storage;
use crate::scope_policy::validate_tree;
use crate::scopes::mapping_conflicts;
use crate::search::{is_indexed, kind_of};
use crate::source_service::narrower_scope;
use crate::store::{Store, StoredScope};

/// One check of the report (`DiagnosticCheck`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: String,
    pub detail: String,
}

impl DiagnosticCheck {
    pub fn new(name: &str, status: &str, detail: impl Into<String>) -> Self {
        DiagnosticCheck {
            name: name.to_owned(),
            status: status.to_owned(),
            detail: detail.into(),
        }
    }
}

/// The report (`DoctorReport`): `overall` is `unhealthy` or `healthy_with_warnings`, never
/// `healthy`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DoctorReport {
    pub root: String,
    pub overall: String,
    pub checks: Vec<DiagnosticCheck>,
}

/// The state of the daemon, supplied by the CLI (`DaemonProbe.Check`).
pub type DaemonProbe<'a> = &'a dyn Fn() -> Result<DiagnosticCheck>;
/// The declaration of the MCP bridge at the root, supplied by the CLI.
pub type McpProbe<'a> = &'a dyn Fn(&Path) -> Result<DiagnosticCheck>;

const NOT_DELIVERED: &str = "Capability not delivered in this version.";

/// The code a failure is reported under (`ErrorCode`): its own code for a `SermofurException`,
/// `invalid_storage` for a failure of the runtime, which the port carries as the codes that
/// `CommandRunner` gives to such exceptions.
fn error_code(error: &SermofurError) -> &str {
    match error.code.as_str() {
        "invalid_storage_or_path" | "storage_error" | "storage_busy" => "invalid_storage",
        code => code,
    }
}

/// `Inspect`: the report of the instance at `root`. Only what `InstanceDoctor` lets escape
/// (a link found on the marker, a failure of a probe) is an error of the command.
pub fn inspect(
    root: &Path,
    owners: &dyn FileOwnership,
    daemon: Option<DaemonProbe>,
    mcp: Option<McpProbe>,
) -> Result<DoctorReport> {
    let mut checks = Vec::new();
    if let Some(case) = describe_invalid_marker(root, owners)? {
        checks.push(DiagnosticCheck::new(
            "instance",
            "error",
            format!("invalid_instance: {case}"),
        ));
        return report(root, checks, daemon, mcp);
    }
    // `createdAt` without offset: read in the local zone (decision A5).
    let configuration = match read_configuration(root, false, crate::format::local_offset_of_clock)
    {
        Ok(configuration) => configuration,
        Err(error) => {
            checks.push(DiagnosticCheck::new(
                "instance",
                "error",
                error_code(&error),
            ));
            return report(root, checks, daemon, mcp);
        }
    };
    checks.push(DiagnosticCheck::new(
        "instance",
        "ok",
        "Compatible configuration and version.",
    ));
    let inspected = Store::open(root, configuration.instance_id, true)
        .and_then(|store| inspect_store(root, &store, &mut checks));
    if let Err(error) = inspected {
        checks.push(DiagnosticCheck::new(
            "instance_storage",
            "error",
            error_code(&error),
        ));
    }
    report(root, checks, daemon, mcp)
}

fn report(
    root: &Path,
    mut checks: Vec<DiagnosticCheck>,
    daemon: Option<DaemonProbe>,
    mcp: Option<McpProbe>,
) -> Result<DoctorReport> {
    checks.push(match daemon {
        Some(probe) => probe()?,
        None => DiagnosticCheck::new("daemon", "warning", NOT_DELIVERED),
    });
    checks.push(DiagnosticCheck::new("laya", "warning", NOT_DELIVERED));
    checks.push(DiagnosticCheck::new("model", "warning", NOT_DELIVERED));
    checks.push(match mcp {
        Some(probe) => probe(root)?,
        None => DiagnosticCheck::new("mcp", "warning", NOT_DELIVERED),
    });
    let overall = if checks.iter().any(|check| check.status == "error") {
        "unhealthy"
    } else {
        "healthy_with_warnings"
    };
    Ok(DoctorReport {
        root: root.to_string_lossy().into_owned(),
        overall: overall.to_owned(),
        checks,
    })
}

fn inspect_store(root: &Path, store: &Store, checks: &mut Vec<DiagnosticCheck>) -> Result<()> {
    let connection = &store.connection;
    // `Store::open` validated the schema and the identity, as `ValidateSchema`.
    checks.push(DiagnosticCheck::new(
        "schema",
        "ok",
        "Consistent SQLite schema and identity.",
    ));
    let integrity = scalar(connection, "PRAGMA integrity_check")?;
    checks.push(DiagnosticCheck::new(
        "storage",
        if integrity == "ok" { "ok" } else { "error" },
        "SQLite integrity check.",
    ));
    let references = query_strings(connection, "PRAGMA foreign_key_check")?.len();
    checks.push(DiagnosticCheck::new(
        "orphan_records",
        status(references == 0, "error"),
        format!("{references} inconsistent references."),
    ));
    let scopes = inspect_scopes(root, store, checks)?;
    inspect_memory(root, store, &scopes, checks)?;
    inspect_search_index(connection, &scopes, checks)?;
    checks.push(DiagnosticCheck::new(
        "sqlite",
        "ok",
        format!(
            "Native version {}; journal {}.",
            scalar(connection, "SELECT sqlite_version()")?,
            scalar(connection, "PRAGMA journal_mode")?
        ),
    ));
    Ok(())
}

fn status(ok: bool, otherwise: &'static str) -> &'static str {
    if ok { "ok" } else { otherwise }
}

/// `scopes`, `scope_mappings` and `scope_overlap`; an invalid tree stops the inspection.
fn inspect_scopes(
    root: &Path,
    store: &Store,
    checks: &mut Vec<DiagnosticCheck>,
) -> Result<Vec<StoredScope>> {
    let scopes = store.read_scopes()?;
    validate_tree(&scopes)?;
    checks.push(DiagnosticCheck::new("scopes", "ok", "Valid scope tree."));
    let missing = count_missing_mappings(root, &scopes)?;
    let mut misnamed = 0;
    for scope in &scopes {
        if let Some(mapping) = scope.relative_path.as_deref()
            && is_misnamed(root, mapping)?
        {
            misnamed += 1;
        }
    }
    checks.push(DiagnosticCheck::new(
        "scope_mappings",
        status(missing + misnamed == 0, "warning"),
        if misnamed == 0 {
            format!("{missing} missing mappings.")
        } else {
            format!(
                "{missing} missing mappings, {misnamed} named otherwise on disk (such a scope may not hold the sources of its folder)."
            )
        },
    ));
    let conflicts = mapping_conflicts(root, &scopes)?;
    checks.push(DiagnosticCheck::new(
        "scope_overlap",
        status(conflicts == 0, "error"),
        format!("{conflicts} conflicting mapping pairs."),
    ));
    Ok(scopes)
}

/// A mapped folder that disappeared is counted; a mapping outside the instance or through a
/// link stops the inspection (`CountMissingMappings`).
fn count_missing_mappings(root: &Path, scopes: &[StoredScope]) -> Result<usize> {
    let mut missing = 0;
    for mapping in scopes.iter().filter_map(|s| s.relative_path.as_deref()) {
        let mapped = normalize_mapping(root, mapping)?;
        if !is_directory(&mapped) {
            missing += 1;
            continue;
        }
        validate_directory(&mapped)?;
    }
    Ok(missing)
}

/// `Directory.Exists`: a folder, links followed.
fn is_directory(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir())
}

/// A mapping stored under another name than its folder on disk, beyond what the comparison
/// absorbs (`IsMisnamed`). A folder that cannot be listed is not misnamed; any other refusal is.
fn is_misnamed(root: &Path, mapping: &str) -> Result<bool> {
    let check = || -> Result<bool> {
        let full = normalize_mapping(root, mapping)?;
        if !is_directory(&full) {
            // Counted as missing, not as misnamed.
            return Ok(false);
        }
        let relative = relativize_mapping(root, &full)?;
        Ok(!same_text(&canonical_case(root, &relative)?, &relative))
    };
    match check() {
        Ok(misnamed) => Ok(misnamed),
        Err(error) if error.code == "invalid_path" => Ok(false),
        // ponytail: `invalid_storage_or_path` stands for IO failures (true in .NET) and argument
        // failures (thrown in .NET) alike; reading a folder that exists only meets the first.
        Err(_) => Ok(true),
    }
}

/// `memory_consistency`, `projections` and `record_columns`.
fn inspect_memory(
    root: &Path,
    store: &Store,
    scopes: &[StoredScope],
    checks: &mut Vec<DiagnosticCheck>,
) -> Result<()> {
    let visible: HashSet<String> = scopes.iter().map(|scope| scope.id.clone()).collect();
    let records = store.read_records(&visible, None)?;
    let mut invalid = 0;
    for record in &records {
        if !record
            .scope_id
            .as_ref()
            .is_some_and(|s| visible.contains(s))
            || record.id.is_nil()
            || record.revision < 1
        {
            invalid += 1;
        }
        let json = || record.content_json.as_deref().ok_or_else(invalid_storage);
        match record.kind {
            RecordKind::Claim => {
                read::<ClaimContent>(json()?, &crate::format::local_offset_of_clock)?;
            }
            RecordKind::Retex => {
                read::<RetexContent>(json()?, &crate::format::local_offset_of_clock)?;
            }
            RecordKind::Source => {
                read::<SourceContent>(json()?, &crate::format::local_offset_of_clock)?;
            }
            RecordKind::Evidence => {
                let evidence: EvidenceContent =
                    read(json()?, &crate::format::local_offset_of_clock)?;
                // `SingleOrDefault`: two records with the same identifier are an
                // `InvalidOperationException`.
                let mut cited = records.iter().filter(|r| r.id == evidence.claim_id);
                let claim = cited.next();
                if cited.next().is_some() {
                    return Err(invalid_storage());
                }
                if !claim.is_some_and(|claim| {
                    claim.kind == RecordKind::Claim && claim.scope_id == record.scope_id
                }) {
                    invalid += 1;
                }
            }
        }
        let history = store.read_history(record.id, &visible)?;
        let current = match history.last() {
            None => false,
            Some(last) => {
                let state = last.new_state.as_deref().ok_or_else(invalid_storage)?;
                same_record(
                    &read::<MemoryRecord>(state, &crate::format::local_offset_of_clock)?,
                    record,
                )
            }
        };
        if !current {
            invalid += 1;
        }
    }
    checks.push(DiagnosticCheck::new(
        "memory_consistency",
        status(invalid == 0, "error"),
        format!("{invalid} inconsistencies detected."),
    ));
    let mut stale = 0;
    for record in &records {
        // `IsCurrent`, not `IsCurrentSafe`: a failure to read stops the inspection.
        if !is_current(root, record)?.map_err(|_| invalid_storage())? {
            stale += 1;
        }
    }
    checks.push(DiagnosticCheck::new(
        "projections",
        status(stale == 0, "warning"),
        format!("{stale} missing or diverging projections; run export to rebuild."),
    ));
    let rows = query_strings(
        &store.connection,
        "SELECT id FROM records WHERE
    id != json_extract(payload,'$.id') OR scope_id != json_extract(payload,'$.scopeId') OR
    revision != json_extract(payload,'$.revision') OR lower(kind) != json_extract(payload,'$.kind')",
    )?
    .len();
    checks.push(DiagnosticCheck::new(
        "record_columns",
        status(rows == 0, "error"),
        format!("{rows} inconsistent metadata rows."),
    ));
    Ok(())
}

/// Record equality of C#: every member, the timestamp compared as an instant.
fn same_record(a: &MemoryRecord, b: &MemoryRecord) -> bool {
    let provenance = match (&a.provenance, &b.provenance) {
        (Some(x), Some(y)) => {
            x.origin == y.origin
                && x.actor == y.actor
                && x.timestamp.utc_ticks() == y.timestamp.utc_ticks()
        }
        (None, None) => true,
        _ => false,
    };
    a.id == b.id
        && a.scope_id == b.scope_id
        && a.kind == b.kind
        && a.status == b.status
        && a.revision == b.revision
        && a.content_json == b.content_json
        && provenance
}

/// `fts5`, `search_index` (the index holds exactly the indexable objects, by identifier, scope
/// and kind) and `source_scopes` (a source belongs to the narrowest scope holding its file).
fn inspect_search_index(
    connection: &Connection,
    scopes: &[StoredScope],
    checks: &mut Vec<DiagnosticCheck>,
) -> Result<()> {
    let fts5 = scalar(
        connection,
        "SELECT sqlite_compileoption_used('ENABLE_FTS5')",
    )? == "1";
    checks.push(DiagnosticCheck::new(
        "fts5",
        status(fts5, "error"),
        "SQLite full-text search engine.",
    ));
    let records: Vec<MemoryRecord> = query_strings(
        connection,
        "SELECT payload FROM records WHERE kind IN ('Claim','Retex','Source')",
    )?
    .iter()
    .map(|payload| read(payload, &crate::format::local_offset_of_clock))
    .collect::<Result<_>>()?;
    let indexed: HashSet<String> = query_strings(
        connection,
        "SELECT DISTINCT object_id, scope_id, kind FROM search",
    )?
    .into_iter()
    .collect();
    let mut expected = HashSet::new();
    for record in &records {
        if is_indexed(record)? {
            expected.insert(format!(
                "{}|{}|{}",
                record.id.hyphenated(),
                record.scope_id.as_deref().unwrap_or(""),
                kind_of(record.kind)?
            ));
        }
    }
    let missing = expected.iter().filter(|e| !indexed.contains(*e)).count();
    let extra = indexed.iter().filter(|e| !expected.contains(*e)).count();
    checks.push(DiagnosticCheck::new(
        "search_index",
        status(missing + extra == 0, "error"),
        format!(
            "{missing} objects missing from the index or misplaced, {extra} entries without object; run index rebuild."
        ),
    ));
    let mut misplaced = 0;
    for source in records.iter().filter(|r| r.kind == RecordKind::Source) {
        let json = source.content_json.as_deref().ok_or_else(invalid_storage)?;
        let content: SourceContent = read(json, &crate::format::local_offset_of_clock)?;
        // A source without path or scope: a `NullReferenceException` in .NET (PARITY.md).
        let path = content.relative_path.ok_or_else(invalid_storage)?;
        let owner = source.scope_id.as_deref().ok_or_else(invalid_storage)?;
        if narrower_scope(&path, owner, scopes)?.is_some() {
            misplaced += 1;
        }
    }
    checks.push(DiagnosticCheck::new(
        "source_scopes",
        status(misplaced == 0, "error"),
        format!("{misplaced} sources attached to a broader scope than their file."),
    ));
    Ok(())
}

/// `SqliteStore.Scalar` read as `Convert.ToString`: the first column of the first row, the empty
/// text without a row.
fn scalar(connection: &Connection, sql: &str) -> Result<String> {
    Ok(rows(connection, sql)?
        .into_iter()
        .next()
        .and_then(|row| row.into_iter().next())
        .unwrap_or_default())
}

/// `SqliteStore.QueryStrings`: each row as its values joined by `|`.
fn query_strings(connection: &Connection, sql: &str) -> Result<Vec<String>> {
    Ok(rows(connection, sql)?
        .into_iter()
        .map(|row| row.join("|"))
        .collect())
}

/// The rows of a query, each value as `object.ToString()` gives it, a NULL as the empty text.
fn rows(connection: &Connection, sql: &str) -> Result<Vec<Vec<String>>> {
    let mut statement = connection.prepare(sql).map_err(|e| storage(&e))?;
    let columns = statement.column_count();
    let mut rows = statement.query([]).map_err(|e| storage(&e))?;
    let mut out = Vec::new();
    while let Some(row) = rows.next().map_err(|e| storage(&e))? {
        let mut values = Vec::with_capacity(columns);
        for index in 0..columns {
            values.push(match row.get_ref(index).map_err(|e| storage(&e))? {
                ValueRef::Null => String::new(),
                ValueRef::Integer(value) => value.to_string(),
                // ponytail: `Double.ToString()` and Rust agree on short values only; a real
                // number reaches these queries only through a database altered by hand.
                ValueRef::Real(value) => value.to_string(),
                ValueRef::Text(text) => String::from_utf8_lossy(text).into_owned(),
                ValueRef::Blob(_) => "System.Byte[]".to_owned(),
            });
        }
        out.push(values);
    }
    Ok(out)
}

/// `DaemonProbe.Check` up to the connection: the endpoint of the daemon of the current user is
/// absent, foreign, or present. Talking to a present daemon comes with its client (lot 7.5);
/// until then a present endpoint is reported as the oracle reports a daemon it does not probe.
pub fn daemon_check(
    variable: &dyn Fn(&str) -> Option<String>,
    owners: &dyn FileOwnership,
) -> Result<DiagnosticCheck> {
    // The endpoint only: an unusable profile folder does not stop the report, as in .NET.
    let endpoint = match DaemonPaths::locate(variable, &system_folder) {
        Ok((paths, _)) => paths.endpoint,
        Err(error) if error.code != "invalid_storage_or_path" => {
            return Ok(DiagnosticCheck::new(
                "daemon",
                "error",
                format!("{}: {}", error.code, error.message),
            ));
        }
        Err(error) => return Err(error),
    };
    Ok(match endpoint_state(&endpoint, owners)? {
        EndpointState::Foreign => DiagnosticCheck::new(
            "daemon",
            "error",
            "foreign_endpoint: the daemon endpoint belongs to another account; commands run directly.",
        ),
        EndpointState::Absent => {
            DiagnosticCheck::new("daemon", "warning", "absent: commands run directly.")
        }
        // ponytail: no client before lot 7.5.
        EndpointState::Present => DiagnosticCheck::new("daemon", "warning", NOT_DELIVERED),
    })
}

#[derive(Debug, PartialEq, Eq)]
enum EndpointState {
    Absent,
    Present,
    Foreign,
}

/// `IpcEndpoint.Inspect`: on Windows a listing of the pipe file system (opening the pipe would
/// take a server instance); elsewhere the private folder of the socket, then the socket, both
/// of the current user.
fn endpoint_state(endpoint: &Path, owners: &dyn FileOwnership) -> Result<EndpointState> {
    if cfg!(windows) {
        let name = endpoint.to_string_lossy();
        let listing = std::fs::read_dir(r"\\.\pipe\").map_err(|_| invalid_storage())?;
        for entry in listing {
            let entry = entry.map_err(|_| invalid_storage())?;
            if entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(&name)
            {
                return Ok(EndpointState::Present);
            }
        }
        return Ok(EndpointState::Absent);
    }
    let folder = endpoint.parent().unwrap_or(endpoint);
    let Some(status) = owners.inspect(folder).map_err(|_| invalid_storage())? else {
        return Ok(EndpointState::Absent);
    };
    if !status.is_directory || !status.is_owned_by_current_user || !is_private(folder)? {
        return Ok(EndpointState::Foreign);
    }
    Ok(
        match owners.inspect(endpoint).map_err(|_| invalid_storage())? {
            None => EndpointState::Absent,
            Some(socket)
                if socket.is_owned_by_current_user
                    && !socket.is_directory
                    && !socket.is_regular_file =>
            {
                EndpointState::Present
            }
            Some(_) => EndpointState::Foreign,
        },
    )
}

/// No permission for the group nor the others (`IsPrivate`).
#[cfg(unix)]
fn is_private(folder: &Path) -> Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(folder)
        .map_err(|_| invalid_storage())?
        .permissions()
        .mode();
    Ok(mode & 0o077 == 0)
}

#[cfg(not(unix))]
fn is_private(_folder: &Path) -> Result<bool> {
    Ok(true)
}

/// The `mcp` check of the CLI: no declaration file at all is "not declared". Reading a file
/// that exists (`.mcp.json`, the Codex tables) comes with the editors of lots 8.1 and 8.2;
/// until then such a file is reported as the oracle reports a bridge it does not probe.
pub fn mcp_check(
    root: &Path,
    variable: &dyn Fn(&str) -> Option<String>,
) -> Result<DiagnosticCheck> {
    // ponytail: the profile folder comes from USERPROFILE or HOME; .NET asks the system
    // (`GetFolderPath(UserProfile)`, the passwd entry on Unix). Without the variable there is
    // no user configuration to look at, never a path relative to the working directory. Lot
    // 8.3 ports the exact lookup with `CodexConfigFile.UserPath`.
    let codex_home = variable("CODEX_HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            let profile = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
            variable(profile)
                .filter(|home| !home.is_empty())
                .map(|home| Path::new(&home).join(".codex"))
        });
    let mut candidates = vec![
        root.join(".mcp.json"),
        root.join(".codex").join("config.toml"),
    ];
    candidates.extend(codex_home.map(|home| home.join("config.toml")));
    // `File.Exists`: a file, links followed.
    if candidates
        .iter()
        .any(|file| std::fs::metadata(file).is_ok_and(|metadata| metadata.is_file()))
    {
        // ponytail: declarations read with the editors of lots 8.1 and 8.2.
        return Ok(DiagnosticCheck::new("mcp", "warning", NOT_DELIVERED));
    }
    Ok(DiagnosticCheck::new(
        "mcp",
        "warning",
        "Not declared to an MCP host: smf mcp install, or smf mcp install --host codex.",
    ))
}
