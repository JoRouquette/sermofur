//! The server with a controlled executor (`DaemonSessionTests`, `DaemonRobustnessTests`):
//! timeouts, the writing turn, a client gone, one run per connection, the drain and the size
//! of a response. Only codes and kinds are checked here; the texts are compared with the
//! daemon of the oracle by `smf/tests/daemon_server.rs`.

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use sermofur_core::error::Result;
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_daemon::endpoint::{Stream, connect};
use sermofur_daemon::framing::{self, IpcMessage, MAX_RESPONSE_BYTES, kind};
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::{
    CommandExecutor, CommandOutcome, CommandPlan, DaemonLimits, DaemonServer, ServingGate,
    read_message,
};
use tokio::io::AsyncWriteExt;

/// `argv[0]`: `sleep` (milliseconds in `argv[1]`), `big` (an output over 16 MiB) or anything
/// else (answers at once). Every command writes but `read`.
struct Fake;

impl CommandExecutor for Fake {
    fn plan(&self, argv: &[String], cwd: &str) -> Result<CommandPlan> {
        Ok(CommandPlan {
            path: cwd.to_owned(),
            writes: argv[0] != "read",
        })
    }

    fn execute(&self, argv: &[String], _cwd: &str) -> CommandOutcome {
        let stdout = match argv[0].as_str() {
            "sleep" => {
                thread::sleep(Duration::from_millis(argv[1].parse().expect("ms")));
                String::new()
            }
            "big" => "x".repeat(MAX_RESPONSE_BYTES + 1),
            _ => "done".to_owned(),
        };
        CommandOutcome {
            exit_code: 0,
            stdout,
            stderr: String::new(),
        }
    }
}

struct Fixture {
    folder: PathBuf,
    root: String,
    paths: DaemonPaths,
    server: Arc<DaemonServer>,
    serving: Option<thread::JoinHandle<()>>,
}

impl Fixture {
    fn new(name: &str, request_timeout: Duration) -> Self {
        Self::with(
            name,
            DaemonLimits {
                request_timeout,
                minimum_run_time: Some(Duration::ZERO),
                ..DaemonLimits::default()
            },
        )
    }

