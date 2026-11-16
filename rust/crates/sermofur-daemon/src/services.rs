//! The service manager of the user session, as `Sermofur.Daemon.Services` of the .NET oracle:
//! the definition of what the service runs (`ServiceDefinition`), the copy kept beside the
//! journal (`InstalledDefinition`), and the three managers: scheduled task, systemd user
//! service, launchd agent. Their command-line tools run through a [`ProcessRunner`], replaced
//! by a fake one in the tests: no test installs a real service.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::NEW_LINE;
use sermofur_core::atomic::{self, WriteOptions};
use sermofur_core::daemon_paths::{
    DaemonPaths, HOME_VARIABLE, SpecialFolder, environment_text, system_folder,
};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::json::quoted_basic_latin;
use sermofur_core::local_paths::is_path_fully_qualified;
use sermofur_core::stj::{self, Node};

use crate::file_turn::{self, FileError};
use crate::registry::lenient_text;
use crate::server::DaemonLimits;

/// `ServiceDefinition`: what the service runs, the smf that installed it with `daemon run`.
/// The environment keeps the order of its variables, as the `Dictionary` of .NET does; a value
/// is `None` only when a `service.json` read from disk holds a `null`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceDefinition {
    pub executable: String,
    pub arguments: Vec<String>,
    pub version: String,
    pub environment: Vec<(String, Option<String>)>,
}

/// `ServiceDefinition.CarriedVariables`: copied from the installing process when they are set.
pub const CARRIED_VARIABLES: [&str; 3] = ["DOTNET_ROOT", "PATH", HOME_VARIABLE];

const PATH_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

impl ServiceDefinition {
    /// `ServiceDefinition.ForCurrentProcess`: the running smf with `daemon run` (and
    /// `--supervise`), and the carried variables of this process. `dropped` gets the name of a
    /// variable left out, `narrowed` the name of a PATH kept with fewer entries; never a value.
    /// The port is a native executable: the `dotnet Sermofur.Cli.dll` case of .NET has no
    /// equivalent.
    pub fn for_current_process(
        version: &str,
        supervise: bool,
        dropped: &mut dyn FnMut(&str),
        narrowed: &mut dyn FnMut(&str),
    ) -> Result<Self> {
        let executable = std::env::current_exe().map_err(|_| {
            SermofurError::new("service_install_failed", "The path of smf is unknown.", 3)
        })?;
        let mut arguments = vec!["daemon".to_owned(), "run".to_owned()];
        if supervise {
            arguments.push("--supervise".to_owned());
        }
        let mut environment = Vec::new();
        for name in CARRIED_VARIABLES {
            let set = environment_text(name);
            match carried(name, set.as_deref(), narrowed) {
                Some(value) => environment.push((name.to_owned(), Some(value))),
                // Held by the installing shell, dropped here: the name is reported.
                None if set.is_some_and(|set| !set.is_empty()) => dropped(name),
                None => {}
            }
        }
        Ok(Self {
            executable: executable.to_string_lossy().into_owned(),
            arguments,
            version: version.to_owned(),
            environment,
        })
    }

    /// `CarriedEnvironment`: the carried variables of this definition only, each holding
    /// absolute paths, filtered again because `service.json` may have been edited.
    pub fn carried_environment(
        &self,
        dropped: &mut dyn FnMut(&str),
        narrowed: &mut dyn FnMut(&str),
    ) -> Vec<(String, String)> {
        let mut kept: Vec<(String, String)> = Vec::new();
        for (name, value) in &self.environment {
            let value = CARRIED_VARIABLES
                .contains(&name.as_str())
                .then(|| carried(name, value.as_deref(), narrowed))
                .flatten();
            match value {
                Some(value) => match kept.iter_mut().find(|(known, _)| known == name) {
                    Some(entry) => entry.1 = value,
                    None => kept.push((name.clone(), value)),
                },
                None => dropped(name),
            }
        }
        kept
    }
}

/// `ServiceDefinition.Carried`: an absolute path, or for PATH its absolute entries only.
pub fn carried(name: &str, value: Option<&str>, narrowed: &mut dyn FnMut(&str)) -> Option<String> {
    let value = value.filter(|value| !value.is_empty())?;
    if name != "PATH" {
        return is_path_fully_qualified(value).then(|| value.to_owned());
    }
    let held: Vec<&str> = value
        .split(PATH_SEPARATOR)
        .filter(|entry| !entry.is_empty())
        .collect();
    let entries: Vec<&str> = held
        .iter()
        .copied()
        .filter(|entry| is_path_fully_qualified(entry))
        .collect();
    if entries.is_empty() {
        return None;
    }
    if entries.len() != held.len() {
        narrowed(name);
    }
    Some(entries.join(&PATH_SEPARATOR.to_string()))
}

