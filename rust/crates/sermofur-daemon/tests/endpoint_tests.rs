//! Ports of the endpoint cases of `DaemonTests` (`IpcEndpoint`), and the cross-process cases
//! with the daemon of the .NET oracle (`SMF_ORACLE`): each side refuses to serve on the
//! endpoint of the other, and the port talks to the oracle through it. Every home is fresh,
//! under the temporary directory.

use std::fs;
use std::future::Future;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::ownership::SystemOwnership;
use sermofur_daemon::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_daemon::endpoint::{EndpointState, Listener, Stream, connect, inspect, listen};
use sermofur_daemon::framing::{self, IpcMessage, MAX_RESPONSE_BYTES, kind};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// A fresh home under the temporary directory, removed on drop.
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
            "smfe-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("the home");
        Self(folder)
    }

    fn paths(&self) -> DaemonPaths {
        let text = self.0.to_string_lossy().into_owned();
        DaemonPaths::resolve(&|key| (key == HOME_VARIABLE).then(|| text.clone()), &|_| {
            String::new()
        })
        .expect("the paths of a home")
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}

fn state(paths: &DaemonPaths) -> EndpointState {
    inspect(paths, &SystemOwnership).expect("the endpoint state")
}

fn refusal<T>(outcome: sermofur_core::error::Result<T>) -> (String, i32) {
    match outcome {
        Ok(_) => panic!("refused expected"),
        Err(error) => (error.code, error.exit_code),
    }
}

async fn send(stream: &mut Stream, message: &IpcMessage) {
    let frame = framing::encode(message, framing::MAX_BODY_BYTES).expect("a frame");
    stream.write_all(&frame).await.expect("written");
    stream.flush().await.expect("flushed");
}

async fn receive(stream: &mut Stream) -> IpcMessage {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await.expect("a header");
    let size = u32::from_le_bytes(header) as usize;
    assert!(size > 0 && size <= MAX_RESPONSE_BYTES, "{size}");
    let mut body = vec![0u8; size];
    stream.read_exact(&mut body).await.expect("a body");
    framing::decode(&body).expect("a decodable message")
}

fn hello(cwd: &Path) -> IpcMessage {
    let mut message = IpcMessage::new(kind::HELLO);
    message.protocol = Some(framing::CURRENT_PROTOCOL);
    message.tool_version = Some(env!("CARGO_PKG_VERSION").to_owned());
    message.cwd = Some(cwd.to_string_lossy().into_owned());
    message.pid = i32::try_from(std::process::id()).ok();
    message
}

#[test]
fn no_daemon_means_no_client() {
    let home = Home::new("absent");
    let paths = home.paths();
    assert_eq!(state(&paths), EndpointState::Absent);
    let started = Instant::now();
    let stream = run(connect(&paths, &SystemOwnership, Duration::from_secs(5))).expect("no error");
    assert!(stream.is_none());
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_millis(100), "{elapsed:?}");
}