    fn with(name: &str, limits: DaemonLimits) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(format!(
            "smfv-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        let root = folder.join("instance");
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        let home = folder.join("home").to_string_lossy().into_owned();
        let paths =
            DaemonPaths::resolve(&|key| (key == HOME_VARIABLE).then(|| home.clone()), &|_| {
                String::new()
            })
            .expect("paths");
        let registry = InstanceRegistry::new(&paths.registry_file());
        registry
            .register(&root, DateTimeOffset::now_utc())
            .expect("registered");
        let server = Arc::new(DaemonServer::new(
            paths.clone(),
            "test",
            Arc::new(Fake),
            ServingGate::new(registry),
            DaemonLog::new(&paths.log_file()),
            limits,
        ));
        let running = Arc::clone(&server);
        let (ready, listening) = std::sync::mpsc::channel();
        let serving = thread::spawn(move || {
            runtime()
                .block_on(running.run(&SystemOwnership, || ready.send(()).expect("sent")))
                .expect("served");
        });
        listening
            .recv_timeout(Duration::from_secs(10))
            .expect("listening");
        Self {
            root: root.to_string_lossy().into_owned(),
            folder,
            paths,
            server,
            serving: Some(serving),
        }
    }

    fn journal(&self) -> String {
        fs::read_to_string(self.paths.log_file()).unwrap_or_default()
    }

    /// Stops the server and waits for the end of its drain.
    fn stop(&mut self) {
        self.server.stop();
        if let Some(serving) = self.serving.take() {
            serving.join().expect("the server ends");
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.stop();
        let _ = fs::remove_dir_all(&self.folder);
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

async fn session(fixture: &Fixture) -> Stream {
    let mut stream = connect(&fixture.paths, &SystemOwnership, Duration::from_secs(5))
        .await
        .expect("no refusal")
        .expect("a server");
    let mut hello = IpcMessage::new(kind::HELLO);
    hello.protocol = Some(1);
    hello.tool_version = Some("test".to_owned());
    hello.cwd = Some(fixture.root.clone());
    send(&mut stream, &hello).await;
    assert_eq!(
        answer(&mut stream).await.map(|m| m.kind),
        Some("welcome".to_owned())
    );
    stream
}

fn run(fixture: &Fixture, id: i64, argv: &[&str]) -> IpcMessage {
    let mut message = IpcMessage::new(kind::RUN);
    message.id = Some(id);
    message.cwd = Some(fixture.root.clone());
    message.argv = Some(argv.iter().map(|a| Some((*a).to_owned())).collect());
    message
}

async fn send(stream: &mut Stream, message: &IpcMessage) {
    let frame = framing::encode(message, usize::MAX).expect("encoded");
    stream.write_all(&frame).await.expect("sent");
}

async fn answer(stream: &mut Stream) -> Option<IpcMessage> {
    read_message(stream, MAX_RESPONSE_BYTES)
        .await
        .ok()
        .flatten()
}

fn code(message: Option<IpcMessage>) -> (String, Option<String>, Option<i32>) {
    let message = message.expect("an answer");
    (message.kind, message.code, message.exit_code)
}

fn block_on<F: Future>(future: F) -> F::Output {
    runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(30), future)
            .await
            .expect("in time")
    })
}

fn error(code: &str, exit: i32) -> (String, Option<String>, Option<i32>) {
    ("error".to_owned(), Some(code.to_owned()), Some(exit))
}

#[test]
fn a_timeout_answers_but_never_frees_the_writing_turn() {
    let fixture = Fixture::new("timeout", Duration::from_millis(400));
    block_on(async {
        let mut first = session(&fixture).await;
        send(&mut first, &run(&fixture, 1, &["sleep", "1500"])).await;
        assert_eq!(code(answer(&mut first).await), error("request_timeout", 3));
        // The first command still runs and holds the turn of the instance.
        let mut second = session(&fixture).await;
        send(&mut second, &run(&fixture, 2, &["quick"])).await;
        assert_eq!(code(answer(&mut second).await), error("daemon_busy", 3));
        // Once it ends, the turn is free again.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        send(&mut second, &run(&fixture, 3, &["quick"])).await;
        let done = answer(&mut second).await.expect("a result");
        assert_eq!(
            (done.kind.as_str(), done.stdout.as_deref()),
            ("result", Some("done"))
        );
    });
}

#[test]
fn one_run_at_a_time_per_connection() {
    let fixture = Fixture::new("one-run", Duration::from_secs(10));
    block_on(async {
        let mut stream = session(&fixture).await;
        send(&mut stream, &run(&fixture, 1, &["sleep", "300"])).await;
        send(&mut stream, &run(&fixture, 2, &["quick"])).await;
        // `Refuse` sends `IpcMessage.Failure` without an exit code.
        let refused = code(answer(&mut stream).await);
        assert_eq!(
            refused,
            ("error".to_owned(), Some("protocol_error".to_owned()), None)
        );
    });
}

#[test]
fn a_client_gone_during_a_command_is_journaled() {
    let mut fixture = Fixture::new("gone", Duration::from_secs(10));
    block_on(async {
        let mut stream = session(&fixture).await;
        send(&mut stream, &run(&fixture, 1, &["sleep", "300"])).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        drop(stream);
    });
    // The stop drains the command left behind.
    fixture.stop();
    let journal = fixture.journal();
    for event in ["client_gone", "abandoned", "stopped"] {
        assert!(
            journal.contains(&format!("\"event\":\"{event}\"")),
            "{journal}"
        );
    }
}

#[test]
fn a_stop_drains_the_running_command_and_delivers_its_answer() {
    let mut fixture = Fixture::new("drain", Duration::from_secs(10));
    block_on(async {
        let mut stream = session(&fixture).await;
        send(&mut stream, &run(&fixture, 1, &["sleep", "500"])).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        fixture.server.stop();
        let done = answer(&mut stream).await.expect("the answer");
        assert_eq!((done.kind.as_str(), done.exit_code), ("result", Some(0)));
    });
    fixture.stop();
    assert!(fixture.journal().contains("\"event\":\"stopped\""));
}

#[test]
fn an_output_over_16_mib_is_not_returned() {
    let fixture = Fixture::new("big", Duration::from_secs(30));
    block_on(async {
        let mut stream = session(&fixture).await;
        send(&mut stream, &run(&fixture, 1, &["big"])).await;
        assert_eq!(
            code(answer(&mut stream).await),
            error("response_too_large", 1)
        );
    });
}

/// `NoPlaceAtAllIsRefused`: a read that finds no place within its wait is `daemon_busy`.
#[test]
fn a_read_without_a_place_is_refused() {
    let fixture = Fixture::with(
        "no-place",
        DaemonLimits {
            max_executions: 1,
            read_place_timeout: Duration::from_millis(200),
            minimum_run_time: Some(Duration::ZERO),
            ..DaemonLimits::default()
        },
    );
    block_on(async {
        let mut writer = session(&fixture).await;
        send(&mut writer, &run(&fixture, 1, &["sleep", "800"])).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut reader = session(&fixture).await;
        send(&mut reader, &run(&fixture, 2, &["read"])).await;
        assert_eq!(code(answer(&mut reader).await), error("daemon_busy", 3));
        let done = answer(&mut writer).await.expect("a result");
        assert_eq!(done.kind, "result");
    });
}

/// `ShutdownRefusesAWriteThatHasNotStarted` and `ShutdownLetsAStartedWriteAnswer`.
#[test]
fn a_stop_refuses_a_write_that_has_not_started() {
    let fixture = Fixture::new("stop-queued", Duration::from_secs(10));
    block_on(async {
        let mut started = session(&fixture).await;
        send(&mut started, &run(&fixture, 1, &["sleep", "500"])).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        let mut queued = session(&fixture).await;
        send(&mut queued, &run(&fixture, 2, &["quick"])).await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        fixture.server.stop();
        assert_eq!(code(answer(&mut queued).await), error("daemon_stopping", 3));
        let done = answer(&mut started).await.expect("a result");
        assert_eq!((done.kind.as_str(), done.exit_code), ("result", Some(0)));
    });
}
