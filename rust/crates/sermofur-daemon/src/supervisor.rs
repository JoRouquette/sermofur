//! `Supervisor`: runs `smf daemon run` as a child and starts it again when it fails, slower
//! after five restarts in a minute, and gives up after three quick failures in a row.
//!
//! The child inherits the whole environment of the supervisor, the recorded variables laid on
//! top, as `ProcessStartInfo.Environment` does (gap E3, reproduced). Under Windows the daemon
//! lives in a job object (`WindowsJob`, crate `win32job`, decision of John of 2026-11-17), so it
//! never outlives its supervisor, even when the supervisor is killed.

use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::error::Result;

use crate::log::DaemonLog;
use crate::services::{ServiceDefinition, exit_code};

/// `Supervisor.ChildStart`: what starts the daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChildStart {
    pub program: String,
    pub arguments: Vec<String>,
    /// Set on top of the inherited environment.
    pub environment: Vec<(String, String)>,
}

impl ChildStart {
    /// The child definition with the carried variables of the installed one; `dropped` and
    /// `narrowed` get the names of the variables left out or shortened, never their values.
    pub fn new(
        child: &ServiceDefinition,
        installed: Option<&ServiceDefinition>,
        dropped: &mut dyn FnMut(&str),
        narrowed: &mut dyn FnMut(&str),
    ) -> Self {
        Self {
            program: child.executable.clone(),
            arguments: child.arguments.clone(),
            environment: installed
                .map(|installed| installed.carried_environment(dropped, narrowed))
                .unwrap_or_default(),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.arguments)
            .envs(self.environment.iter().map(|(k, v)| (k, v)));
        command
    }
}

const QUICK_FAILURE: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_millis(50);

pub struct Supervisor {
    child: ChildStart,
    log: DaemonLog,
    request_stop: Box<dyn Fn() + Send + Sync>,
    pub restart_delay: Duration,
}

impl Supervisor {
    pub fn new(
        child: ChildStart,
        log: DaemonLog,
        request_stop: Box<dyn Fn() + Send + Sync>,
    ) -> Self {
        Self {
            child,
            log,
            request_stop,
            restart_delay: Duration::from_millis(500),
        }
    }

    /// `Supervisor.Run`: the exit code of the supervisor; `stop` set ends it after the daemon.
    /// Under Windows, a job object that cannot be made is `invalid_storage_or_path`, as the
    /// `InvalidOperationException` of the oracle that `CommandRunner` maps.
    pub fn run(&self, stop: &AtomicBool) -> Result<i32> {
        let code = self.supervise(stop);
        self.log.flush_before_exit();
        code
    }

    fn supervise(&self, stop: &AtomicBool) -> Result<i32> {
        #[cfg(windows)]
        let job = windows_job()?;
        let mut quick_failures = 0;
        let mut restarts: Vec<Instant> = Vec::new();
        loop {
            let alive = Instant::now();
            let Ok(mut daemon) = self.child.command().spawn() else {
                // `Process.Start` throws: the supervisor ends as a failed process.
                self.log
                    .write("supervised_give_up", Some("not_started"), None, None);
                return Ok(1);
            };
            // `job.Assign(daemon)`: the oracle does not check the result either.
            #[cfg(windows)]
            let _ = job
                .assign_process(std::os::windows::io::AsRawHandle::as_raw_handle(&daemon) as isize);
            let pid = i32::try_from(daemon.id()).ok();
            self.log.write("supervised_start", None, pid, None);
            let status = loop {
                if stop.load(Ordering::SeqCst) {
                    self.stop_daemon(&mut daemon);
                    return Ok(0);
                }
                match daemon.try_wait() {
                    Ok(Some(status)) => break status,
                    Ok(None) => thread::sleep(POLL),
                    Err(_) => {
                        // The state of the daemon is unknown: stop it rather than leave it alone.
                        self.stop_daemon(&mut daemon);
                        self.log
                            .write("supervised_give_up", Some("wait_failed"), pid, None);
                        return Ok(1);
                    }
                }
            };
            let code = exit_code(status);
            if code == 0 {
                self.log.write("supervised_stop", None, None, None);
                return Ok(0);
            }
            let lived = alive.elapsed();
            let exit = format!("exit_{code}");
            self.log.write(
                "supervised_exit",
                Some(&exit),
                pid,
                i64::try_from(lived.as_millis()).ok(),
            );
            quick_failures = if lived < QUICK_FAILURE {
                quick_failures + 1
            } else {
                0
            };
            if quick_failures >= 3 {
                self.log
                    .write("supervised_give_up", Some(&exit), None, None);
                return Ok(code);
            }
            let now = Instant::now();
            restarts.push(now);
            restarts.retain(|at| now - *at <= Duration::from_secs(60));
            // More than five restarts in a minute: slow down rather than spin.
            let delay = if restarts.len() > 5 {
                Duration::from_secs(5)
            } else {
                self.restart_delay
            };
            let waited = Instant::now();
            while waited.elapsed() < delay {
                if stop.load(Ordering::SeqCst) {
                    return Ok(0);
                }
                thread::sleep(POLL.min(delay));
            }
        }
    }

    /// The daemon drains its started commands before it exits (ADR 0016); killed after 5 s.
    // ponytail: kills the daemon, not its children (`Kill(true)` of .NET); the daemon runs its
    // commands in its own process.
    fn stop_daemon(&self, daemon: &mut Child) {
        (self.request_stop)();
        let waited = Instant::now();
        while waited.elapsed() < Duration::from_secs(5) {
            // A failed wait proves nothing: wait on, then kill.
            if matches!(daemon.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(POLL);
        }
        let _ = daemon.kill();
        let _ = daemon.wait();
    }
}

/// `WindowsJob.Create`: a job object that kills its processes when its last handle closes, that
/// is when the supervisor ends, even when it is killed.
#[cfg(windows)]
fn windows_job() -> Result<win32job::Job> {
    let mut limits = win32job::ExtendedLimitInfo::new();
    limits.limit_kill_on_job_close();
    win32job::Job::create_with_limit_info(&limits)
        .map_err(|_| sermofur_core::instance::invalid_storage())
}