/// `ServiceManagers.Same`: same executable, version, arguments and environment, PATH left out
/// (compared without case): it differs from one shell to the next.
pub fn same(left: Option<&ServiceDefinition>, right: &ServiceDefinition) -> bool {
    let Some(left) = left else {
        return false;
    };
    let without_path = |definition: &ServiceDefinition| {
        let mut kept: Vec<(String, Option<String>)> = Vec::new();
        for (name, value) in &definition.environment {
            if name.eq_ignore_ascii_case("PATH") {
                continue;
            }
            kept.retain(|(known, _)| known != name);
            kept.push((name.clone(), value.clone()));
        }
        kept.sort();
        kept
    };
    left.executable == right.executable
        && left.version == right.version
        && left.arguments == right.arguments
        && without_path(left) == without_path(right)
}

/// State of the service as its manager sees it (`ServiceStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
}

/// A service manager of the user session (`IServiceManager`).
pub trait ServiceManager {
    /// Name shown by status: scheduled task, systemd user service, launchd agent.
    fn name(&self) -> &'static str;

    /// `None` when usable, or why not (container, WSL without systemd).
    fn unavailable(&self) -> Option<String>;

    fn query(&self) -> ServiceStatus;

    /// `service_install_failed` when this manager cannot run the definition; called before
    /// anything is stopped or written.
    fn validate(&self, _definition: &ServiceDefinition) -> Result<()> {
        Ok(())
    }

    /// Writes the definition, registers it and starts it. On failure, puts the previous
    /// registration back without starting it where the manager allows.
    fn install(
        &self,
        definition: &ServiceDefinition,
        previous: Option<&ServiceDefinition>,
    ) -> Result<()>;

    /// Stops and removes the service; nothing else.
    fn uninstall(&self) -> Result<()>;

    fn start(&self) -> Result<()>;

    fn stop(&self) -> Result<()>;
}

/// `IProcessRunner`: runs the command-line tools of the service managers.
pub trait ProcessRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> ProcessOutcome;
}

/// `ProcessOutcome`: exit code (-1 not started, -2 timed out), output and error.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProcessOutcome {
    pub exit_code: i32,
    pub output: String,
    pub error: String,
}

impl ProcessOutcome {
    pub fn new(exit_code: i32, output: &str, error: &str) -> Self {
        Self {
            exit_code,
            output: output.to_owned(),
            error: error.to_owned(),
        }
    }

    pub fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

/// `ServiceErrors.Failed`: `service_install_failed`, with the first line of the error, or of
/// the output when the error is blank.
fn failed(action: &str, outcome: &ProcessOutcome) -> SermofurError {
    let text = if outcome.error.trim().is_empty() {
        &outcome.output
    } else {
        &outcome.error
    };
    let line = text.trim().split('\n').next().unwrap_or_default().trim();
    SermofurError::new(
        "service_install_failed",
        format!("{action} failed (exit {}): {line}", outcome.exit_code),
        3,
    )
}

/// An I/O failure of a manager file, as the `IOException` of the oracle.
fn io(error: std::io::Error) -> SermofurError {
    FileError::from(error).into()
}

/// `ServiceManagers.ForCurrentSystem`, with the system tools.
pub fn for_current_system(paths: &DaemonPaths) -> Box<dyn ServiceManager> {
    let runner = Box::new(SystemProcessRunner);
    if cfg!(windows) {
        // ponytail: `USERDOMAIN\USERNAME` where .NET asks the system; they differ only if the
        // variables were changed.
        let user = format!(
            r"{}\{}",
            environment_text("USERDOMAIN").unwrap_or_default(),
            environment_text("USERNAME").unwrap_or_default()
        );
        Box::new(ScheduledTask::new(runner, &user, &paths.state_directory))
    } else if cfg!(target_os = "macos") {
        Box::new(LaunchAgent::new(
            runner,
            &Path::new(&system_folder(SpecialFolder::UserProfile))
                .join("Library")
                .join("LaunchAgents"),
            user_id(),
            &paths.state_directory,
        ))
    } else {
        // `XDG_CONFIG_HOME` when set and not empty, `~/.config` otherwise.
        let config = environment_text("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(
                || Path::new(&system_folder(SpecialFolder::UserProfile)).join(".config"),
                PathBuf::from,
            );
        Box::new(SystemdUser::new(
            runner,
            &config.join("systemd").join("user"),
        ))
    }
}

#[cfg(unix)]
fn user_id() -> u32 {
    sermofur_core::ownership::effective_user_id()
}

#[cfg(not(unix))]
fn user_id() -> u32 {
    0
}

/// `WindowsScheduledTask`: a task of the user session, started at logon.
pub struct ScheduledTask {
    runner: Box<dyn ProcessRunner>,
    user_id: String,
    work_directory: PathBuf,
    resolve: fn(&str) -> Option<PathBuf>,
}

pub const TASK_NAME: &str = r"Sermofur\Daemon";

impl ScheduledTask {
    pub fn new(runner: Box<dyn ProcessRunner>, user_id: &str, work_directory: &Path) -> Self {
        Self {
            runner,
            user_id: user_id.to_owned(),
            work_directory: work_directory.to_owned(),
            resolve: |name| resolve(name, None),
        }
    }

