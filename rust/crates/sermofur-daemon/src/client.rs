//! One session with the daemon of the current user, as `DaemonClient` of the .NET oracle: a
//! hello, then commands in sequence. Used by the router of the command line and by `doctor`.
//!
//! Cancellation is the drop of the future, where .NET passes a token: a caller that bounds a
//! command (the router) wraps [`DaemonClient::run`] in its own timeout.

use std::time::Duration;

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::FileOwnership;
use tokio::io::AsyncWriteExt;

use crate::endpoint::{self, Stream};
use crate::framing::{
    self, CURRENT_PROTOCOL, FrameError, IpcMessage, MAX_BODY_BYTES, MAX_RESPONSE_BYTES, kind,
};
use crate::server::{gather, read_message};

/// The daemon stopped after receiving a command: its result is unknown (`InterruptedCode`).
pub const INTERRUPTED_CODE: &str = "daemon_interrupted";

/// An open session (`DaemonClient`).
pub struct DaemonClient {
    stream: Stream,
    working_directory: String,
    next_id: i64,
    daemon_version: String,
}

impl DaemonClient {
    /// Version reported by the daemon in its welcome.
    pub fn daemon_version(&self) -> &str {
        &self.daemon_version
    }

    /// `ConnectAsync`: a session for `working_directory`, or `None` when no daemon answers
    /// within `timeout` (for the connection, then again for the hello). `foreign_endpoint`
    /// comes before any exchange; a refusal of the hello comes with the code of the daemon
    /// (`daemon_version_mismatch`, `protocol_error`), and the command must then not run beside
    /// that daemon.
    pub async fn connect(
        paths: &DaemonPaths,
        owners: &dyn FileOwnership,
        tool_version: &str,
        working_directory: &str,
        timeout: Duration,
    ) -> Result<Option<Self>> {
        let Some(mut stream) = endpoint::connect(paths, owners, timeout).await? else {
            return Ok(None);
        };
        let mut hello = IpcMessage::new(kind::HELLO);
        hello.protocol = Some(CURRENT_PROTOCOL);
        hello.tool_version = Some(tool_version.to_owned());
        hello.cwd = Some(working_directory.to_owned());
        hello.pid = i32::try_from(std::process::id()).ok();
        let frame = framing::encode(&hello, MAX_BODY_BYTES)?;
        let exchanged = tokio::time::timeout(timeout, async {
            send(&mut stream, &frame).await?;
            read_message(&mut stream, MAX_BODY_BYTES).await
        })
        .await;
        let answer = match exchanged {
            // A daemon that stops answering mid-hello counts as no daemon.
            Err(_) | Ok(Err(FrameError::Io(_))) => return Ok(None),
            Ok(Err(FrameError::Refused(refusal))) => return Err(refusal),
            // Closed without a word: a daemon that is dying or stopping counts as none.
            Ok(Ok(None)) => return Ok(None),
            Ok(Ok(Some(answer))) => answer,
        };
        if answer.kind == kind::WELCOME
            && let Some(version) = answer.tool_version
        {
            return Ok(Some(Self {
                stream,
                working_directory: working_directory.to_owned(),
                next_id: 0,
                daemon_version: version,
            }));
        }
        let error = answer.kind == kind::ERROR;
        if error && answer.code.as_deref() == Some("daemon_version_mismatch") {
            return Err(SermofurError::new(
                "daemon_version_mismatch",
                version_mismatch_message(
                    answer.tool_version.as_deref(),
                    tool_version,
                    answer.message.as_deref(),
                ),
                3,
            ));
        }
        let code = match answer.code {
            Some(code) if error => code,
            _ => "protocol_error".to_owned(),
        };
        let message = answer
            .message
            .unwrap_or_else(|| format!("Unexpected answer '{}' to the hello.", answer.kind));
        Err(SermofurError::new(&code, message, 3))
    }

