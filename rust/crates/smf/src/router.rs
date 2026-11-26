//! `CliRouter` (lot 7.5): a command goes to the daemon when it runs, has the version of this
//! command line and serves the instance; it runs directly otherwise. A read that the daemon
//! could not finish runs again directly; a write whose result is unknown never does.

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use sermofur_core::VERSION;
use sermofur_core::daemon_paths::{DaemonPaths, EndpointState};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::{FileOwnership, SystemOwnership};
use sermofur_daemon::client::{DaemonClient, INTERRUPTED_CODE};
use sermofur_daemon::endpoint::{self, inspect};
use sermofur_daemon::framing::{IpcMessage, kind};
use sermofur_daemon::server::DaemonLimits;

use crate::args::CommandArguments;
use crate::daemon_run::local_only;
use crate::memory_commands::writes;
use crate::runner::{run_in, write_error};

/// Set to 1 to never try the daemon (diagnosis).
pub const NO_DAEMON_VARIABLE: &str = "SERMOFUR_NO_DAEMON";

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);

/// `Program`: the command line of this process, from its current folder in its long form.
pub fn route(arguments: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let router = Router {
        no_daemon: std::env::var_os(NO_DAEMON_VARIABLE).is_some_and(|value| value == "1"),
        paths: &DaemonPaths::for_current_user,
        // `DaemonLimits.ClientTimeout`: the request timeout of the daemon and 10 s to answer.
        answer_timeout: DaemonLimits::default().request_timeout + Duration::from_secs(10),
    };
    // `LongPath.Of(Directory.GetCurrentDirectory())`: a short 8.3 name would not match the
    // registry of the daemon. A current folder that cannot be read runs directly, where the
    // runner reports it.
    let folder = std::env::current_dir()
        .ok()
        .map(|folder| sermofur_core::discovery::expand_short_names(&folder));
    router.run(arguments, folder.as_deref(), out, err)
}

/// The settings of a [`Router`]: the tests give their own.
pub(crate) struct Router<'a> {
    pub no_daemon: bool,
    pub paths: &'a dyn Fn() -> Result<DaemonPaths>,
    pub answer_timeout: Duration,
}

/// Why a command did not get an answer to print.
enum Failure {
    /// Our daemon answered, refused or stopped mid-command (`DaemonAnswer`): never run directly
    /// beside it, and never replay a write whose result is unknown.
    Daemon(SermofurError),
    /// No daemon of ours, or the command never reached it.
    Unreached,
}

impl Router<'_> {
    /// `CliRouter.Run`.
    pub fn run(
        &self,
        arguments: &[String],
        folder: Option<&Path>,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> i32 {
        let direct = |out: &mut dyn Write, err: &mut dyn Write| {
            run_in(arguments, folder, "direct", out, err)
        };
        let Some(folder) = folder.filter(|_| !self.no_daemon && routable(arguments)) else {
            return direct(out, err);
        };
        let json = CommandArguments::has_flag(arguments, "json");
        let reads = || CommandArguments::parse(arguments).is_ok_and(|parsed| !writes(&parsed));
        let answer = match self.send(arguments, folder) {
            Ok(Some(answer)) if answer.kind != kind::NOT_SERVING => answer,
            Ok(_) | Err(Failure::Unreached) => return direct(out, err),
            Err(Failure::Daemon(error)) => {
                if error.code == INTERRUPTED_CODE && reads() {
                    return direct(out, err);
                }
                write_error(err, &error, json);
                return error.exit_code;
            }
        };
        if answer.kind == kind::RESULT
            && let Some(exit_code) = answer.exit_code
        {
            // `TextWriter.Write`: a missing text writes nothing; a closed stream is ignored.
            let _ = out.write_all(answer.stdout.unwrap_or_default().as_bytes());
            let _ = err.write_all(answer.stderr.unwrap_or_default().as_bytes());
            return exit_code;
        }
        let code = answer.code.unwrap_or_else(|| "protocol_error".to_owned());
        if matches!(
            code.as_str(),
            "response_too_large" | "daemon_stopping" | "daemon_busy"
        ) && reads()
        {
            // A read can run again directly: its output has no size limit there, a read that
            // did not start in a stopping or busy daemon changes nothing, and reads never wait
            // for the writer of the instance. A write is never sent twice.
            return direct(out, err);
        }
        let message = answer.message.unwrap_or_else(|| code.clone());
        let exit_code = answer.exit_code.unwrap_or(3);
        write_error(err, &SermofurError::new(&code, message, exit_code), json);
        exit_code
    }

    /// `CliRouter.Send`: the answer of the daemon, or `None` when no daemon answers.
    fn send(
        &self,
        arguments: &[String],
        folder: &Path,
    ) -> std::result::Result<Option<IpcMessage>, Failure> {
        let paths = (self.paths)().map_err(|_| Failure::Unreached)?;
        // Metadata only: with no daemon, the cost is one stat on Unix; on Windows, three
        // listings of the pipe file system (`endpoint_state`): 5.2 ms for 676 pipes (lot 7.6).
        if inspect(&paths, &SystemOwnership).map_err(|_| Failure::Unreached)?
            != EndpointState::Present
        {
            return Ok(None);
        }
        let folder = folder.to_string_lossy().into_owned();
        block_on(async {
            let Some(mut client) = session(&paths, &SystemOwnership, &folder).await? else {
                return Ok(None);
            };
            // A daemon alive but stuck must not hold the command line: past its own request
            // timeout and the time to answer, it counts as gone mid-command.
            match tokio::time::timeout(self.answer_timeout, client.run(arguments)).await {
                Ok(Ok(answer)) => Ok(Some(answer)),
                // The command was sent: the daemon may have run it.
                Ok(Err(error)) if error.code == INTERRUPTED_CODE || error.code == "protocol_error" => {
                    Err(Failure::Daemon(error))
                }
                // Never sent (too large, or the endpoint went away).
                Ok(Err(_)) => Err(Failure::Unreached),
                Err(_) => Err(Failure::Daemon(SermofurError::new(
                    INTERRUPTED_CODE,
                    format!(
                        "The daemon did not answer within {:.0} s; the result of the command is unknown. Check before running it again.",
                        self.answer_timeout.as_secs_f64()
                    ),
                    3,
                ))),
            }
        })
        .unwrap_or(Err(Failure::Unreached))
    }
}