    /// The tool lookup of `Unavailable`, replaced in the tests.
    #[must_use]
    pub fn resolving(mut self, resolve: fn(&str) -> Option<PathBuf>) -> Self {
        self.resolve = resolve;
        self
    }

    fn schtasks(&self, arguments: &[&str]) -> ProcessOutcome {
        self.runner.run("schtasks", arguments)
    }

    fn create(&self, file: &Path) -> ProcessOutcome {
        let file = file.to_string_lossy();
        self.schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &file, "/F"])
    }

    /// `File.WriteAllText(file, xml, Encoding.Unicode)`: UTF-16 with its byte order mark.
    fn write_task(&self, file: &Path, definition: &ServiceDefinition) -> Result<()> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in task_xml(definition, &self.user_id).encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(file, bytes).map_err(io)
    }

    fn register(&self, file: &Path, previous: Option<&ServiceDefinition>) -> Result<()> {
        let created = self.create(file);
        if !created.succeeded() {
            return Err(failed("schtasks /Create", &created));
        }
        let started = self.schtasks(&["/Run", "/TN", TASK_NAME]);
        if !started.succeeded() {
            self.restore(previous, file);
            return Err(failed("schtasks /Run", &started));
        }
        Ok(())
    }

    /// Puts the previous task back without running it, or removes the new one.
    fn restore(&self, previous: Option<&ServiceDefinition>, file: &Path) {
        if let Some(previous) = previous
            && self.write_task(file, previous).is_ok()
            && self.create(file).succeeded()
        {
            return;
        }
        self.schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]);
    }
}

impl ServiceManager for ScheduledTask {
    fn name(&self) -> &'static str {
        "scheduled task"
    }

    fn unavailable(&self) -> Option<String> {
        (self.resolve)("schtasks")
            .is_none()
            .then(|| "schtasks.exe is not in the system folder of Windows.".to_owned())
    }

    fn query(&self) -> ServiceStatus {
        let query = self.schtasks(&["/Query", "/TN", TASK_NAME, "/FO", "CSV", "/NH"]);
        ServiceStatus {
            installed: query.succeeded(),
            running: query.succeeded() && task_running(&query.output),
        }
    }

    fn validate(&self, definition: &ServiceDefinition) -> Result<()> {
        if definition
            .environment
            .iter()
            .any(|(name, _)| name == HOME_VARIABLE)
        {
            // The task starts without the variables of this shell.
            return Err(SermofurError::new(
                "service_install_failed",
                format!(
                    "{HOME_VARIABLE} is set; the scheduled task cannot carry it. Unset it to install the service, or run the daemon by hand with smf daemon run."
                ),
                3,
            ));
        }
        Ok(())
    }

    fn install(
        &self,
        definition: &ServiceDefinition,
        previous: Option<&ServiceDefinition>,
    ) -> Result<()> {
        self.validate(definition)?;
        fs::create_dir_all(&self.work_directory).map_err(io)?;
        let file = self.work_directory.join("task.xml");
        self.write_task(&file, definition)?;
        let registered = self.register(&file, previous);
        let removed = fs::remove_file(&file);
        registered?;
        removed.map_err(io)
    }

    fn uninstall(&self) -> Result<()> {
        self.schtasks(&["/End", "/TN", TASK_NAME]);
        self.schtasks(&["/Delete", "/TN", TASK_NAME, "/F"]);
        Ok(())
    }

    fn start(&self) -> Result<()> {
        let started = self.schtasks(&["/Run", "/TN", TASK_NAME]);
        if started.succeeded() {
            Ok(())
        } else {
            Err(failed("schtasks /Run", &started))
        }
    }

    fn stop(&self) -> Result<()> {
        self.schtasks(&["/End", "/TN", TASK_NAME]);
        Ok(())
    }
}

/// The status column of `schtasks /Query /FO CSV` is localized: English `"Running"`, French
/// `"En cours"` or `"En cours d'exécution"`, compared without case.
fn task_running(output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    output.contains("\"running\"") || output.contains("\"en cours")
}

