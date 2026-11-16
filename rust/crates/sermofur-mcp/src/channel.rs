//! `DaemonChannel`: the channel of the bridge to the daemon. One session for the launch folder,
//! opened on the first call, opened again after the daemon went away, one command in flight at
//! a time. The bridge never opens an instance: every failure becomes an error that names the
//! remedy.
//!
//! Cancellation is the drop of the future (a `notifications/cancelled`, or the end of the
//! server): the session is out of the slot while a command runs, so a dropped call drops its
//! session, and the daemon learns of the departure from the closed connection.

use std::path::Path;
use std::time::Duration;

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::discover;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::DaemonClient;
use sermofur_daemon::framing::{IpcMessage, kind};
use sermofur_daemon::server::CommandOutcome;
use tokio::sync::Mutex;

use crate::tools::CommandChannel;

/// A daemon alive but stuck must not hold the bridge (`ProbeTimeout`).
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

pub struct DaemonChannel {
    paths: DaemonPaths,
    tool_version: String,
    launch_folder: String,
    session: Mutex<Option<DaemonClient>>,
}

impl DaemonChannel {
    pub fn new(paths: DaemonPaths, tool_version: &str, launch_folder: &str) -> Self {
        Self {
            paths,
            tool_version: tool_version.to_owned(),
            launch_folder: launch_folder.to_owned(),
            session: Mutex::new(None),
        }
    }

    /// The open session if the daemon still answers on it (a `status` round trip, which changes
    /// nothing), else a new one: a daemon restarted since the last call is found before any
    /// command is sent.
    async fn session(&self, open: Option<DaemonClient>) -> Result<DaemonClient> {
        if let Some(mut client) = open
            && matches!(
                tokio::time::timeout(PROBE_TIMEOUT, client.status()).await,
                Ok(Ok(_))
            )
        {
            return Ok(client);
        }
        DaemonClient::connect(
            &self.paths,
            &SystemOwnership,
            &self.tool_version,
            &self.launch_folder,
            CONNECT_TIMEOUT,
        )
        .await?
        .ok_or_else(unavailable)
    }

    fn outcome(&self, answer: IpcMessage) -> Result<CommandOutcome> {
        if answer.kind == kind::RESULT
            && let Some(exit_code) = answer.exit_code
        {
            return Ok(CommandOutcome {
                exit_code,
                stdout: answer.stdout.unwrap_or_default(),
                stderr: answer.stderr.unwrap_or_default(),
            });
        }
        if answer.kind == kind::NOT_SERVING {
            // Discovery only reads the folders between the launch folder and the instance, to
            // tell "no instance" from "instance not registered".
            match discover(Path::new(&self.launch_folder), &SystemOwnership) {
                Err(error) if error.code == "no_instance" => {
                    return Err(SermofurError::new(
                        error.code,
                        format!(
                            "{} Run, in the project: smf init, then smf daemon register",
                            error.message
                        ),
                        error.exit_code,
                    ));
                }
                Err(error) => return Err(error),
                Ok(_) => {}
            }
            return Err(SermofurError::new(
                "not_served",
                format!(
                    "The instance of {} is not registered with the daemon. Run, in the project: smf daemon register",
                    self.launch_folder
                ),
                3,
            ));
        }
        Err(SermofurError::new(
            answer.code.unwrap_or_else(|| "protocol_error".to_owned()),
            answer
                .message
                .unwrap_or_else(|| "Unexpected answer of the daemon.".to_owned()),
            answer.exit_code.unwrap_or(3),
        ))
    }
}

impl CommandChannel for DaemonChannel {
    async fn run(&self, argv: Vec<String>) -> Result<CommandOutcome> {
        let mut slot = self.session.lock().await;
        let mut client = self.session(slot.take()).await?;
        match client.run(&argv).await {
            Ok(answer) => {
                *slot = Some(client);
                self.outcome(answer)
            }
            // The only `invalid_storage_or_path` of `run` is a failed send (the `IOException`
            // of the oracle): the daemon never saw the command.
            Err(error) if error.code == "invalid_storage_or_path" => Err(unavailable()),
            // Interrupted or out of step: this session can no longer pair a request with its
            // answer; it is dropped, and an interrupted write is never sent again.
            Err(error) => Err(error),
        }
    }
}

fn unavailable() -> SermofurError {
    SermofurError::new(
        "daemon_unavailable",
        "No Sermofur daemon answers. Run: smf daemon install (or smf daemon start), then smf daemon register in the project.",
        3,
    )
}