/// Lot 7.6, minor 4 of the review of lot 7.5: with no daemon, `inspect` lists the pipe file
/// system three times under Windows. Measured on 50 calls, against one listing alone; the
/// bound is the one of `no_daemon_means_no_client`.
#[cfg(windows)]
#[test]
fn inspect_without_a_daemon_lists_the_pipes_three_times() {
    let home = Home::new("listings");
    let paths = home.paths();
    let calls: u32 = 50;
    let started = Instant::now();
    let mut pipes = 0;
    for _ in 0..calls {
        pipes = fs::read_dir(r"\\.\pipe\").expect("the pipes").count();
    }
    let listing = started.elapsed() / calls;
    let started = Instant::now();
    for _ in 0..calls {
        assert_eq!(state(&paths), EndpointState::Absent);
    }
    let inspected = started.elapsed() / calls;
    eprintln!(
        "inspect without a daemon: {:.3} ms per call; one listing of {pipes} pipes: {:.3} ms",
        inspected.as_secs_f64() * 1000.0,
        listing.as_secs_f64() * 1000.0
    );
    assert!(inspected < Duration::from_millis(100), "{inspected:?}");
}

#[test]
fn second_daemon_on_the_same_endpoint_is_refused() {
    let home = Home::new("second");
    let paths = home.paths();
    run(async {
        let _first = listen(&paths, &SystemOwnership).await.expect("the first");
        assert_eq!(state(&paths), EndpointState::Present);
        assert_eq!(
            refusal(listen(&paths, &SystemOwnership).await),
            ("daemon_already_running".to_owned(), 3)
        );
    });
}

#[test]
fn a_frame_goes_both_ways() {
    let home = Home::new("frame");
    let paths = home.paths();
    let cwd = home.0.clone();
    run(async move {
        let mut listener: Listener = listen(&paths, &SystemOwnership).await.expect("listening");
        let server = tokio::spawn(async move {
            let mut stream = listener.accept().await.expect("a client");
            let request = receive(&mut stream).await;
            let mut answer = IpcMessage::new(kind::WELCOME);
            answer.tool_version = request.tool_version;
            send(&mut stream, &answer).await;
            (request.kind, request.cwd)
        });
        let mut client = connect(&paths, &SystemOwnership, Duration::from_secs(5))
            .await
            .expect("no error")
            .expect("a stream");
        send(&mut client, &hello(&cwd)).await;
        let answer = receive(&mut client).await;
        assert_eq!(answer.kind, kind::WELCOME);
        assert_eq!(
            answer.tool_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
        let (request, request_cwd) = server.await.expect("the server");
        assert_eq!(request, kind::HELLO);
        assert_eq!(request_cwd, Some(cwd.to_string_lossy().into_owned()));
    });
}

/// Two clients one after the other: the first takes the instance made ahead, the second the
/// one made by the next `accept` (under Windows, a pipe not there yet is tried again).
#[test]
fn two_clients_are_served_one_after_the_other() {
    let home = Home::new("two");
    let paths = home.paths();
    let cwd = home.0.clone();
    run(async move {
        let mut listener: Listener = listen(&paths, &SystemOwnership).await.expect("listening");
        let server = tokio::spawn(async move {
            for _ in 0..2 {
                let mut stream = listener.accept().await.expect("a client");
                let request = receive(&mut stream).await;
                send(&mut stream, &IpcMessage::new(kind::WELCOME)).await;
                assert_eq!(request.kind, kind::HELLO);
            }
        });
        let exchange = |paths: DaemonPaths, cwd: PathBuf| async move {
            let mut client = connect(&paths, &SystemOwnership, Duration::from_secs(5))
                .await
                .expect("no error")
                .expect("a stream");
            send(&mut client, &hello(&cwd)).await;
            receive(&mut client).await.kind
        };
        assert_eq!(exchange(paths.clone(), cwd.clone()).await, kind::WELCOME);
        // The second client finds the next instance, made by the second `accept`.
        assert_eq!(exchange(paths, cwd).await, kind::WELCOME);
        server.await.expect("the server");
    });
}

#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    use sermofur_core::ownership::SystemOwnership;
    use sermofur_daemon::endpoint::{EndpointState, connect, listen};

    use super::{Home, refusal, run, state};

    fn mode(path: &std::path::Path) -> u32 {
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777
    }

    fn set_mode(path: &std::path::Path, mode: u32) {
        fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("the mode");
    }

    fn foreign() -> (String, i32) {
        ("foreign_endpoint".to_owned(), 4)
    }

    #[test]
    fn the_folder_is_private_and_the_socket_too() {
        let home = Home::new("modes");
        let paths = home.paths();
        run(async {
            let listener = listen(&paths, &SystemOwnership).await.expect("listening");
            assert_eq!(mode(&paths.runtime_directory().expect("a folder")), 0o700);
            assert_eq!(mode(&paths.endpoint), 0o600);
            drop(listener);
            // The socket goes with the listener.
            assert!(!paths.endpoint.exists());
            assert_eq!(state(&paths), EndpointState::Absent);
        });
    }

    /// `RuntimeFolderOpenToOthersIsForeign`.
    #[test]
    fn runtime_folder_open_to_others_is_foreign() {
        let home = Home::new("open");
        let paths = home.paths();
        let folder = paths.runtime_directory().expect("a folder");
        run(async {
            let _listener = listen(&paths, &SystemOwnership).await.expect("listening");
            set_mode(&folder, 0o701);
            assert_eq!(state(&paths), EndpointState::Foreign);
            let connected = connect(&paths, &SystemOwnership, Duration::from_secs(1)).await;
            assert_eq!(refusal(connected), foreign());
            assert_eq!(refusal(listen(&paths, &SystemOwnership).await), foreign());
            set_mode(&folder, 0o700);
        });
    }

    #[test]
    fn a_regular_file_in_place_of_the_socket_is_foreign() {
        let home = Home::new("file");
        let paths = home.paths();
        let folder = paths.runtime_directory().expect("a folder");
        fs::create_dir_all(&folder).expect("the folder");
        set_mode(&folder, 0o700);
        fs::write(&paths.endpoint, b"planted").expect("a file");
        assert_eq!(state(&paths), EndpointState::Foreign);
        run(async {
            let connected = connect(&paths, &SystemOwnership, Duration::from_secs(1)).await;
            assert_eq!(refusal(connected), foreign());
            assert_eq!(refusal(listen(&paths, &SystemOwnership).await), foreign());
        });
        assert_eq!(fs::read(&paths.endpoint).expect("kept"), b"planted");
    }

    #[test]
    fn a_stale_socket_is_removed() {
        let home = Home::new("stale");
        let paths = home.paths();
        let folder = paths.runtime_directory().expect("a folder");
        fs::create_dir_all(&folder).expect("the folder");
        set_mode(&folder, 0o700);
        // A socket left by a crash: std does not remove it on drop.
        drop(std::os::unix::net::UnixListener::bind(&paths.endpoint).expect("a socket"));
        assert_eq!(state(&paths), EndpointState::Present);
        run(async {
            let stale = connect(&paths, &SystemOwnership, Duration::from_millis(500)).await;
            assert!(stale.expect("no error").is_none());
            let _listener = listen(&paths, &SystemOwnership).await.expect("listening");
            let stream = connect(&paths, &SystemOwnership, Duration::from_secs(1)).await;
            assert!(stream.expect("no error").is_some());
        });
    }
}

/// The `smf` of the oracle, when `SMF_ORACLE` names it.
fn oracle() -> Option<String> {
    let oracle = std::env::var("SMF_ORACLE").ok();
    if oracle.is_none() {
        eprintln!("cross-process case skipped: SMF_ORACLE is not set");
    }
    oracle
}

/// Kills a process of the oracle when the test ends, whatever happens.
struct Process(Child);

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn daemon_of_the_oracle(oracle: &str, home: &Path) -> Process {
    Process(
        Command::new(oracle)
            .args(["daemon", "run"])
            .env(HOME_VARIABLE, home)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the daemon of the oracle starts"),
    )
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(30), "{what}");
        thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn the_port_talks_to_the_daemon_of_the_oracle_and_never_serves_beside_it() {
    let Some(oracle) = oracle() else { return };
    let home = Home::new("cross-oracle");
    let paths = home.paths();
    let mut daemon = daemon_of_the_oracle(&oracle, &home.0);
    wait_until("the oracle opens no endpoint", || {
        state(&paths) == EndpointState::Present
    });
    run(async {
        assert_eq!(
            refusal(listen(&paths, &SystemOwnership).await),
            ("daemon_already_running".to_owned(), 3)
        );
        let mut stream = connect(&paths, &SystemOwnership, Duration::from_secs(5))
            .await
            .expect("no error")
            .expect("a stream to the oracle");
        send(&mut stream, &hello(&home.0)).await;
        let answer = receive(&mut stream).await;
        eprintln!(
            "cross-read ok: the oracle answers the hello of the port with {:?} ({:?}, {:?})",
            answer.kind, answer.tool_version, answer.code
        );
        assert_eq!(answer.kind, kind::WELCOME);
        assert_eq!(
            answer.tool_version.as_deref(),
            Some(env!("CARGO_PKG_VERSION"))
        );
    });
    let _ = daemon.0.kill();
    let _ = daemon.0.wait();
    drop(daemon);
    // Windows: the pipe goes with the process. Unix: the socket stays and the port removes it.
    run(async {
        let started = Instant::now();
        let listener = loop {
            match listen(&paths, &SystemOwnership).await {
                Ok(listener) => break listener,
                Err(error) => {
                    assert!(started.elapsed() < Duration::from_secs(10), "{error:?}");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            }
        };
        drop(listener);
    });
    assert_eq!(state(&paths), EndpointState::Absent);
}

#[test]
fn the_daemon_of_the_oracle_never_serves_beside_the_port() {
    let Some(oracle) = oracle() else { return };
    let home = Home::new("cross-port");
    let paths = home.paths();
    run(async {
        let _listener = listen(&paths, &SystemOwnership).await.expect("listening");
        let mut daemon = daemon_of_the_oracle(&oracle, &home.0);
        let started = Instant::now();
        let status = loop {
            if let Some(status) = daemon.0.try_wait().expect("the oracle") {
                break status;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "the oracle serves beside the port"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        };
        let mut output = String::new();
        if let Some(mut out) = daemon.0.stdout.take() {
            let _ = out.read_to_string(&mut output);
        }
        if let Some(mut err) = daemon.0.stderr.take() {
            let _ = err.read_to_string(&mut output);
        }
        eprintln!(
            "cross-read ok: the oracle beside the port exits {:?}: {}",
            status.code(),
            output.trim()
        );
        assert_eq!(status.code(), Some(3), "{output}");
        assert!(
            output.contains("daemon_already_running: A Sermofur daemon already serves this user."),
            "{output}"
        );
    });
}
