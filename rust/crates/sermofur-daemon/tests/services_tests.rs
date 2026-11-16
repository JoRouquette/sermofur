//! The service managers (lot 7.7): ports of the texts and scripted flows of `ServiceTests` and
//! `ServiceHardeningTests`, every system tool replaced by a scripted runner (no real service is
//! installed), then the texts against what the .NET oracle wrote (`rust/tests/golden/services.json`
//! under Windows, `services-unix.json` elsewhere, recorded by `rust/tools/services-oracle`).

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::Value;
use sermofur_daemon::daemon_paths::DaemonPaths;
use sermofur_daemon::file_turn::{is_held, take, wait_for_release};
use sermofur_daemon::services::{
    self, LABEL, LaunchAgent, ProcessOutcome, ProcessRunner, ScheduledTask, ServiceDefinition,
    ServiceManager, SystemdUser, plist, quote_argument, same, stop_timeout, task_xml, unit,
};
use sermofur_daemon::supervisor::ChildStart;

fn definition(
    executable: &str,
    arguments: &[&str],
    environment: &[(&str, &str)],
) -> ServiceDefinition {
    ServiceDefinition {
        executable: executable.to_owned(),
        arguments: arguments.iter().map(|&a| a.to_owned()).collect(),
        version: "0.4.0".to_owned(),
        environment: environment
            .iter()
            .map(|&(k, v)| (k.to_owned(), Some(v.to_owned())))
            .collect(),
    }
}

/// `ServiceTests.Definition`.
fn quoted_definition() -> ServiceDefinition {
    definition(
        "/opt/tools/smf",
        &["daemon", "run", "a b", "100%", "q\"uote"],
        &[
            ("PATH", "/usr/bin:/bin"),
            ("DOTNET_ROOT", "/opt/dotnet & co"),
        ],
    )
}

/// `ServiceHardeningTests.Definition`.
fn plain(environment: &[(&str, &str)]) -> ServiceDefinition {
    definition("/opt/smf", &["daemon", "run"], environment)
}

/// `ScriptedRunner`: every call recorded as `program arguments…`, answered by `answer`.
#[derive(Clone)]
struct Scripted {
    calls: Rc<RefCell<Vec<String>>>,
    answer: Rc<dyn Fn(&str) -> ProcessOutcome>,
}

impl Scripted {
    fn new(answer: impl Fn(&str) -> ProcessOutcome + 'static) -> Self {
        Self {
            calls: Rc::default(),
            answer: Rc::new(answer),
        }
    }

    /// `RecordingRunner`: everything succeeds.
    fn recording() -> Self {
        Self::new(|_| ProcessOutcome::default())
    }

    fn verbs(&self) -> Vec<String> {
        self.calls
            .borrow()
            .iter()
            .map(|call| call.split(' ').nth(1).unwrap_or_default().to_owned())
            .collect()
    }
}

impl ProcessRunner for Scripted {
    fn run(&self, program: &str, arguments: &[&str]) -> ProcessOutcome {
        let call = format!("{program} {}", arguments.join(" "));
        self.calls.borrow_mut().push(call.clone());
        (self.answer)(&call)
    }
}

/// A runner that must not be called.
struct NoRunner;

impl ProcessRunner for NoRunner {
    fn run(&self, _: &str, _: &[&str]) -> ProcessOutcome {
        panic!("Nothing may run.")
    }
}

struct Folder(PathBuf);

