//! Lot 7.4: the daemon of the port (`smf daemon run`) seen from a raw client (`endpoint` and
//! `framing`, the client of lot 7.5 does not exist yet). Ports of the server cases of
//! `DaemonTests`, `DaemonSessionTests`, `DaemonLoadTests` and `DaemonRobustnessTests`; with
//! `SMF_ORACLE` (the `smf` of .NET 0.4.3), every exchange is replayed on the daemon of the
//! oracle and must get the same answers, and the .NET `smf` runs its commands through the
//! daemon of the port. Every home is fresh, under the temporary directory; every daemon is
//! killed at the end of its test, even on failure.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_daemon::endpoint::connect;
use sermofur_daemon::framing::{self, IpcMessage, MAX_BODY_BYTES, MAX_RESPONSE_BYTES, kind};
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::read_message;
use tokio::io::AsyncWriteExt;

const SMF: &str = env!("CARGO_BIN_EXE_smf");
const LIMIT: Duration = Duration::from_secs(30);

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
            "smfs-{name}-{}-{}",
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

    /// A new instance, registered when asked.
    fn instance(&self, name: &str, registered: bool) -> PathBuf {
        let root = self.0.join(name);
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        if registered {
            InstanceRegistry::new(&self.paths().registry_file())
                .register(&root, DateTimeOffset::now_utc())
                .expect("registered");
        }
        root
    }

    /// `daemon run` of `program` under this home, once it says it listens.
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
        let line = lines
            .recv_timeout(LIMIT)
            .expect("the daemon listens in time");
        assert!(line.contains("listening on"), "{line}");
        daemon
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A daemon process, killed on drop.
struct Daemon(Child);

