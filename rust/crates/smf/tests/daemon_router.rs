//! Lots 7.5 and 7.6: the command line of the port through a daemon (`router` and `DaemonClient`), in real
//! processes. Ports of `RouterTests`; with `SMF_ORACLE` (the `smf` of .NET 0.4.3), the `smf` of
//! the port is also routed by the daemon of the oracle and the `smf` of the oracle by the daemon
//! of the port, and their outputs are compared. Lot 7.6 adds `daemon register`, `unregister`,
//! `instances` and `status`, compared with those of the oracle on fresh homes. Every home is fresh, under the temporary
//! directory; every daemon is stopped at the end of its test, even on failure. The tests of this
//! file run one at a time, as the `Daemon timing` collection of .NET: their timeouts and their
//! measures must not compete for the machine.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::DaemonClient;
use sermofur_daemon::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_daemon::endpoint;
use sermofur_daemon::framing::{self, IpcMessage, MAX_BODY_BYTES, kind};
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::{
    CommandExecutor, CommandOutcome, CommandPlan, DaemonLimits, DaemonServer, ServingGate, first,
    read_message,
};
use tokio::io::AsyncWriteExt;

const SMF: &str = env!("CARGO_BIN_EXE_smf");
const VERSION: &str = sermofur_core::VERSION;
const LIMIT: Duration = Duration::from_secs(30);

/// One test at a time (`[Collection("Daemon timing")]`).
fn alone() -> std::sync::MutexGuard<'static, ()> {
    static TURN: Mutex<()> = Mutex::new(());
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn oracle() -> Option<String> {
    std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty())
}

/// A fresh folder under the temporary directory with a daemon home, removed on drop.
struct Home(PathBuf);

impl Home {
    fn new(name: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(format!(
            "smfr-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(folder.join("home")).expect("the home");
        Self(folder)
    }

    fn home(&self) -> PathBuf {
        self.0.join("home")
    }

    fn paths(&self) -> DaemonPaths {
        let text = self.home().to_string_lossy().into_owned();
        DaemonPaths::resolve(&|key| (key == HOME_VARIABLE).then(|| text.clone()), &|_| {
            String::new()
        })
        .expect("the paths of a home")
    }

    /// A new instance (`TestInstance`).
    fn instance(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        root
    }

    /// `daemon register` (lot 7.6): the instance goes in the registry of this home.
    fn register(&self, root: &Path) {
        InstanceRegistry::new(&self.paths().registry_file())
            .register(root, DateTimeOffset::now_utc())
            .expect("registered");
    }

    /// `RealDaemon.Start`: `daemon run` of `program` under this home, once it listens.
    fn daemon(&self, program: &str) -> Daemon {
        let mut child = Command::new(program)
            .args(["daemon", "run"])
            .env(HOME_VARIABLE, self.home())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the daemon starts");
        let stdout = child.stdout.take().expect("stdout");
        let daemon = Daemon(child);
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            let mut line = String::new();
            let _ = BufReader::new(stdout).read_line(&mut line);
            let _ = sender.send(line);
        });
        let line = lines.recv_timeout(LIMIT).expect("the daemon listens");
        assert!(line.contains("listening on"), "{program}: {line}");
        daemon
    }

    /// `RealDaemon.Cli`: `program` in `cwd` under this home, through the daemon or not.
    fn cli(&self, program: &str, cwd: &Path, no_daemon: bool, args: &[&str]) -> Run {
        cli(
            program,
            &self.home(),
            cwd,
            no_daemon,
            args.iter().map(OsStr::new),
        )
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A daemon process, killed on drop (`RealDaemon.StopAsync`).
struct Daemon(Child);

impl Daemon {
    /// A `shutdown` request, then the end of the process: the endpoint is gone on every system
    /// (a killed daemon leaves its socket on Unix).
    fn shutdown(mut self, paths: &DaemonPaths) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let answer = runtime.block_on(async {
            let mut stream = endpoint::connect(paths, &SystemOwnership, Duration::from_secs(5))
                .await
                .expect("no refusal")
                .expect("a daemon answers");
            let frame =
                framing::encode(&IpcMessage::new(kind::SHUTDOWN), MAX_BODY_BYTES).expect("encoded");
            stream.write_all(&frame).await.expect("sent");
            read_message(&mut stream, MAX_BODY_BYTES)
                .await
                .ok()
                .flatten()
        });
        assert_eq!(answer.map(|a| a.kind), Some("stopping".to_owned()));
        let started = Instant::now();
        while self.0.try_wait().expect("the status").is_none() {
            assert!(started.elapsed() < LIMIT, "the daemon does not stop");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// `RawCliResult`: exit code and raw output bytes.
#[derive(Debug, PartialEq, Eq)]
struct Run {
    exit_code: Option<i32>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

impl Run {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.stdout).unwrap_or_else(|_| panic!("JSON: {self}"))
    }

    fn mode(&self) -> String {
        self.json()["mode"].as_str().expect("a mode").to_owned()
    }

    /// The `daemon` check of a `doctor --json`.
    fn daemon_check(&self) -> Value {
        self.json()["checks"]
            .as_array()
            .expect("checks")
            .iter()
            .find(|check| check["name"] == "daemon")
            .expect("a daemon check")
            .clone()
    }
}

impl std::fmt::Display for Run {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "exit {:?}\n{}{}",
            self.exit_code,
            String::from_utf8_lossy(&self.stdout),
            String::from_utf8_lossy(&self.stderr)
        )
    }
}

fn cli<'a>(
    program: &str,
    home: &Path,
    cwd: &Path,
    no_daemon: bool,
    args: impl IntoIterator<Item = &'a OsStr>,
) -> Run {
    let mut command = Command::new(program);
    command
        .args(args)
        .current_dir(cwd)
        .env(HOME_VARIABLE, home)
        .stdin(Stdio::null());
    if no_daemon {
        command.env("SERMOFUR_NO_DAEMON", "1");
    } else {
        command.env_remove("SERMOFUR_NO_DAEMON");
    }
    let output = command.output().expect("the command line runs");
    Run {
        exit_code: output.status.code(),
        stdout: output.stdout,
        stderr: output.stderr,
    }
}

