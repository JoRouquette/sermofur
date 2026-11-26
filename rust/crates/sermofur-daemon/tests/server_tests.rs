//! The server with a controlled executor (`DaemonSessionTests`, `DaemonRobustnessTests`):
//! timeouts, the writing turn, a client gone, one run per connection, the drain and the size
//! of a response. Only codes and kinds are checked here; the texts are compared with the
//! daemon of the oracle by `smf/tests/daemon_server.rs`.

#[path = "../../../tests/support/scratch.rs"]
mod scratch;

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::error::Result;
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::DaemonClient;
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
        Self::serving(name, limits, Arc::new(Fake))
    }

    /// A fixture whose commands run on a [`Gated`] executor, released when the guard drops.
    fn gated(name: &str, limits: DaemonLimits) -> (Self, Released) {
        let gated = Arc::new(Gated::default());
        let fixture = Self::serving(name, limits, Arc::clone(&gated) as Arc<dyn CommandExecutor>);
        (fixture, Released(gated))
    }

    fn serving(name: &str, limits: DaemonLimits, executor: Arc<dyn CommandExecutor>) -> Self {
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(scratch::name("sv", name));
        let _ = fs::remove_dir_all(&folder);
        let root = folder.join("instance");
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        let home = folder.join(scratch::HOME).to_string_lossy().into_owned();
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
            executor,
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
    // Gated like the oracle, not timed: on a loaded machine a sleeping command could end before
    // the stop and let the queued write start.
    let (mut fixture, gated) = Fixture::gated(
        "stop-queued",
        DaemonLimits {
            request_timeout: Duration::from_secs(10),
            ..DaemonLimits::default()
        },
    );
    block_on(async {
        let mut held = session(&fixture).await;
        send(&mut held, &run(&fixture, 1, &["block", "write"])).await;
        until(|| gated.blocked() == 1).await;
        let mut queued = session(&fixture).await;
        send(&mut queued, &run(&fixture, 2, &["queued", "write"])).await;
        until(|| gated.planned("queued")).await;
        fixture.server.stop();
        assert_eq!(code(answer(&mut queued).await), error("daemon_stopping", 3));
        gated.release();
        let done = answer(&mut held).await.expect("a result");
        assert_eq!((done.kind.as_str(), done.exit_code), ("result", Some(0)));
    });
    // `await stopped`: the drain ends once the held command has answered.
    fixture.stop();
    assert!(fixture.journal().contains("\"event\":\"stopped\""));
    assert!(!gated.ran("queued"));
}

/// `GatedExecutor`: answers its first argument; "write" anywhere plans a write, "block" waits
/// until the test releases it. It records what it planned and what ran.
#[derive(Default)]
struct Gated {
    state: Mutex<Gate>,
    changed: Condvar,
}

#[derive(Default)]
struct Gate {
    released: bool,
    blocked: usize,
    planned: Vec<String>,
    ran: Vec<String>,
}

impl Gated {
    fn state(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn release(&self) {
        self.state().released = true;
        self.changed.notify_all();
    }

    fn planned(&self, command: &str) -> bool {
        self.state().planned.iter().any(|c| c == command)
    }

    fn ran(&self, command: &str) -> bool {
        self.state().ran.iter().any(|c| c == command)
    }

    fn blocked(&self) -> usize {
        self.state().blocked
    }
}

impl CommandExecutor for Gated {
    fn plan(&self, argv: &[String], cwd: &str) -> Result<CommandPlan> {
        self.state().planned.push(argv[0].clone());
        Ok(CommandPlan {
            path: cwd.to_owned(),
            writes: argv.iter().any(|a| a == "write"),
        })
    }

    fn execute(&self, argv: &[String], _cwd: &str) -> CommandOutcome {
        let mut state = self.state();
        state.ran.push(argv[0].clone());
        if argv[0] == "block" {
            state.blocked += 1;
            let _ = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(30), |s| !s.released)
                .unwrap_or_else(PoisonError::into_inner);
        }
        let stdout = match argv[0].as_str() {
            "big" => big(),
            "huge" => "x".repeat(MAX_RESPONSE_BYTES + 1),
            _ => argv[0].clone(),
        };
        CommandOutcome {
            exit_code: 0,
            stdout,
            stderr: String::new(),
        }
    }
}

/// `GatedExecutor.Big`: 300 000 characters, 600 000 UTF-8 bytes beyond ASCII.
fn big() -> String {
    "é€ ".repeat(100_000)
}

/// Releases the [`Gated`] executor on drop, so that a failed test never leaves a command blocked.
struct Released(Arc<Gated>);

impl std::ops::Deref for Released {
    type Target = Gated;

    fn deref(&self) -> &Gated {
        &self.0
    }
}

impl Drop for Released {
    fn drop(&mut self) {
        self.0.release();
    }
}

