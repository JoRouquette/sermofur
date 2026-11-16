//! `smf daemon …`, as `DaemonCommands.Run` (lot 7.6): `register`, `unregister`, `instances` and
//! `status`, always run by the command line itself, never through the daemon. `run` is
//! [`crate::daemon_run`]; `install`, `uninstall`, `start`, `stop` and `restart` come with the
//! services (lot 7.7). Text and `--json` print the same JSON, as `CommandRunner.Write`.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use sermofur_core::VERSION;
use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::get_full_path;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::DaemonClient;
use sermofur_daemon::framing::IpcMessage;
use sermofur_daemon::registry::{InstanceRegistry, RegisteredInstance, RegistryEntry};
use sermofur_daemon::services::{self, ServiceManager, ServiceStatus};

use crate::args::CommandArguments;
use crate::router::{block_on, temporary_folder};
use crate::runner::{combine, not_ported, to_json, write_line};

const SUBCOMMANDS: &str =
    "install, uninstall, start, stop, restart, status, register, unregister, instances, run";

/// `DaemonCommands.DefaultStatusTimeout`: how long a daemon that welcomed the command line has
/// to answer a status.
pub const DEFAULT_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// `DaemonCommands.Run`.
pub fn run(
    args: &mut CommandArguments,
    json: bool,
    working_directory: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    if args.positionals().len() < 2 {
        return Err(SermofurError::invalid_arguments(format!(
            "Subcommand required: {SUBCOMMANDS}."
        )));
    }
    let paths = DaemonPaths::for_current_user()?;
    let subcommand = args.positionals()[1].clone();
    if subcommand == "run" {
        return crate::daemon_run::run(args, paths, json, out, err);
    }
    // `Path.GetFullPath(args.Option("path", workingDirectory), workingDirectory)`.
    let target = match args.option("path") {
        Some(value) => get_full_path(&combine(value, working_directory))?,
        None => get_full_path(working_directory)?,
    };
    args.require_count(2)?;
    args.validate_used()?;
    let registry = InstanceRegistry::new(&paths.registry_file());
    let body = match subcommand.as_str() {
        "register" => to_json(&Registered::from(
            registry.register(&target, DateTimeOffset::now_local())?,
        ))?,
        "unregister" => to_json(&Registered::from(registry.unregister(&target)?))?,
        "instances" => to_json(
            &registry
                .list()?
                .into_iter()
                .map(Listed::from)
                .collect::<Vec<_>>(),
        )?,
        "status" => to_json(&status(
            &paths,
            services::for_current_system().as_ref(),
            DEFAULT_STATUS_TIMEOUT,
        )?)?,
        "install" | "uninstall" | "start" | "stop" | "restart" => return Err(not_ported()),
        _ => {
            return Err(SermofurError::invalid_arguments(format!(
                "Unknown daemon subcommand '{subcommand}'; expected one of: {SUBCOMMANDS}."
            )));
        }
    };
    write_line(out, &body);
    Ok(0)
}

/// `RegisteredInstance` as `RecordJson` writes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Registered {
    root: Option<String>,
    registered_at: Option<String>,
}

impl From<RegisteredInstance> for Registered {
    fn from(value: RegisteredInstance) -> Self {
        Self {
            root: value.root,
            registered_at: value.registered_at,
        }
    }
}

/// `RegistryEntry` as `RecordJson` writes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Listed {
    root: String,
    registered_at: Option<String>,
    missing: bool,
}

impl From<RegistryEntry> for Listed {
    fn from(value: RegistryEntry) -> Self {
        Self {
            root: value.root,
            registered_at: value.registered_at,
            missing: value.missing,
        }
    }
}

/// The anonymous object of `DaemonCommands.Status`, as described in contracts/cli.md.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusBody {
    pub state: &'static str,
    pub manager: &'static str,
    pub installed: bool,
    pub running: bool,
    pub answering: Option<bool>,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub pid: Option<i32>,
    pub registry: Option<String>,
    pub started_at: Option<String>,
    pub instances_open: Option<i32>,
    pub clients: Option<i32>,
}