/// Hash-free `TestInstance.Snapshot`: every file under `.sermofur` with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(folder: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(folder).expect("a folder") {
            let path = entry.expect("an entry").path();
            if path.is_dir() {
                walk(&path, files);
            } else {
                let bytes = fs::read(&path).expect("a file");
                files.insert(path, bytes);
            }
        }
    }
    let mut files = BTreeMap::new();
    walk(&root.join(".sermofur"), &mut files);
    files
}

/// Runs `serve` on a runtime of its own thread until the value is dropped.
struct Served {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Served {
    fn start<F: Future<Output = ()> + 'static>(
        serve: impl FnOnce(Arc<AtomicBool>) -> F + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let (ready, listening) = mpsc::channel();
        let thread = thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            runtime.block_on(async move {
                let stopped = Arc::clone(&flag);
                let serving = serve(flag);
                let _ = ready.send(());
                let _ = first(serving, async {
                    while !stopped.load(Ordering::SeqCst) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                    }
                })
                .await;
            });
        });
        listening.recv_timeout(LIMIT).expect("serving");
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Served {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Plans nothing: the daemon of another version never gets a command.
struct NoCommand;

impl CommandExecutor for NoCommand {
    fn plan(&self, _argv: &[String], _cwd: &str) -> Result<CommandPlan> {
        Err(SermofurError::new("internal_error", "No command here.", 70))
    }

    fn execute(&self, _argv: &[String], _cwd: &str) -> CommandOutcome {
        unreachable!("never planned")
    }
}

/// `TestDaemon.Start(version: …)`: a real server of the port, in this process, that reports
/// `version`.
fn daemon_of_version(home: &Home, version: &'static str) -> Served {
    let paths = home.paths();
    let opened = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&opened);
    let served = Served::start(move |_| async move {
        let server = DaemonServer::new(
            paths.clone(),
            version,
            Arc::new(NoCommand),
            ServingGate::new(InstanceRegistry::new(&paths.registry_file())),
            DaemonLog::new(&paths.log_file()),
            DaemonLimits::default(),
        );
        let _ = server
            .run(&SystemOwnership, || seen.store(true, Ordering::SeqCst))
            .await;
    });
    wait_until("the daemon listens", || opened.load(Ordering::SeqCst));
    served
}

/// `StubDaemon.Silent` before the hello: a daemon alive that never answers.
fn silent_daemon(home: &Home) -> Served {
    let paths = home.paths();
    let opened = Arc::new(AtomicBool::new(false));
    let seen = Arc::clone(&opened);
    let served = Served::start(move |_| async move {
        let mut listener = endpoint::listen(&paths, &SystemOwnership)
            .await
            .expect("the endpoint opens");
        seen.store(true, Ordering::SeqCst);
        let mut held = Vec::new();
        while let Ok(stream) = listener.accept().await {
            held.push(stream);
        }
    });
    wait_until("the endpoint opens", || opened.load(Ordering::SeqCst));
    served
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < LIMIT, "{what}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// The commands of `RoutedCommandsWriteWhatTheDirectCliWrites`, after the seeding writes.
fn compared_commands(claim: &str) -> Vec<Vec<String>> {
    // A GUID that names no record (`Guid.NewGuid()` once for both runs in .NET).
    let unknown = "0f8c2a51-6a8e-4f0e-9a4b-3d2e1c0b9a87";
    [
        &["claim", "list", "--json"][..],
        &["claim", "show", claim],
        &["evidence", "list"],
        &["retex", "list", "--json"],
        &["scope", "tree"],
        &["scope", "current", "--json"],
        &["recall", "parity"],
        &["challenge", claim, "--json"],
        &["root"],
        &["claim", "show", unknown],
        &["claim", "nope"],
        &["claim", "add", "x", "--origin", "nobody", "--json"],
        &["export", "--json"],
    ]
    .iter()
    .map(|command| command.iter().map(|word| (*word).to_owned()).collect())
    .collect()
}

/// Each command of `program` gives the same bytes and exit code through the daemon of `home`
/// as directly.
fn assert_routed_as_direct(program: &str, home: &Home, root: &Path, claim: &str) {
    for command in compared_commands(claim) {
        let words: Vec<&str> = command.iter().map(String::as_str).collect();
        let direct = home.cli(program, root, true, &words);
        let routed = home.cli(program, root, false, &words);
        assert_eq!(
            direct,
            routed,
            "{program} {} differs:\n{direct}\n---\n{routed}",
            command.join(" ")
        );
    }
    let direct = home.cli(program, root, true, &["status", "--json"]);
    let routed = home.cli(program, root, false, &["status", "--json"]);
    assert_eq!(direct.mode(), "direct", "{direct}");
    assert_eq!(routed.mode(), "daemon", "{routed}");
    let mut served = routed.json();
    served["mode"] = Value::from("direct");
    assert_eq!(served, direct.json());
}

/// Port of `RouterTests.RoutedCommandsWriteWhatTheDirectCliWrites`. With `SMF_ORACLE`, the
/// `smf` of the port is also routed by the daemon of the oracle, and the `smf` of the oracle
/// by the daemon of the port: the same bytes as directly, `mode` of `status` apart.
#[test]
fn routed_commands_write_what_the_direct_cli_writes() {
    let _alone = alone();
    let home = Home::new("parity");
    let root = home.instance("instance");
    let daemon = home.daemon(SMF);
    let added = home.cli(
        SMF,
        &root,
        false,
        &["claim", "add", "parity ée", "--origin", "user", "--json"],
    );
    let claim = added.json()["id"].as_str().expect("an id").to_owned();
    // Not checked, as in .NET: `observation` is not an evidence kind, and the refusal is one
    // more command that must not change what the routed commands print.
    home.cli(
        SMF,
        &root,
        false,
        &[
            "evidence",
            "add",
            &claim,
            "observation",
            "seen",
            "--lineage",
            "test",
            "--origin",
            "user",
        ],
    );
    let retex = home.cli(
        SMF,
        &root,
        false,
        &[
            "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin", "llm",
        ],
    );
    assert_eq!(retex.exit_code, Some(0), "{retex}");
    home.register(&root);
    assert_routed_as_direct(SMF, &home, &root, &claim);
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: no cross reading");
        return;
    };
    assert_routed_as_direct(&oracle, &home, &root, &claim);
    drop(daemon);
    let dotnet = Home::new("parity-dotnet");
    dotnet.register(&root);
    let _daemon = dotnet.daemon(&oracle);
    assert_routed_as_direct(SMF, &dotnet, &root, &claim);
}