/// `SecurityElement.Escape`.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// `WindowsScheduledTask.TaskXml`: an interactive task at the logon of `user_id`, restarted on
/// failure, never stopped for its duration.
pub fn task_xml(definition: &ServiceDefinition, user_id: &str) -> String {
    let arguments = definition
        .arguments
        .iter()
        .map(|argument| quote_argument(argument))
        .collect::<Vec<_>>()
        .join(" ");
    let (version, user) = (escape(&definition.version), escape(user_id));
    let (command, arguments) = (escape(&definition.executable), escape(&arguments));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Sermofur daemon {version}</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>true</Hidden>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>"#
    )
}

/// `WindowsScheduledTask.QuoteArgument`: the quoting that `CommandLineToArgvW` reads back.
pub fn quote_argument(argument: &str) -> String {
    if !argument.is_empty() && !argument.contains([' ', '\t', '"']) {
        return argument.to_owned();
    }
    let mut quoted = String::from("\"");
    let mut backslashes = 0;
    for c in argument.chars() {
        if c == '\\' {
            backslashes += 1;
            continue;
        }
        let count = if c == '"' {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        quoted.push_str(&"\\".repeat(count));
        backslashes = 0;
        quoted.push(c);
    }
    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

/// `SystemdUserService`.
pub struct SystemdUser {
    runner: Box<dyn ProcessRunner>,
    unit_directory: PathBuf,
}

pub const UNIT_NAME: &str = "sermofur.service";

impl SystemdUser {
    pub fn new(runner: Box<dyn ProcessRunner>, unit_directory: &Path) -> Self {
        Self {
            runner,
            unit_directory: unit_directory.to_owned(),
        }
    }

    pub fn unit_file(&self) -> PathBuf {
        self.unit_directory.join(UNIT_NAME)
    }

    fn systemctl(&self, arguments: &[&str]) -> ProcessOutcome {
        let mut all = vec!["--user"];
        all.extend_from_slice(arguments);
        self.runner.run("systemctl", &all)
    }

    fn check(&self, action: &str) -> Result<()> {
        let outcome = self.systemctl(&[action, UNIT_NAME]);
        if outcome.succeeded() {
            Ok(())
        } else {
            Err(failed(&format!("systemctl --user {action}"), &outcome))
        }
    }
}

impl ServiceManager for SystemdUser {
    fn name(&self) -> &'static str {
        "systemd user service"
    }

    fn unavailable(&self) -> Option<String> {
        let outcome = self.systemctl(&["show-environment"]);
        (!outcome.succeeded()).then(|| {
            let why = if outcome.exit_code == -1 {
                "systemctl not found"
            } else {
                "systemctl --user failed"
            };
            format!("no systemd user manager in this session ({why}).")
        })
    }

    fn query(&self) -> ServiceStatus {
        ServiceStatus {
            installed: file_exists(&self.unit_file()),
            running: self.systemctl(&["is-active", UNIT_NAME]).output.trim() == "active",
        }
    }

    fn validate(&self, definition: &ServiceDefinition) -> Result<()> {
        unit(definition).map(|_| ())
    }

    fn install(&self, definition: &ServiceDefinition, _: Option<&ServiceDefinition>) -> Result<()> {
        let file = self.unit_file();
        // The previous unit is kept and restored as it was.
        let previous = if file_exists(&file) {
            Some(fs::read(&file).map_err(io)?)
        } else {
            None
        };
        let text = unit(definition)?;
        fs::create_dir_all(&self.unit_directory).map_err(io)?;
        fs::write(&file, text).map_err(io)?;
        let reload = self.systemctl(&["daemon-reload"]);
        let enable = if reload.succeeded() {
            self.systemctl(&["enable", UNIT_NAME])
        } else {
            reload
        };
        let restart = if enable.succeeded() {
            self.systemctl(&["restart", UNIT_NAME])
        } else {
            enable
        };
        if restart.succeeded() {
            return Ok(());
        }
        match previous {
            None => {
                // A first install leaves nothing behind, not even the enable link.
                self.systemctl(&["disable", UNIT_NAME]);
                fs::remove_file(&file).map_err(io)?;
            }
            Some(previous) => fs::write(&file, previous).map_err(io)?,
        }
        self.systemctl(&["daemon-reload"]);
        Err(failed("systemctl --user", &restart))
    }

    fn uninstall(&self) -> Result<()> {
        self.systemctl(&["disable", "--now", UNIT_NAME]);
        let file = self.unit_file();
        if file_exists(&file) {
            fs::remove_file(&file).map_err(io)?;
        }
        self.systemctl(&["daemon-reload"]);
        Ok(())
    }

    fn start(&self) -> Result<()> {
        self.check("start")
    }

    fn stop(&self) -> Result<()> {
        self.check("stop")
    }
}

/// `DaemonLimits.Default.StopTimeout`: the drain, plus ten seconds.
pub fn stop_timeout() -> Duration {
    DaemonLimits::default().drain_timeout() + Duration::from_secs(10)
}

/// The variables of a definition ordered by name, as `OrderBy(pair => pair.Key)`.
// ponytail: ordinal order where .NET compares with the current culture; they agree on the
// carried names (capitals and underscores), the only ones `ForCurrentProcess` writes.
fn ordered(environment: &[(String, Option<String>)]) -> Vec<(&str, &str)> {
    let mut pairs: Vec<(&str, &str)> = environment
        .iter()
        .map(|(name, value)| (name.as_str(), value.as_deref().unwrap_or_default()))
        .collect();
    pairs.sort_by(|left, right| left.0.cmp(right.0));
    pairs
}

/// `SystemdUserService.Unit`: every word quoted, `%` doubled, `$` doubled in `ExecStart`;
/// a line break or NUL is `service_install_failed`.
pub fn unit(definition: &ServiceDefinition) -> Result<String> {
    fn quote(value: &str) -> Result<String> {
        if value.contains(['\n', '\r', '\0']) {
            return Err(SermofurError::new(
                "service_install_failed",
                "A path or variable of the service holds a line break or NUL; it cannot go into a unit file.",
                3,
            ));
        }
        Ok(format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('%', "%%")
        ))
    }
    let mut unit = format!(
        "[Unit]\nDescription=Sermofur daemon {}\n\n[Service]\nType=simple\nExecStart=",
        definition.version
    );
    let words = std::iter::once(&definition.executable)
        .chain(&definition.arguments)
        .map(|word| quote(word).map(|quoted| quoted.replace('$', "$$")))
        .collect::<Result<Vec<_>>>()?;
    unit.push_str(&words.join(" "));
    unit.push('\n');
    for (name, value) in ordered(&definition.environment) {
        // Environment= expands specifiers (%) but not variables ($).
        unit.push_str(&format!(
            "Environment={}\n",
            quote(&format!("{name}={value}"))?
        ));
    }
    unit.push_str("Restart=on-failure\nRestartSec=1\n");
    unit.push_str(&format!("TimeoutStopSec={}\n\n", stop_timeout().as_secs()));
    unit.push_str("[Install]\nWantedBy=default.target\n");
    Ok(unit)
}

