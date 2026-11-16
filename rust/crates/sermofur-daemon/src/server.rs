//! The daemon server, as `DaemonServer` and `ServingGate` of the .NET tool (decision A2: tokio
//! for the sessions, the commands themselves run synchronously by `spawn_blocking`).
//!
//! The server knows nothing of the commands: a [`CommandExecutor`] given by the command line
//! plans and runs them. This keeps `sermofur-daemon` free of a dependency on `smf`.

use std::future::{Future, poll_fn};
use std::path::Path;
use std::pin::pin;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::task::Poll;
use std::time::{Duration, Instant, SystemTime};

use sermofur_core::daemon_paths::DaemonPaths;
use sermofur_core::discovery::discover;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::format::DateTimeOffset;
use sermofur_core::local_paths::{contains, same_root};
use sermofur_core::ownership::{FileOwnership, SystemOwnership};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::{sleep, sleep_until, timeout};

use crate::endpoint::{self, Stream};
use crate::framing::{
    self, CURRENT_PROTOCOL, FrameError, IpcMessage, MAX_BODY_BYTES, MAX_RESPONSE_BYTES, kind,
};
use crate::log::DaemonLog;
use crate::registry::{InstanceRegistry, RegistryStamp};

/// Time limits of the daemon (`DaemonLimits`).
#[derive(Clone, Debug)]
pub struct DaemonLimits {
    pub hello_timeout: Duration,
    pub request_timeout: Duration,
    /// Commands running at once; at least one.
    pub max_executions: usize,
    /// `None`: a twentieth of the request timeout.
    pub minimum_run_time: Option<Duration>,
    pub read_place_timeout: Duration,
}

impl Default for DaemonLimits {
    fn default() -> Self {
        let processors = std::thread::available_parallelism().map_or(1, usize::from);
        Self {
            hello_timeout: Duration::from_secs(10),
            request_timeout: Duration::from_secs(60),
            max_executions: processors.saturating_sub(1).max(2),
            minimum_run_time: None,
            read_place_timeout: Duration::from_secs(5),
        }
    }
}

impl DaemonLimits {
    pub fn drain_timeout(&self) -> Duration {
        self.request_timeout + Duration::from_secs(5)
    }

    fn minimum_run_time(&self) -> Duration {
        self.minimum_run_time.unwrap_or(self.request_timeout / 20)
    }
}

/// What a command touches: the folder it works on, and whether it may write there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPlan {
    pub path: String,
    pub writes: bool,
}

/// What a command printed and returned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandOutcome {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Plans and runs one command line for a client (`ICommandExecutor`).
pub trait CommandExecutor: Send + Sync + 'static {
    /// A refusal is answered to the client as an `error` with its code and exit code.
    fn plan(&self, argv: &[String], cwd: &str) -> Result<CommandPlan>;
    fn execute(&self, argv: &[String], cwd: &str) -> CommandOutcome;
}

type Clock = Box<dyn Fn() -> SystemTime + Send + Sync>;
type Refused = Box<dyn Fn(&str) + Send + Sync>;

/// Serves only the instances of the registry (`ServingGate`): an instance is open from its
/// first command until it is idle or unregistered.
pub struct ServingGate {
    registry: InstanceRegistry,
    owners: Box<dyn FileOwnership + Send + Sync>,
    idle: Duration,
    clock: Clock,
    refused: Refused,
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    roots: Vec<String>,
    loaded: Option<RegistryStamp>,
    recent: bool,
    // ponytail: a list searched by `same_root`, at most 256 roots.
    open: Vec<(String, SystemTime)>,
}

/// A registry written less than this long ago is read again even if its stamp did not change.
const RECENT_WRITE: Duration = Duration::from_secs(3);

impl ServingGate {
    /// The gate of the daemon: real file system, 10 minutes of idleness, system clock.
    pub fn new(registry: InstanceRegistry) -> Self {
        Self::with(
            registry,
            Box::new(SystemOwnership),
            Duration::from_secs(600),
            Box::new(SystemTime::now),
            Box::new(|_| {}),
        )
    }

    pub fn with(
        registry: InstanceRegistry,
        owners: Box<dyn FileOwnership + Send + Sync>,
        idle: Duration,
        clock: Clock,
        refused: Refused,
    ) -> Self {
        Self {
            registry,
            owners,
            idle,
            clock,
            refused,
            state: Mutex::default(),
        }
    }

