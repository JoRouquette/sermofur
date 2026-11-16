//! `smf mcp install` and `smf mcp uninstall` (`McpCommands`, `McpDeclaration`): always run by
//! the CLI itself, never through the daemon. `smf mcp serve` comes with lot 8.4.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sermofur_core::atomic::{self, LinkPolicy, WriteOptions};
use sermofur_core::daemon_paths::{DaemonPaths, EndpointState, environment_text};
use sermofur_core::discovery::{discover, get_full_path};
use sermofur_core::doctor::DiagnosticCheck;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::local_paths::same_root;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::endpoint;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_mcp::{codex_config, mcp_config};

use crate::args::CommandArguments;
use crate::runner::{combine, not_ported, to_json, write_line};

pub const CLAUDE_CODE: &str = "claude-code";
pub const CODEX: &str = "codex";
pub const PROJECT_SCOPE: &str = "project";
pub const USER_SCOPE: &str = "user";

/// Result of `smf mcp install` or `uninstall` (`DeclarationReport`, ADR 0016 for `replaced`).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclarationReport {
    file: String,
    changed: bool,
    declared: bool,
    command: Option<String>,
    portable: bool,
    next: Vec<String>,
    host: &'static str,
    replaced: Option<String>,
}

pub fn run(
    args: &mut CommandArguments,
    working_directory: &Path,
    out: &mut dyn Write,
) -> Result<i32> {
    if args.positionals().len() < 2 {
        return Err(SermofurError::invalid_arguments(
            "Subcommand required: serve, install or uninstall.",
        ));
    }
    let subcommand = args.positionals()[1].clone();
    if subcommand == "serve" {
        // ponytail: the stdio bridge is lot 8.4.
        return Err(not_ported());
    }
    let target = match args.option("path") {
        Some(value) => get_full_path(&combine(value, working_directory))?,
        None => get_full_path(working_directory)?,
    };
    let host = args.option("host").unwrap_or(CLAUDE_CODE).to_owned();
    let scope = args.option("scope").unwrap_or(PROJECT_SCOPE).to_owned();
    args.require_count(2)?;
    args.validate_used()?;
    let report = match subcommand.as_str() {
        "install" => install(&target, &host, &scope)?,
        "uninstall" => uninstall(&target, &host, &scope)?,
        _ => {
            return Err(SermofurError::invalid_arguments(format!(
                "Unknown mcp subcommand '{subcommand}'; expected serve, install or uninstall."
            )));
        }
    };
    // `Write(result, json)`: a report is always written as JSON.
    write_line(out, &to_json(&report)?);
    Ok(0)
}

/// The `mcp` check of `doctor`, with the hosts that declare the bridge for the root.
pub fn doctor_check(root: &Path) -> Result<DiagnosticCheck> {
    let declared = declarations(root);
    Ok(if declared.is_empty() {
        DiagnosticCheck::new(
            "mcp",
            "warning",
            "Not declared to an MCP host: smf mcp install, or smf mcp install --host codex.",
        )
    } else {
        DiagnosticCheck::new("mcp", "ok", format!("Declared to {}.", declared.join(", ")))
    })
}

/// Hosts that declare the bridge for an instance root, with where (`Declarations`).
pub fn declarations(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let file = root.join(mcp_config::FILE_NAME);
    if is_file(&file)
        && std::fs::read(&file).is_ok_and(|bytes| mcp_config::has_entry(&bytes).unwrap_or(false))
    {
        found.push(format!("{CLAUDE_CODE} ({})", mcp_config::FILE_NAME));
    }
    if codex_declares(&codex_config::project_path(root)) {
        found.push(format!("{CODEX} (.codex/config.toml)"));
    }
    if codex_declares(&codex_config::user_path(environment_text)) {
        found.push(format!("{CODEX} (user configuration)"));
    }
    found
}

