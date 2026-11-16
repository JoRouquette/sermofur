//! The channel of the bridge to the daemon (`DaemonChannel`) and the cancellation of a call,
//! on a daemon server of this process (`TestDaemon` with a `GatedExecutor`) or a false daemon
//! (`StubDaemon`). Ports of `McpTests.OtherVersionAndRestartedDaemon` and of the MCP cases of
//! `DaemonSessionTests`.

use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use sermofur_core::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_core::error::Result;
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::client::INTERRUPTED_CODE;
use sermofur_daemon::framing::{self, IpcMessage, MAX_BODY_BYTES, kind};
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::{
    CommandExecutor, CommandOutcome, CommandPlan, DaemonLimits, DaemonServer, ServingGate,
    read_message,
};
use sermofur_mcp::channel::DaemonChannel;
use sermofur_mcp::tools::{self, CommandChannel};
use tokio::io::AsyncWriteExt;

const VERSION: &str = sermofur_core::VERSION;

/// A folder with an instance and a daemon home, removed on drop.
struct Scratch {
    folder: PathBuf,
    root: String,
    paths: DaemonPaths,
}

impl Scratch {
    fn new(name: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            std::fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(format!(
            "smfm-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let root = folder.join("instance");
        std::fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        let home = folder.join("home").to_string_lossy().into_owned();
        let paths =
            DaemonPaths::resolve(&|key| (key == HOME_VARIABLE).then(|| home.clone()), &|_| {
                String::new()
            })
            .expect("paths");
        InstanceRegistry::new(&paths.registry_file())
            .register(&root, DateTimeOffset::now_utc())
            .expect("registered");
        Self {
            root: root.to_string_lossy().into_owned(),
            folder,
            paths,
        }
    }

    fn channel(&self) -> DaemonChannel {
        DaemonChannel::new(self.paths.clone(), VERSION, &self.root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// `GatedExecutor`: answers its first argument (`status` answers a JSON object); the command
/// it blocks on (`block` by default) waits until the test releases it.
#[derive(Default)]
struct Gated {
    state: Mutex<Gate>,
    changed: Condvar,
    blocking: Option<&'static str>,
}

#[derive(Default)]
struct Gate {
    released: bool,
    blocked: usize,
}

impl Gated {
    fn blocking(command: &'static str) -> Self {
        Self {
            blocking: Some(command),
            ..Self::default()
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, Gate> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn release(&self) {
        self.state().released = true;
        self.changed.notify_all();
    }

    fn wait_blocked(&self) {
        for _ in 0..1000 {
            if self.state().blocked > 0 {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the command started within 10 s");
    }
}

impl CommandExecutor for Gated {
    fn plan(&self, _argv: &[String], cwd: &str) -> Result<CommandPlan> {
        Ok(CommandPlan {
            path: cwd.to_owned(),
            writes: false,
        })
    }

    fn execute(&self, argv: &[String], _cwd: &str) -> CommandOutcome {
        let state = self.state();
        if argv[0] == self.blocking.unwrap_or("block") {
            let mut state = state;
            state.blocked += 1;
            let _ = self
                .changed
                .wait_timeout_while(state, Duration::from_secs(30), |s| !s.released)
                .unwrap_or_else(PoisonError::into_inner);
        } else {
            drop(state);
        }
        CommandOutcome {
            exit_code: 0,
            stdout: if argv[0] == "status" {
                "{\"status\":true}".to_owned()
            } else {
                argv[0].clone()
            },
            stderr: String::new(),
        }
    }
}

/// `TestDaemon.StartAt`: a daemon server of this process on the paths of `scratch`, stopped
/// on drop.
struct TestDaemon {
    server: Arc<DaemonServer>,
    thread: Option<thread::JoinHandle<()>>,
}

impl TestDaemon {
    fn start(scratch: &Scratch, version: &str, executor: Arc<Gated>) -> Self {
        let server = Arc::new(DaemonServer::new(
            scratch.paths.clone(),
            version,
            executor,
            ServingGate::new(InstanceRegistry::new(&scratch.paths.registry_file())),
            DaemonLog::new(&scratch.paths.log_file()),
            DaemonLimits {
                minimum_run_time: Some(Duration::ZERO),
                ..DaemonLimits::default()
            },
        ));
        let running = Arc::clone(&server);
        let (ready, listening) = mpsc::channel();
        let thread = thread::spawn(move || {
            runtime()
                .block_on(running.run(&SystemOwnership, || ready.send(()).expect("sent")))
                .expect("served");
        });
        listening
            .recv_timeout(Duration::from_secs(10))
            .expect("listening");
        Self {
            server,
            thread: Some(thread),
        }
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.server.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

/// The text of a `tools/call` result, and whether it is an error.
async fn status(channel: &DaemonChannel) -> (bool, String) {
    let result = tools::call("sermofur_status", None, "t", channel, None)
        .await
        .expect("a result");
    let error = result.ends_with(",\"isError\":true}");
    (error, result)
}

/// Port of `McpTests.OtherVersionAndRestartedDaemon`.
#[test]
fn other_version_and_restarted_daemon() {
    let scratch = Scratch::new("restart");
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime(), async {
        {
            let _other = TestDaemon::start(&scratch, "0.0.0-other", Arc::default());
            let (error, text) = status(&scratch.channel()).await;
            assert!(error, "{text}");
            assert!(text.contains("\"daemon_version_mismatch: "), "{text}");
        }
        let channel = Rc::new(scratch.channel());
        {
            let _first = TestDaemon::start(&scratch, VERSION, Arc::default());
            assert!(!status(&channel).await.0);
            // Parallel calls of the host share one session without mixing answers.
            let calls: Vec<_> = (0..10)
                .map(|_| {
                    let channel = Rc::clone(&channel);
                    tokio::task::spawn_local(async move { status(&channel).await })
                })
                .collect();
            for call in calls {
                let (error, text) = call.await.expect("joined");
                assert!(!error, "{text}");
                assert!(text.contains("{\"status\":true}"), "{text}");
            }
        }
        let (error, text) = status(&channel).await;
        assert!(error && text.contains("\"daemon_unavailable: "), "{text}");
        // Same bridge, new daemon: the session is opened again without restarting the bridge.
        let _second = TestDaemon::start(&scratch, VERSION, Arc::default());
        assert!(!status(&channel).await.0);
    });
}

/// Port of `DaemonSessionTests.CancelledMcpCallLeavesTheNextOneItsOwnAnswer`: the cancel is
/// the drop of the call.
#[test]
fn a_cancelled_mcp_call_leaves_the_next_one_its_own_answer() {
    let scratch = Scratch::new("cancel");
    let gated = Arc::new(Gated::default());
    let _daemon = TestDaemon::start(&scratch, VERSION, Arc::clone(&gated));
    let channel = Rc::new(scratch.channel());
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime(), async {
        let slow = {
            let channel = Rc::clone(&channel);
            tokio::task::spawn_local(async move { channel.run(vec!["block".to_owned()]).await })
        };
        let waiting = Arc::clone(&gated);
        tokio::task::spawn_blocking(move || waiting.wait_blocked())
            .await
            .expect("blocked");
        slow.abort();
        assert!(slow.await.expect_err("cancelled").is_cancelled());
        gated.release();
        let next = channel.run(vec!["quick".to_owned()]).await.expect("quick");
        assert_eq!(next.stdout, "quick");
        let after = channel.run(vec!["other".to_owned()]).await.expect("other");
        assert_eq!(after.stdout, "other");
    });
}

/// Port of `DaemonSessionTests.InterruptedMcpWriteIsReportedAndNeverSentAgain`: a false daemon
/// welcomes the bridge, then closes the session on its first command.
#[test]
fn an_interrupted_mcp_write_is_reported_and_never_sent_again() {
    let scratch = Scratch::new("interrupted");
    let runs = Arc::new(AtomicUsize::new(0));
    let (ready, listening) = mpsc::channel();
    let (paths, counted) = (scratch.paths.clone(), Arc::clone(&runs));
    thread::spawn(move || {
        runtime().block_on(async move {
            let mut listener = sermofur_daemon::endpoint::listen(&paths, &SystemOwnership)
                .await
                .expect("listening");
            ready.send(()).expect("ready");
            while let Ok(mut stream) = listener.accept().await {
                if !matches!(read_message(&mut stream, MAX_BODY_BYTES).await, Ok(Some(_))) {
                    continue;
                }
                let mut welcome = IpcMessage::new(kind::WELCOME);
                welcome.protocol = Some(1);
                welcome.tool_version = Some(VERSION.to_owned());
                let frame = framing::encode(&welcome, usize::MAX).expect("frame");
                let _ = stream.write_all(&frame).await;
                let _ = stream.flush().await;
                if let Ok(Some(request)) = read_message(&mut stream, MAX_BODY_BYTES).await
                    && request.kind == kind::RUN
                {
                    counted.fetch_add(1, Ordering::SeqCst);
                }
            }
        });
    });
    listening
        .recv_timeout(Duration::from_secs(10))
        .expect("the stub listens");
    let channel = scratch.channel();
    let error = runtime()
        .block_on(
            channel.run(
                ["claim", "add", "once", "--origin", "llm", "--actor", "test"]
                    .map(str::to_owned)
                    .to_vec(),
            ),
        )
        .expect_err("interrupted");
    assert_eq!(error.code, INTERRUPTED_CODE);
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

/// The lines a server writes, one by one.
struct Lines(mpsc::Sender<String>, Vec<u8>);

impl Write for Lines {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        for byte in bytes {
            if *byte == b'\n' {
                let line = String::from_utf8(std::mem::take(&mut self.1)).expect("UTF-8");
                let _ = self.0.send(line);
            } else {
                self.1.push(*byte);
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

const STATUS: &str =
    r#"{"jsonrpc":"2.0","id":ID,"method":"tools/call","params":{"name":"sermofur_status"}}"#;

/// A server on a pipe, its answers line by line, ended when the pipe closes.
fn serving(
    scratch: &Scratch,
) -> (
    std::io::PipeWriter,
    mpsc::Receiver<String>,
    thread::JoinHandle<()>,
) {
    let (input, host) = std::io::pipe().expect("a pipe");
    let (sender, answers) = mpsc::channel();
    let channel = scratch.channel();
    let server = thread::spawn(move || {
        let mut out = Lines(sender, Vec::new());
        sermofur_mcp::server::serve(
            channel,
            VERSION,
            None,
            input,
            &mut out,
            std::future::pending(),
        )
        .expect("served");
    });
    (host, answers, server)
}

fn send(host: &mut std::io::PipeWriter, line: &str) {
    host.write_all(line.as_bytes()).expect("sent");
    host.write_all(
        b"
",
    )
    .expect("sent");
}

/// `notifications/cancelled` ends a running call without an answer; the next call gets its
/// own (the SDK sends no answer to a request cancelled by its client).
#[test]
fn a_cancelled_call_gets_no_answer_and_the_next_one_gets_its_own() {
    let scratch = Scratch::new("notified");
    let gated = Arc::new(Gated::blocking("status"));
    let _daemon = TestDaemon::start(&scratch, VERSION, Arc::clone(&gated));
    let (mut host, answers, server) = serving(&scratch);
    send(&mut host, &STATUS.replace("ID", "1"));
    gated.wait_blocked();
    send(
        &mut host,
        r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
    );
    gated.release();
    send(&mut host, &STATUS.replace("ID", "2"));
    let answer = answers
        .recv_timeout(Duration::from_secs(10))
        .expect("an answer");
    assert!(answer.ends_with(r#""id":2,"jsonrpc":"2.0"}"#), "{answer}");
    assert!(answer.contains(r#"{"status":true}"#), "{answer}");
    send(&mut host, r#"{"jsonrpc":"2.0","id":3,"method":"ping"}"#);
    assert_eq!(
        answers.recv_timeout(Duration::from_secs(10)).expect("pong"),
        r#"{"result":{},"id":3,"jsonrpc":"2.0"}"#
    );
    drop(host);
    server.join().expect("the server ends");
    assert!(
        answers.try_recv().is_err(),
        "no answer to the cancelled call"
    );
}

/// The end of stdin ends the server while a call still runs; that call gets no answer.
#[test]
fn the_end_of_stdin_ends_the_server_during_a_call() {
    let scratch = Scratch::new("eof");
    let gated = Arc::new(Gated::blocking("status"));
    let _daemon = TestDaemon::start(&scratch, VERSION, Arc::clone(&gated));
    let (mut host, answers, server) = serving(&scratch);
    send(&mut host, &STATUS.replace("ID", "1"));
    gated.wait_blocked();
    drop(host);
    server.join().expect("the server ends before the call");
    gated.release();
    assert!(
        answers.try_recv().is_err(),
        "no answer after the end of stdin"
    );
}