/// Port of `RouterTests.WritesThroughTheDaemonAreSeenDirectly`.
#[test]
fn writes_through_the_daemon_are_seen_directly() {
    let _alone = alone();
    let home = Home::new("writes");
    let root = home.instance("instance");
    let _daemon = home.daemon(SMF);
    home.register(&root);
    let added = home.cli(
        SMF,
        &root,
        false,
        &[
            "claim",
            "add",
            "via the daemon",
            "--origin",
            "user",
            "--json",
        ],
    );
    assert_eq!(added.exit_code, Some(0), "{added}");
    let listed = home.cli(SMF, &root, true, &["claim", "list", "--json"]);
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("via the daemon"),
        "{listed}"
    );
}

/// Port of `RouterTests.UnregisteredInstanceAndStoppedDaemonRunDirectly`.
#[test]
fn unregistered_instance_and_stopped_daemon_run_directly() {
    let _alone = alone();
    let home = Home::new("stopped");
    let root = home.instance("instance");
    let daemon = home.daemon(SMF);
    let status = || home.cli(SMF, &root, false, &["status", "--json"]);
    assert_eq!(status().mode(), "direct");
    home.register(&root);
    assert_eq!(status().mode(), "daemon");
    drop(daemon);
    let watch = Instant::now();
    assert_eq!(status().mode(), "direct");
    eprintln!(
        "status with a stopped daemon: {} ms (process included)",
        watch.elapsed().as_millis()
    );
}

/// Port of `RouterTests.DaemonOfAnotherVersionStopsTheCommandWithoutWriting`: the refusal of
/// the hello stops a write (exit 3, `daemon_version_mismatch` with the remedy), nothing is
/// written, and `doctor` reports the daemon as an error (exit 5). With `SMF_ORACLE`, the oracle
/// meets the same daemon with the same code and exit codes.
#[test]
fn daemon_of_another_version_stops_the_command_without_writing() {
    let _alone = alone();
    let home = Home::new("other-version");
    let root = home.instance("instance");
    let _daemon = daemon_of_version(&home, "0.0.0-other");
    home.register(&root);
    let before = snapshot(&root);
    let programs = std::iter::once(SMF.to_owned()).chain(oracle());
    for program in programs {
        let refused = home.cli(
            &program,
            &root,
            false,
            &["claim", "add", "x", "--origin", "user", "--json"],
        );
        assert_eq!(refused.exit_code, Some(3), "{program}: {refused}");
        assert!(refused.stdout.is_empty(), "{program}: {refused}");
        let error: Value = serde_json::from_slice(&refused.stderr).expect("a JSON error");
        assert_eq!(error["code"], "daemon_version_mismatch", "{program}");
        assert!(
            error["message"]
                .as_str()
                .is_some_and(|m| m.contains("smf daemon restart")),
            "{program}: {error}"
        );
        assert_eq!(snapshot(&root), before, "{program}");
        let doctor = home.cli(&program, &root, false, &["doctor", "--json"]);
        assert_eq!(doctor.exit_code, Some(5), "{program}: {doctor}");
        let check = doctor.daemon_check();
        assert_eq!(check["status"], "error", "{program}: {check}");
        assert!(
            check["detail"]
                .as_str()
                .is_some_and(|d| d
                    .starts_with("daemon_version_mismatch: The daemon runs Sermofur 0.0.0-other;")),
            "{program}: {check}"
        );
    }
}

/// Port of `RouterTests.DoctorReportsTheDaemon`: `running: version …` while the daemon runs,
/// `absent` once it stopped on a `shutdown` request; on Unix, `not_answering` for the socket a
/// killed daemon leaves (the .NET test kills the daemon under Windows, where the pipe goes
/// with the process); and, as `StubDaemon.Silent`, `not_answering` for a daemon that never
/// answers the hello. With `SMF_ORACLE`, the daemon check of the oracle is the same.
#[test]
fn doctor_reports_the_daemon() {
    let _alone = alone();
    let home = Home::new("doctor");
    let root = home.instance("instance");
    let programs: Vec<String> = std::iter::once(SMF.to_owned()).chain(oracle()).collect();
    let checks = |expected: &str| {
        for program in &programs {
            let doctor = home.cli(program, &root, false, &["doctor", "--json"]);
            let check = doctor.daemon_check();
            assert_eq!(
                (check["status"].as_str(), check["detail"].as_str()),
                (
                    Some(expected.split_once('|').expect("status|detail").0),
                    Some(expected.split_once('|').expect("status|detail").1)
                ),
                "{program}: {doctor}"
            );
        }
    };
    let daemon = home.daemon(SMF);
    checks(&format!("ok|running: version {VERSION}."));
    daemon.shutdown(&home.paths());
    checks("warning|absent: commands run directly.");
    let killed = home.daemon(SMF);
    drop(killed);
    if cfg!(unix) {
        assert!(
            home.paths().endpoint.exists(),
            "a killed daemon leaves its socket"
        );
        checks("warning|not_answering: commands run directly.");
    } else {
        checks("warning|absent: commands run directly.");
    }
    let silent = silent_daemon(&home);
    let watch = Instant::now();
    checks("warning|not_answering: commands run directly.");
    eprintln!(
        "doctor with a silent daemon: {} ms for {} tools",
        watch.elapsed().as_millis(),
        programs.len()
    );
    drop(silent);
}

