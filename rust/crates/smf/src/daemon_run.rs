//! `smf daemon run` (lot 7.4), as `DaemonCommands.Serve` and `CliCommandExecutor`: the generic
//! server of `sermofur-daemon` runs the commands of its clients with the runner of this crate.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::{expand_short_names, get_full_path};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::SystemOwnership;
use sermofur_core::{VERSION, json};
use sermofur_daemon::file_turn;
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::{
    CommandExecutor, CommandOutcome, CommandPlan, DaemonLimits, DaemonServer, ServingGate, first,
};

use crate::args::{
    CommandArguments, HELP, HELP_SHORTCUT, VERSION as VERSION_OPTION, VERSION_SHORTCUT,
};
use crate::memory_commands::writes;
use crate::runner::{combine, not_ported, run_in, write_line};

/// `CliRouter.LocalOnly`: help, version, no command, and the commands that run only in the
/// process of the user.
pub(crate) fn local_only(argv: &[String]) -> bool {
    if argv.is_empty()
        || CommandArguments::asks(argv, HELP, HELP_SHORTCUT)
        || CommandArguments::asks(argv, VERSION_OPTION, VERSION_SHORTCUT)
    {
        return true;
    }
    // A parsing error is not a command the daemon could run; the runner reports it.
    CommandArguments::parse(argv).is_ok_and(|parsed| {
        parsed
            .positionals()
            .first()
            .is_none_or(|command| matches!(command.as_str(), "init" | "doctor" | "daemon" | "mcp"))
    })
}

/// `CliCommandExecutor`: the commands of a client, run in this process with its `cwd`.
pub(crate) struct CliCommandExecutor;

impl CommandExecutor for CliCommandExecutor {
    fn plan(&self, argv: &[String], cwd: &str) -> Result<CommandPlan> {
        if local_only(argv) {
            return Err(SermofurError::new(
                "cli_only",
                "This command runs only in the CLI itself, never through the daemon.",
                3,
            ));
        }
        let planned = || -> Result<CommandPlan> {
            let mut args = CommandArguments::parse(argv)?;
            let base = Path::new(cwd);
            let target = args.option("path").map(str::to_owned);
            let full = match target {
                Some(value) => get_full_path(&combine(&value, base))?,
                None => get_full_path(base)?,
            };
            // `LongPath.Of`: the 8.3 names expanded, only when a `~` may hide one.
            let full = if cfg!(windows) && full.to_string_lossy().contains('~') {
                expand_short_names(&full)
            } else {
                full
            };
            Ok(CommandPlan {
                path: full.to_string_lossy().into_owned(),
                writes: args.positionals().is_empty() || writes(&args),
            })
        };
        // A command line the runner will refuse is planned as a write on the `cwd`.
        Ok(planned().unwrap_or_else(|_| CommandPlan {
            path: cwd.to_owned(),
            writes: true,
        }))
    }

    fn execute(&self, argv: &[String], cwd: &str) -> CommandOutcome {
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_code = run_in(
            argv,
            Some(Path::new(cwd)),
            "daemon",
            &mut stdout,
            &mut stderr,
        );
        CommandOutcome {
            exit_code,
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
        }
    }
}