impl Daemon {
    /// Waits for the end of the process; `None` past the limit.
    fn exit(&mut self) -> Option<i32> {
        for _ in 0..300 {
            if let Some(status) = self.0.try_wait().expect("the status") {
                return status.code();
            }
            thread::sleep(Duration::from_millis(100));
        }
        None
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn oracle() -> Option<String> {
    std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty())
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(async { tokio::time::timeout(LIMIT, future).await.expect("in time") })
}

/// What a client sends: a message, or raw bytes.
#[derive(Clone)]
enum Item {
    Message(Box<IpcMessage>),
    Bytes(Vec<u8>),
}

/// One connection: each item is sent, then one answer is read (`None`: closed). The session
/// ends after the last item.
fn exchange(paths: &DaemonPaths, items: &[Item]) -> Vec<Option<IpcMessage>> {
    block_on(async {
        let mut stream = connect(paths, &SystemOwnership, Duration::from_secs(5))
            .await
            .expect("no refusal")
            .expect("a daemon answers");
        let mut answers = Vec::new();
        for item in items {
            let bytes = match item {
                Item::Message(message) => framing::encode(message, usize::MAX).expect("encoded"),
                Item::Bytes(bytes) => bytes.clone(),
            };
            if stream.write_all(&bytes).await.is_err() {
                answers.push(None);
                break;
            }
            let _ = stream.flush().await;
            answers.push(
                read_message(&mut stream, MAX_RESPONSE_BYTES)
                    .await
                    .ok()
                    .flatten(),
            );
        }
        answers
    })
}

fn hello(version: &str, cwd: &Path) -> Item {
    let mut message = IpcMessage::new(kind::HELLO);
    message.protocol = Some(1);
    message.tool_version = Some(version.to_owned());
    message.cwd = Some(cwd.to_string_lossy().into_owned());
    message.pid = Some(4242);
    Item::Message(Box::new(message))
}

fn run(id: i64, cwd: &Path, argv: &[&str]) -> Item {
    let mut message = IpcMessage::new(kind::RUN);
    message.id = Some(id);
    message.cwd = Some(cwd.to_string_lossy().into_owned());
    message.argv = Some(argv.iter().map(|a| Some((*a).to_owned())).collect());
    Item::Message(Box::new(message))
}

fn just(kind: &str) -> Item {
    Item::Message(Box::new(IpcMessage::new(kind)))
}

const VERSION: &str = sermofur_core::VERSION;

/// The exchanges compared between the two daemons: hello, run, and each refusal.
fn scenarios(registered: &Path, unregistered: &Path) -> Vec<(&'static str, Vec<Item>)> {
    let mut wrong_protocol = IpcMessage::new(kind::HELLO);
    wrong_protocol.protocol = Some(2);
    wrong_protocol.tool_version = Some(VERSION.to_owned());
    wrong_protocol.cwd = Some(registered.to_string_lossy().into_owned());
    let mut other_cwd = IpcMessage::new(kind::RUN);
    other_cwd.id = Some(1);
    other_cwd.argv = Some(vec![Some("status".to_owned())]);
    other_cwd.cwd = Some(unregistered.to_string_lossy().into_owned());
    let mut too_large = (300_000u32).to_le_bytes().to_vec();
    too_large.extend_from_slice(b"{}");
    vec![
        (
            "status_through_the_daemon",
            vec![
                hello(VERSION, registered),
                run(1, registered, &["status", "--json"]),
                run(2, registered, &["claim", "list"]),
            ],
        ),
        (
            "command_error_through_the_daemon",
            vec![
                hello(VERSION, registered),
                run(7, registered, &["claim", "show", "nope"]),
            ],
        ),
        ("version_mismatch", vec![hello("9.9.9", registered)]),
        ("hello_expected", vec![just(kind::STATUS)]),
        ("unknown_first_kind", vec![just("bogus")]),
        (
            "wrong_protocol",
            vec![Item::Message(Box::new(wrong_protocol))],
        ),
        ("relative_cwd", vec![hello(VERSION, Path::new("relative"))]),
        (
            "not_serving",
            vec![
                hello(VERSION, unregistered),
                run(3, unregistered, &["status"]),
            ],
        ),
        (
            "cli_only",
            vec![hello(VERSION, registered), run(4, registered, &["init"])],
        ),
        (
            "unknown_kind_in_session",
            vec![hello(VERSION, registered), just("bogus")],
        ),
        (
            "run_without_its_cwd",
            vec![
                hello(VERSION, registered),
                Item::Message(Box::new(other_cwd)),
            ],
        ),
        ("frame_too_large", vec![Item::Bytes(too_large)]),
        (
            "malformed_frame",
            vec![Item::Bytes(vec![2, 0, 0, 0, b'{', b'x'])],
        ),
    ]
}

/// The answers of a daemon to every scenario.
fn answers(program: &str, home: &Home, registered: &Path, unregistered: &Path) -> Vec<String> {
    let _daemon = home.daemon(program);
    let paths = home.paths();
    scenarios(registered, unregistered)
        .into_iter()
        .map(|(name, items)| format!("{name}: {:?}", exchange(&paths, &items)))
        .collect()
}

#[test]
fn the_daemon_runs_registered_instances_and_refuses_the_rest() {
    let home = Home::new("answers");
    let registered = home.instance("registered", true);
    let unregistered = home.instance("unregistered", false);
    let port = answers(SMF, &home, &registered, &unregistered);
    let status = &port[0];
    assert!(status.contains("kind: \"welcome\""), "{status}");
    assert!(status.contains(r#"\"mode\": \"daemon\""#), "{status}");
    assert!(port[2].contains("daemon_version_mismatch"), "{}", port[2]);
    assert!(port[7].contains("not_serving"), "{}", port[7]);
    assert!(port[8].contains("cli_only"), "{}", port[8]);
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: answers of the oracle not compared");
        return;
    };
    let dotnet = answers(&oracle, &home, &registered, &unregistered);
    for (port, dotnet) in port.iter().zip(&dotnet) {
        assert_eq!(port, dotnet);
    }
}

/// `status` of the .NET tool through the daemon of the port: mode `daemon`, the rest as
/// when it runs directly.
#[test]
fn the_dotnet_command_line_runs_through_the_daemon_of_the_port() {
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: skipped");
        return;
    };
    let home = Home::new("dotnet-client");
    let root = home.instance("instance", true);
    let dotnet = |no_daemon: bool| {
        let mut command = Command::new(&oracle);
        command
            .args(["status", "--json"])
            .current_dir(&root)
            .env(HOME_VARIABLE, home.home());
        if no_daemon {
            command.env("SERMOFUR_NO_DAEMON", "1");
        } else {
            command.env_remove("SERMOFUR_NO_DAEMON");
        }
        let output = command.output().expect("the oracle runs");
        assert_eq!(output.status.code(), Some(0));
        String::from_utf8(output.stdout).expect("UTF-8")
    };
    let direct = dotnet(true);
    assert!(direct.contains(r#""mode": "direct""#), "{direct}");
    let _daemon = home.daemon(SMF);
    let served = dotnet(false);
    assert_eq!(
        served,
        direct.replace(r#""mode": "direct""#, r#""mode": "daemon""#)
    );
    let claims = Command::new(&oracle)
        .args(["claim", "list"])
        .current_dir(&root)
        .env(HOME_VARIABLE, home.home())
        .output()
        .expect("the oracle runs");
    let direct = Command::new(&oracle)
        .args(["claim", "list"])
        .current_dir(&root)
        .env(HOME_VARIABLE, home.home())
        .env("SERMOFUR_NO_DAEMON", "1")
        .output()
        .expect("the oracle runs");
    assert_eq!(
        (claims.status.code(), claims.stdout, claims.stderr),
        (direct.status.code(), direct.stdout, direct.stderr)
    );
}

/// `DaemonLoadTests`: clients served at the same time, each on its own connection.
#[test]
fn simultaneous_clients_are_all_served() {
    let home = Home::new("load");
    let root = home.instance("instance", true);
    let _daemon = home.daemon(SMF);
    let paths = home.paths();
    let clients: Vec<_> = (0..8)
        .map(|n| {
            let paths = paths.clone();
            let root = root.clone();
            thread::spawn(move || {
                exchange(
                    &paths,
                    &[
                        hello(VERSION, &root),
                        run(n, &root, &["claim", "list", "--json"]),
                    ],
                )
            })
        })
        .collect();
    for client in clients {
        let answers = client.join().expect("a client");
        let result = answers[1].as_ref().expect("an answer");
        assert_eq!(
            (result.kind.as_str(), result.exit_code),
            ("result", Some(0))
        );
    }
}

/// A shutdown request: `stopping`, then the process ends with 0 and the journal says so.
#[test]
fn a_shutdown_request_stops_the_daemon() {
    let home = Home::new("shutdown");
    let root = home.instance("instance", true);
    let mut daemon = home.daemon(SMF);
    let paths = home.paths();
    let status = exchange(&paths, &[hello(VERSION, &root), just(kind::STATUS)]);
    let status = status[1].as_ref().expect("a status");
    assert_eq!(status.tool_version.as_deref(), Some(VERSION));
    assert_eq!(status.instances_open, Some(0));
    assert_eq!(status.clients, Some(1));
    let answers = exchange(&paths, &[just(kind::SHUTDOWN)]);
    assert_eq!(
        answers[0].as_ref().map(|m| m.kind.as_str()),
        Some("stopping")
    );
    assert_eq!(daemon.exit(), Some(0));
    let journal = fs::read_to_string(paths.log_file()).expect("a journal");
    for event in ["started", "shutdown_requested", "stopped"] {
        assert!(
            journal.contains(&format!("\"event\":\"{event}\"")),
            "{journal}"
        );
    }
    let limit = MAX_BODY_BYTES;
    assert!(limit < MAX_RESPONSE_BYTES);
}

/// `SecondDaemonOnTheSameEndpointIsRefused` for the whole command: the lock refuses it.
#[test]
fn a_second_daemon_is_refused() {
    let home = Home::new("second");
    let _daemon = home.daemon(SMF);
    let output = Command::new(SMF)
        .args(["daemon", "run"])
        .env(HOME_VARIABLE, home.home())
        .output()
        .expect("smf runs");
    assert_eq!(output.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&output.stderr).contains("daemon_already_running"));
}