/// Port of `RouterTests.TenWritersNeverMeetStorageBusy` (SC-004 and SC-005): ten clients write
/// at once for 3 s (10 s in .NET; 60 s with `SERMOFUR_PERFORMANCE=1`) and never meet
/// `storage_busy`, nothing is lost or doubled, and the cost of the daemon stays within 50 ms
/// at the 95th percentile. .NET measures the direct run in its own process; a Rust test cannot
/// call the runner of the binary, so both sides are measured as processes: a routed `smf`
/// against a direct one.
#[test]
fn ten_writers_never_meet_storage_busy() {
    let _alone = alone();
    let home = Home::new("writers");
    let root = home.instance("instance");
    let _daemon = home.daemon(SMF);
    home.register(&root);
    let duration = if std::env::var("SERMOFUR_PERFORMANCE").as_deref() == Ok("1") {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(3)
    };
    let clock = Instant::now();
    let writers: Vec<_> = (0..10)
        .map(|writer| {
            let paths = home.paths();
            let root = root.to_string_lossy().into_owned();
            thread::spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("a runtime");
                runtime.block_on(async {
                    let mut client = DaemonClient::connect(
                        &paths,
                        &SystemOwnership,
                        VERSION,
                        &root,
                        Duration::from_secs(5),
                    )
                    .await
                    .expect("no refusal")
                    .expect("a daemon answers");
                    let mut count = 0;
                    while clock.elapsed() < duration {
                        let argv: Vec<String> = [
                            "claim",
                            "add",
                            &format!("w{writer}-{count}"),
                            "--origin",
                            "user",
                            "--key",
                            &format!("k{writer}-{count}"),
                            "--json",
                        ]
                        .iter()
                        .map(|word| (*word).to_owned())
                        .collect();
                        let result = client.run(&argv).await.expect("an answer");
                        assert_eq!(result.exit_code, Some(0), "{result:?}");
                        count += 1;
                    }
                    count
                })
            })
        })
        .collect();
    let written: usize = writers
        .into_iter()
        .map(|writer| writer.join().expect("a writer"))
        .sum();
    let listed = home.cli(SMF, &root, true, &["claim", "list", "--json"]);
    let stored = listed.json().as_array().map(Vec::len);
    assert_eq!(stored, Some(written));
    eprintln!(
        "{written} claims written by 10 clients in {} s",
        duration.as_secs()
    );
    let unknown = "claim show 7a1d3c55-0b6e-4c9f-8d2a-5e4f3b2a1c0d";
    let words: Vec<&str> = unknown.split(' ').collect();
    let mut direct = Vec::new();
    let mut routed = Vec::new();
    for _ in 0..60 {
        let one = Instant::now();
        home.cli(SMF, &root, true, &words);
        direct.push(one.elapsed());
        let one = Instant::now();
        let served = home.cli(SMF, &root, false, &words);
        routed.push(one.elapsed());
        assert_eq!(served.exit_code, Some(1), "{served}");
    }
    let p95 = |values: &mut Vec<Duration>| {
        values.sort();
        values[values.len() * 95 / 100 - 1]
    };
    let (direct, routed) = (p95(&mut direct), p95(&mut routed));
    let overhead = routed.as_secs_f64() * 1000.0 - direct.as_secs_f64() * 1000.0;
    eprintln!("p95 direct {direct:?}, through the daemon {routed:?}, overhead {overhead:.1} ms");
    assert!(overhead <= 50.0, "overhead {overhead:.1} ms");
}

/// Lot 7.5, named deviation « Substitut isolé à l'écriture d'une trame »: under Windows, an
/// argument of the command line can carry a lone surrogate. The port reads it with U+FFFD
/// (`to_string_lossy`); the oracle puts the UTF-16 in its `run` frame, where its encoder writes
/// `�`. The daemon of the port then stores U+FFFD for both tools: the claim written
/// through it by either tool, and directly by the port, has the same text.
#[cfg(windows)]
#[test]
fn a_lone_surrogate_in_an_argument_reaches_the_daemon_as_a_replacement_character() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let _alone = alone();
    let home = Home::new("surrogate");
    let root = home.instance("instance");
    let _daemon = home.daemon(SMF);
    home.register(&root);
    let text = OsString::from_wide(&[u16::from(b'a'), 0xD800, u16::from(b'b')]);
    let add = |program: &str, no_daemon: bool| {
        let args = [
            OsStr::new("claim"),
            OsStr::new("add"),
            &text,
            OsStr::new("--origin"),
            OsStr::new("user"),
            OsStr::new("--json"),
        ];
        let added = cli(program, &home.home(), &root, no_daemon, args);
        assert_eq!(added.exit_code, Some(0), "{program}: {added}");
        added.json()
    };
    let mode = home.cli(SMF, &root, false, &["status", "--json"]).mode();
    assert_eq!(mode, "daemon", "the instance is served");
    let routed = add(SMF, false);
    let direct = add(SMF, true);
    let statement = |claim: &Value| {
        let content = claim["contentJson"].as_str().expect("a content");
        serde_json::from_str::<Value>(content).expect("JSON")["text"].clone()
    };
    assert_eq!(statement(&routed), Value::from("a\u{FFFD}b"), "{routed}");
    assert_eq!(statement(&direct), statement(&routed));
    if let Some(oracle) = oracle() {
        let dotnet = add(&oracle, false);
        assert_eq!(statement(&dotnet), statement(&routed), "{dotnet}");
    }
}

