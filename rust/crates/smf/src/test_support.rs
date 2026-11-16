//! Generators shared by the property tests of the command line (`PropertyTests.cs` in .NET).

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::sample::select;

/// The words that make up generated command lines: options, the separator, shortcuts, plain
/// words and the empty string. Same list as the .NET property tests.
const WORDS: [&str; 15] = [
    "--json",
    "--path",
    "--actor",
    "--text",
    "--contradicts",
    "--",
    "--x",
    "claim",
    "add",
    "value",
    "-v",
    "-h",
    "--help",
    "--version",
    "",
];

/// Any command line of up to 20 words taken from [`WORDS`].
pub fn command_line() -> impl Strategy<Value = Vec<String>> {
    vec(select(&WORDS[..]).prop_map(str::to_string), 0..=20)
}

/// A command line of up to 130 arguments, around the limit of 128, whose words are either from
/// [`WORDS`], an option-like text (`--` followed by arbitrary printable characters, accents and
/// characters above the Basic Multilingual Plane included), or any printable text.
pub fn arbitrary_line() -> impl Strategy<Value = Vec<String>> {
    let word = prop_oneof![
        select(&WORDS[..]).prop_map(str::to_string),
        "--\\PC{0,8}",
        "\\PC{0,40}",
    ];
    vec(word, 0..=130)
}

/// A fresh folder under the temporary directory with an instance and a daemon home, removed
/// on drop (`TestInstance` and the home of `StubDaemon.StartAlone`).
pub struct Scratch {
    pub folder: std::path::PathBuf,
    pub root: std::path::PathBuf,
    pub paths: sermofur_core::daemon_paths::DaemonPaths,
}

impl Scratch {
    pub fn new(name: &str) -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            std::fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(format!(
            "smfu-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&folder);
        let root = folder.join("instance");
        std::fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        let home = folder.join("home").to_string_lossy().into_owned();
        let paths = sermofur_core::daemon_paths::DaemonPaths::resolve(
            &|key| (key == sermofur_core::daemon_paths::HOME_VARIABLE).then(|| home.clone()),
            &|_| String::new(),
        )
        .expect("the paths of a home");
        Self {
            folder,
            root,
            paths,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.folder);
    }
}

/// What a [`StubDaemon`] does with each `run` (`StubDaemon` of .NET).
#[derive(Clone, Copy)]
pub enum Behavior {
    /// Reads the requests and never answers (alive but stuck).
    Silent,
    /// Closes the session without answering.
    Close,
    /// Answers a `result` to another request.
    WrongId,
    /// Answers `response_too_large`, exit 1.
    TooLarge,
    /// Answers an error of this code, exit 3.
    Refuse(&'static str),
}

/// A false daemon on the endpoint of `paths`: welcomes every client (or refuses the hello with
/// `protocol_error`), then handles each run with its behaviour. Stopped on drop.
pub struct StubDaemon {
    runs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl StubDaemon {
    pub fn start(
        paths: &sermofur_core::daemon_paths::DaemonPaths,
        behavior: Behavior,
        refuse_hello: bool,
    ) -> Self {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        let runs = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (ready, listening) = std::sync::mpsc::channel();
        let (counted, stopped, paths) = (Arc::clone(&runs), Arc::clone(&stop), paths.clone());
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("a runtime");
            runtime.block_on(async move {
                let mut listener = sermofur_daemon::endpoint::listen(
                    &paths,
                    &sermofur_core::ownership::SystemOwnership,
                )
                .await
                .expect("the endpoint opens");
                ready.send(()).expect("listening");
                let serving = async {
                    while let Ok(stream) = listener.accept().await {
                        tokio::spawn(hold(stream, behavior, refuse_hello, Arc::clone(&counted)));
                    }
                };
                let _ = sermofur_daemon::server::first(serving, async {
                    while !stopped.load(Ordering::SeqCst) {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                })
                .await;
            });
        });
        listening
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the stub listens");
        Self {
            runs,
            stop,
            thread: Some(thread),
        }
    }

    /// Run frames received so far.
    pub fn runs(&self) -> usize {
        self.runs.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl Drop for StubDaemon {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

async fn hold(
    mut stream: sermofur_daemon::endpoint::Stream,
    behavior: Behavior,
    refuse_hello: bool,
    runs: std::sync::Arc<std::sync::atomic::AtomicUsize>,
) {
    use sermofur_daemon::framing::{IpcMessage, MAX_BODY_BYTES, kind};
    use sermofur_daemon::server::read_message;
    use tokio::io::AsyncWriteExt;
    async fn write(stream: &mut sermofur_daemon::endpoint::Stream, message: &IpcMessage) {
        let frame = sermofur_daemon::framing::encode(message, usize::MAX).expect("encoded");
        let _ = stream.write_all(&frame).await;
        let _ = stream.flush().await;
    }
    if !matches!(read_message(&mut stream, MAX_BODY_BYTES).await, Ok(Some(_))) {
        return;
    }
    if refuse_hello {
        write(
            &mut stream,
            &IpcMessage::failure("protocol_error", "Refused by the test.", None),
        )
        .await;
        return;
    }
    let mut welcome = IpcMessage::new(kind::WELCOME);
    welcome.protocol = Some(1);
    welcome.tool_version = Some(sermofur_core::VERSION.to_owned());
    write(&mut stream, &welcome).await;
    while let Ok(Some(request)) = read_message(&mut stream, MAX_BODY_BYTES).await {
        if request.kind != kind::RUN {
            continue;
        }
        runs.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let answer = match behavior {
            Behavior::Silent => continue,
            Behavior::Close => return,
            Behavior::WrongId => {
                let mut result = IpcMessage::new(kind::RESULT);
                result.id = request.id.map(|id| id + 1);
                result.exit_code = Some(0);
                result.stdout = Some(String::new());
                result.stderr = Some(String::new());
                result
            }
            Behavior::TooLarge => {
                let mut failure =
                    IpcMessage::failure("response_too_large", "Too large.", request.id);
                failure.exit_code = Some(1);
                failure
            }
            Behavior::Refuse(code) => {
                let text = format!("Refused by the test ({code}).");
                let mut failure = IpcMessage::failure(code, &text, request.id);
                failure.exit_code = Some(3);
                failure
            }
        };
        write(&mut stream, &answer).await;
    }
}