/// `DaemonClient.ConnectAsync` as `CliRouter.Send` meets it: an error of the endpoint itself
/// (not ours, or a listing that fails: the `IOException` of the oracle) is no daemon of ours,
/// and the command line still works; any error of the hello comes from our daemon.
async fn session(
    paths: &DaemonPaths,
    owners: &dyn FileOwnership,
    folder: &str,
) -> std::result::Result<Option<DaemonClient>, Failure> {
    let Some(stream) = endpoint::connect(paths, owners, CONNECT_TIMEOUT)
        .await
        .map_err(|_| Failure::Unreached)?
    else {
        return Ok(None);
    };
    DaemonClient::hello(stream, VERSION, folder, CONNECT_TIMEOUT)
        .await
        .map_err(Failure::Daemon)
}

/// `CliRouter.Routable` without the variable: help, version, no command, `init`, `doctor`,
/// `daemon …` and `mcp …` always run here, and so does a line the runner will refuse.
pub(crate) fn routable(arguments: &[String]) -> bool {
    !local_only(arguments) && CommandArguments::parse(arguments).is_ok()
}

/// `DaemonProbe.Connect` for `doctor`: a hello of one second from the temporary folder; the
/// version of the daemon, or `None` when nothing answers.
pub(crate) fn probe(paths: &DaemonPaths) -> Result<Option<String>> {
    let folder = temporary_folder();
    block_on(async {
        let client = DaemonClient::connect(
            paths,
            &SystemOwnership,
            VERSION,
            &folder,
            Duration::from_secs(1),
        )
        .await?;
        Ok(client.map(|client| client.daemon_version().to_owned()))
    })?
}

/// `Path.GetTempPath()`: the folder a probe or a status says hello from. It ends with a
/// separator; `temp_dir` may not.
pub(crate) fn temporary_folder() -> String {
    let mut folder = std::env::temp_dir().to_string_lossy().into_owned();
    if !folder.ends_with(std::path::MAIN_SEPARATOR) {
        folder.push(std::path::MAIN_SEPARATOR);
    }
    folder
}