impl Folder {
    fn new() -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let folder = std::env::temp_dir().join(format!(
            "smf-hard-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&folder).expect("a folder");
        Self(folder)
    }
}

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Port of `ServiceTests.SystemdUnitQuotesEveryArgument`.
#[test]
fn the_systemd_unit_quotes_every_argument() {
    let unit = unit(&quoted_definition()).expect("a unit");
    assert!(unit.contains(
        "ExecStart=\"/opt/tools/smf\" \"daemon\" \"run\" \"a b\" \"100%%\" \"q\\\"uote\"\n"
    ));
    assert!(unit.contains("Environment=\"DOTNET_ROOT=/opt/dotnet & co\"\n"));
    assert!(unit.contains("Restart=on-failure\nRestartSec=1\n"));
    assert!(unit.contains("WantedBy=default.target"));
}

/// Port of `ServiceTests.LaunchAgentEscapesItsValues`.
#[test]
fn the_launch_agent_escapes_its_values() {
    let plist = plist(&quoted_definition(), Path::new("/logs"));
    assert!(
        plist
            .replace("&quot;", "\"")
            .contains("<string>q\"uote</string>")
    );
    assert!(plist.contains("<key>DOTNET_ROOT</key><string>/opt/dotnet &amp; co</string>"));
    assert!(plist.contains("<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>"));
    assert!(plist.contains(&format!("<key>Label</key><string>{LABEL}</string>")));
}

/// Port of `ServiceTests.WindowsTaskRunsInteractivelyWithQuotedArguments`.
#[test]
fn the_windows_task_runs_interactively_with_quoted_arguments() {
    let xml = task_xml(&quoted_definition(), r"PC\john");
    assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
    assert!(!xml.contains("S4U"));
    assert!(xml.contains(r"<UserId>PC\john</UserId>"));
    assert!(xml.contains("<RestartOnFailure>"));
    assert!(xml.contains("daemon run &quot;a b&quot; 100% &quot;q\\&quot;uote&quot;"));
    assert_eq!(quote_argument("C:\\a b\\"), "\"C:\\a b\\\\\"");
    assert_eq!(quote_argument(""), "\"\"");
}

/// Port of `ServiceTests.SystemdRefusesAnImpossibleUnitBeforeAnythingStops`.
#[test]
fn systemd_refuses_an_impossible_unit_before_anything_stops() {
    let systemd = SystemdUser::new(Box::new(NoRunner), &std::env::temp_dir());
    let error = systemd
        .validate(&plain(&[("PATH", "/bin\n/x")]))
        .expect_err("refused");
    assert_eq!(error.code, "service_install_failed");
}

/// Port of `ServiceHardeningTests.SystemToolsNeverComeFromARelativeFolder`.
#[cfg(unix)]
#[test]
fn system_tools_never_come_from_a_relative_folder() {
    let folder = Folder::new();
    let tool = folder.0.join("sermofur-fake-tool");
    fs::write(&tool, "#!/bin/sh\n").expect("a tool");
    let current = std::env::current_dir().expect("the current folder");
    let relative = pathdiff(&folder.0, &current);
    assert_eq!(
        services::resolve("sermofur-fake-tool", Some(&format!(".:{relative}"))),
        None
    );
    assert_eq!(
        services::resolve("sermofur-fake-tool", Some(&folder.0.to_string_lossy())),
        Some(tool)
    );
    assert!(services::resolve("sh", Some("")).expect("sh").is_absolute());
}

/// `Path.GetRelativePath` for two absolute folders.
#[cfg(unix)]
fn pathdiff(target: &Path, base: &Path) -> String {
    let (target, base): (Vec<_>, Vec<_>) =
        (target.components().collect(), base.components().collect());
    let common = target.iter().zip(&base).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_owned(); base.len() - common];
    parts.extend(
        target[common..]
            .iter()
            .map(|part| part.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

/// Port of `ServiceHardeningTests.SchtasksComesFromTheSystemFolder`.
#[cfg(windows)]
#[test]
fn schtasks_comes_from_the_system_folder() {
    let current = std::env::current_dir().expect("the current folder");
    let current = current.to_string_lossy();
    let root = std::env::var("SystemRoot").expect("SystemRoot");
    assert_eq!(
        services::resolve("schtasks", Some(&current)),
        Some(Path::new(&root).join("System32").join("schtasks.exe"))
    );
    assert_eq!(
        services::resolve("sermofur-fake-tool", Some(&current)),
        None
    );
}

/// Port of `ServiceHardeningTests.SystemdUnitEscapesDollarsAndRefusesLineBreaks`.
#[test]
fn the_systemd_unit_escapes_dollars_and_refuses_line_breaks() {
    let dollars = definition(
        "/home/a$b/smf",
        &["daemon", "run"],
        &[("DOTNET_ROOT", "/opt/$dotnet")],
    );
    let text = unit(&dollars).expect("a unit");
    assert!(text.contains("ExecStart=\"/home/a$$b/smf\""));
    assert!(text.contains("Environment=\"DOTNET_ROOT=/opt/$dotnet\""));
    let broken = ServiceDefinition {
        environment: vec![("PATH".to_owned(), Some("/bin\nExecStartPre=/x".to_owned()))],
        ..dollars
    };
    assert_eq!(
        unit(&broken).expect_err("refused").code,
        "service_install_failed"
    );
}

/// Port of `ServiceHardeningTests.RemovingWhatWasNeverInstalledSucceeds`.
#[test]
fn removing_what_was_never_installed_succeeds() {
    let parent = Folder::new();
    let folder = parent.0.join("missing");
    let runner = Scripted::recording();
    SystemdUser::new(Box::new(runner.clone()), &folder.join("units"))
        .uninstall()
        .expect("removed");
    LaunchAgent::new(
        Box::new(runner),
        &folder.join("agents"),
        501,
        &folder.join("logs"),
    )
    .uninstall()
    .expect("removed");
    let paths = DaemonPaths {
        config_directory: folder.clone(),
        state_directory: folder.clone(),
        endpoint: folder.join("s"),
    };
    services::delete_installed(&paths).expect("removed");
    assert!(!folder.exists());
}

/// Port of `ServiceHardeningTests.ReinstallIgnoresAnotherPath`.
#[test]
fn a_reinstall_ignores_another_path() {
    let installed = plain(&[("PATH", "/a"), ("DOTNET_ROOT", "/d")]);
    assert!(same(
        Some(&installed),
        &plain(&[("PATH", "/b"), ("DOTNET_ROOT", "/d")])
    ));
    assert!(!same(
        Some(&installed),
        &plain(&[("PATH", "/a"), ("DOTNET_ROOT", "/e")])
    ));
}

/// Port of `ServiceHardeningTests.WindowsTaskRefusesADaemonHomeItCannotCarry`.
#[test]
fn the_windows_task_refuses_a_daemon_home_it_cannot_carry() {
    let folder = Folder::new();
    let runner = Scripted::recording();
    let task = ScheduledTask::new(Box::new(runner.clone()), r"PC\john", &folder.0);
    let home = plain(&[("SERMOFUR_DAEMON_HOME", r"C:\elsewhere")]);
    assert_eq!(
        task.validate(&home).expect_err("refused").code,
        "service_install_failed"
    );
    assert_eq!(
        task.install(&home, None).expect_err("refused").code,
        "service_install_failed"
    );
    assert!(runner.calls.borrow().is_empty());
}

/// Port of `ServiceHardeningTests.SupervisedDaemonGetsTheRecordedEnvironment`.
#[test]
fn the_supervised_daemon_gets_the_recorded_environment() {
    let recorded = if cfg!(windows) {
        r"C:\dotnet-recorded"
    } else {
        "/opt/dotnet-recorded"
    };
    let start = ChildStart::new(
        &plain(&[]),
        Some(&plain(&[("DOTNET_ROOT", recorded)])),
        &mut |_| {},
        &mut |_| {},
    );
    assert_eq!(
        start.environment,
        vec![("DOTNET_ROOT".to_owned(), recorded.to_owned())]
    );
    assert_eq!(start.arguments, ["daemon", "run"]);
}

/// Port of `ServiceHardeningTests.StoppersWaitLongerThanTheDrainWithOneValue`.
#[test]
fn stoppers_wait_longer_than_the_drain_with_one_value() {
    let limits = sermofur_daemon::server::DaemonLimits::default();
    assert!(stop_timeout() > limits.drain_timeout());
    let seconds = stop_timeout().as_secs();
    assert!(
        unit(&plain(&[]))
            .expect("a unit")
            .contains(&format!("TimeoutStopSec={seconds}\n"))
    );
    assert!(
        plist(&plain(&[]), Path::new("/tmp/logs")).contains(&format!(
            "<key>ExitTimeOut</key><integer>{seconds}</integer>"
        ))
    );
}

fn utf16(file: &str) -> String {
    let bytes = fs::read(file).expect("the task file");
    assert_eq!(&bytes[..2], [0xFF, 0xFE], "a byte order mark");
    let units: Vec<u16> = bytes[2..]
        .chunks(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).expect("UTF-16")
}

/// Port of `ServiceHardeningTests.FailedTaskStartPutsThePreviousTaskBackWithoutStartingIt`.
#[test]
fn a_failed_task_start_puts_the_previous_task_back_without_starting_it() {
    let folder = Folder::new();
    let registered: Rc<RefCell<Vec<String>>> = Rc::default();
    let seen = Rc::clone(&registered);
    let runner = Scripted::new(move |call| {
        if call.contains("/Create") {
            let file = &call[call.find("/XML ").expect("/XML") + 5..call.len() - 3];
            seen.borrow_mut().push(utf16(file));
        }
        if call.contains("/Run") {
            ProcessOutcome::new(1, "", "refused")
        } else {
            ProcessOutcome::default()
        }
    });
    let task = ScheduledTask::new(Box::new(runner.clone()), r"PC\jérôme", &folder.0)
        .resolving(|_| Some(PathBuf::from("schtasks.exe")));
    let previous = definition(
        r"C:\Users\Jérôme\.dotnet\tools\smf.exe",
        &["daemon", "run", "--supervise"],
        &[],
    );
    assert!(task.install(&plain(&[]), Some(&previous)).is_err());
    assert_eq!(runner.verbs(), ["/Create", "/Run", "/Create"]);
    let registered = registered.borrow();
    assert!(registered[1].contains(r"C:\Users\Jérôme\.dotnet\tools\smf.exe"));
    assert!(registered[1].contains(r"PC\jérôme"));
    assert!(registered[0].contains("/opt/smf"));
}

/// Port of `ServiceHardeningTests.FailedTaskStartRemovesTheNewTaskWhenNothingCanBePutBack`.
#[test]
fn a_failed_task_start_removes_the_new_task_when_nothing_can_be_put_back() {
    for had_previous in [false, true] {
        let folder = Folder::new();
        let creates = Rc::new(RefCell::new(0));
        let runner = Scripted::new(move |call| {
            let refused = call.contains("/Run")
                || (call.contains("/Create") && {
                    *creates.borrow_mut() += 1;
                    *creates.borrow() == 2
                });
            if refused {
                ProcessOutcome::new(1, "", "refused")
            } else {
                ProcessOutcome::default()
            }
        });
        let task = ScheduledTask::new(Box::new(runner.clone()), r"PC\john", &folder.0)
            .resolving(|_| Some(PathBuf::from("schtasks.exe")));
        let previous = had_previous.then(|| plain(&[]));
        assert!(task.install(&plain(&[]), previous.as_ref()).is_err());
        let expected: &[&str] = if had_previous {
            &["/Create", "/Run", "/Create", "/Delete"]
        } else {
            &["/Create", "/Run", "/Delete"]
        };
        assert_eq!(runner.verbs(), expected, "previous: {had_previous}");
    }
}

/// Port of `ServiceHardeningTests.MissingSchtasksMakesTheManagerUnavailable`.
#[test]
fn a_missing_schtasks_makes_the_manager_unavailable() {
    let task = ScheduledTask::new(
        Box::new(Scripted::recording()),
        r"PC\john",
        &std::env::temp_dir(),
    )
    .resolving(|_| None);
    assert!(
        task.unavailable()
            .expect("unavailable")
            .contains("schtasks")
    );
}

/// Port of `ServiceHardeningTests.LaunchdIsGivenTimeToRemoveTheAgentBeforeLoadingIt`.
#[test]
fn launchd_is_given_time_to_remove_the_agent_before_loading_it() {
    let folder = Folder::new();
    let (prints, bootstraps) = (Rc::new(RefCell::new(0)), Rc::new(RefCell::new(0)));
    let (printed, loaded) = (Rc::clone(&prints), Rc::clone(&bootstraps));
    let runner = Scripted::new(move |call| {
        if call.starts_with("launchctl print gui/501/") {
            *printed.borrow_mut() += 1;
            return ProcessOutcome::new(if *printed.borrow() <= 2 { 0 } else { 113 }, "", "");
        }
        if call.starts_with("launchctl bootstrap") {
            *loaded.borrow_mut() += 1;
            return if *loaded.borrow() == 1 {
                ProcessOutcome::new(5, "", "Bootstrap failed: 5: Input/output error")
            } else {
                ProcessOutcome::default()
            };
        }
        ProcessOutcome::default()
    });
    let waits: Rc<RefCell<Vec<Duration>>> = Rc::default();
    let waited = Rc::clone(&waits);
    let agent = LaunchAgent::new(
        Box::new(runner),
        &folder.0.join("agents"),
        501,
        &folder.0.join("logs"),
    )
    .waiting(Box::new(move |pause| waited.borrow_mut().push(pause)));
    agent.install(&plain(&[]), None).expect("installed");
    assert_eq!((*prints.borrow(), *bootstraps.borrow()), (3, 2));
    assert_eq!(
        *waits.borrow(),
        [
            Duration::from_millis(100),
            Duration::from_millis(100),
            Duration::from_secs(1)
        ]
    );
}

/// Port of `DaemonSessionTests.StopperWaitsForTheLockOfTheDaemon` (reported from lot 7.4).
#[test]
fn the_stopper_waits_for_the_lock_of_the_daemon() {
    let folder = Folder::new();
    let lock_file = folder.0.join("daemon.lock");
    let held = take(&lock_file).expect("the lock");
    assert!(is_held(&lock_file));
    let waiting = {
        let lock_file = lock_file.clone();
        std::thread::spawn(move || wait_for_release(&lock_file, Duration::from_secs(10)))
    };
    std::thread::sleep(Duration::from_millis(300));
    assert!(!waiting.is_finished());
    drop(held);
    assert!(waiting.join().expect("the wait"));
    assert!(wait_for_release(
        &folder.0.join("absent.lock"),
        Duration::ZERO
    ));
}

fn golden() -> Option<Value> {
    let name = if cfg!(windows) {
        "services.json"
    } else {
        "services-unix.json"
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    let text = fs::read_to_string(path).ok()?;
    Some(serde_json::from_str(&text).expect("the golden file is valid JSON"))
}

fn text(value: &Value) -> String {
    value.as_str().expect("a string").to_owned()
}

fn pairs(value: &Value) -> Vec<(String, Option<String>)> {
    value
        .as_array()
        .expect("pairs")
        .iter()
        .map(|pair| (text(&pair[0]), pair[1].as_str().map(str::to_owned)))
        .collect()
}

fn described(value: &Value) -> ServiceDefinition {
    ServiceDefinition {
        executable: text(&value["executable"]),
        arguments: value["arguments"]
            .as_array()
            .expect("arguments")
            .iter()
            .map(|argument| argument.as_str().unwrap_or_default().to_owned())
            .collect(),
        version: text(&value["version"]),
        environment: pairs(&value["environment"]),
    }
}

fn base64(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for b in text.bytes().filter(|b| *b != b'=') {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => panic!("not base64: {b}"),
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xFF).expect("a byte"));
        }
    }
    out
}

/// The texts of the managers, `QuoteArgument`, `Same`, `CarriedEnvironment`, the bytes of
/// `service.json` and the reads of 23 crafted `service.json` files (minor 2 of the review of
/// lot 7.6: replayed without `SMF_ORACLE`), against what the .NET oracle did.
#[test]
fn the_service_texts_match_the_oracle() {
    let golden =
        golden().expect("rust/tests/golden/services*.json, from rust/tools/services-oracle");
    assert_eq!(
        golden["stopTimeoutSeconds"].as_u64(),
        Some(stop_timeout().as_secs())
    );
    let users = [r"PC\jérôme", r"PC\a&b<c>"];
    for (index, entry) in golden["definitions"]
        .as_array()
        .expect("definitions")
        .iter()
        .enumerate()
    {
        let definition = described(entry);
        match unit(&definition) {
            Ok(unit) => assert_eq!(Some(unit.as_str()), entry["unit"].as_str(), "unit {index}"),
            Err(error) => assert_eq!(Some(error.code.as_str()), entry["unitError"].as_str()),
        }
        assert_eq!(
            plist(&definition, Path::new("/logs")),
            text(&entry["plist"]),
            "plist {index}"
        );
        assert_eq!(
            task_xml(&definition, users[index % users.len()]),
            text(&entry["taskXml"]),
            "task {index}"
        );
        assert_eq!(
            services::definition_json(&definition).into_bytes(),
            base64(&text(&entry["serviceJson"])),
            "service.json {index}"
        );
    }
    for pair in golden["quotes"].as_array().expect("quotes") {
        assert_eq!(quote_argument(&text(&pair[0])), text(&pair[1]));
    }
    for case in golden["same"].as_array().expect("comparisons") {
        assert_eq!(
            same(Some(&described(&case["left"])), &described(&case["right"])),
            case["same"].as_bool().expect("a boolean"),
            "{case}"
        );
    }
    for case in golden["carried"].as_array().expect("environments") {
        let (mut dropped, mut narrowed) = (Vec::new(), Vec::new());
        let carried = plain(&[]);
        let kept = ServiceDefinition {
            environment: pairs(&case["environment"]),
            ..carried
        }
        .carried_environment(&mut |name| dropped.push(Value::from(name)), &mut |name| {
            narrowed.push(Value::from(name))
        });
        let kept: Vec<_> = kept.into_iter().map(|(k, v)| (k, Some(v))).collect();
        assert_eq!(kept, pairs(&case["carried"]), "{case}");
        assert_eq!(Value::from(dropped), case["dropped"], "{case}");
        assert_eq!(Value::from(narrowed), case["narrowed"], "{case}");
    }
    for (index, case) in golden["reads"]
        .as_array()
        .expect("reads")
        .iter()
        .enumerate()
    {
        let read = services::sound_definition(&base64(&text(&case["content"])));
        let expected = (!case["read"].is_null()).then(|| described(&case["read"]));
        assert_eq!(read, expected, "read {index}");
    }
}