fn install(folder: &Path, host: &str, scope: &str) -> Result<DeclarationReport> {
    check(host, scope)?;
    let (command, arguments, portable) = launcher();
    let mut next;
    let (file, changed, replaced) = if host == CLAUDE_CODE {
        let file = folder.join(mcp_config::FILE_NAME);
        let current = read_bytes(&file)?;
        let updated = mcp_config::with_entry(current.as_deref(), &command, &arguments)?;
        let written = updated
            .as_ref()
            .map(|u| write(&file, u, None))
            .transpose()?;
        next = followed(&file, written.as_deref())?;
        next.extend(pending(folder)?);
        if !portable {
            next.push(format!(
                "The entry names {command}, a path of this machine: put the folder of smf on the PATH before sharing {}.",
                mcp_config::FILE_NAME
            ));
        }
        next.push("Start claude in the project and approve the sermofur server.".to_owned());
        (file, updated.is_some(), written)
    } else {
        let file = codex_file(folder, scope);
        let (current, bom) = read_text(&file)?;
        let updated = codex_config::with_block(current.as_deref(), &command, &arguments)?;
        let written = updated
            .as_ref()
            .map(|u| write_text(&file, u, bom))
            .transpose()?;
        next = followed(&file, written.as_deref())?;
        next.extend(pending(folder)?);
        if !portable {
            next.push(format!(
                "The table names {command}, a path of this machine: put the folder of smf on the PATH."
            ));
        }
        next.push(if scope == PROJECT_SCOPE {
            "Codex loads .codex/config.toml only in a trusted project: start codex in the project and trust it.".to_owned()
        } else {
            "Start codex in a project served by the daemon.".to_owned()
        });
        (file, updated.is_some(), written)
    };
    Ok(DeclarationReport {
        replaced: elsewhere(&file, replaced.as_deref())?,
        file: file.to_string_lossy().into_owned(),
        changed,
        declared: true,
        command: Some(command),
        portable,
        next,
        host: if host == CLAUDE_CODE {
            CLAUDE_CODE
        } else {
            CODEX
        },
    })
}

fn uninstall(folder: &Path, host: &str, scope: &str) -> Result<DeclarationReport> {
    check(host, scope)?;
    let (file, written) = if host == CLAUDE_CODE {
        let file = folder.join(mcp_config::FILE_NAME);
        let updated = match read_bytes(&file)? {
            Some(current) => mcp_config::without_entry(&current)?,
            None => None,
        };
        let written = updated.map(|u| write(&file, &u, None)).transpose()?;
        (file, written)
    } else {
        let file = codex_file(folder, scope);
        let (current, bom) = read_text(&file)?;
        let updated = match current {
            Some(current) => codex_config::without_block(&current)?,
            None => None,
        };
        let written = updated.map(|u| write_text(&file, &u, bom)).transpose()?;
        (file, written)
    };
    Ok(DeclarationReport {
        next: followed(&file, written.as_deref())?,
        replaced: elsewhere(&file, written.as_deref())?,
        changed: written.is_some(),
        file: file.to_string_lossy().into_owned(),
        declared: false,
        command: None,
        portable: true,
        host: if host == CLAUDE_CODE {
            CLAUDE_CODE
        } else {
            CODEX
        },
    })
}

fn check(host: &str, scope: &str) -> Result<()> {
    if host != CLAUDE_CODE && host != CODEX {
        return Err(SermofurError::invalid_arguments(format!(
            "Unknown host '{host}'; expected {CLAUDE_CODE} or {CODEX}."
        )));
    }
    if scope != PROJECT_SCOPE && scope != USER_SCOPE {
        return Err(SermofurError::invalid_arguments(format!(
            "Unknown scope '{scope}'; expected {PROJECT_SCOPE} or {USER_SCOPE}."
        )));
    }
    if host == CLAUDE_CODE && scope == USER_SCOPE {
        return Err(SermofurError::invalid_arguments(
            "The user scope of Claude Code is not written by smf (~/.claude.json is rewritten by Claude Code): use claude mcp add --scope user sermofur -- smf mcp serve.",
        ));
    }
    Ok(())
}

fn codex_file(folder: &Path, scope: &str) -> PathBuf {
    if scope == USER_SCOPE {
        codex_config::user_path(environment_text)
    } else {
        codex_config::project_path(folder)
    }
}

fn codex_declares(file: &Path) -> bool {
    is_file(file)
        && read_text(file).is_ok_and(|(content, _)| {
            content.is_some_and(|text| codex_config::declares(&text).unwrap_or(false))
        })
}

/// `File.Exists`: a file, links followed.
fn is_file(file: &Path) -> bool {
    std::fs::metadata(file).is_ok_and(|metadata| metadata.is_file())
}

fn read_bytes(file: &Path) -> Result<Option<Vec<u8>>> {
    if !is_file(file) {
        return Ok(None);
    }
    std::fs::read(file)
        .map(Some)
        .map_err(|error| io_error(file, &error))
}