/// The current folder with a lone surrogate under Windows, inside a registered instance: the
/// port sends its `cwd` with U+FFFD, as the frame of the oracle (`�`). The daemon finds
/// no such folder and answers `not_serving`: the command runs directly, for both tools.
#[cfg(windows)]
#[test]
fn a_lone_surrogate_in_the_current_folder_is_sent_as_the_oracle_sends_it() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    let _alone = alone();
    let home = Home::new("surrogate-cwd");
    let root = home.instance("instance");
    home.register(&root);
    let folder = root.join(OsString::from_wide(&[u16::from(b'i'), 0xDC00]));
    fs::create_dir_all(&folder).expect("a folder with a lone surrogate");
    let programs: Vec<String> = std::iter::once(SMF.to_owned()).chain(oracle()).collect();
    // Lot 7.6, minor 5 of the review of lot 7.5: the same, with SMF_ORACLE, on the daemon of
    // the oracle.
    for server in &programs {
        let daemon = home.daemon(server);
        for program in &programs {
            let routed = home.cli(program, &folder, false, &["status", "--json"]);
            assert_eq!(routed.exit_code, Some(0), "{server}, {program}: {routed}");
            assert_eq!(routed.mode(), "direct", "{server}, {program}: {routed}");
        }
        assert_eq!(
            home.cli(SMF, &root, false, &["status", "--json"]).mode(),
            "daemon",
            "{server}: the instance itself is served"
        );
        // The daemon is reached from that folder and answers `not_serving`: the cwd the port
        // sends (U+FFFD for the surrogate) names no registered folder.
        let cwd = folder.to_string_lossy().into_owned();
        assert!(cwd.contains('\u{FFFD}'), "{cwd}");
        let paths = home.paths();
        let answer = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(async {
                let mut client =
                    DaemonClient::connect(&paths, &SystemOwnership, VERSION, &cwd, LIMIT)
                        .await
                        .expect("no refusal")
                        .expect("a daemon answers");
                client
                    .run(&["status".to_owned(), "--json".to_owned()])
                    .await
                    .expect("an answer")
            });
        assert_eq!(answer.kind, kind::NOT_SERVING, "{server}: {answer:?}");
        eprintln!("surrogate cwd on the daemon of {server}: {}", answer.kind);
        daemon.shutdown(&paths);
    }
}

/// `program` with the daemon home `home`, from `cwd`: exit code, output and error, with `home`
/// written `<home>` (as typed and JSON-escaped) and each `registeredAt` value `<date>`.
fn shown(program: &str, home: &Path, cwd: &Path, args: &[&str]) -> String {
    let run = cli(program, home, cwd, true, args.iter().map(OsStr::new));
    let folder = home.to_string_lossy();
    let text = run
        .to_string()
        .replace(&folder.replace('\\', "\\\\"), "<home>")
        .replace(&*folder, "<home>");
    let key = "\"registeredAt\": \"";
    let (mut out, mut rest) = (String::new(), text.as_str());
    while let Some(at) = rest.find(key) {
        out.push_str(&rest[..at + key.len()]);
        rest = &rest[at + key.len()..];
        out.push_str("<date>");
        rest = &rest[rest.find('"').unwrap_or(rest.len())..];
    }
    out.push_str(rest);
    out
}

/// Port of `DaemonTests.RegistryCommandsWorkInARealProcess`, for the port and, with
/// `SMF_ORACLE`, for the oracle.
#[test]
fn registry_commands_work_in_a_real_process() {
    let _alone = alone();
    for program in std::iter::once(SMF.to_owned()).chain(oracle()) {
        let home = Home::new("registry");
        let root = home.instance("instance");
        let text = root.to_str().expect("a Unicode path");
        let run = |args: &[&str]| home.cli(&program, &home.0, true, args);
        let registered = run(&["daemon", "register", "--path", text, "--json"]);
        assert_eq!(registered.exit_code, Some(0), "{program}: {registered}");
        assert_eq!(registered.json()["root"], Value::from(text), "{program}");
        let listed = run(&["daemon", "instances", "--json"]);
        assert_eq!(listed.json().as_array().map(Vec::len), Some(1), "{listed}");
        let removed = run(&["daemon", "unregister", "--path", text]);
        assert_eq!(removed.exit_code, Some(0), "{program}: {removed}");
        let again = run(&["daemon", "unregister", "--path", text, "--json"]);
        assert_eq!(again.exit_code, Some(1), "{program}: {again}");
        let error: Value = serde_json::from_slice(&again.stderr).expect("a JSON error");
        assert_eq!(error["code"], "not_registered", "{program}");
        assert_eq!(run(&["daemon", "nope"]).exit_code, Some(1), "{program}");
    }
}

