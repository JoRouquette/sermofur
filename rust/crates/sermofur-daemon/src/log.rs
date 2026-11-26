//! Local journal of the daemon, as `DaemonLog` of the .NET oracle (`rust/tests/golden/
//! daemon-files.json`, section `logs`, is the truth): one JSON object per line, local time
//! with its offset, three files of 1 MiB at most. Only events, codes, client process IDs and
//! durations are written.
//!
//! A caller never waits for the file: [`DaemonLog::write`] queues the line and returns. One
//! thread per journal writes the queued lines in batches, under the lock file
//! `daemon.log.lock`, shared with every other process (daemon, supervisor, MCP bridges, .NET
//! ones included). Lines are lost when the queue is full, when another process holds the turn
//! for a whole second, when the file cannot be written, or when the process ends before
//! [`DaemonLog::flush_before_exit`].

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_core::format::DateTimeOffset;

use crate::file_turn::try_take;
use crate::framing::{int_field, string_field};

/// Largest journal file before it rotates.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Files kept: `daemon.log`, `daemon.log.1`, `daemon.log.2`.
pub const KEPT_FILES: usize = 3;
/// Lines waiting for the writer, at most; further lines are dropped.
pub const MAX_PENDING_LINES: usize = 4096;
/// Lines written in one turn at most, before the turn goes back to the others.
pub const MAX_BATCH_LINES: usize = 512;
/// How long a process gives its last lines before it exits.
pub const EXIT_FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

const TURN_TIMEOUT: Duration = Duration::from_secs(1);
const TURN_POLL: Duration = Duration::from_millis(1);

/// The clock of the journal.
pub type Clock = Box<dyn Fn() -> DateTimeOffset + Send + Sync>;

/// One journal object of a process.
#[derive(Clone)]
pub struct DaemonLog {
    inner: Arc<Inner>,
}

struct Inner {
    file: PathBuf,
    clock: Clock,
    max_file_bytes: u64,
    pending: Mutex<VecDeque<Vec<u8>>>,
    queued: AtomicUsize,
    writing: AtomicBool,
}

impl DaemonLog {
    /// A journal on `file`, dated by the local clock.
    pub fn new(file: &Path) -> Self {
        Self::with_clock(file, Box::new(DateTimeOffset::now_local), MAX_FILE_BYTES)
    }

    /// A journal with its own clock and file limit, for tests.
    pub fn with_clock(file: &Path, clock: Clock, max_file_bytes: u64) -> Self {
        Self {
            inner: Arc::new(Inner {
                file: file.to_path_buf(),
                clock,
                max_file_bytes,
                pending: Mutex::new(VecDeque::new()),
                queued: AtomicUsize::new(0),
                writing: AtomicBool::new(false),
            }),
        }
    }

    /// Queues one line; never blocks on the file.
    pub fn write(
        &self,
        event: &str,
        code: Option<&str>,
        client_pid: Option<i32>,
        duration_ms: Option<i64>,
    ) {
        let inner = &self.inner;
        let mut line = format!("{{\"time\":\"{}\"", (inner.clock)().to_log());
        string_field(&mut line, "event", Some(event));
        string_field(&mut line, "code", code);
        int_field(&mut line, "pid", client_pid.map(i64::from));
        int_field(&mut line, "durationMs", duration_ms);
        line.push_str("}\n");
        if inner.queued.fetch_add(1, Ordering::SeqCst) >= MAX_PENDING_LINES {
            inner.queued.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        inner.pending().push_back(line.into_bytes());
        if inner
            .writing
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            let writer = Arc::clone(inner);
            // A thread of its own: waiting for the turn must not hold anything else.
            let spawned = thread::Builder::new()
                .name("sermofur-journal".to_owned())
                .spawn(move || writer.write_queued());
            if spawned.is_err() {
                inner.drop_all();
                inner.writing.store(false, Ordering::SeqCst);
            }
        }
    }

    /// Waits until the queued lines are written, or the timeout passes; true when nothing is
    /// left.
    pub fn flush(&self, timeout: Duration) -> bool {
        let started = Instant::now();
        while self.inner.queued.load(Ordering::SeqCst) > 0
            || self.inner.writing.load(Ordering::SeqCst)
        {
            if started.elapsed() >= timeout {
                return false;
            }
            thread::sleep(Duration::from_millis(2));
        }
        true
    }

    /// Writes the queued lines before the process exits.
    pub fn flush_before_exit(&self) -> bool {
        self.flush(EXIT_FLUSH_TIMEOUT)
    }
}

impl Inner {
    fn pending(&self) -> std::sync::MutexGuard<'_, VecDeque<Vec<u8>>> {
        self.pending
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn take_line(&self) -> Option<Vec<u8>> {
        let line = self.pending().pop_front();
        if line.is_some() {
            self.queued.fetch_sub(1, Ordering::SeqCst);
        }
        line
    }

    fn write_queued(&self) {
        loop {
            self.write_batch();
            self.writing.store(false, Ordering::SeqCst);
            // A line queued after the batch and before the flag went down started no writer;
            // lines left by a bounded batch are written in the next turn.
            if self.pending().is_empty()
                || self
                    .writing
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
            {
                return;
            }
        }
    }

    /// One turn: at most [`MAX_BATCH_LINES`] lines; a failure loses this batch only.
    fn write_batch(&self) {
        let mut taken = 0;
        let mut lock_file = self.file.clone().into_os_string();
        lock_file.push(".lock");
        let Ok(Some(_turn)) = try_take(Path::new(&lock_file), TURN_TIMEOUT, TURN_POLL) else {
            self.drop_batch(taken);
            return;
        };
        let Some((mut journal, mut length)) = self.open_journal() else {
            self.drop_batch(taken);
            return;
        };
        while taken < MAX_BATCH_LINES {
            let Some(line) = self.take_line() else {
                return;
            };
            taken += 1;
            if length >= self.max_file_bytes {
                drop(journal);
                let reopened = self.rotate().then(|| self.open_journal()).flatten();
                let Some(reopened) = reopened else {
                    self.drop_batch(taken);
                    return;
                };
                (journal, length) = reopened;
            }
            if journal.write_all(&line).is_err() {
                self.drop_batch(taken);
                return;
            }
            length += line.len() as u64;
        }
    }

    /// The journal opened for appending, shared for reading, writing and deletion, with its
    /// length.
    fn open_journal(&self) -> Option<(File, u64)> {
        let journal = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file)
            .ok()?;
        let length = journal.metadata().ok()?.len();
        Some((journal, length))
    }

    /// `daemon.log.1` becomes `daemon.log.2`, then `daemon.log` becomes `daemon.log.1`.
    fn rotate(&self) -> bool {
        let numbered = |index: usize| {
            let mut name = self.file.clone().into_os_string();
            name.push(format!(".{index}"));
            PathBuf::from(name)
        };
        for index in (1..KEPT_FILES).rev() {
            let source = if index == 1 {
                self.file.clone()
            } else {
                numbered(index - 1)
            };
            if source.is_file() && fs::rename(&source, numbered(index)).is_err() {
                return false;
            }
        }
        true
    }

    /// Drops the rest of a failed batch, leaving the lines queued after it.
    fn drop_batch(&self, taken: usize) {
        for _ in taken..MAX_BATCH_LINES {
            if self.take_line().is_none() {
                return;
            }
        }
    }

    fn drop_all(&self) {
        while self.take_line().is_some() {}
    }
}
