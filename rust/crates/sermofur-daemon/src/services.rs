//! The service manager of the user session, as `Sermofur.Daemon.Services` of the .NET oracle.
//! Lot 7.6 ports the part that `smf daemon status` reads: the name of the manager, whether it
//! is usable, whether the service is installed, and the executable recorded in `service.json`.
//! Install, uninstall, start and stop come with lot 7.7.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::daemon_paths::{DaemonPaths, SpecialFolder, environment_text, system_folder};
use sermofur_core::error::Result;
use sermofur_core::local_paths::is_path_fully_qualified;
use sermofur_core::stj::{self, Node};

use crate::file_turn::FileError;
use crate::registry::lenient_text;

/// State of the service as its manager sees it (`ServiceStatus`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServiceStatus {
    pub installed: bool,
    pub running: bool,
}

/// A service manager of the user session (`IServiceManager`, read part).
pub trait ServiceManager {
    /// Name shown by status: scheduled task, systemd user service, launchd agent.
    fn name(&self) -> &'static str;

    /// `None` when usable, or why not (container, WSL without systemd).
    fn unavailable(&self) -> Option<String>;

    fn query(&self) -> ServiceStatus;
}

/// `ServiceManagers.ForCurrentSystem`.
pub fn for_current_system() -> Box<dyn ServiceManager> {
    if cfg!(windows) {
        Box::new(ScheduledTask)
    } else if cfg!(target_os = "macos") {
        Box::new(LaunchAgent {
            agent_directory: Path::new(&system_folder(SpecialFolder::UserProfile))
                .join("Library")
                .join("LaunchAgents"),
            uid: user_id(),
        })
    } else {
        // `XDG_CONFIG_HOME` when set and not empty, `~/.config` otherwise.
        let config = environment_text("XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(
                || Path::new(&system_folder(SpecialFolder::UserProfile)).join(".config"),
                PathBuf::from,
            );
        Box::new(SystemdUser {
            unit_directory: config.join("systemd").join("user"),
        })
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

/// `WindowsScheduledTask`.
struct ScheduledTask;

const TASK_NAME: &str = r"Sermofur\Daemon";

impl ServiceManager for ScheduledTask {
    fn name(&self) -> &'static str {
        "scheduled task"
    }

    fn unavailable(&self) -> Option<String> {
        resolve("schtasks", None)
            .is_none()
            .then(|| "schtasks.exe is not in the system folder of Windows.".to_owned())
    }

    fn query(&self) -> ServiceStatus {
        let query = run(
            "schtasks",
            &["/Query", "/TN", TASK_NAME, "/FO", "CSV", "/NH"],
        );
        ServiceStatus {
            installed: query.succeeded(),
            running: query.succeeded() && task_running(&query.output),
        }
    }
}

/// The status column of `schtasks /Query /FO CSV` is localized: English `"Running"`, French
/// `"En cours"` or `"En cours d'exécution"`, compared without case.
fn task_running(output: &str) -> bool {
    let output = output.to_ascii_lowercase();
    output.contains("\"running\"") || output.contains("\"en cours")
}

/// `SystemdUserService`.
struct SystemdUser {
    unit_directory: PathBuf,
}

const UNIT_NAME: &str = "sermofur.service";

impl ServiceManager for SystemdUser {
    fn name(&self) -> &'static str {
        "systemd user service"
    }

    fn unavailable(&self) -> Option<String> {
        let outcome = run("systemctl", &["--user", "show-environment"]);
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
            installed: file_exists(&self.unit_directory.join(UNIT_NAME)),
            running: run("systemctl", &["--user", "is-active", UNIT_NAME])
                .output
                .trim()
                == "active",
        }
    }
}

/// `LaunchAgentService`.
struct LaunchAgent {
    agent_directory: PathBuf,
    uid: u32,
}

const LABEL: &str = "io.github.jorouquette.sermofur";

impl ServiceManager for LaunchAgent {
    fn name(&self) -> &'static str {
        "launchd agent"
    }

    fn unavailable(&self) -> Option<String> {
        let domain = format!("gui/{}", self.uid);
        (!run("launchctl", &["print", &domain]).succeeded()).then(|| {
            format!(
                "no launchd session domain {domain} (no user logged in to the graphical session)."
            )
        })
    }

    fn query(&self) -> ServiceStatus {
        let printed = run(
            "launchctl",
            &["print", &format!("gui/{}/{LABEL}", self.uid)],
        );
        ServiceStatus {
            installed: file_exists(&self.agent_directory.join(format!("{LABEL}.plist"))),
            running: printed.succeeded() && printed.output.contains("state = running"),
        }
    }
}