/// Lot 7.6, with `SMF_ORACLE`: `register`, `unregister`, `instances` and `status` of the port
/// print what the oracle prints, each on a fresh home, `registeredAt` apart; then a registry
/// written by one tool is read and changed by the other.
#[test]
fn registry_commands_print_what_the_oracle_prints() {
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: no cross reading");
        return;
    };
    let _alone = alone();
    let home = Home::new("registry-cross");
    let root = home.instance("instance");
    fs::create_dir_all(root.join("sub")).expect("a subfolder");
    home.instance("moved");
    let cwd = &home.0;
    let steps: &[&[&str]] = &[
        &["daemon", "instances"],
        &["daemon", "register", "--path", "instance"],
        &["daemon", "register", "--path", "instance/sub", "--json"],
        &["daemon", "register", "--path", "moved", "--json"],
        &["daemon", "register"],
        &["daemon", "register", "--path", "missing"],
        &["daemon", "instances", "--json"],
        &["daemon", "status", "--json"],
        &["daemon", "status"],
        &["daemon", "unregister", "--path", "instance/sub"],
        &["daemon", "unregister", "--path", "instance", "--json"],
        &["daemon", "unregister", "--path", "elsewhere"],
        &["daemon", "instances"],
        &["daemon"],
        &["daemon", "--json"],
        &["daemon", "nope"],
        &["daemon", "status", "extra"],
        &["daemon", "instances", "--path"],
        &["daemon", "instances", "--bogus", "x"],
    ];
    let (port_home, oracle_home) = (cwd.join("home-port"), cwd.join("home-oracle"));
    let compare = |step: &[&str], context: &str| {
        assert_eq!(
            shown(SMF, &port_home, cwd, step),
            shown(&oracle, &oracle_home, cwd, step),
            "{context}{}",
            step.join(" ")
        );
    };
    for step in steps {
        compare(step, "");
    }
    // A moved instance is listed as missing and removed by its registered path.
    fs::rename(cwd.join("moved"), cwd.join("gone")).expect("moved");
    compare(&["daemon", "instances", "--json"], "moved: ");
    compare(
        &["daemon", "unregister", "--path", "moved", "--json"],
        "moved: ",
    );
    // Written by one, read and changed by the other.
    let same_home = |writer: &str, reader: &str, home: &Path| {
        let add = ["daemon", "register", "--path", "instance"];
        let registered = shown(writer, home, cwd, &add);
        assert!(registered.starts_with("exit Some(0)"), "{registered}");
        let listed = |program: &str| shown(program, home, cwd, &["daemon", "instances", "--json"]);
        assert_eq!(listed(reader), listed(writer));
        let remove = ["daemon", "unregister", "--path", "instance"];
        assert_eq!(shown(reader, home, cwd, &remove), registered);
        assert_eq!(listed(writer), listed(reader));
    };
    same_home(&oracle, SMF, &oracle_home);
    same_home(SMF, &oracle, &port_home);
    // A registry that cannot be used is reported the same way, and left untouched.
    for content in ["not json", "{\"version\":2,\"instances\":[]}"] {
        for folder in [&port_home, &oracle_home] {
            fs::write(folder.join("config").join("instances.json"), content).expect("written");
        }
        compare(&["daemon", "instances"], content);
        compare(
            &["daemon", "register", "--path", "instance", "--json"],
            content,
        );
        let kept = fs::read_to_string(port_home.join("config").join("instances.json"));
        assert_eq!(kept.expect("kept"), content);
    }
    eprintln!("registry cross-read ok");
}

/// `daemon status --json` of `program` under `home`, with `clients` checked then left out: each
/// status is a client of its own, and the count of the previous one may not have dropped yet.
fn status_of(program: &str, home: &Home) -> Value {
    let run = home.cli(program, &home.0, true, &["daemon", "status", "--json"]);
    assert_eq!(run.exit_code, Some(0), "{program}: {run}");
    let mut status = run.json();
    if status["answering"] == Value::Bool(true) {
        assert!(
            status["clients"].as_i64().is_some_and(|n| n >= 1),
            "{status}"
        );
    }
    status["clients"] = Value::Null;
    status
}

/// Lot 7.6: `daemon status` reports the daemon of the home: absent, running and answering,
/// of another version, or silent before its hello. With `SMF_ORACLE`, the oracle prints the
/// same object, `clients` apart, for the daemon of the port and for its own.
#[test]
fn status_reports_the_daemon_as_the_oracle_does() {
    let _alone = alone();
    let home = Home::new("status");
    let programs: Vec<String> = std::iter::once(SMF.to_owned()).chain(oracle()).collect();
    let states = |expected: &[&str]| {
        let port = status_of(SMF, &home);
        assert!(
            port["state"]
                .as_str()
                .is_some_and(|s| expected.contains(&s)),
            "{port}"
        );
        for program in &programs[1..] {
            assert_eq!(status_of(program, &home), port, "{program}");
        }
        port
    };
    // Without a daemon: `absent`, or `service_manager_unavailable` (WSL without systemd).
    states(&["absent", "service_manager_unavailable"]);
    let daemon = home.daemon(SMF);
    let running = states(&["running"]);
    assert_eq!(
        (&running["answering"], &running["version"]),
        (&Value::Bool(true), &Value::from(VERSION))
    );
    assert_eq!(running["pid"], Value::from(daemon.0.id()), "{running}");
    daemon.shutdown(&home.paths());
    if let Some(oracle) = oracle() {
        let dotnet = home.daemon(&oracle);
        states(&["running"]);
        dotnet.shutdown(&home.paths());
    }
    let other = daemon_of_version(&home, "0.0.0-other");
    states(&["version_mismatch"]);
    drop(other);
    // Silent before its hello: no daemon answered within the second of the connection.
    let silent = silent_daemon(&home);
    states(&["absent", "service_manager_unavailable"]);
    drop(silent);
}