/// `daemon run`: serves in the foreground until a `shutdown` request, Ctrl+C or SIGTERM.
pub fn run(
    args: &mut CommandArguments,
    paths: DaemonPaths,
    json: bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<i32> {
    let supervise = args.flag("supervise");
    args.require_count(2)?;
    args.validate_used()?;
    if supervise {
        // The job object of the Windows supervisor waits for the decision on its crate.
        if cfg!(windows) {
            return Err(not_ported());
        }
        return supervise_daemon(paths);
    }
    let _lock = file_turn::acquire(&paths)?;
    let log = DaemonLog::new(&paths.log_file());
    let refusals = log.clone();
    let gate = ServingGate::new(InstanceRegistry::new(&paths.registry_file())).refusing(Box::new(
        move |code| refusals.write("not_served", Some(code), None, None),
    ));
    let endpoint = paths.endpoint.to_string_lossy().into_owned();
    let drain_log = log.clone();
    let server = DaemonServer::new(
        paths,
        VERSION,
        Arc::new(CliCommandExecutor),
        gate,
        log,
        DaemonLimits::default(),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| SermofurError::new("internal_error", error.to_string(), 70))?;
    let served = runtime.block_on(async {
        // Registered before the endpoint opens, as `SignalStop.Register` before `RunAsync`.
        let mut signals = Signals::register()
            .map_err(|error| SermofurError::new("internal_error", error.to_string(), 70))?;
        // `SignalStop`: the first signal drains the started commands; a second one ends the
        // process at once, with the code of the default action of that signal.
        let stop_on_signals = async {
            let stopping = AtomicBool::new(false);
            loop {
                let code = signals.next().await;
                let first = handle(&stopping, || {
                    drain_log.write("draining", None, None, None);
                    // With `--json`, stderr holds only the JSON error: no progress line.
                    if !json {
                        write_line(
                            err,
                            "smf: finishing the running commands; a second Ctrl+C or SIGTERM stops at once.",
                        );
                    }
                    server.stop();
                });
                if !first {
                    std::process::exit(code);
                }
            }
        };
        let serving = server.run(&SystemOwnership, || {
            // One line, even in JSON: a supervisor or a test reads it as the signal of readiness.
            // `JsonSerializer.Serialize` without options: compact, default encoder.
            let line = if json {
                format!(
                    "{{\"listening\":{},\"version\":{}}}",
                    json::quoted_basic_latin(&endpoint),
                    json::quoted_basic_latin(VERSION)
                )
            } else {
                format!("Sermofur daemon {VERSION} listening on {endpoint}")
            };
            write_line(out, &line);
        });
        match first(serving, stop_on_signals).await {
            Ok(served) => served,
            // The signal future never finishes: it exits the process.
            Err(()) => Ok(()),
        }
    });
    // After `drain_timeout`, a command may still run on a blocking thread: the process leaves
    // without waiting for it, as the background threads of the oracle (review of lot 7.4).
    runtime.shutdown_background();
    served?;
    Ok(0)
}

/// The signals of `SignalStop`: SIGINT and SIGTERM on Unix; on Windows, as .NET maps them,
/// `CTRL_C_EVENT` and `CTRL_SHUTDOWN_EVENT`.
#[cfg(unix)]
struct Signals(tokio::signal::unix::Signal, tokio::signal::unix::Signal);

#[cfg(windows)]
struct Signals(
    tokio::signal::windows::CtrlC,
    tokio::signal::windows::CtrlShutdown,
);

impl Signals {
    #[cfg(unix)]
    fn register() -> std::io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self(
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
        ))
    }

    #[cfg(windows)]
    fn register() -> std::io::Result<Self> {
        use tokio::signal::windows::{ctrl_c, ctrl_shutdown};
        Ok(Self(ctrl_c()?, ctrl_shutdown()?))
    }

    /// Waits for the next signal; returns the exit code of its default action: 128 plus the
    /// number of the signal on Unix (the oracle dies by the signal itself, which a shell
    /// reports with the same number), `STATUS_CONTROL_C_EXIT` on Windows.
    async fn next(&mut self) -> i32 {
        let interrupt = first(self.0.recv(), self.1.recv()).await.is_ok();
        if cfg!(windows) {
            // STATUS_CONTROL_C_EXIT, 0xC000013A, as an `i32` exit code.
            -1_073_741_510
        } else if interrupt {
            130
        } else {
            143
        }
    }
}

/// `DaemonCommands.Supervise`: the daemon as a child, started again when it fails. The first
/// signal stops the daemon and the supervisor; a second one ends the supervisor at once.
fn supervise_daemon(paths: DaemonPaths) -> Result<i32> {
    use sermofur_daemon::services::{self, ServiceDefinition};
    use sermofur_daemon::supervisor::{ChildStart, Supervisor};
    let child = ServiceDefinition::for_current_process(VERSION, false, &mut |_| {}, &mut |_| {})?;
    let log = DaemonLog::new(&paths.log_file());
    let installed = services::read_installed(&paths)?;
    if installed.is_none() && services::installed_definition_file(&paths).is_file() {
        // The file is there but its shape does not hold: the daemon starts with no recorded
        // environment at all.
        log.write("installed_definition_unsound", None, None, None);
    }
    // The name of the variable, never its value: the journal is read by eye.
    let start = ChildStart::new(
        &child,
        installed.as_ref(),
        &mut |name| log.write("supervised_variable_dropped", Some(name), None, None),
        &mut |name| log.write("supervised_path_entries_dropped", Some(name), None, None),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&stop);
    std::thread::spawn(move || {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        runtime.block_on(async {
            let Ok(mut signals) = Signals::register() else {
                return;
            };
            loop {
                let code = signals.next().await;
                if !handle(&flag, || {}) {
                    std::process::exit(code);
                }
            }
        });
    });
    let stopper = paths.clone();
    let supervisor = Supervisor::new(
        start,
        log,
        Box::new(move || {
            crate::daemon_commands::shutdown(&stopper, &mut |_| {});
        }),
    );
    Ok(supervisor.run(&stop))
}

/// `SignalStop.Handle`: the first signal runs `on_first` and asks the stop, then lets the
/// process live; a later one finds the stop asked and returns `false`: the process ends at once.
pub(crate) fn handle(stop: &AtomicBool, on_first: impl FnOnce()) -> bool {
    if stop.load(Ordering::SeqCst) {
        return false;
    }
    on_first();
    stop.store(true, Ordering::SeqCst);
    true
}

#[cfg(test)]
mod signal_tests {
    use super::*;

    /// Port of `ServiceTests.SecondSignalEndsTheProcess` (reported from lot 7.6).
    #[test]
    fn a_second_signal_ends_the_process() {
        let stop = AtomicBool::new(false);
        let mut first = 0;
        assert!(handle(&stop, || first += 1));
        assert!(stop.load(Ordering::SeqCst));
        assert!(!handle(&stop, || first += 1));
        assert_eq!(first, 1);
    }
}