/// Runs a future on a runtime of the current thread (decision A2: tokio for the daemon and its
/// client only); a runtime that cannot start is `internal_error`.
pub(crate) fn block_on<F: Future>(future: F) -> Result<F::Output> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map(|runtime| runtime.block_on(future))
        .map_err(|error| SermofurError::new("internal_error", error.to_string(), 70))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// Port of `RouterTests.AdministrationAndHelpAlwaysRunHere`.
    #[test]
    fn administration_and_help_always_run_here() {
        assert!(!routable(&line(&[])));
        assert!(!routable(&line(&["daemon", "status"])));
        assert!(!routable(&line(&["init"])));
        assert!(!routable(&line(&["doctor", "--json"])));
        assert!(!routable(&line(&["claim", "add", "-h"])));
        assert!(!routable(&line(&["--version"])));
        assert!(routable(&line(&["claim", "list"])));
        assert!(routable(&line(&["--path", "x", "status"])));
    }

    use crate::test_support::{Behavior, Scratch, StubDaemon};

    /// `new CliRouter(output, error, answerTimeout)` on the endpoint of `scratch`: exit code,
    /// output and error.
    fn routed(scratch: &Scratch, timeout: Duration, words: &[&str]) -> (i32, String, String) {
        let paths = || Ok(scratch.paths.clone());
        let router = Router {
            no_daemon: false,
            paths: &paths,
            answer_timeout: timeout,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = router.run(&line(words), Some(&scratch.root), &mut out, &mut err);
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).expect("UTF-8");
        (code, text(out), text(err))
    }

    /// `DaemonTests.Direct(root, ["claim", "list", "--json"])`: the claims really stored.
    fn stored(scratch: &Scratch) -> String {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let words = line(&["claim", "list", "--json"]);
        assert_eq!(
            run_in(&words, Some(&scratch.root), "direct", &mut out, &mut err),
            0
        );
        String::from_utf8(out).expect("UTF-8").trim().to_owned()
    }

    const ANSWER: Duration = Duration::from_secs(70);

    /// Port of `DaemonSessionTests.InterruptedWriteIsNeverReplayedDirectly`.
    #[test]
    fn an_interrupted_write_is_never_replayed_directly() {
        let scratch = Scratch::new("interrupted-write");
        let _stub = StubDaemon::start(&scratch.paths, Behavior::Close, false);
        let (code, _, err) = routed(
            &scratch,
            ANSWER,
            &["claim", "add", "only once", "--origin", "user"],
        );
        assert_eq!(code, 3, "{err}");
        assert!(err.contains(INTERRUPTED_CODE), "{err}");
        assert_eq!(stored(&scratch), "[]");
    }

    /// Port of `DaemonSessionTests.InterruptedReadRunsDirectly`.
    #[test]
    fn an_interrupted_read_runs_directly() {
        let scratch = Scratch::new("interrupted-read");
        let stub = StubDaemon::start(&scratch.paths, Behavior::Close, false);
        let (code, out, err) = routed(&scratch, ANSWER, &["claim", "list", "--json"]);
        assert_eq!(code, 0, "{err}");
        assert_eq!(out.trim(), "[]");
        // Sent to the daemon first, then run directly.
        assert_eq!(stub.runs(), 1);
    }

    /// Port of `DaemonSessionTests.RefusedHelloIsNeverFollowedByADirectRun`.
    #[test]
    fn a_refused_hello_is_never_followed_by_a_direct_run() {
        let scratch = Scratch::new("refused-hello");
        let _stub = StubDaemon::start(&scratch.paths, Behavior::Close, true);
        let (code, out, err) = routed(&scratch, ANSWER, &["claim", "list", "--json"]);
        assert_eq!(code, 3, "{err}");
        assert!(err.contains("protocol_error"), "{err}");
        assert_eq!(out, "");
    }

    /// Port of `DaemonSessionTests.AnswerToAnotherRequestIsAProtocolError`.
    #[test]
    fn an_answer_to_another_request_is_a_protocol_error() {
        let scratch = Scratch::new("wrong-id");
        let _stub = StubDaemon::start(&scratch.paths, Behavior::WrongId, false);
        let folder = scratch.root.to_string_lossy().into_owned();
        let error = block_on(async {
            let mut client = DaemonClient::connect(
                &scratch.paths,
                &SystemOwnership,
                VERSION,
                &folder,
                Duration::from_secs(5),
            )
            .await
            .expect("no refusal")
            .expect("a daemon answers");
            client.run(&line(&["status"])).await.expect_err("refused")
        })
        .expect("a runtime");
        assert_eq!(error.code, "protocol_error");
    }

    /// Port of `DaemonSessionTests.OversizedReadRunsDirectlyButAnOversizedWriteIsNotReplayed`.
    #[test]
    fn an_oversized_read_runs_directly_but_an_oversized_write_is_not_replayed() {
        let scratch = Scratch::new("too-large");
        let stub = StubDaemon::start(&scratch.paths, Behavior::TooLarge, false);
        let (code, out, err) = routed(&scratch, ANSWER, &["claim", "list", "--json"]);
        assert_eq!((code, out.trim()), (0, "[]"), "{err}");
        assert_eq!(stub.runs(), 1, "the read went through the daemon first");
        let (code, _, err) = routed(
            &scratch,
            ANSWER,
            &["claim", "add", "big", "--origin", "user"],
        );
        assert_eq!(code, 1, "{err}");
        assert!(err.contains("response_too_large"), "{err}");
        assert_eq!(stub.runs(), 2, "the write is sent once");
        assert_eq!(stored(&scratch), "[]");
    }

    /// Port of `DaemonLoadTests.SilentDaemonMakesTheCliReplayAReadAndReportAWrite`: with an
    /// answer timeout of 1 s, a read runs again directly and a write is reported, in time.
    #[test]
    fn a_silent_daemon_makes_the_cli_replay_a_read_and_report_a_write() {
        let scratch = Scratch::new("silent");
        let stub = StubDaemon::start(&scratch.paths, Behavior::Silent, false);
        let watch = std::time::Instant::now();
        let second = Duration::from_secs(1);
        let (code, out, err) = routed(&scratch, second, &["claim", "list", "--json"]);
        assert_eq!((code, out.trim()), (0, "[]"), "{err}");
        assert_eq!(stub.runs(), 1, "the read went through the daemon first");
        let (code, _, err) = routed(
            &scratch,
            second,
            &["claim", "add", "unknown fate", "--origin", "user"],
        );
        assert_eq!(code, 3, "{err}");
        assert!(err.contains(INTERRUPTED_CODE), "{err}");
        assert_eq!(stub.runs(), 2, "the write is sent once");
        assert!(
            watch.elapsed() < Duration::from_secs(8),
            "{:?}",
            watch.elapsed()
        );
        assert_eq!(stored(&scratch), "[]");
    }

    /// Port of `DaemonLoadTests.CliRunsARefusedReadDirectlyButNeverAWrite` (both cases): one run
    /// for the read, one for the write, never sent again.
    #[test]
    fn the_cli_runs_a_refused_read_directly_but_never_a_write() {
        for refusal in ["daemon_stopping", "daemon_busy"] {
            let scratch = Scratch::new("refused-run");
            let stub = StubDaemon::start(&scratch.paths, Behavior::Refuse(refusal), false);
            let (code, out, err) = routed(&scratch, ANSWER, &["claim", "list", "--json"]);
            assert_eq!((code, out.trim()), (0, "[]"), "{refusal}: {err}");
            let (code, _, err) = routed(
                &scratch,
                ANSWER,
                &["claim", "add", "not sent twice", "--origin", "user"],
            );
            assert_eq!(code, 3, "{refusal}: {err}");
            assert!(err.contains(refusal), "{err}");
            assert_eq!(stub.runs(), 2, "{refusal}");
            assert_eq!(stored(&scratch), "[]");
        }
    }

    /// Lot 7.6, minor 1 of the review of lot 7.5: an error of the endpoint met by the second
    /// inspection, inside `ConnectAsync`, is no daemon of ours (`foreign_endpoint`, or the
    /// `IOException` of a listing that fails): the command runs directly, as in .NET, never
    /// reported as an answer of the daemon.
    #[cfg(unix)]
    #[test]
    fn an_error_of_the_endpoint_is_no_daemon_of_ours() {
        use sermofur_core::ownership::EntryStatus;

        struct Unreadable;
        impl FileOwnership for Unreadable {
            fn inspect(&self, _: &Path) -> std::io::Result<Option<EntryStatus>> {
                Err(std::io::Error::other("unreadable"))
            }
        }
        struct Foreign;
        impl FileOwnership for Foreign {
            fn inspect(&self, _: &Path) -> std::io::Result<Option<EntryStatus>> {
                Ok(Some(EntryStatus {
                    is_regular_file: false,
                    is_directory: true,
                    is_owned_by_current_user: false,
                }))
            }
        }
        let scratch = Scratch::new("endpoint-error");
        let stub = StubDaemon::start(&scratch.paths, Behavior::Close, false);
        for owners in [&Unreadable as &dyn FileOwnership, &Foreign] {
            let reached = block_on(session(&scratch.paths, owners, "/")).expect("a runtime");
            assert!(matches!(reached, Err(Failure::Unreached)));
        }
        assert_eq!(stub.runs(), 0);
    }
}
