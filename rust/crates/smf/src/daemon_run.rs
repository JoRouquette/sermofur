//! `smf daemon run` (lot 7.4), as `DaemonCommands.Serve` and `CliCommandExecutor`: the generic
//! server of `sermofur-daemon` runs the commands of its clients with the runner of this crate.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::{expand_short_names, get_full_path};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::ownership::SystemOwnership;
use sermofur_core::{VERSION, json};
use sermofur_daemon::file_turn;
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::{
    CommandExecutor, CommandOutcome, CommandPlan, DaemonLimits, DaemonServer, ServingGate,
};

use crate::args::{
    CommandArguments, HELP, HELP_SHORTCUT, VERSION as VERSION_OPTION, VERSION_SHORTCUT,
};
use crate::memory_commands::writes;
use crate::runner::{combine, not_ported, run_in, write_line};

/// `CliRouter.LocalOnly`: help, version, no command, and the commands that run only in the
/// process of the user.
fn local_only(argv: &[String]) -> bool {
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
struct CliCommandExecutor;

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

#[derive(Serialize)]
struct ListeningBody<'a> {
    listening: &'a str,
    version: &'a str,
}

/// `daemon run`: serves in the foreground until a `shutdown` request.
pub fn run(
    args: &mut CommandArguments,
    json: bool,
    out: &mut dyn Write,
    _err: &mut dyn Write,
) -> Result<i32> {
    let supervise = args.flag("supervise");
    args.require_count(2)?;
    args.validate_used()?;
    if supervise {
        // The supervisor comes with the services (lot 7.7).
        return Err(not_ported());
    }
    let paths = DaemonPaths::for_current_user()?;
    let _lock = file_turn::acquire(&paths)?;
    let log = DaemonLog::new(&paths.log_file());
    let refusals = log.clone();
    let gate = ServingGate::new(InstanceRegistry::new(&paths.registry_file())).refusing(Box::new(
        move |code| refusals.write("not_served", Some(code), None, None),
    ));
    let endpoint = paths.endpoint.to_string_lossy().into_owned();
    let server = DaemonServer::new(
        paths,
        VERSION,
        Arc::new(CliCommandExecutor),
        gate,
        log,
        DaemonLimits::default(),
    );
    // ponytail: no handler of Ctrl+C or SIGTERM yet (the `signal` feature of tokio brings a new
    // crate); a signal ends the process at once, the commands are not drained.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| SermofurError::new("internal_error", error.to_string(), 70))?;
    let served = runtime.block_on(server.run(&SystemOwnership, || {
        // One line, even in JSON: a supervisor or a test reads it as the signal of readiness.
        let line = if json {
            json::to_string(&ListeningBody {
                listening: &endpoint,
                version: VERSION,
            })
            .unwrap_or_default()
        } else {
            format!("Sermofur daemon {VERSION} listening on {endpoint}")
        };
        write_line(out, &line);
    }));
    // After `drain_timeout`, a command may still run on a blocking thread: the process leaves
    // without waiting for it, as the background threads of the oracle (review of lot 7.4).
    runtime.shutdown_background();
    served?;
    Ok(0)
}