/// `File.Exists`: an entry that is not a folder.
fn file_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir())
}

/// `ProcessOutcome`: exit code (-1 not started, -2 timed out), output and error.
struct ProcessOutcome {
    exit_code: i32,
    output: String,
}

impl ProcessOutcome {
    fn succeeded(&self) -> bool {
        self.exit_code == 0
    }
}

const TOOL_TIMEOUT: Duration = Duration::from_secs(30);

/// `SystemProcessRunner.Run`: the system tool by its absolute path, its output read whole, 30 s
/// at most. The standard error is drained but no reader of this lot uses it.
fn run(program: &str, arguments: &[&str]) -> ProcessOutcome {
    let failed = |exit_code| ProcessOutcome {
        exit_code,
        output: String::new(),
    };
    let Some(path) = resolve(program, environment_text("PATH").as_deref()) else {
        return failed(-1);
    };
    let Ok(mut child) = Command::new(path)
        .args(arguments)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return failed(-1);
    };
    let output = drain(child.stdout.take());
    let _error = drain(child.stderr.take());
    let deadline = Instant::now() + TOOL_TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            // ponytail: kills the tool, not its children (`Kill(true)` of .NET); the system
            // tools read here start none.
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return failed(-2);
            }
            Err(_) => return failed(-1),
        }
    };
    ProcessOutcome {
        exit_code: exit_code(status),
        output: String::from_utf8_lossy(&output.join().unwrap_or_default()).into_owned(),
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
fn exit_code(status: std::process::ExitStatus) -> i32 {
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
fn resolve(name: &str, search_path: Option<&str>) -> Option<PathBuf> {
    if is_path_fully_qualified(name) {
        return Some(PathBuf::from(name));
    }
    if cfg!(windows) {
        let file = if Path::new(name).extension().is_some() {
            name.to_owned()
        } else {
            format!("{name}.exe")
        };
        let root = environment_text("SystemRoot").unwrap_or_else(|| r"C:\Windows".to_owned());
        let candidate = Path::new(&root).join("System32").join(file);
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

/// `InstalledDefinition.File`.
pub fn installed_definition_file(paths: &DaemonPaths) -> PathBuf {
    paths.state_directory.join("service.json")
}

/// `InstalledDefinition.Read(paths)?.Executable`: the executable of what was installed, or
/// `None` when nothing was: no file, a file that is not JSON, or one whose shape does not hold.
/// A file that cannot be read is `invalid_storage_or_path`, as the `IOException` of the oracle.
pub fn installed_executable(paths: &DaemonPaths) -> Result<Option<String>> {
    let file = installed_definition_file(paths);
    if !file_exists(&file) {
        return Ok(None);
    }
    let content = fs::read(&file).map_err(FileError::from)?;
    Ok(sound_executable(&content))
}

/// `JsonSerializer.Deserialize<ServiceDefinition>` with the default options, then `Sound`: an
/// executable and a version that are not blank, and arguments that hold `daemon run` side by
/// side. Any `JsonException` (wrong type, malformed JSON) is `None`; an environment is read
/// only to be refused when it does not hold strings.
fn sound_executable(content: &[u8]) -> Option<String> {
    let node = stj::parse(&lenient_text(content)?).ok()?;
    let properties = stj::object(&node).ok()??;
    let mut executable = None;
    let mut version = None;
    let mut arguments: Option<Vec<Option<String>>> = None;
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
                for (_, variable) in stj::object(value).ok()?.unwrap_or_default() {
                    stj::text(variable).ok()?;
                }
            }
            _ => {}
        }
    }
    let blank = |text: &Option<String>| {
        text.as_deref()
            .is_none_or(|text| text.chars().all(char::is_whitespace))
    };
    let runs_daemon = arguments.is_some_and(|arguments| {
        arguments
            .windows(2)
            .any(|pair| pair[0].as_deref() == Some("daemon") && pair[1].as_deref() == Some("run"))
    });
    if blank(&executable) || blank(&version) || !runs_daemon {
        return None;
    }
    executable
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
}