/// `DaemonCommands.Status`: what the service manager, `service.json` and the daemon itself say,
/// read in that order.
pub(crate) fn status(
    paths: &DaemonPaths,
    manager: &dyn ServiceManager,
    status_timeout: Duration,
) -> Result<StatusBody> {
    let unavailable = manager.unavailable();
    let service = if unavailable.is_none() {
        manager.query()
    } else {
        ServiceStatus::default()
    };
    let executable = services::installed_executable(paths)?;
    let (state, answer) = match answer(paths, status_timeout) {
        Ok(Some(answer)) => ("running", Some(answer)),
        Ok(None) if unavailable.is_some() => ("service_manager_unavailable", None),
        Ok(None) if service.installed => ("installed_stopped", None),
        Ok(None) => ("absent", None),
        Err(error) if error.code == "daemon_version_mismatch" => ("version_mismatch", None),
        Err(error) if error.code == "foreign_endpoint" => ("foreign_endpoint", None),
        Err(error) => return Err(error),
    };
    let (version, status) = match answer {
        Some((version, status)) => (Some(version), status),
        None => (None, None),
    };
    let field = |read: fn(&IpcMessage) -> Option<i32>| status.as_ref().and_then(read);
    Ok(StatusBody {
        state,
        manager: manager.name(),
        installed: service.installed,
        running: version.is_some(),
        answering: version.as_ref().map(|_| status.is_some()),
        version,
        executable,
        pid: field(|message| message.pid),
        registry: status.as_ref().and_then(|message| message.registry.clone()),
        started_at: status
            .as_ref()
            .and_then(|message| message.started_at.clone()),
        instances_open: field(|message| message.instances_open),
        clients: field(|message| message.clients),
    })
}

/// `DaemonCommands.Answer`: the version of a running daemon of this version and its `status`
/// message, `None` for the message when the daemon welcomed the command line but kept its
/// status to itself past `status_timeout`; `None` when no daemon answers.
fn answer(
    paths: &DaemonPaths,
    status_timeout: Duration,
) -> Result<Option<(String, Option<IpcMessage>)>> {
    let folder = temporary_folder();
    block_on(async {
        let Some(mut client) = DaemonClient::connect(
            paths,
            &SystemOwnership,
            VERSION,
            &folder,
            Duration::from_secs(1),
        )
        .await?
        else {
            return Ok(None);
        };
        let version = client.daemon_version().to_owned();
        let status = match tokio::time::timeout(status_timeout, client.status()).await {
            Ok(status) => Some(status?),
            Err(_) => None,
        };
        Ok(Some((version, status)))
    })?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{Behavior, Scratch, StubDaemon};

    /// A manager that is usable and has nothing installed.
    struct NothingInstalled;

    impl ServiceManager for NothingInstalled {
        fn name(&self) -> &'static str {
            "fake"
        }

        fn unavailable(&self) -> Option<String> {
            None
        }

        fn query(&self) -> ServiceStatus {
            ServiceStatus::default()
        }
    }

    /// Port of `ServiceTests.StatusOfASilentDaemonSaysItRunsButDoesNotAnswer`, with a status
    /// timeout of 1 s as the `DaemonWith` of .NET.
    #[test]
    fn the_status_of_a_silent_daemon_says_it_runs_but_does_not_answer() {
        let scratch = Scratch::new("silent-status");
        let _stub = StubDaemon::start(&scratch.paths, Behavior::Silent, false);
        let watch = std::time::Instant::now();
        let body =
            status(&scratch.paths, &NothingInstalled, Duration::from_secs(1)).expect("a status");
        assert!(
            watch.elapsed() < Duration::from_secs(6),
            "{:?}",
            watch.elapsed()
        );
        assert_eq!(
            (body.state, body.running, body.answering),
            ("running", true, Some(false))
        );
        assert_eq!(body.version.as_deref(), Some(VERSION));
        assert_eq!((body.pid, body.started_at), (None, None));
    }

    #[test]
    fn the_status_without_a_daemon_is_absent() {
        let scratch = Scratch::new("absent-status");
        let body =
            status(&scratch.paths, &NothingInstalled, DEFAULT_STATUS_TIMEOUT).expect("a status");
        assert_eq!(
            (body.state, body.running, body.answering, body.executable),
            ("absent", false, None, None)
        );
    }
}