    /// `RunAsync`: a `result`, a `not_serving` or an `error` message. A failure while sending is
    /// `request_too_large` or an I/O error (`invalid_storage_or_path`): the daemon never ran
    /// the command. A failure once the command is sent is [`INTERRUPTED_CODE`]: the daemon may
    /// have run it; an answer to another request is `protocol_error`.
    pub async fn run(&mut self, argv: &[String]) -> Result<IpcMessage> {
        self.next_id += 1;
        let id = self.next_id;
        let mut request = IpcMessage::new(kind::RUN);
        request.id = Some(id);
        request.argv = Some(argv.iter().cloned().map(Some).collect());
        request.cwd = Some(self.working_directory.clone());
        let frame = framing::encode(&request, MAX_BODY_BYTES)?;
        send(&mut self.stream, &frame)
            .await
            .map_err(endpoint::io_failure)?;
        let read = match gather(&mut self.stream, MAX_RESPONSE_BYTES).await {
            Ok(bytes) => framing::read_answer(&mut bytes.as_slice(), MAX_RESPONSE_BYTES),
            Err(error) => Err(FrameError::Io(error)),
        };
        let Ok(Some(answer)) = read else {
            return Err(SermofurError::new(
                INTERRUPTED_CODE,
                "The daemon stopped during the command; its result is unknown. Check before running it again.",
                3,
            ));
        };
        let carries_id = answer.kind == kind::RESULT || answer.kind == kind::NOT_SERVING;
        if answer.id.map_or(carries_id, |other| other != id) {
            let answered = answer.id.map_or_else(|| "?".to_owned(), |v| v.to_string());
            return Err(SermofurError::new(
                "protocol_error",
                format!("The daemon answered request {answered} instead of {id}."),
                3,
            ));
        }
        Ok(answer)
    }
}

async fn send(stream: &mut Stream, frame: &[u8]) -> std::io::Result<()> {
    stream.write_all(frame).await?;
    stream.flush().await
}

/// `VersionMismatchMessage`: the refusal of a version, with the remedy of the side that is
/// behind.
pub fn version_mismatch_message(
    daemon_version: Option<&str>,
    client_version: &str,
    fallback: Option<&str>,
) -> String {
    let Some(daemon_version) = daemon_version else {
        return fallback.map_or_else(
            || format!("The daemon runs another version than {client_version}."),
            str::to_owned,
        );
    };
    if compare_versions(client_version, daemon_version).is_lt() {
        format!(
            "The daemon runs Sermofur {daemon_version}; this client is the older {client_version}. Restart the program that started it (for the MCP server: restart the sermofur server in Claude Code or Codex), or update smf."
        )
    } else {
        format!(
            "The daemon runs Sermofur {daemon_version}; this client is {client_version}. Run: smf daemon restart"
        )
    }
}

/// `CompareVersions`: `System.Version` of the part before the first `-`; equal when either
/// does not parse.
fn compare_versions(left: &str, right: &str) -> std::cmp::Ordering {
    match (version(left), version(right)) {
        (Some(a), Some(b)) => a.cmp(&b),
        _ => std::cmp::Ordering::Equal,
    }
}

/// `Version.TryParse`: two to four non-negative `int` parts; a missing part is -1.
fn version(text: &str) -> Option<[i64; 4]> {
    let core = text.split('-').next().unwrap_or(text);
    let parts: Vec<&str> = core.split('.').collect();
    if !(2..=4).contains(&parts.len()) {
        return None;
    }
    let mut value = [-1; 4];
    for (slot, part) in value.iter_mut().zip(parts) {
        // `int.Parse` with `NumberStyles.Integer`: white space around, a sign.
        let number: i32 = part
            .trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\u{b}' | '\u{c}' | '\r'))
            .parse()
            .ok()?;
        *slot = i64::from(number);
        if *slot < 0 {
            return None;
        }
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Port of `DaemonSessionTests.VersionRemedyNamesTheSideThatIsBehind`.
    #[test]
    fn the_version_remedy_names_the_side_that_is_behind() {
        for (daemon, client, remedy) in [
            ("9.0.0", "0.4.0", "restart the sermofur server"),
            ("0.3.0", "0.4.0", "smf daemon restart"),
        ] {
            let text = version_mismatch_message(Some(daemon), client, None);
            assert!(text.contains(remedy), "{text}");
        }
        assert_eq!(
            version_mismatch_message(None, "0.4.0", None),
            "The daemon runs another version than 0.4.0."
        );
        assert_eq!(
            compare_versions("0.0.0-dev", "0.0.0-other"),
            std::cmp::Ordering::Equal
        );
        assert_eq!(version("1"), None);
        assert_eq!(version("1.2. 3"), Some([1, 2, 3, -1]));
        assert_eq!(version("1.-2"), None);
    }
}
