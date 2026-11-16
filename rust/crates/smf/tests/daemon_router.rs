//! Lot 7.5: the command line of the port through a daemon (`router` and `DaemonClient`), in real
//! processes. Ports of `RouterTests`; with `SMF_ORACLE` (the `smf` of .NET 0.4.3), the `smf` of
//! the port is also routed by the daemon of the oracle and the `smf` of the oracle by the daemon
//! of the port, and their outputs are compared. Every home is fresh, under the temporary
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
    let _daemon = home.daemon(SMF);
    let programs = std::iter::once(SMF.to_owned()).chain(oracle());
    for program in programs {
        let routed = home.cli(&program, &folder, false, &["status", "--json"]);
        assert_eq!(routed.exit_code, Some(0), "{program}: {routed}");
        assert_eq!(routed.mode(), "direct", "{program}: {routed}");
    }
    assert_eq!(
        home.cli(SMF, &root, false, &["status", "--json"]).mode(),
        "daemon",
        "the instance itself is served"
    );
    // The daemon is reached from that folder and answers `not_serving`: the cwd the port sends
    // (U+FFFD for the surrogate) names no registered folder.
    let cwd = folder.to_string_lossy().into_owned();
    assert!(cwd.contains('\u{FFFD}'), "{cwd}");
    let paths = home.paths();
    let answer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(async {
            let mut client = DaemonClient::connect(&paths, &SystemOwnership, VERSION, &cwd, LIMIT)
                .await
                .expect("no refusal")
                .expect("a daemon answers");
            client
                .run(&["status".to_owned(), "--json".to_owned()])
                .await
                .expect("an answer")
        });
    assert_eq!(answer.kind, kind::NOT_SERVING, "{answer:?}");
}
