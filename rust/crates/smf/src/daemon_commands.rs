//! `smf daemon …`, as `DaemonCommands.Run`: `register`, `unregister`, `instances`, `status`
//! (lot 7.6) and `install`, `uninstall`, `start`, `stop`, `restart` (lot 7.7), always run by the
//! command line itself, never through the daemon. `run` is [`crate::daemon_run`]. Text and
//! `--json` print the same JSON, as `CommandRunner.Write`.

use std::io::Write;
use std::path::Path;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use sermofur_core::VERSION;
use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::get_full_path;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::DaemonClient;
use sermofur_daemon::endpoint;
use sermofur_daemon::file_turn::is_held;
use sermofur_daemon::framing::{self, IpcMessage, MAX_BODY_BYTES, kind};
use sermofur_daemon::registry::{InstanceRegistry, RegisteredInstance, RegistryEntry};
use sermofur_daemon::server::read_message;
use sermofur_daemon::services::{self, ServiceDefinition, ServiceManager, ServiceStatus};
use tokio::io::AsyncWriteExt;

use crate::args::CommandArguments;
use crate::router::{block_on, temporary_folder};
use crate::runner::{combine, to_json, write_line};

const SUBCOMMANDS: &str =
    "install, uninstall, start, stop, restart, status, register, unregister, instances, run";

/// `DaemonCommands.DefaultStatusTimeout`: how long a daemon that welcomed the command line has
/// to answer a status.
pub const DEFAULT_STATUS_TIMEOUT: Duration = Duration::from_secs(5);

/// `DaemonCommands.StartTimeout`: how long a started service has to answer.
pub const DEFAULT_START_TIMEOUT: Duration = Duration::from_secs(15);

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
        "status" | "install" | "uninstall" | "start" | "stop" | "restart" => {
            let manager = services::for_current_system(&paths);
            let mut pilot = Pilot {
                paths: &paths,
                manager: manager.as_ref(),
                start_timeout: DEFAULT_START_TIMEOUT,
                status_timeout: DEFAULT_STATUS_TIMEOUT,
                // `error: json ? null : error`.
                progress: (!json).then_some(err),
            };
            to_json(&pilot.run(&subcommand)?)?
        }
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

/// The piloting subcommands of `DaemonCommands`, on one manager and one set of paths.
pub(crate) struct Pilot<'a> {
    pub paths: &'a DaemonPaths,
    pub manager: &'a dyn ServiceManager,
    pub start_timeout: Duration,
    pub status_timeout: Duration,
    /// The `error` writer of .NET: `None` in JSON.
    pub progress: Option<&'a mut dyn Write>,
}