/// Waits, at most 10 s, for a condition set by the server.
async fn until(condition: impl Fn() -> bool) {
    for _ in 0..1000 {
        if condition() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("the condition holds within 10 s");
}

/// `PlacesTaken`: every place taken by a "block" read of its own client, until the release.
async fn take_places(fixture: &Fixture, gated: &Gated, places: usize) -> Vec<Stream> {
    let mut holders = Vec::new();
    for id in 0..places {
        let mut holder = session(fixture).await;
        let id = i64::try_from(id).expect("an id");
        send(&mut holder, &run(fixture, id, &["block"])).await;
        holders.push(holder);
    }
    until(|| gated.blocked() == places).await;
    holders
}

/// `PlacesTaken.FinishAsync`: every blocked read answers once released.
async fn finish(holders: Vec<Stream>) {
    for mut holder in holders {
        let done = answer(&mut holder).await.expect("a result");
        assert_eq!((done.kind.as_str(), done.exit_code), ("result", Some(0)));
    }
}

/// `new DaemonLimits(5 s, 20 s) { MaxExecutions = places }`.
fn places(max_executions: usize) -> DaemonLimits {
    DaemonLimits {
        hello_timeout: Duration::from_secs(5),
        request_timeout: Duration::from_secs(20),
        max_executions,
        ..DaemonLimits::default()
    }
}

fn kind_code(message: Option<IpcMessage>) -> (String, Option<String>) {
    let (kind, code, _) = code(message);
    (kind, code)
}

fn refusal(code: &str) -> (String, Option<String>) {
    ("error".to_owned(), Some(code.to_owned()))
}

/// `ReadWaitingForAPlaceGivesUpLongBeforeTheDeadline`.
#[test]
fn a_read_waiting_for_a_place_gives_up_long_before_the_deadline() {
    let (fixture, gated) = Fixture::gated(
        "read-gives-up",
        DaemonLimits {
            read_place_timeout: Duration::from_secs(1),
            ..places(2)
        },
    );
    block_on(async {
        let holders = take_places(&fixture, &gated, 2).await;
        let mut third = session(&fixture).await;
        let watch = Instant::now();
        send(&mut third, &run(&fixture, 9, &["extra"])).await;
        assert_eq!(kind_code(answer(&mut third).await), refusal("daemon_busy"));
        // The read limit, not the 20 s deadline.
        assert!(
            watch.elapsed() < Duration::from_secs(5),
            "{:?}",
            watch.elapsed()
        );
        gated.release();
        finish(holders).await;
    });
    assert!(!gated.ran("extra"));
}

/// `PlaceThatComesTooLateIsGivenBackWithoutStarting`.
#[test]
fn a_place_that_comes_too_late_is_given_back_without_starting() {
    // One place; a command must have 28 s of its 30 s left to start: a place that comes
    // within 2 s is in time, one that comes after 3 s is too late.
    let (fixture, gated) = Fixture::gated(
        "too-late",
        DaemonLimits {
            hello_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(30),
            max_executions: 1,
            minimum_run_time: Some(Duration::from_secs(28)),
            ..DaemonLimits::default()
        },
    );
    block_on(async {
        let mut holder = session(&fixture).await;
        send(&mut holder, &run(&fixture, 1, &["block"])).await;
        until(|| gated.blocked() == 1).await;
        let mut late = session(&fixture).await;
        send(&mut late, &run(&fixture, 2, &["late", "write"])).await;
        until(|| gated.planned("late")).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        gated.release();
        assert_eq!(kind_code(answer(&mut late).await), refusal("daemon_busy"));
        assert_eq!(code(answer(&mut holder).await).2, Some(0));
        // The place and the turn both came back: the next write starts at once.
        let mut next = session(&fixture).await;
        send(&mut next, &run(&fixture, 3, &["next", "write"])).await;
        let (kind, _, exit) = code(answer(&mut next).await);
        assert_eq!((kind.as_str(), exit), ("result", Some(0)));
    });
    assert!(!gated.ran("late"));
}

/// `FullPlacesLeaveHelloAndStatusFreeAndAnExtraReadWaitsItsTurn`.
#[test]
fn full_places_leave_hello_and_status_free_and_an_extra_read_waits_its_turn() {
    let (fixture, gated) = Fixture::gated(
        "places-full",
        DaemonLimits {
            // The extra read must still wait when the hello of a slow runner takes long.
            read_place_timeout: Duration::from_secs(20),
            ..places(2)
        },
    );
    block_on(async {
        let holders = take_places(&fixture, &gated, 2).await;
        let mut extra = session(&fixture).await;
        send(&mut extra, &run(&fixture, 9, &["extra"])).await;
        until(|| gated.planned("extra")).await;
        // Both places are taken: the hello and the status do not wait for them.
        let watch = Instant::now();
        let mut probe = session(&fixture).await;
        send(&mut probe, &IpcMessage::new(kind::STATUS)).await;
        assert_eq!(
            answer(&mut probe).await.map(|m| m.kind),
            Some(kind::STATUS.to_owned())
        );
        assert!(
            watch.elapsed() < Duration::from_secs(5),
            "{:?}",
            watch.elapsed()
        );
        // No answer within 200 ms: the extra read still waits for a place.
        let waiting = tokio::time::timeout(Duration::from_millis(200), answer(&mut extra)).await;
        assert!(waiting.is_err(), "{waiting:?}");
        assert!(!gated.ran("extra"));
        gated.release();
        let done = answer(&mut extra).await.expect("a result");
        assert_eq!(done.stdout.as_deref(), Some("extra"));
        finish(holders).await;
    });
}

/// `WriterThatLeavesWhileWaitingForAPlaceGivesItsTurnBack`.
#[test]
fn a_writer_that_leaves_while_waiting_for_a_place_gives_its_turn_back() {
    let (fixture, gated) = Fixture::gated("writer-leaves", places(2));
    block_on(async {
        let holders = take_places(&fixture, &gated, 2).await;
        // This write takes the turn of the instance, then waits for a place, and leaves.
        let mut leaver = session(&fixture).await;
        send(&mut leaver, &run(&fixture, 8, &["left", "write"])).await;
        until(|| gated.planned("left")).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(leaver);
        tokio::time::sleep(Duration::from_millis(200)).await;
        gated.release();
        finish(holders).await;
        // A turn kept by the leaver would make this write wait for the whole 20 s deadline.
        let mut after = session(&fixture).await;
        send(&mut after, &run(&fixture, 9, &["after", "write"])).await;
        let written = tokio::time::timeout(Duration::from_secs(10), answer(&mut after))
            .await
            .expect("long before the deadline");
        let (kind, _, exit) = code(written);
        assert_eq!((kind.as_str(), exit), ("result", Some(0)));
    });
    assert!(!gated.ran("left"));
}

/// `ShutdownRefusesAReadWaitingForAPlace`.
#[test]
fn a_stop_refuses_a_read_waiting_for_a_place() {
    let (mut fixture, gated) = Fixture::gated("stop-waiting-read", places(2));
    block_on(async {
        let holders = take_places(&fixture, &gated, 2).await;
        let mut third = session(&fixture).await;
        send(&mut third, &run(&fixture, 9, &["extra"])).await;
        until(|| gated.planned("extra")).await;
        fixture.server.stop();
        assert_eq!(
            kind_code(answer(&mut third).await),
            refusal("daemon_stopping")
        );
        gated.release();
        finish(holders).await;
    });
    fixture.stop();
    assert!(!gated.ran("extra"));
}

/// `ClientThatLeavesAbandonsItsQueuedWrite`.
#[test]
fn a_client_that_leaves_abandons_its_queued_write() {
    let (fixture, gated) = Fixture::gated("leaves-queued", DaemonLimits::default());
    block_on(async {
        let mut holder = session(&fixture).await;
        send(&mut holder, &run(&fixture, 1, &["block", "write"])).await;
        until(|| gated.blocked() == 1).await;
        let mut leaver = session(&fixture).await;
        send(&mut leaver, &run(&fixture, 2, &["queued", "write"])).await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        drop(leaver);
        tokio::time::sleep(Duration::from_millis(200)).await;
        gated.release();
        assert_eq!(code(answer(&mut holder).await).2, Some(0));
        let mut after = session(&fixture).await;
        send(&mut after, &run(&fixture, 3, &["quick", "write"])).await;
        assert_eq!(code(answer(&mut after).await).2, Some(0));
    });
    assert!(!gated.ran("queued"));
}

/// Port of `DaemonSessionTests.LargeNonAsciiOutputIsAnsweredOnce`, through the client of the
/// port: a large non-ASCII output comes back whole, one over 16 MiB is `response_too_large`
/// (exit 1), the session stays usable, and each command runs exactly once.
#[test]
fn a_large_non_ascii_output_is_answered_once() {
    let (fixture, gated) = Fixture::gated("non-ascii", DaemonLimits::default());
    block_on(async {
        let mut client = DaemonClient::connect(
            &fixture.paths,
            &SystemOwnership,
            "test",
            &fixture.root,
            Duration::from_secs(5),
        )
        .await
        .expect("no refusal")
        .expect("a daemon answers");
        let run = |argv: &str| vec![argv.to_owned()];
        let answer = client.run(&run("big")).await.expect("an answer");
        assert_eq!(answer.kind, kind::RESULT);
        assert!(answer.stdout == Some(big()), "the output differs");
        let huge = client.run(&run("huge")).await.expect("an answer");
        assert_eq!(code(Some(huge)), error("response_too_large", 1));
        let quick = client.run(&run("quick")).await.expect("an answer");
        assert_eq!(quick.stdout.as_deref(), Some("quick"));
    });
    assert_eq!(gated.state().ran, ["big", "huge", "quick"]);
}