/// `LaunchAgentService`.
pub struct LaunchAgent {
    runner: Box<dyn ProcessRunner>,
    agent_directory: PathBuf,
    uid: u32,
    log_directory: PathBuf,
    wait: Box<dyn Fn(Duration)>,
}

pub const LABEL: &str = "io.github.jorouquette.sermofur";

impl LaunchAgent {
    pub fn new(
        runner: Box<dyn ProcessRunner>,
        agent_directory: &Path,
        uid: u32,
        log_directory: &Path,
    ) -> Self {
        Self {
            runner,
            agent_directory: agent_directory.to_owned(),
            uid,
            log_directory: log_directory.to_owned(),
            wait: Box::new(thread::sleep),
        }
    }

    /// The pause between two probes, replaced in the tests.
    #[must_use]
    pub fn waiting(mut self, wait: Box<dyn Fn(Duration)>) -> Self {
        self.wait = wait;
        self
    }

    pub fn plist_file(&self) -> PathBuf {
        self.agent_directory.join(format!("{LABEL}.plist"))
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.uid)
    }

    fn target(&self) -> String {
        format!("gui/{}/{LABEL}", self.uid)
    }

    fn launchctl(&self, arguments: &[&str]) -> ProcessOutcome {
        self.runner.run("launchctl", arguments)
    }

    /// Unloads the agent and waits up to five seconds for launchd to forget it.
    fn boot_out(&self) {
        let target = self.target();
        self.launchctl(&["bootout", &target]);
        for _ in 0..50 {
            if !self.launchctl(&["print", &target]).succeeded() {
                return;
            }
            (self.wait)(Duration::from_millis(100));
        }
    }

    /// Loads the agent; once more after a second when launchd is still removing the old one.
    fn bootstrap(&self) -> ProcessOutcome {
        let (domain, file) = (self.domain(), self.plist_file());
        let file = file.to_string_lossy();
        let loaded = self.launchctl(&["bootstrap", &domain, &file]);
        let still_removing = loaded.exit_code == 5 || loaded.error.contains("failed: 5:");
        if loaded.succeeded() || !still_removing {
            return loaded;
        }
        (self.wait)(Duration::from_secs(1));
        self.launchctl(&["bootstrap", &domain, &file])
    }
}