impl Pilot<'_> {
    pub fn run(&mut self, subcommand: &str) -> Result<StatusBody> {
        match subcommand {
            "install" => self.install(),
            "uninstall" => self.uninstall(),
            "start" => self.start(),
            "stop" => self.stop(),
            "restart" => {
                self.stop()?;
                self.start()
            }
            _ => self.status(),
        }
    }

    fn say(&mut self, line: &str) {
        if let Some(progress) = self.progress.as_deref_mut() {
            write_line(progress, line);
        }
    }

    fn status(&self) -> Result<StatusBody> {
        status(self.paths, self.manager, self.status_timeout)
    }

    /// `DaemonCommands.Install`.
    fn install(&mut self) -> Result<StatusBody> {
        self.usable()?;
        let (mut dropped, mut narrowed) = (Vec::new(), Vec::new());
        let definition = ServiceDefinition::for_current_process(
            VERSION,
            cfg!(windows),
            &mut |name| dropped.push(name.to_owned()),
            &mut |name| narrowed.push(name.to_owned()),
        )?;
        for name in dropped {
            self.say(&format!(
                "smf: {name} is not carried to the service: its value is not an absolute path"
            ));
        }
        for name in narrowed {
            self.say(&format!(
                "smf: {name} is carried to the service without the entries that are not absolute paths"
            ));
        }
        if !self.current(&definition)? {
            // A definition the manager cannot run is refused before the running daemon stops.
            self.manager.validate(&definition)?;
            // Whatever its version: only a daemon that ran is started again after a failure.
            let was_running = self.daemon_runs();
            self.request_shutdown();
            // Written first: the Windows supervisor reads it as soon as the task starts.
            let previous = services::read_installed(self.paths)?;
            services::write_installed(self.paths, &definition)?;
            if let Err(error) = self.manager.install(&definition, previous.as_ref()) {
                match previous {
                    None => services::delete_installed(self.paths)?,
                    Some(previous) => {
                        // Restored before the previous service starts: its supervisor reads it.
                        services::write_installed(self.paths, &previous)?;
                        if was_running {
                            self.restart_previous();
                        }
                    }
                }
                return Err(error);
            }
            self.wait_for_daemon()?;
        }
        self.status()
    }

    /// The `current` of `Install`: installed, the same definition, and a daemon of this version
    /// that answers. A daemon of another version is not current (E4 corrected, as `DaemonRuns`):
    /// it is replaced instead of failing the install.
    fn current(&self, definition: &ServiceDefinition) -> Result<bool> {
        if !self.manager.query().installed
            || !services::same(services::read_installed(self.paths)?.as_ref(), definition)
        {
            return Ok(false);
        }
        match answer(self.paths, self.status_timeout) {
            Ok(Some((version, status))) => Ok(status.is_some() && version == VERSION),
            Ok(None) => Ok(false),
            Err(error) if error.code == "daemon_version_mismatch" => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// `DaemonCommands.RestartPrevious`: a failure here is a line, never the error of the
    /// install.
    fn restart_previous(&mut self) {
        let outcome = (|| -> Result<bool> {
            if !self.manager.query().installed {
                return Ok(false);
            }
            if !self.daemon_runs() {
                self.manager.start()?;
            }
            // The previous daemon usually runs an older version: any answer counts.
            let waited = Instant::now();
            while !self.daemon_runs() {
                if waited.elapsed() >= self.start_timeout {
                    return Err(SermofurError::new(
                        "daemon_unavailable",
                        format!(
                            "no daemon answered within {} s",
                            self.start_timeout.as_secs_f64().round()
                        ),
                        3,
                    ));
                }
                thread::sleep(Duration::from_millis(200));
            }
            Ok(true)
        })();
        match outcome {
            Ok(true) => {}
            Ok(false) => {
                self.say("smf: no service is installed any more; run: smf daemon install");
            }
            Err(error) => self.say(&format!(
                "smf: the previous service could not be started again ({}); run: smf daemon start",
                error.message
            )),
        }
    }

    /// `DaemonCommands.DaemonRuns`: a daemon answers, whatever its version.
    fn daemon_runs(&self) -> bool {
        match answer(self.paths, self.status_timeout) {
            Ok(answer) => answer.is_some(),
            Err(error) => error.code == "daemon_version_mismatch",
        }
    }

    fn uninstall(&mut self) -> Result<StatusBody> {
        self.request_shutdown();
        self.manager.uninstall()?;
        services::delete_installed(self.paths)?;
        self.status()
    }

    fn start(&mut self) -> Result<StatusBody> {
        self.installed()?;
        if answer(self.paths, self.status_timeout)?.is_none() {
            self.manager.start()?;
            self.wait_for_daemon()?;
        }
        self.status()
    }

    fn stop(&mut self) -> Result<StatusBody> {
        self.installed()?;
        self.request_shutdown();
        self.manager.stop()?;
        self.status()
    }

    fn usable(&self) -> Result<()> {
        match self.manager.unavailable() {
            None => Ok(()),
            Some(reason) => Err(SermofurError::new(
                "service_manager_unavailable",
                format!(
                    "Cannot install the daemon as a service: {reason} The CLI keeps working directly; smf daemon run serves in the foreground."
                ),
                3,
            )),
        }
    }

    fn installed(&self) -> Result<()> {
        self.usable()?;
        if self.manager.query().installed {
            Ok(())
        } else {
            Err(SermofurError::new(
                "daemon_unavailable",
                "The daemon is not installed; run: smf daemon install",
                3,
            ))
        }
    }

    /// `DaemonCommands.RequestShutdown`.
    fn request_shutdown(&mut self) {
        let progress = &mut self.progress;
        shutdown(self.paths, &mut |line| {
            if let Some(progress) = progress.as_deref_mut() {
                write_line(progress, line);
            }
        });
    }

    /// `DaemonCommands.WaitForDaemon`.
    fn wait_for_daemon(&self) -> Result<()> {
        let waited = Instant::now();
        while waited.elapsed() < self.start_timeout {
            if connect(self.paths, Duration::from_millis(500))? {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(200));
        }
        Err(SermofurError::new(
            "daemon_unavailable",
            format!(
                "The service is registered but the daemon did not answer within {} s; see the journal {}.",
                self.start_timeout.as_secs_f64().round(),
                self.paths.log_file().display()
            ),
            3,
        ))
    }
}

/// `DaemonCommands.Connect`: a daemon of this version welcomed the command line.
fn connect(paths: &DaemonPaths, timeout: Duration) -> Result<bool> {
    let folder = temporary_folder();
    block_on(async {
        DaemonClient::connect(paths, &SystemOwnership, VERSION, &folder, timeout)
            .await
            .map(|client| client.is_some())
    })?
}

/// `DaemonCommands.ShutdownAsync`: asks the daemon to stop and waits for its lock, the stop
/// timeout at most; `false` when it did not exit in time. The lines go to `progress`.
pub(crate) fn shutdown(paths: &DaemonPaths, progress: &mut dyn FnMut(&str)) -> bool {
    let limit = services::stop_timeout();
    let waited = Instant::now();
    let mut delivered = ask_to_stop(paths, Duration::from_secs(1));
    let mut told = false;
    while is_held(&paths.lock_file()) {
        if waited.elapsed() >= limit {
            progress(&format!(
                "smf: the daemon did not exit within {} s; it may be stopped by force.",
                limit.as_secs()
            ));
            return false;
        }
        if !told && waited.elapsed() >= Duration::from_secs(1) {
            progress(&format!(
                "smf: waiting for the daemon to finish its running commands ({} s at most)...",
                limit.as_secs()
            ));
            told = true;
        }
        if !delivered {
            delivered = ask_to_stop(paths, Duration::from_millis(200));
        }
        thread::sleep(Duration::from_millis(if delivered { 100 } else { 500 }));
    }
    true
}

/// `DaemonCommands.AskToStopAsync`: `true` when the daemon answered `stopping`.
fn ask_to_stop(paths: &DaemonPaths, timeout: Duration) -> bool {
    block_on(async {
        // Not an endpoint of ours, or nothing there: nothing to ask.
        let Ok(Some(mut stream)) = endpoint::connect(paths, &SystemOwnership, timeout).await else {
            return false;
        };
        let Ok(frame) = framing::encode(&IpcMessage::new(kind::SHUTDOWN), MAX_BODY_BYTES) else {
            return false;
        };
        if stream.write_all(&frame).await.is_err() || stream.flush().await.is_err() {
            return false;
        }
        // A daemon already stopping closes the connection without a word.
        let reply = tokio::time::timeout(
            Duration::from_secs(2),
            read_message(&mut stream, MAX_BODY_BYTES),
        )
        .await;
        matches!(reply, Ok(Ok(Some(reply))) if reply.kind == kind::STOPPING)
    })
    .unwrap_or(false)
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

        fn install(&self, _: &ServiceDefinition, _: Option<&ServiceDefinition>) -> Result<()> {
            Ok(())
        }

        fn uninstall(&self) -> Result<()> {
            Ok(())
        }

        fn start(&self) -> Result<()> {
            Ok(())
        }

        fn stop(&self) -> Result<()> {
            Ok(())
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

/// Ports of `ServiceTests` on a simulated service manager, as the .NET `FakeManager`: its
/// daemon is a real `DaemonServer` in this process, on the paths of a scratch home. No real
/// service is installed.
#[cfg(test)]
mod pilot_tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
    use std::thread::JoinHandle;

    use sermofur_daemon::file_turn::{acquire, take};
    use sermofur_daemon::log::DaemonLog;
    use sermofur_daemon::server::{DaemonLimits, DaemonServer, ServingGate};
    use sermofur_daemon::services::{delete_installed, read_installed, write_installed};

    use super::*;
    use crate::daemon_run::CliCommandExecutor;
    use crate::test_support::{Behavior, Scratch, StubDaemon};

    /// `TestDaemonOnPaths`: a daemon server on a thread, holding the daemon lock until it has
    /// exited when `holds_lock`.
    struct OnPaths {
        server: Arc<DaemonServer>,
        done: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl OnPaths {
        fn start(paths: &DaemonPaths, version: &str, holds_lock: bool) -> Self {
            let server = Arc::new(DaemonServer::new(
                paths.clone(),
                version,
                Arc::new(CliCommandExecutor),
                ServingGate::new(InstanceRegistry::new(&paths.registry_file())),
                DaemonLog::new(&paths.log_file()),
                DaemonLimits::default(),
            ));
            let lock = holds_lock.then(|| acquire(paths).expect("the daemon lock"));
            let done = Arc::new(AtomicBool::new(false));
            let (ready, listening) = std::sync::mpsc::channel();
            let (serving, ended) = (Arc::clone(&server), Arc::clone(&done));
            let thread = std::thread::spawn(move || {
                let _ = block_on(serving.run(&SystemOwnership, || {
                    let _ = ready.send(());
                }));
                drop(lock);
                ended.store(true, Ordering::SeqCst);
            });
            let _ = listening.recv_timeout(Duration::from_secs(10));
            Self {
                server,
                done,
                thread: Some(thread),
            }
        }

        fn served(&self) -> bool {
            self.done.load(Ordering::SeqCst)
        }
    }

    impl Drop for OnPaths {
        fn drop(&mut self) {
            self.server.stop();
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// `FakeManager`.
    #[derive(Default)]
    struct Fake {
        paths: Option<DaemonPaths>,
        reason: Option<String>,
        fail_install: bool,
        rejects: bool,
        daemon_version: Option<String>,
        installed: AtomicBool,
        stops: AtomicU32,
        starts: AtomicU32,
        installs: AtomicU32,
        daemon: Mutex<Option<OnPaths>>,
        before_start: Mutex<Option<Box<dyn FnMut() + Send>>>,
    }

    impl Fake {
        fn on(scratch: &Scratch) -> Self {
            Self {
                paths: Some(scratch.paths.clone()),
                ..Self::default()
            }
        }

        fn installed(self) -> Self {
            self.installed.store(true, Ordering::SeqCst);
            self
        }
    }

    impl ServiceManager for Fake {
        fn name(&self) -> &'static str {
            "simulated"
        }

        fn unavailable(&self) -> Option<String> {
            self.reason.clone()
        }

        fn query(&self) -> ServiceStatus {
            ServiceStatus {
                installed: self.installed.load(Ordering::SeqCst),
                running: self.daemon.lock().expect("the daemon").is_some(),
            }
        }

        fn validate(&self, _: &ServiceDefinition) -> Result<()> {
            if self.rejects {
                return Err(SermofurError::new(
                    "service_install_failed",
                    "simulated rejection",
                    3,
                ));
            }
            Ok(())
        }

        fn install(&self, _: &ServiceDefinition, _: Option<&ServiceDefinition>) -> Result<()> {
            if self.fail_install {
                return Err(SermofurError::new(
                    "service_install_failed",
                    "simulated refusal",
                    3,
                ));
            }
            self.installs.fetch_add(1, Ordering::SeqCst);
            self.installed.store(true, Ordering::SeqCst);
            self.start()
        }

        fn uninstall(&self) -> Result<()> {
            self.stop()?;
            self.installed.store(false, Ordering::SeqCst);
            Ok(())
        }

        /// A daemon stopped by a shutdown request is started anew, as a service manager would.
        fn start(&self) -> Result<()> {
            let gone = self
                .daemon
                .lock()
                .expect("the daemon")
                .as_ref()
                .is_none_or(OnPaths::served);
            if gone {
                if let Some(before) = self.before_start.lock().expect("the hook").as_mut() {
                    before();
                }
                self.starts.fetch_add(1, Ordering::SeqCst);
                self.daemon.lock().expect("the daemon").take();
                let version = self.daemon_version.as_deref().unwrap_or(VERSION);
                let paths = self.paths.as_ref().expect("the paths");
                *self.daemon.lock().expect("the daemon") =
                    Some(OnPaths::start(paths, version, true));
            }
            Ok(())
        }

        fn stop(&self) -> Result<()> {
            self.stops.fetch_add(1, Ordering::SeqCst);
            self.daemon.lock().expect("the daemon").take();
            Ok(())
        }
    }

    /// `DaemonWith`: the subcommand on the fake, its JSON or its error code and exit code.
    fn pilot<'a>(
        scratch: &'a Scratch,
        manager: &'a Fake,
        subcommand: &str,
        progress: Option<&'a mut (dyn Write + 'a)>,
    ) -> std::result::Result<serde_json::Value, (String, i32)> {
        Pilot {
            paths: &scratch.paths,
            manager,
            start_timeout: Duration::from_secs(10),
            status_timeout: Duration::from_secs(1),
            progress,
        }
        .run(subcommand)
        .map(|body| serde_json::from_str(&to_json(&body).expect("JSON")).expect("an object"))
        .map_err(|error| (error.code, error.exit_code))
    }

    fn state(value: &serde_json::Value) -> (&str, bool, Option<&str>) {
        (
            value["state"].as_str().expect("a state"),
            value["running"].as_bool().expect("running"),
            value["version"].as_str(),
        )
    }

    fn connect_as(paths: &DaemonPaths, version: &str) -> bool {
        block_on(async {
            DaemonClient::connect(
                paths,
                &SystemOwnership,
                version,
                &temporary_folder(),
                Duration::from_millis(500),
            )
            .await
            .is_ok_and(|client| client.is_some())
        })
        .expect("a runtime")
    }

    fn old_definition(environment: Vec<(String, Option<String>)>) -> ServiceDefinition {
        ServiceDefinition {
            executable: "/opt/old/smf".to_owned(),
            arguments: vec!["daemon".to_owned(), "run".to_owned()],
            version: "0.3.9".to_owned(),
            environment,
        }
    }

    /// The lock of a daemon that drains, held by the test.
    fn hold_the_lock(paths: &DaemonPaths) -> fs::File {
        fs::create_dir_all(&paths.state_directory).expect("the state folder");
        take(&paths.lock_file()).expect("the lock")
    }

    /// Port of `UnavailableManagerRefusesInstallAndWritesNothing`.
    #[test]
    fn an_unavailable_manager_refuses_install_and_writes_nothing() {
        let scratch = Scratch::new("unavailable");
        let manager = Fake {
            reason: Some("no systemd user manager in this session.".to_owned()),
            ..Fake::on(&scratch)
        };
        assert_eq!(
            pilot(&scratch, &manager, "install", None),
            Err(("service_manager_unavailable".to_owned(), 3))
        );
        assert_eq!(manager.installs.load(Ordering::SeqCst), 0);
        assert_eq!(read_installed(&scratch.paths).expect("read"), None);
    }

    /// Port of `PilotingANotInstalledServiceIsDaemonUnavailable` (start, stop, restart).
    #[test]
    fn piloting_a_service_not_installed_is_daemon_unavailable() {
        let scratch = Scratch::new("not-installed");
        for subcommand in ["start", "stop", "restart"] {
            assert_eq!(
                pilot(&scratch, &Fake::on(&scratch), subcommand, None),
                Err(("daemon_unavailable".to_owned(), 3)),
                "{subcommand}"
            );
        }
    }

    /// Port of `InstallIsIdempotentAndUninstallKeepsTheData`.
    #[test]
    fn install_is_idempotent_and_uninstall_keeps_the_data() {
        let scratch = Scratch::new("idempotent");
        let registry = InstanceRegistry::new(&scratch.paths.registry_file());
        registry
            .register(&scratch.root, DateTimeOffset::now_local())
            .expect("registered");
        let registered = fs::read(scratch.paths.registry_file()).expect("the registry");
        let data = fs::read(scratch.root.join(".sermofur").join("config.toml")).ok();
        let manager = Fake::on(&scratch);
        let installed = pilot(&scratch, &manager, "install", None).expect("installed");
        assert_eq!(state(&installed), ("running", true, Some(VERSION)));
        assert_eq!(manager.installs.load(Ordering::SeqCst), 1);
        let again = pilot(&scratch, &manager, "install", None).expect("installed");
        assert_eq!(state(&again), ("running", true, Some(VERSION)));
        assert_eq!(manager.installs.load(Ordering::SeqCst), 1);
        for (subcommand, expected) in [
            ("status", "running"),
            ("stop", "installed_stopped"),
            ("start", "running"),
            ("restart", "running"),
            ("uninstall", "absent"),
            ("uninstall", "absent"),
        ] {
            let body = pilot(&scratch, &manager, subcommand, None).expect(subcommand);
            assert_eq!(state(&body).0, expected, "{subcommand}");
        }
        assert_eq!(
            fs::read(scratch.paths.registry_file()).ok(),
            Some(registered)
        );
        assert_eq!(
            fs::read(scratch.root.join(".sermofur").join("config.toml")).ok(),
            data
        );
        assert_eq!(read_installed(&scratch.paths).expect("read"), None);
    }

    /// Port of `FailedRegistrationLeavesNoDefinition`.
    #[test]
    fn a_failed_registration_leaves_no_definition() {
        let scratch = Scratch::new("failed-registration");
        let manager = Fake {
            fail_install: true,
            ..Fake::on(&scratch)
        };
        assert_eq!(
            pilot(&scratch, &manager, "install", None),
            Err(("service_install_failed".to_owned(), 3))
        );
        assert_eq!(read_installed(&scratch.paths).expect("read"), None);
    }

    /// Port of `FailedReinstallRestoresThePreviousDefinition`.
    #[test]
    fn a_failed_reinstall_restores_the_previous_definition() {
        let scratch = Scratch::new("failed-reinstall");
        let previous = old_definition(vec![(
            "DOTNET_ROOT".to_owned(),
            Some("/opt/dotnet".to_owned()),
        )]);
        write_installed(&scratch.paths, &previous).expect("written");
        let manager = Fake {
            fail_install: true,
            ..Fake::on(&scratch)
        };
        assert_eq!(
            pilot(&scratch, &manager, "install", None).map_err(|e| e.0),
            Err("service_install_failed".to_owned())
        );
        assert_eq!(
            read_installed(&scratch.paths).expect("read"),
            Some(previous)
        );
    }

    /// Port of `RefusedDefinitionLeavesTheRunningDaemonAlone`.
    #[test]
    fn a_refused_definition_leaves_the_running_daemon_alone() {
        let scratch = Scratch::new("refused");
        let running = Fake::on(&scratch);
        running.start().expect("started");
        let refusing = Fake {
            rejects: true,
            ..Fake::on(&scratch)
        };
        assert_eq!(
            pilot(&scratch, &refusing, "install", None),
            Err(("service_install_failed".to_owned(), 3))
        );
        assert!(connect_as(&scratch.paths, VERSION));
        assert_eq!(read_installed(&scratch.paths).expect("read"), None);
    }

    /// Port of `StopWaitsForADrainingDaemonBeforeTheServiceManager`.
    #[test]
    fn stop_waits_for_a_draining_daemon_before_the_service_manager() {
        let scratch = Scratch::new("draining");
        let draining = hold_the_lock(&scratch.paths);
        let manager = Fake::on(&scratch).installed();
        std::thread::scope(|scope| {
            let stop = scope.spawn(|| pilot(&scratch, &manager, "stop", None));
            std::thread::sleep(Duration::from_millis(800));
            assert!(!stop.is_finished());
            assert_eq!(manager.stops.load(Ordering::SeqCst), 0);
            drop(draining);
            assert!(stop.join().expect("the stop").is_ok());
        });
        assert_eq!(manager.stops.load(Ordering::SeqCst), 1);
    }

    /// Port of `StopSaysItIsWaitingOnStderrOnly`.
    #[test]
    fn stop_says_it_is_waiting_on_stderr_only() {
        let scratch = Scratch::new("waiting-line");
        let draining = hold_the_lock(&scratch.paths);
        let manager = Fake::on(&scratch).installed();
        let text = Mutex::new(Vec::new());
        struct Shared<'a>(&'a Mutex<Vec<u8>>);
        impl Write for Shared<'_> {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().expect("the text").extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let said = || String::from_utf8_lossy(&text.lock().expect("the text")).into_owned();
        let body = std::thread::scope(|scope| {
            let stop = scope.spawn(|| {
                let mut progress = Shared(&text);
                pilot(&scratch, &manager, "stop", Some(&mut progress))
            });
            // The lock is held until the line shows: no race with the one-second threshold.
            let waited = Instant::now();
            while !said().contains("waiting for the daemon") && waited.elapsed() < LONG {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert_eq!(manager.stops.load(Ordering::SeqCst), 0);
            drop(draining);
            stop.join().expect("the stop")
        });
        assert!(said().contains("waiting for the daemon to finish its running commands"));
        // The JSON on stdout stays one object.
        assert_eq!(body.expect("stopped")["state"], "installed_stopped");
    }

    const LONG: Duration = Duration::from_secs(15);

    /// Port of `StopReachesADaemonThatStartsListeningLate`.
    #[test]
    fn stop_reaches_a_daemon_that_starts_listening_late() {
        let scratch = Scratch::new("late");
        let starting = hold_the_lock(&scratch.paths);
        let manager = Fake::on(&scratch).installed();
        std::thread::scope(|scope| {
            let stop = scope.spawn(|| pilot(&scratch, &manager, "stop", None));
            std::thread::sleep(Duration::from_millis(300));
            // The daemon holds its lock but only now opens its endpoint.
            let late = OnPaths::start(&scratch.paths, VERSION, false);
            assert_eq!(manager.stops.load(Ordering::SeqCst), 0);
            drop(starting);
            assert!(stop.join().expect("the stop").is_ok());
            drop(late);
        });
        assert_eq!(manager.stops.load(Ordering::SeqCst), 1);
    }

    /// Port of `FailedUpgradeStartsThePreviousOlderDaemonAgainWithoutAWarning`.
    #[test]
    fn a_failed_upgrade_starts_the_previous_older_daemon_again_without_a_warning() {
        let scratch = Scratch::new("failed-upgrade");
        write_installed(&scratch.paths, &old_definition(Vec::new())).expect("written");
        let manager = Fake {
            fail_install: true,
            daemon_version: Some("0.3.9".to_owned()),
            ..Fake::on(&scratch).installed()
        };
        manager.start().expect("started");
        let mut progress = Vec::new();
        let result = pilot(&scratch, &manager, "install", Some(&mut progress));
        assert_eq!(
            result.map_err(|e| e.0),
            Err("service_install_failed".to_owned())
        );
        // Started once before the upgrade, once again after its failure.
        assert_eq!(manager.starts.load(Ordering::SeqCst), 2);
        assert!(!String::from_utf8_lossy(&progress).contains("could not be started again"));
        assert!(connect_as(&scratch.paths, "0.3.9"));
    }

    /// Port of `FailedReinstallLeavesAStoppedServiceStopped`.
    #[test]
    fn a_failed_reinstall_leaves_a_stopped_service_stopped() {
        let scratch = Scratch::new("stopped-stays");
        write_installed(&scratch.paths, &old_definition(Vec::new())).expect("written");
        let manager = Fake {
            fail_install: true,
            ..Fake::on(&scratch).installed()
        };
        let result = pilot(&scratch, &manager, "install", None);
        assert_eq!(
            result.map_err(|e| e.0),
            Err("service_install_failed".to_owned())
        );
        assert_eq!(manager.starts.load(Ordering::SeqCst), 0);
        assert!(!connect_as(&scratch.paths, VERSION));
    }

    /// Port of `InstallReplacesADaemonThatDoesNotAnswer`.
    #[test]
    fn install_replaces_a_daemon_that_does_not_answer() {
        let scratch = Scratch::new("replace-silent");
        let manager = Fake::on(&scratch);
        pilot(&scratch, &manager, "install", None).expect("installed");
        // The installed daemon is replaced by one that welcomes clients and answers nothing.
        manager.stop().expect("stopped");
        let silent = Arc::new(Mutex::new(Some(StubDaemon::start(
            &scratch.paths,
            Behavior::Silent,
            false,
        ))));
        let silencing = Arc::clone(&silent);
        *manager.before_start.lock().expect("the hook") = Some(Box::new(move || {
            silencing.lock().expect("the stub").take();
        }));
        let body = pilot(&scratch, &manager, "install", None).expect("installed");
        assert_eq!(manager.installs.load(Ordering::SeqCst), 2);
        assert_eq!(body["state"], "running");
        assert_eq!(body["answering"], true);
        silent.lock().expect("the stub").take();
    }

    /// E4 corrected: a daemon of another version under the same definition is replaced, as a
    /// daemon that does not answer, instead of failing with `daemon_version_mismatch`.
    #[test]
    fn install_replaces_a_daemon_of_another_version_under_the_same_definition() {
        let scratch = Scratch::new("e4");
        let definition = ServiceDefinition::for_current_process(
            VERSION,
            cfg!(windows),
            &mut |_| {},
            &mut |_| {},
        )
        .expect("a definition");
        write_installed(&scratch.paths, &definition).expect("written");
        let older = Fake {
            daemon_version: Some("0.0.1".to_owned()),
            ..Fake::on(&scratch).installed()
        };
        older.start().expect("started");
        let manager = Fake::on(&scratch).installed();
        let body = pilot(&scratch, &manager, "install", None).expect("installed");
        assert_eq!(manager.installs.load(Ordering::SeqCst), 1);
        assert_eq!(state(&body), ("running", true, Some(VERSION)));
        drop(older);
    }

    /// Port of `ServiceHardeningTests.RemovingWhatWasNeverInstalledSucceeds`, the definition
    /// part: nothing installed, not even the state folder, is nothing to remove.
    #[test]
    fn deleting_a_definition_never_written_creates_nothing() {
        let scratch = Scratch::new("never-installed");
        delete_installed(&scratch.paths).expect("nothing to remove");
        assert!(!scratch.paths.state_directory.exists());
    }
}