/// The text of a configuration and whether it starts with a byte order mark; strict UTF-8.
fn read_text(file: &Path) -> Result<(Option<String>, bool)> {
    let Some(bytes) = read_bytes(file)? else {
        return Ok((None, false));
    };
    let bom = bytes.starts_with(&[0xEF, 0xBB, 0xBF]);
    let body = bytes[if bom { 3 } else { 0 }..].to_vec();
    match String::from_utf8(body) {
        Ok(text) => Ok((Some(text), bom)),
        Err(_) => Err(SermofurError::new(
            "invalid_mcp_config",
            format!("{} is not UTF-8; it is left untouched.", file.display()),
            3,
        )),
    }
}

fn io_error(file: &Path, error: &std::io::Error) -> SermofurError {
    SermofurError::new(
        "invalid_mcp_config",
        format!("{} cannot be read: {error}", file.display()),
        3,
    )
}

/// A Codex configuration may hold the tokens of other servers: a new one is private.
fn write_text(file: &Path, content: &str, bom: bool) -> Result<PathBuf> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io_error(file, &error))?;
    }
    let mut bytes = if bom {
        vec![0xEF, 0xBB, 0xBF]
    } else {
        Vec::new()
    };
    bytes.extend_from_slice(content.as_bytes());
    write(file, &bytes, Some(0o600))
}

/// A followed link replaces its target, so the file stays where the user put it (ADR 0016).
fn write(file: &Path, content: &[u8], new_file_mode: Option<u32>) -> Result<PathBuf> {
    Ok(atomic::write(
        file,
        content,
        &WriteOptions {
            new_file_mode,
            policy: LinkPolicy::FollowToTarget,
            ..WriteOptions::default()
        },
    )?)
}

/// The file actually replaced when a followed link moved the write elsewhere.
fn elsewhere(file: &Path, written: Option<&Path>) -> Result<Option<String>> {
    let Some(written) = written else {
        return Ok(None);
    };
    // `Path.GetFullPath`: lexical, short 8.3 names kept, `/` turned into `\` on Windows
    // (`GetFullPathNameW`), where `get_full_path` expands the short names.
    let full = std::path::absolute(file).map_err(|error| io_error(file, &error))?;
    let written = written.to_string_lossy().into_owned();
    let same = same_root(&written, &full.to_string_lossy());
    Ok((!same).then_some(written))
}

fn followed(file: &Path, written: Option<&Path>) -> Result<Vec<String>> {
    Ok(elsewhere(file, written)?
        .map(|replaced| {
            format!(
                "{} is a link: the configuration was written in {replaced}.",
                file.display()
            )
        })
        .into_iter()
        .collect())
}

/// `smf mcp serve` by name when the folder of this smf is on the PATH, its full path otherwise.
fn launcher() -> (String, Vec<String>, bool) {
    let arguments = vec!["mcp".to_owned(), "serve".to_owned()];
    let process = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("smf"));
    let name = process.file_stem().map_or_else(
        || "smf".to_owned(),
        |stem| stem.to_string_lossy().into_owned(),
    );
    let folder = process.parent().map(|f| f.to_string_lossy().into_owned());
    let separator = if cfg!(windows) { ';' } else { ':' };
    let on_path = folder.is_some_and(|folder| {
        environment_text("PATH")
            .unwrap_or_default()
            .split(separator)
            .filter(|entry| !entry.is_empty())
            .any(|entry| same_root(entry, &folder))
    });
    if on_path {
        (name, arguments, true)
    } else {
        (process.to_string_lossy().into_owned(), arguments, false)
    }
}

/// What remains to do for the bridge to answer in this project.
fn pending(folder: &Path) -> Result<Vec<String>> {
    let paths = match DaemonPaths::for_current_user() {
        Ok(paths) => paths,
        Err(error) => return Ok(vec![error.message]),
    };
    let mut pending = Vec::new();
    if !matches!(
        endpoint::inspect(&paths, &SystemOwnership)?,
        EndpointState::Present
    ) {
        pending.push("No daemon runs: smf daemon install".to_owned());
    }
    let registered = discover(folder, &SystemOwnership).and_then(|root| {
        let root = root.to_string_lossy().into_owned();
        Ok(InstanceRegistry::new(&paths.registry_file())
            .read()?
            .iter()
            .any(|entry| entry.root.as_deref().is_some_and(|r| same_root(r, &root))))
    });
    match registered {
        Ok(true) => {}
        Ok(false) => {
            pending.push("The instance is not registered: smf daemon register".to_owned());
        }
        Err(error) => pending.push(format!("{}: {}", error.code, error.message)),
    }
    Ok(pending)
}