impl ServiceManager for LaunchAgent {
    fn name(&self) -> &'static str {
        "launchd agent"
    }

    fn unavailable(&self) -> Option<String> {
        let domain = self.domain();
        (!self.launchctl(&["print", &domain]).succeeded()).then(|| {
            format!(
                "no launchd session domain {domain} (no user logged in to the graphical session)."
            )
        })
    }

    fn query(&self) -> ServiceStatus {
        let printed = self.launchctl(&["print", &self.target()]);
        ServiceStatus {
            installed: file_exists(&self.plist_file()),
            running: printed.succeeded() && printed.output.contains("state = running"),
        }
    }

    fn install(&self, definition: &ServiceDefinition, _: Option<&ServiceDefinition>) -> Result<()> {
        let file = self.plist_file();
        let previous = if file_exists(&file) {
            Some(fs::read(&file).map_err(io)?)
        } else {
            None
        };
        fs::create_dir_all(&self.agent_directory).map_err(io)?;
        fs::create_dir_all(&self.log_directory).map_err(io)?;
        // Loaded agents are reloaded, never stacked.
        self.boot_out();
        fs::write(&file, plist(definition, &self.log_directory)).map_err(io)?;
        let loaded = self.bootstrap();
        if loaded.succeeded() {
            return Ok(());
        }
        match previous {
            None => fs::remove_file(&file).map_err(io)?,
            Some(previous) => {
                fs::write(&file, previous).map_err(io)?;
                self.bootstrap();
            }
        }
        Err(failed("launchctl bootstrap", &loaded))
    }

    fn uninstall(&self) -> Result<()> {
        self.launchctl(&["bootout", &self.target()]);
        let file = self.plist_file();
        if file_exists(&file) {
            fs::remove_file(&file).map_err(io)?;
        }
        Ok(())
    }

    fn start(&self) -> Result<()> {
        let started = self.launchctl(&["kickstart", &self.target()]);
        if started.succeeded() {
            Ok(())
        } else {
            Err(failed("launchctl kickstart", &started))
        }
    }

    fn stop(&self) -> Result<()> {
        self.launchctl(&["kill", "SIGTERM", &self.target()]);
        Ok(())
    }
}

/// `LaunchAgentService.Plist`.
pub fn plist(definition: &ServiceDefinition, log_directory: &Path) -> String {
    let mut plist = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    plist.push_str("<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n");
    plist.push_str("<plist version=\"1.0\">\n<dict>\n");
    plist.push_str(&format!("  <key>Label</key><string>{LABEL}</string>\n"));
    plist.push_str("  <key>ProgramArguments</key>\n  <array>\n");
    for argument in std::iter::once(&definition.executable).chain(&definition.arguments) {
        plist.push_str(&format!("    <string>{}</string>\n", escape(argument)));
    }
    plist.push_str("  </array>\n");
    plist.push_str("  <key>RunAtLoad</key><true/>\n");
    plist.push_str("  <key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n");
    plist.push_str("  <key>ThrottleInterval</key><integer>1</integer>\n");
    plist.push_str(&format!(
        "  <key>ExitTimeOut</key><integer>{}</integer>\n",
        stop_timeout().as_secs()
    ));
    plist.push_str("  <key>EnvironmentVariables</key>\n  <dict>\n");
    for (name, value) in ordered(&definition.environment) {
        plist.push_str(&format!(
            "    <key>{}</key><string>{}</string>\n",
            escape(name),
            escape(value)
        ));
    }
    plist.push_str("  </dict>\n");
    plist.push_str(&format!(
        "  <key>StandardErrorPath</key><string>{}</string>\n",
        escape(&log_directory.join("launchd.err").to_string_lossy())
    ));
    plist.push_str("</dict>\n</plist>\n");
    plist
}

/// `File.Exists`: an entry that is not a folder.
fn file_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir())
}

/// `SystemProcessRunner`: the system tool by its absolute path, its output read whole, 30 s
/// at most.
pub struct SystemProcessRunner;

const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