    /// Called with the code of each path refused (the daemon journals `not_served`).
    #[must_use]
    pub fn refusing(mut self, refused: Refused) -> Self {
        self.refused = refused;
        self
    }

    fn state(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Instances used within the idle limit.
    pub fn open_count(&self) -> i32 {
        let mut state = self.state();
        let limit = (self.clock)().checked_sub(self.idle);
        state
            .open
            .retain(|(_, used)| limit.is_none_or(|limit| *used >= limit));
        i32::try_from(state.open.len()).unwrap_or(i32::MAX)
    }

    /// The registered root that serves `path`, or `None`: not registered, not an instance, or
    /// an instance nested in a registered one without being registered itself.
    pub fn resolve(&self, path: &str) -> Option<String> {
        let candidate = {
            let mut state = self.state();
            self.reload(&mut state);
            let target = Path::new(path);
            // `OrderByDescending(Root.Length).FirstOrDefault()`: the first of the longest.
            state
                .roots
                .iter()
                .filter(|root| contains(Path::new(root), target))
                .fold(None::<&String>, |best, root| match best {
                    Some(best) if utf16_len(best) >= utf16_len(root) => Some(best),
                    _ => Some(root),
                })
                .cloned()
        };
        let Some(candidate) = candidate else {
            (self.refused)("not_registered");
            return None;
        };
        let root = match discover(Path::new(path), self.owners.as_ref()) {
            Ok(root) => root,
            Err(error) => {
                (self.refused)(&error.code);
                return None;
            }
        };
        if !same_root(&root.to_string_lossy(), &candidate) {
            (self.refused)("nested_instance_not_registered");
            return None;
        }
        let now = (self.clock)();
        let mut state = self.state();
        match state
            .open
            .iter_mut()
            .find(|(r, _)| same_root(r, &candidate))
        {
            Some(entry) => entry.1 = now,
            None => state.open.push((candidate.clone(), now)),
        }
        Some(candidate)
    }

    fn reload(&self, state: &mut GateState) {
        let stamp = self.registry.stamp();
        if state.loaded == Some(stamp) && !state.recent {
            return;
        }
        state.roots = match self.registry.read() {
            Ok(entries) => entries.into_iter().filter_map(|e| e.root).collect(),
            Err(error) => {
                (self.refused)(&error.code);
                Vec::new()
            }
        };
        state.loaded = Some(stamp);
        // `DateTime.UtcNow - WrittenUtc`, never the injected clock; a future date is not recent.
        state.recent = stamp.is_some_and(|(written, _)| {
            SystemTime::now()
                .duration_since(written)
                .is_ok_and(|age| age < RECENT_WRITE)
        });
        let GateState { roots, open, .. } = state;
        open.retain(|(root, _)| roots.iter().any(|entry| same_root(entry, root)));
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// The daemon server: one session per client, one command at a time per session.
pub struct DaemonServer {
    shared: Arc<Shared>,
}

struct Shared {
    paths: DaemonPaths,
    tool_version: String,
    executor: Arc<dyn CommandExecutor>,
    gate: ServingGate,
    log: DaemonLog,
    limits: DaemonLimits,
    // ponytail: a list searched by `same_root`, at most 256 roots.
    writers: Mutex<Vec<(String, Arc<Semaphore>)>>,
    executions: Arc<Semaphore>,
    started_at: String,
    clients: AtomicI32,
    stop: watch::Sender<bool>,
    /// One clone per command still running, released by the command itself: the drain waits
    /// for them, a timeout does not.
    works: Arc<()>,
}

/// How a session ends: quietly (end of stream, I/O failure, stop), or refused with a code
/// that the journal keeps.
enum End {
    Quiet,
    Refused(SermofurError),
}

impl DaemonServer {
    pub fn new(
        paths: DaemonPaths,
        tool_version: &str,
        executor: Arc<dyn CommandExecutor>,
        gate: ServingGate,
        log: DaemonLog,
        limits: DaemonLimits,
    ) -> Self {
        let executions = Arc::new(Semaphore::new(limits.max_executions.max(1)));
        Self {
            shared: Arc::new(Shared {
                paths,
                tool_version: tool_version.to_owned(),
                executor,
                gate,
                log,
                limits,
                writers: Mutex::default(),
                executions,
                started_at: DateTimeOffset::now_local().to_seconds(),
                clients: AtomicI32::new(0),
                stop: watch::channel(false).0,
                works: Arc::new(()),
            }),
        }
    }

    /// Asks the server to stop: no new client, the started commands are drained.
    pub fn stop(&self) {
        self.shared.stop.send_replace(true);
    }

    /// Serves until [`DaemonServer::stop`] or a `shutdown` request; `listening` is called once
    /// the endpoint is open. Fails only when the endpoint cannot be opened.
    pub async fn run(&self, owners: &dyn FileOwnership, listening: impl FnOnce()) -> Result<()> {
        let shared = &self.shared;
        let mut listener = endpoint::listen(&shared.paths, owners).await?;
        shared.log.write("started", None, None, None);
        listening();
        let mut stop = shared.stop.subscribe();
        let mut sessions: Vec<JoinHandle<()>> = Vec::new();
        loop {
            let accepted = first(listener.accept(), fired(&mut stop)).await;
            match accepted {
                Ok(Ok(stream)) => {
                    sessions.retain(|session| !session.is_finished());
                    sessions.push(tokio::spawn(serve(Arc::clone(shared), stream)));
                }
                // An I/O failure of one connection never stops the daemon; a short pause keeps a
                // persistent failure from spinning the only thread of the runtime.
                Ok(Err(_)) => sleep(Duration::from_millis(10)).await,
                Err(()) => break,
            }
        }
        // Windows: a client that connects while the daemon drains is closed at once instead
        // of waiting for an instance; on Unix the socket is removed now.
        #[cfg(windows)]
        let refuser = tokio::spawn(async move {
            loop {
                if listener.accept().await.is_err() {
                    sleep(Duration::from_millis(10)).await;
                }
            }
        });
        #[cfg(not(windows))]
        drop(listener);
        let drained = async {
            for session in sessions {
                let _ = session.await;
            }
            // ponytail: polled every 10 ms; a notification if the drain must react faster.
            while Arc::strong_count(&shared.works) > 1 {
                sleep(Duration::from_millis(10)).await;
            }
        };
        if timeout(shared.limits.drain_timeout(), drained)
            .await
            .is_err()
        {
            shared.log.write("drain_timeout", None, None, None);
        }
        #[cfg(windows)]
        {
            refuser.abort();
            let _ = refuser.await;
        }
        shared.log.write("stopped", None, None, None);
        shared.log.flush_before_exit();
        Ok(())
    }
}

async fn serve(shared: Arc<Shared>, stream: Stream) {
    shared.clients.fetch_add(1, Ordering::SeqCst);
    let (mut reader, mut writer) = tokio::io::split(stream);
    let mut pid = None;
    let ended = async {
        let Some(hello) = read_hello(&shared, &mut reader, &mut writer).await? else {
            return Ok(());
        };
        pid = hello.pid;
        session(&shared, reader, &mut writer, &hello).await
    }
    .await;
    if let Err(End::Refused(error)) = ended {
        shared.log.write("refused", Some(&error.code), pid, None);
    }
    shared.clients.fetch_sub(1, Ordering::SeqCst);
}

async fn read_hello(
    shared: &Shared,
    reader: &mut ReadHalf<Stream>,
    writer: &mut WriteHalf<Stream>,
) -> std::result::Result<Option<IpcMessage>, End> {
    let mut stop = shared.stop.subscribe();
    let read = first(read_message(reader, MAX_BODY_BYTES), fired(&mut stop));
    let hello = match timeout(shared.limits.hello_timeout, read).await {
        Ok(Ok(Ok(Some(hello)))) => hello,
        Ok(Ok(Ok(None))) => return Ok(None),
        Ok(Ok(Err(error))) => return Err(frame_failure(writer, error).await),
        Ok(Err(())) | Err(_) => return Err(End::Quiet),
    };
    if hello.kind == kind::SHUTDOWN {
        accept_shutdown(shared, writer, None).await?;
        return Ok(None);
    }
    if hello.kind != kind::HELLO {
        let text = format!("Expected a hello, not '{}'.", hello.kind);
        return Err(refuse(writer, IpcMessage::failure("protocol_error", &text, None)).await);
    }
    let version = &shared.tool_version;
    if hello.tool_version.as_deref() != Some(version.as_str()) {
        let text = format!(
            "The daemon runs Sermofur {version}; this client is {}. Run: smf daemon restart",
            hello.tool_version.as_deref().unwrap_or_default()
        );
        let mut refusal = IpcMessage::failure("daemon_version_mismatch", &text, None);
        refusal.tool_version = Some(version.clone());
        return Err(refuse(writer, refusal).await);
    }
    let absolute = hello.cwd.as_deref().is_some_and(|cwd| {
        !cwd.is_empty() && sermofur_core::local_paths::is_path_fully_qualified(cwd)
    });
    if hello.protocol != Some(CURRENT_PROTOCOL) || !absolute {
        let text = format!("Expected a hello of protocol {CURRENT_PROTOCOL} with an absolute cwd.");
        return Err(refuse(writer, IpcMessage::failure("protocol_error", &text, None)).await);
    }
    let mut welcome = IpcMessage::new(kind::WELCOME);
    welcome.protocol = Some(CURRENT_PROTOCOL);
    welcome.tool_version = Some(version.clone());
    write_message(writer, &welcome, MAX_BODY_BYTES).await?;
    Ok(Some(hello))
}

type Read = std::result::Result<Option<IpcMessage>, FrameError>;

async fn session(
    shared: &Arc<Shared>,
    mut reader: ReadHalf<Stream>,
    writer: &mut WriteHalf<Stream>,
    hello: &IpcMessage,
) -> std::result::Result<(), End> {
    // The reader reads the next message while a command runs: an end of stream then means
    // the client is gone, a message means a second run on the same connection.
    let (sender, mut messages) = mpsc::channel::<Read>(1);
    let _reading = AbortOnDrop(tokio::spawn(async move {
        loop {
            let read = read_message(&mut reader, MAX_BODY_BYTES).await;
            let last = !matches!(read, Ok(Some(_)));
            if sender.send(read).await.is_err() || last {
                return;
            }
        }
    }));
    let mut stop = shared.stop.subscribe();
    let mut pending: Option<Option<Read>> = None;
    loop {
        let next = match pending.take() {
            Some(next) => next,
            None => match first(messages.recv(), fired(&mut stop)).await {
                Ok(next) => next,
                Err(()) => return Err(End::Quiet),
            },
        };
        let message = match next {
            Some(Ok(Some(message))) => message,
            Some(Ok(None)) => return Ok(()),
            Some(Err(error)) => return Err(frame_failure(writer, error).await),
            None => return Err(End::Quiet),
        };
        if message.kind == kind::STATUS {
            write_message(writer, &status_message(shared), MAX_RESPONSE_BYTES).await?;
            continue;
        }
        if message.kind == kind::SHUTDOWN {
            accept_shutdown(shared, writer, hello.pid).await?;
            return Ok(());
        }
        if message.kind != kind::RUN {
            let text = format!("Unknown message kind '{}'.", message.kind);
            return Err(refuse(writer, IpcMessage::failure("protocol_error", &text, None)).await);
        }
        if message.cwd != hello.cwd || message.argv.is_none() || message.id.is_none() {
            let text = "A run carries an id, argv and the cwd of its hello.";
            return Err(refuse(writer, IpcMessage::failure("protocol_error", text, None)).await);
        }
        let (gone, gone_watch) = watch::channel(false);
        let run = tokio::spawn(run_request(
            Arc::clone(shared),
            message,
            hello.pid,
            gone_watch,
        ));
        let mut run = pin!(run);
        let finished = match first(run.as_mut(), messages.recv()).await {
            Ok(finished) => finished,
            Err(Some(Ok(None)) | None) => {
                gone.send_replace(true);
                shared.log.write("client_gone", None, hello.pid, None);
                return Ok(());
            }
            Err(Some(Ok(Some(_)))) => {
                let text = "One run at a time per connection.";
                return Err(
                    refuse(writer, IpcMessage::failure("protocol_error", text, None)).await,
                );
            }
            // A broken next message is answered after the result of the command.
            Err(failed) => {
                pending = Some(failed);
                run.await
            }
        };
        match finished {
            Ok(Some((answer, writes))) => write_answer(writer, &answer, writes).await?,
            Ok(None) => return Err(End::Quiet),
            Err(_) => {
                shared.log.write("error", Some("panic"), hello.pid, None);
                return Err(End::Quiet);
            }
        }
    }
}

/// Runs one request; `None` when the client went away before the end of the command.
async fn run_request(
    shared: Arc<Shared>,
    message: IpcMessage,
    pid: Option<i32>,
    mut gone: watch::Receiver<bool>,
) -> Option<(IpcMessage, bool)> {
    let started = Instant::now();
    let deadline = tokio::time::Instant::from_std(started + shared.limits.request_timeout);
    let id = message.id;
    let cwd = message.cwd.unwrap_or_default();
    // ponytail: a `null` item of argv is run as an empty argument; not measured on the oracle.
    let argv: Vec<String> = message
        .argv
        .unwrap_or_default()
        .into_iter()
        .map(Option::unwrap_or_default)
        .collect();
    let elapsed = || i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX);
    let plan = match shared.executor.plan(&argv, &cwd) {
        Ok(plan) => plan,
        Err(error) => {
            shared.log.write("refused", Some(&error.code), pid, None);
            return Some((
                failure(&error.code, &error.message, id, error.exit_code),
                false,
            ));
        }
    };
    let Some(root) = shared.gate.resolve(&plan.path) else {
        let mut answer = IpcMessage::new(kind::NOT_SERVING);
        answer.id = id;
        return Some((answer, plan.writes));
    };
    let writer = plan.writes.then(|| {
        let mut writers = shared
            .writers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        match writers.iter().find(|(r, _)| same_root(r, &root)) {
            Some((_, turn)) => Arc::clone(turn),
            None => {
                let turn = Arc::new(Semaphore::new(1));
                writers.push((root, Arc::clone(&turn)));
                turn
            }
        }
    });
    let request = Request {
        shared: &shared,
        id,
        pid,
        deadline,
        elapsed: &elapsed,
    };
    let permits = match acquire(&request, writer, &mut gone).await {
        Wait::Got(permits) => permits,
        Wait::Answer(answer) => return Some((*answer, plan.writes)),
        Wait::Gone => return None,
    };
    let executor = Arc::clone(&shared.executor);
    let works = Arc::clone(&shared.works);
    // The permits and the tracker go with the command: a timeout never frees the turn.
    let work = tokio::task::spawn_blocking(move || {
        let _held = (permits, works);
        executor.execute(&argv, &cwd)
    });
    match first(work, first(sleep_until(deadline), fired(&mut gone))).await {
        Ok(Ok(outcome)) => {
            let code = (outcome.exit_code != 0).then(|| format!("exit_{}", outcome.exit_code));
            shared
                .log
                .write("run", code.as_deref(), pid, Some(elapsed()));
            let mut result = IpcMessage::new(kind::RESULT);
            result.id = id;
            result.exit_code = Some(outcome.exit_code);
            result.stdout = Some(outcome.stdout);
            result.stderr = Some(outcome.stderr);
            Some((result, plan.writes))
        }
        Ok(Err(_)) => {
            shared
                .log
                .write("error", Some("panic"), pid, Some(elapsed()));
            None
        }
        Err(Ok(())) => {
            shared
                .log
                .write("timeout", Some("request_timeout"), pid, Some(elapsed()));
            // `{RequestTimeout.TotalSeconds:0}`: rounded half away from zero.
            let seconds = shared.limits.request_timeout.as_secs_f64().round();
            let text = format!(
                "The command took longer than {seconds} s; it keeps running and may still apply. Check before running it again."
            );
            Some((failure("request_timeout", &text, id, 3), plan.writes))
        }
        Err(Err(())) => {
            shared.log.write("abandoned", None, pid, Some(elapsed()));
            None
        }
    }
}

struct Request<'a> {
    shared: &'a Shared,
    id: Option<i64>,
    pid: Option<i32>,
    deadline: tokio::time::Instant,
    elapsed: &'a (dyn Fn() -> i64 + Sync),
}

enum Wait<T> {
    Got(T),
    Answer(Box<IpcMessage>),
    Gone,
}

type Permits = (Option<OwnedSemaphorePermit>, OwnedSemaphorePermit);

/// The turn of the instance for a writing command, then a place among the running commands
/// (`AcquireAsync`); a reading command waits for a place at most `read_place_timeout`.
async fn acquire(
    request: &Request<'_>,
    writer: Option<Arc<Semaphore>>,
    gone: &mut watch::Receiver<bool>,
) -> Wait<Permits> {
    let limits = &request.shared.limits;
    let turn = match &writer {
        Some(writer) => match wait(request, Arc::clone(writer), request.deadline, gone).await {
            Wait::Got(turn) => Some(turn),
            Wait::Answer(answer) => return Wait::Answer(answer),
            Wait::Gone => return Wait::Gone,
        },
        None => None,
    };
    let limit = if writer.is_some() {
        request.deadline
    } else {
        request
            .deadline
            .min(tokio::time::Instant::now() + limits.read_place_timeout)
    };
    let executions = Arc::clone(&request.shared.executions);
    match wait(request, executions, limit, gone).await {
        Wait::Got(place) => {
            let left = request
                .deadline
                .saturating_duration_since(tokio::time::Instant::now());
            if left < limits.minimum_run_time() {
                return Wait::Answer(Box::new(busy(request)));
            }
            Wait::Got((turn, place))
        }
        Wait::Answer(answer) => Wait::Answer(answer),
        Wait::Gone => Wait::Gone,
    }
}

async fn wait(
    request: &Request<'_>,
    semaphore: Arc<Semaphore>,
    limit: tokio::time::Instant,
    gone: &mut watch::Receiver<bool>,
) -> Wait<OwnedSemaphorePermit> {
    let mut stop = request.shared.stop.subscribe();
    let ended = first(
        semaphore.acquire_owned(),
        first(sleep_until(limit), first(fired(gone), fired(&mut stop))),
    )
    .await;
    match ended {
        Ok(Ok(permit)) => Wait::Got(permit),
        Ok(Err(_)) => Wait::Gone,
        Err(_) if *stop.borrow() => {
            let elapsed = (request.elapsed)();
            request.shared.log.write(
                "stopping",
                Some("daemon_stopping"),
                request.pid,
                Some(elapsed),
            );
            Wait::Answer(Box::new(failure(
                "daemon_stopping",
                "The daemon is stopping; the command did not start.",
                request.id,
                3,
            )))
        }
        Err(_) if !*gone.borrow() => Wait::Answer(Box::new(busy(request))),
        Err(_) => Wait::Gone,
    }
}

fn busy(request: &Request<'_>) -> IpcMessage {
    let elapsed = (request.elapsed)();
    request
        .shared
        .log
        .write("busy", Some("daemon_busy"), request.pid, Some(elapsed));
    failure(
        "daemon_busy",
        "The daemon was too busy to start the command in time; it did not start. Run it again.",
        request.id,
        3,
    )
}

fn status_message(shared: &Shared) -> IpcMessage {
    let mut status = IpcMessage::new(kind::STATUS);
    status.tool_version = Some(shared.tool_version.clone());
    status.pid = i32::try_from(std::process::id()).ok();
    status.registry = Some(shared.paths.registry_file().to_string_lossy().into_owned());
    status.started_at = Some(shared.started_at.clone());
    status.instances_open = Some(shared.gate.open_count());
    status.clients = Some(shared.clients.load(Ordering::SeqCst));
    status
}

async fn accept_shutdown(
    shared: &Shared,
    writer: &mut WriteHalf<Stream>,
    pid: Option<i32>,
) -> std::result::Result<(), End> {
    write_message(writer, &IpcMessage::new(kind::STOPPING), MAX_BODY_BYTES).await?;
    shared.log.write("shutdown_requested", None, pid, None);
    shared.stop.send_replace(true);
    Ok(())
}

/// `IpcMessage.Failure` with an exit code.
fn failure(code: &str, message: &str, id: Option<i64>, exit_code: i32) -> IpcMessage {
    let mut failure = IpcMessage::failure(code, message, id);
    failure.exit_code = Some(exit_code);
    failure
}

/// A frame that could not be read: a refusal is answered, then ends the session.
async fn frame_failure(writer: &mut WriteHalf<Stream>, error: FrameError) -> End {
    match error {
        FrameError::Refused(error) => {
            let answer = failure(&error.code, &error.message, None, error.exit_code);
            if let Err(end) = try_answer(writer, &answer).await {
                return end;
            }
            End::Refused(error)
        }
        FrameError::Io(_) => End::Quiet,
    }
}

/// Answers the refusal, then ends the session with its code (exit 3).
async fn refuse(writer: &mut WriteHalf<Stream>, refusal: IpcMessage) -> End {
    if let Err(end) = try_answer(writer, &refusal).await {
        return end;
    }
    End::Refused(SermofurError::new(
        refusal.code.unwrap_or_default(),
        refusal.message.unwrap_or_default(),
        3,
    ))
}

/// Writes within 2 s; a stream that fails is ignored, a message over the limit is not.
async fn try_answer(
    writer: &mut WriteHalf<Stream>,
    message: &IpcMessage,
) -> std::result::Result<(), End> {
    let frame = framing::encode(message, MAX_BODY_BYTES).map_err(End::Refused)?;
    let _ = timeout(Duration::from_secs(2), send(writer, &frame)).await;
    Ok(())
}

async fn write_message(
    writer: &mut WriteHalf<Stream>,
    message: &IpcMessage,
    limit: usize,
) -> std::result::Result<(), End> {
    let frame = framing::encode(message, limit).map_err(End::Refused)?;
    send(writer, &frame).await.map_err(|_| End::Quiet)
}

/// The result of a command, or `response_too_large` when it does not fit in a frame.
async fn write_answer(
    writer: &mut WriteHalf<Stream>,
    answer: &IpcMessage,
    writes: bool,
) -> std::result::Result<(), End> {
    // `string.Length`: UTF-16 units, as .NET counts them before encoding.
    let characters = answer.stdout.as_deref().map_or(0, utf16_len)
        + answer.stderr.as_deref().map_or(0, utf16_len);
    let frame = Some(characters)
        .filter(|c| *c <= MAX_RESPONSE_BYTES)
        .and_then(|_| framing::encode(answer, MAX_RESPONSE_BYTES).ok());
    let frame = match frame {
        Some(frame) => frame,
        None => framing::encode(&too_large(answer.id, writes), MAX_RESPONSE_BYTES)
            .map_err(End::Refused)?,
    };
    match timeout(Duration::from_secs(5), send(writer, &frame)).await {
        Ok(Ok(())) => Ok(()),
        _ => Err(End::Quiet),
    }
}

fn too_large(id: Option<i64>, writes: bool) -> IpcMessage {
    let mib = MAX_RESPONSE_BYTES / (1024 * 1024);
    let text = if writes {
        format!(
            "The command ran, but its output is over {mib} MiB and was not returned. Check its effect before running it again."
        )
    } else {
        format!(
            "The output of the command is over {mib} MiB; narrow it (from the CLI, it then runs directly)."
        )
    };
    failure("response_too_large", &text, id, 1)
}

async fn send(writer: &mut (impl AsyncWrite + Unpin), frame: &[u8]) -> std::io::Result<()> {
    writer.write_all(frame).await?;
    writer.flush().await
}

/// `Framing.ReadAsync`: the bytes of one frame are gathered, then read by [`framing::read`],
/// so that every refusal and every count of bytes consumed is the one of the synchronous reader.
pub async fn read_message(stream: &mut (impl AsyncRead + Unpin), limit: usize) -> Read {
    let mut bytes = vec![0u8; 4];
    let got = fill(stream, &mut bytes).await?;
    bytes.truncate(got);
    if let Ok(header) = <[u8; 4]>::try_from(bytes.as_slice()) {
        let size = usize::try_from(u32::from_le_bytes(header)).unwrap_or(usize::MAX);
        if size != 0 && size <= limit {
            bytes.resize(4 + size, 0);
            let got = fill(stream, &mut bytes[4..]).await?;
            bytes.truncate(4 + got);
        }
    }
    framing::read(&mut bytes.as_slice(), limit)
}

async fn fill(stream: &mut (impl AsyncRead + Unpin), buffer: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buffer.len() {
        match stream.read(&mut buffer[filled..]).await? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

/// Resolves once the flag is set; never when its sender is gone without setting it.
async fn fired(flag: &mut watch::Receiver<bool>) {
    if flag.wait_for(|set| *set).await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// The first of two futures to finish (`Ok` for the left one, preferred when both are ready);
/// the other is dropped.
pub async fn first<L: Future, R: Future>(
    left: L,
    right: R,
) -> std::result::Result<L::Output, R::Output> {
    let mut left = pin!(left);
    let mut right = pin!(right);
    poll_fn(|context| {
        if let Poll::Ready(value) = left.as_mut().poll(context) {
            return Poll::Ready(Ok(value));
        }
        if let Poll::Ready(value) = right.as_mut().poll(context) {
            return Poll::Ready(Err(value));
        }
        Poll::Pending
    })
    .await
}

struct AbortOnDrop(JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