/// Lot 7.6, with `SMF_ORACLE`: `service.json` is read as the oracle reads it: `status` shows the
/// executable of a sound definition only, whatever the shape of the file.
#[test]
fn the_installed_definition_is_read_as_the_oracle_reads_it() {
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: no cross reading");
        return;
    };
    let _alone = alone();
    let home = Home::new("service-json");
    let state = home.paths().state_directory;
    fs::create_dir_all(&state).expect("the state folder");
    let exe = if cfg!(windows) {
        r"C:\\smf.exe"
    } else {
        "/smf"
    };
    let run = r#""arguments":["daemon","run"]"#;
    let sound = format!(r#"{{"executable":"{exe}",{run},"version":"1"}}"#);
    let texts = [
        sound.clone(),
        format!(r#"{{"executable":"a","executable":"{exe}",{run},"version":"1"}}"#),
        format!(r#"{{"executable":5,"executable":"{exe}",{run},"version":"1"}}"#),
        format!(r#"{{"executable":"\ud800x",{run},"version":"1"}}"#),
        format!("\u{FEFF}{sound}"),
        format!(r#"{{"executable":"{exe}","arguments":["daemon",null,"run"],"version":"1"}}"#),
        format!(
            r#"{{"executable":"{exe}","arguments":[null,"daemon","run"],"version":"1","environment":null}}"#
        ),
        format!(
            r#"{{"executable":"{exe}",{run},"version":"1","environment":{{"A":null,"A":"x"}}}}"#
        ),
        format!(r#"{{"executable":"{exe}",{run},"version":"1","environment":[]}}"#),
        format!(r#"{{"executable":"{exe}",{run},"version":"1","environment":{{"A":1}}}}"#),
        format!(r#"{{"Executable":"{exe}",{run},"version":"1"}}"#),
        format!(r#"{{"executable":"rel.exe",{run},"version":"\u00a0"}}"#),
        r#"{"executable":"rel","arguments":["--x","daemon","run","--supervise"],"version":"v"}"#
            .to_owned(),
        format!(r#"{{"executable":"{exe}",{run},"version":"1",}}"#),
        format!("{sound} "),
        "[]".to_owned(),
        "null".to_owned(),
        String::new(),
        format!(r#"{{"executable":"{exe}","arguments":"daemon run","version":"1"}}"#),
        format!(r#"{{"executable":"{exe}",{run},"version":1}}"#),
    ];
    let cases: Vec<Vec<u8>> = texts
        .into_iter()
        .map(String::into_bytes)
        .chain([
            // Invalid UTF-8 in a value read, then in a value skipped.
            b"{\"executable\":\"x\xff\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"1\"}"
                .to_vec(),
            b"{\"executable\":\"x\",\"arguments\":[\"daemon\",\"run\"],\"version\":\"1\",\"z\":\"\xff\"}"
                .to_vec(),
        ])
        .collect();
    let mut sound_cases = 0;
    for (index, content) in cases.iter().enumerate() {
        fs::write(state.join("service.json"), content).expect("written");
        let port = status_of(SMF, &home);
        assert_eq!(status_of(&oracle, &home), port, "case {index}");
        sound_cases += usize::from(port["executable"].is_string());
    }
    // A folder in place of the file reads as nothing installed.
    fs::remove_file(state.join("service.json")).expect("removed");
    fs::create_dir(state.join("service.json")).expect("a folder");
    assert_eq!(status_of(&oracle, &home), status_of(SMF, &home));
    assert!(sound_cases > 0, "no sound definition among the cases");
    eprintln!(
        "service.json cross-read ok: {} cases, {sound_cases} sound",
        cases.len()
    );
}

/// The pid of the daemon that answers on `paths`, once it differs from `other`.
fn wait_for_pid(paths: &DaemonPaths, other: Option<i32>) -> i32 {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        let pid = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime")
            .block_on(async {
                let mut client = DaemonClient::connect(
                    paths,
                    &SystemOwnership,
                    VERSION,
                    &std::env::temp_dir().to_string_lossy(),
                    Duration::from_millis(500),
                )
                .await
                .ok()
                .flatten()?;
                client.status().await.ok()?.pid
            });
        if let Some(pid) = pid.filter(|pid| Some(*pid) != other) {
            return pid;
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("The supervised daemon did not answer.");
}

/// Ends a process by its pid, as `Process.GetProcessById(pid).Kill()`.
fn kill(pid: i32) {
    let pid = pid.to_string();
    let status = if cfg!(windows) {
        Command::new("taskkill").args(["/F", "/PID", &pid]).output()
    } else {
        Command::new("kill").args(["-9", &pid]).output()
    };
    assert!(status.expect("the kill runs").status.success());
}

/// Port of `ServiceTests.SupervisorRestartsAKilledDaemonAndStopsCleanly`: the supervisor runs
/// a real `smf daemon run`, restarts it after a kill, and stops it on request.
#[test]
fn the_supervisor_restarts_a_killed_daemon_and_stops_cleanly() {
    use sermofur_daemon::supervisor::{ChildStart, Supervisor};
    let home = Home::new("supervisor");
    let paths = home.paths();
    let child = ChildStart {
        program: SMF.to_owned(),
        arguments: vec!["daemon".to_owned(), "run".to_owned()],
        environment: vec![(
            HOME_VARIABLE.to_owned(),
            home.home().to_string_lossy().into_owned(),
        )],
    };
    let log = DaemonLog::new(&home.0.join("supervisor.log"));
    let stopper = paths.clone();
    let supervisor = Supervisor::new(
        child,
        log,
        Box::new(move || {
            let _ = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime")
                .block_on(async {
                    let mut stream =
                        endpoint::connect(&stopper, &SystemOwnership, Duration::from_millis(500))
                            .await
                            .ok()??;
                    let frame =
                        framing::encode(&IpcMessage::new(kind::SHUTDOWN), MAX_BODY_BYTES).ok()?;
                    stream.write_all(&frame).await.ok()?;
                    read_message(&mut stream, MAX_BODY_BYTES).await.ok()?
                });
        }),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let (done, finished) = mpsc::channel();
    let asked = Arc::clone(&stop);
    let supervising = thread::spawn(move || {
        let _ = done.send(supervisor.run(&asked));
    });
    let first = wait_for_pid(&paths, None);
    kill(first);
    let back = Instant::now();
    let second = wait_for_pid(&paths, Some(first));
    assert!(
        back.elapsed() < Duration::from_secs(10),
        "back after {:?}",
        back.elapsed()
    );
    assert_ne!(first, second);
    stop.store(true, Ordering::SeqCst);
    assert_eq!(finished.recv_timeout(Duration::from_secs(20)), Ok(0));
    supervising.join().expect("the supervisor");
    let gone = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(endpoint::connect(
            &paths,
            &SystemOwnership,
            Duration::from_millis(500),
        ));
    assert!(matches!(gone, Ok(None)), "a daemon still answers");
}

/// `ServiceFactAttribute`: a test that installs a real service in the session of the current
/// user runs in CI (`GITHUB_ACTIONS=true`) or when `SERMOFUR_SERVICE_TESTS=1`, never by surprise
/// on a developer machine.
fn real_services_allowed() -> bool {
    std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true")
        || std::env::var("SERMOFUR_SERVICE_TESTS").as_deref() == Ok("1")
}

/// `RealServiceTests.SmfIn`: the real smf, without the daemon home nor the bypass of tests.
fn real_smf(folder: &Path, arguments: &[&str]) -> (i32, Value, String) {
    let output = Command::new(SMF)
        .args(arguments)
        .current_dir(folder)
        .env_remove(HOME_VARIABLE)
        .env_remove("SERMOFUR_NO_DAEMON")
        .output()
        .expect("smf runs");
    let json = serde_json::from_slice(&output.stdout).unwrap_or(Value::Null);
    (
        output.status.code().unwrap_or(-1),
        json,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn real_status(folder: &Path, arguments: &[&str]) -> Value {
    let (code, json, error) = real_smf(folder, arguments);
    assert_eq!(code, 0, "{arguments:?}: {error}");
    json
}

/// Every file under `root` with its bytes (`TestInstance.Snapshot`).
fn instance_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).expect("a folder").flatten() {
            let path = entry.path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.insert(path.clone(), fs::read(&path).unwrap_or_default());
            }
        }
    }
    files
}

/// Port of `RealServiceTests.InstallRestartAfterAKillPilotAndUninstall`, behind the guard of
/// `ServiceFactAttribute`. Under Windows the task runs `daemon run --supervise`, which waits for
/// the job object of the supervisor: the test is ignored there until then.
#[test]
#[cfg_attr(
    windows,
    ignore = "the Windows supervisor waits for its job object (decision on win32job)"
)]
fn a_real_service_installs_restarts_after_a_kill_and_uninstalls() {
    if !real_services_allowed() {
        eprintln!("Installs a real service: runs in CI, or set SERMOFUR_SERVICE_TESTS=1.");
        return;
    }
    let home = Home::new("real-service");
    let root = home.0.join("instance");
    fs::create_dir_all(&root).expect("the root");
    sermofur_core::init::initialize(&root).expect("an instance");
    let temporary = std::env::temp_dir();
    real_smf(&temporary, &["daemon", "uninstall"]);
    let root_text = root.to_string_lossy().into_owned();
    let tested = std::panic::catch_unwind(|| {
        let install = Instant::now();
        let installed = real_status(&temporary, &["daemon", "install", "--json"]);
        eprintln!("install: {:?}, {installed}", install.elapsed());
        assert_eq!(installed["state"], "running");
        assert!(install.elapsed() < Duration::from_secs(60));
        assert_eq!(installed["version"], VERSION);
        assert_eq!(
            real_smf(&temporary, &["daemon", "register", "--path", &root_text]).0,
            0
        );
        let data = instance_files(&root);
        assert_eq!(real_status(&root, &["status", "--json"])["mode"], "daemon");
        let first = installed["pid"].as_i64().expect("a pid");
        kill(i32::try_from(first).expect("a pid"));
        let back = Instant::now();
        let second = loop {
            assert!(
                back.elapsed() < Duration::from_secs(30),
                "the daemon is not back"
            );
            let status = real_status(&temporary, &["daemon", "status", "--json"]);
            if let Some(pid) = status["pid"].as_i64().filter(|pid| *pid != first) {
                break pid;
            }
            thread::sleep(Duration::from_millis(200));
        };
        eprintln!("back after a kill: {:?}", back.elapsed());
        assert!(back.elapsed() < Duration::from_secs(10));
        assert_ne!(first, second);
        for (arguments, state) in [
            (["daemon", "stop", "--json"], "installed_stopped"),
            (["daemon", "start", "--json"], "running"),
            (["daemon", "install", "--json"], "running"),
        ] {
            assert_eq!(real_status(&temporary, &arguments)["state"], state);
        }
        let uninstall = Instant::now();
        assert_eq!(
            real_status(&temporary, &["daemon", "uninstall", "--json"])["state"],
            "absent"
        );
        assert!(uninstall.elapsed() < Duration::from_secs(60));
        assert_eq!(instance_files(&root), data);
        let instances = real_smf(&temporary, &["daemon", "instances", "--json"]).1;
        let name = root
            .file_name()
            .expect("a name")
            .to_string_lossy()
            .into_owned();
        assert!(instances.to_string().contains(&name));
    });
    real_smf(&temporary, &["daemon", "uninstall"]);
    real_smf(&temporary, &["daemon", "unregister", "--path", &root_text]);
    if let Err(panic) = tested {
        std::panic::resume_unwind(panic);
    }
}

/// Review of lot 7.7 (Fort): under Windows, while the supervisor waits for the win32job
/// question, `install`, `start` and `restart` answer `not_ported` before a running daemon is
/// stopped or anything is written; `status` still runs.
#[cfg(windows)]
#[test]
fn install_start_and_restart_are_refused_under_windows_before_any_effect() {
    let _alone = alone();
    let home = Home::new("no-supervisor");
    let _daemon = home.daemon(SMF);
    for subcommand in ["install", "start", "restart"] {
        let output = std::process::Command::new(SMF)
            .args(["daemon", subcommand, "--json"])
            .env(HOME_VARIABLE, home.home())
            .output()
            .expect("smf runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_ne!(output.status.code(), Some(0), "{subcommand}: {stderr}");
        assert!(stderr.contains("not_ported"), "{subcommand}: {stderr}");
        assert!(
            !home.paths().state_directory.join("service.json").exists(),
            "{subcommand}"
        );
    }
    let status = std::process::Command::new(SMF)
        .args(["daemon", "status", "--json"])
        .env(HOME_VARIABLE, home.home())
        .output()
        .expect("smf runs");
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).expect("JSON");
    assert_eq!(status["state"], "running", "{status}");
}