impl ProcessRunner for SystemProcessRunner {
    fn run(&self, program: &str, arguments: &[&str]) -> ProcessOutcome {
        let failed = |exit_code, error: String| ProcessOutcome {
            exit_code,
            output: String::new(),
            error,
        };
        let Some(path) = resolve(program, environment_text("PATH").as_deref()) else {
            return failed(
                -1,
                format!("{program} was not found in the system folders."),
            );
        };
        // Named deviation (PARITY.md, lot 7.7): stdin is the null device, where `ProcessStartInfo`
        // of the oracle leaves it inherited. None of the tools run here reads stdin; a tool that
        // asked for input would get the end of input instead of the console or the pipe of the
        // caller (under `smf mcp serve`, the requests of the host).
        let mut child = match Command::new(path)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => return failed(-1, error.to_string()),
        };
        let output = drain(child.stdout.take());
        let error = drain(child.stderr.take());
        let deadline = Instant::now() + TOOL_TIMEOUT;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                // ponytail: kills the tool, not its children (`Kill(true)` of .NET); the
                // system tools run here start none.
                Ok(None) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return failed(-2, format!("{program} did not answer within 30 s."));
                }
                Err(error) => return failed(-1, error.to_string()),
            }
        };
        let text = |handle: thread::JoinHandle<Vec<u8>>| {
            String::from_utf8_lossy(&handle.join().unwrap_or_default()).into_owned()
        };
        ProcessOutcome {
            exit_code: exit_code(status),
            output: text(output),
            error: text(error),
        }
    }
}

fn drain(pipe: Option<impl Read + Send + 'static>) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// `Process.ExitCode`: on Unix, 128 plus the signal that ended the process.
pub(crate) fn exit_code(status: std::process::ExitStatus) -> i32 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    status.code().unwrap_or(-1)
}

/// `SystemTool.Resolve`: the tool in the system folder of Windows, or in the usual Unix folders
/// then the absolute entries of `search_path`; `None` when it is nowhere. A bare name would be
/// looked up in the current folder first, so a file planted in a project could run instead.
// ponytail: Windows takes `%SystemRoot%\System32` where .NET asks `GetSystemDirectory` (a Win32
// call outside the derogation); they differ only if the variable was changed.
pub fn resolve(name: &str, search_path: Option<&str>) -> Option<PathBuf> {
    if is_path_fully_qualified(name) {
        return Some(PathBuf::from(name));
    }
    if cfg!(windows) {
        let file = if Path::new(name).extension().is_some() {
            name.to_owned()
        } else {
            format!("{name}.exe")
        };
        let candidate = Path::new(&system_root(environment_text("SystemRoot")))
            .join("System32")
            .join(file);
        return file_exists(&candidate).then_some(candidate);
    }
    ["/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        .into_iter()
        .chain(
            search_path
                .unwrap_or_default()
                .split(':')
                .filter(|entry| !entry.is_empty() && is_path_fully_qualified(entry)),
        )
        .map(|folder| Path::new(folder).join(name))
        .find(|candidate| file_exists(candidate))
}

/// `%SystemRoot%` when it is an absolute path, `C:\Windows` otherwise: an empty or relative
/// value would look the tool up in the current folder.
fn system_root(value: Option<String>) -> String {
    value
        .filter(|root| is_path_fully_qualified(root))
        .unwrap_or_else(|| r"C:\Windows".to_owned())
}

/// `InstalledDefinition.File`.
pub fn installed_definition_file(paths: &DaemonPaths) -> PathBuf {
    paths.state_directory.join("service.json")
}

/// `InstalledDefinition.Read(paths)?.Executable`.
pub fn installed_executable(paths: &DaemonPaths) -> Result<Option<String>> {
    Ok(read_installed(paths)?.map(|definition| definition.executable))
}

/// `InstalledDefinition.Read`: what was installed, or `None` when nothing was: no file, a file
/// that is not JSON, or one whose shape does not hold. A file that cannot be read is
/// `invalid_storage_or_path`, as the `IOException` of the oracle.
pub fn read_installed(paths: &DaemonPaths) -> Result<Option<ServiceDefinition>> {
    let file = installed_definition_file(paths);
    if !file_exists(&file) {
        return Ok(None);
    }
    let content = fs::read(&file).map_err(FileError::from)?;
    Ok(sound_definition(&content))
}

/// `JsonSerializer.Deserialize<ServiceDefinition>` with the default options, then `Sound`: an
/// executable and a version that are not blank, and arguments that hold `daemon run` side by
/// side. Any `JsonException` (wrong type, malformed JSON) is `None`. A property met twice keeps
/// its last value; an environment variable met twice keeps its first place and last value.
pub fn sound_definition(content: &[u8]) -> Option<ServiceDefinition> {
    let node = stj::parse(&lenient_text(content)?).ok()?;
    let properties = stj::object(&node).ok()??;
    let mut executable = None;
    let mut version = None;
    let mut arguments: Option<Vec<Option<String>>> = None;
    let mut environment: Vec<(String, Option<String>)> = Vec::new();
    for (name, value) in properties {
        match name {
            "executable" => executable = stj::text(value).ok()?,
            "version" => version = stj::text(value).ok()?,
            "arguments" => {
                arguments = match value {
                    Node::Null => None,
                    Node::Array(items) => Some(
                        items
                            .iter()
                            .map(|item| stj::text(item).ok())
                            .collect::<Option<Vec<_>>>()?,
                    ),
                    _ => return None,
                }
            }
            "environment" => {
                environment.clear();
                for (variable, text) in stj::object(value).ok()?.unwrap_or_default() {
                    let text = stj::text(text).ok()?;
                    match environment.iter_mut().find(|(known, _)| known == variable) {
                        Some(entry) => entry.1 = text,
                        None => environment.push((variable.to_owned(), text)),
                    }
                }
            }
            _ => {}
        }
    }
    let blank = |text: &Option<String>| {
        text.as_deref()
            .is_none_or(|text| text.chars().all(char::is_whitespace))
    };
    let arguments = arguments?;
    let runs_daemon = arguments
        .windows(2)
        .any(|pair| pair[0].as_deref() == Some("daemon") && pair[1].as_deref() == Some("run"));
    if blank(&executable) || blank(&version) || !runs_daemon {
        return None;
    }
    Some(ServiceDefinition {
        executable: executable?,
        // ponytail: a null argument is read as an empty one; it is never launched (the
        // supervisor builds its own definition) and never compared with a current one.
        arguments: arguments
            .into_iter()
            .map(Option::unwrap_or_default)
            .collect(),
        version: version?,
        environment,
    })
}

/// The bytes of `JsonSerializer.SerializeToUtf8Bytes(definition, WriteIndented)`.
pub fn definition_json(definition: &ServiceDefinition) -> String {
    let n = NEW_LINE;
    let text = |value: &Option<String>| {
        value
            .as_deref()
            .map_or_else(|| "null".to_owned(), quoted_basic_latin)
    };
    let arguments = if definition.arguments.is_empty() {
        "[]".to_owned()
    } else {
        let items: Vec<String> = definition
            .arguments
            .iter()
            .map(|argument| format!("{n}    {}", quoted_basic_latin(argument)))
            .collect();
        format!("[{}{n}  ]", items.join(","))
    };
    let environment = if definition.environment.is_empty() {
        "{}".to_owned()
    } else {
        let items: Vec<String> = definition
            .environment
            .iter()
            .map(|(name, value)| format!("{n}    {}: {}", quoted_basic_latin(name), text(value)))
            .collect();
        format!("{{{}{n}  }}", items.join(","))
    };
    format!(
        "{{{n}  \"executable\": {},{n}  \"arguments\": {arguments},{n}  \"version\": {},{n}  \"environment\": {environment}{n}}}",
        quoted_basic_latin(&definition.executable),
        quoted_basic_latin(&definition.version),
    )
}

/// `InstalledDefinition.Write`: the state folder born private, the file never half written.
pub fn write_installed(paths: &DaemonPaths, definition: &ServiceDefinition) -> Result<()> {
    file_turn::create_private_folder(&paths.state_directory)?;
    atomic::write(
        &installed_definition_file(paths),
        definition_json(definition).as_bytes(),
        &WriteOptions::default(),
    )?;
    Ok(())
}

/// `InstalledDefinition.Delete`.
pub fn delete_installed(paths: &DaemonPaths) -> Result<()> {
    let file = installed_definition_file(paths);
    if file_exists(&file) {
        fs::remove_file(&file).map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_status_is_read_in_english_and_in_french() {
        assert!(task_running(
            "\"\\Sermofur\\Daemon\",\"N/A\",\"Running\"\r\n"
        ));
        assert!(task_running(
            "\"\\Sermofur\\Daemon\",\"N/A\",\"En cours d'exécution\""
        ));
        assert!(task_running("\"x\",\"N/A\",\"EN COURS\""));
        assert!(!task_running("\"\\Sermofur\\Daemon\",\"N/A\",\"Ready\""));
    }

    /// Minor 1 of the review of lot 7.6: an empty or relative `%SystemRoot%` would make
    /// schtasks relative to the current folder; only an absolute one is kept.
    #[test]
    fn only_an_absolute_system_root_is_kept() {
        assert_eq!(system_root(None), r"C:\Windows");
        assert_eq!(system_root(Some(String::new())), r"C:\Windows");
        assert_eq!(system_root(Some(r"Windows".to_owned())), r"C:\Windows");
        assert_eq!(system_root(Some(r"\Windows".to_owned())), r"C:\Windows");
        let absolute = if cfg!(windows) { r"D:\Win" } else { "/win" };
        assert_eq!(system_root(Some(absolute.to_owned())), absolute);
    }
}
