//! Ports of `SharedFilesTests` (several processes on one registry and one journal) and the
//! cross-process cases of the lock and the registry with the .NET oracle (`SMF_ORACLE`, the
//! `smf` of .NET 0.4.3): each side sees the lock and the registry of the other. Every folder
//! lives under the temporary directory; `SERMOFUR_DAEMON_HOME` points the oracle there.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use sermofur_daemon::daemon_paths::{DaemonPaths, HOME_VARIABLE};
use sermofur_daemon::file_turn::{acquire, is_held, take, wait_for_release};
use sermofur_daemon::log::DaemonLog;
use sermofur_daemon::registry::InstanceRegistry;

/// A fresh folder under the temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let folder = temporary.join(format!(
            "smfd-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("the scratch folder");
        Self(folder)
    }

    fn instance(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        root
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn home_paths(home: &Path) -> DaemonPaths {
    let text = home.to_string_lossy().into_owned();
    DaemonPaths::resolve(&|key| (key == HOME_VARIABLE).then(|| text.clone()), &|_| {
        String::new()
    })
    .expect("the paths of a home")
}

fn roots(registry: &InstanceRegistry) -> Vec<String> {
    let mut roots: Vec<String> = registry
        .read()
        .expect("readable")
        .into_iter()
        .map(|entry| entry.root.expect("a root"))
        .collect();
    roots.sort();
    roots
}

fn now() -> sermofur_core::format::DateTimeOffset {
    sermofur_core::format::DateTimeOffset::now_utc()
}

#[test]
fn concurrent_registrations_are_all_kept() {
    let scratch = Scratch::new("concurrent");
    let file = home_paths(&scratch.0.join("home")).registry_file();
    let instances: Vec<PathBuf> = (0..8).map(|i| scratch.instance(&format!("i{i}"))).collect();
    thread::scope(|scope| {
        for root in &instances {
            let file = &file;
            scope.spawn(move || {
                InstanceRegistry::new(file)
                    .register(root, now())
                    .expect("registered");
            });
        }
    });
    let mut expected: Vec<String> = instances
        .iter()
        .map(|root| root.to_string_lossy().into_owned())
        .collect();
    expected.sort();
    assert_eq!(roots(&InstanceRegistry::new(&file)), expected);
}

#[test]
fn registry_opened_by_a_reader_is_replaced_once_the_reader_lets_go() {
    let scratch = Scratch::new("reader");
    let file = home_paths(&scratch.0.join("home")).registry_file();
    let registry = InstanceRegistry::new(&file);
    registry
        .register(&scratch.instance("first"), now())
        .expect("first");
    // A reader holds the file the way the daemon reads it, for longer than a read.
    let reader = fs::File::open(&file).expect("reader");
    let released = thread::spawn(move || {
        thread::sleep(Duration::from_millis(150));
        drop(reader);
    });
    registry
        .register(&scratch.instance("second"), now())
        .expect("second");
    released.join().expect("released");
    assert_eq!(registry.read().expect("readable").len(), 2);
}

#[test]
fn journal_shared_by_four_writers_keeps_every_line_across_two_rotations() {
    let scratch = Scratch::new("journal");
    let file = home_paths(&scratch.0).log_file();
    const WRITERS: usize = 4;
    const LINES: usize = 600;
    // About 2.6 files of 100 KiB: two rotations, and everything fits the three files kept.
    const MAX_BYTES: u64 = 100 * 1024;
    thread::scope(|scope| {
        for writer in 0..WRITERS {
            let file = &file;
            scope.spawn(move || {
                // One journal object per writer, as each process has its own.
                let log = DaemonLog::with_clock(
                    file,
                    Box::new(sermofur_core::format::DateTimeOffset::now_utc),
                    MAX_BYTES,
                );
                for line in 0..LINES {
                    log.write(
                        &format!("w{writer}-{line}"),
                        Some("padding_to_make_the_line_longer_than_usual"),
                        None,
                        None,
                    );
                }
                assert!(log.flush(Duration::from_secs(30)));
            });
        }
    });
    let mut events = Vec::new();
    for suffix in ["", ".1", ".2"] {
        let part = PathBuf::from(format!("{}{suffix}", file.display()));
        if part.is_file() {
            assert!(fs::metadata(&part).expect("size").len() <= MAX_BYTES + 512);
            for line in fs::read_to_string(&part).expect("journal").lines() {
                let value: serde_json::Value = serde_json::from_str(line).expect("a JSON line");
                events.push(value["event"].as_str().expect("an event").to_owned());
            }
        }
    }
    assert!(
        PathBuf::from(format!("{}.2", file.display())).is_file(),
        "the journal did not rotate twice"
    );
    assert_eq!(events.len(), WRITERS * LINES);
    events.sort();
    events.dedup();
    assert_eq!(events.len(), WRITERS * LINES);
}

#[test]
fn journal_never_holds_the_caller_while_another_process_has_its_turn() {
    let scratch = Scratch::new("turn");
    let file = home_paths(&scratch.0).log_file();
    fs::create_dir_all(file.parent().expect("state")).expect("state");
    let log = DaemonLog::new(&file);
    let started = Instant::now();
    {
        let _turn = take(&PathBuf::from(format!("{}.lock", file.display()))).expect("the turn");
        for line in 0..100 {
            log.write(&format!("held-{line}"), None, None, None);
        }
        // Waiting for the turn would cost a whole second from the first line.
        let queued = started.elapsed();
        assert!(queued < Duration::from_millis(700), "{queued:?}");
        // Held a while, but given back well within the second the writer waits.
        thread::sleep(Duration::from_millis(400).saturating_sub(started.elapsed()));
    }
    assert!(log.flush(Duration::from_secs(10)));
    assert_eq!(
        fs::read_to_string(&file).expect("journal").lines().count(),
        100
    );
}

/// The `smf` of the oracle, when `SMF_ORACLE` names it.
fn oracle() -> Option<String> {
    let oracle = std::env::var("SMF_ORACLE").ok();
    if oracle.is_none() {
        eprintln!("cross-process case skipped: SMF_ORACLE is not set");
    }
    oracle
}

/// Kills a daemon of the oracle when the test ends, whatever happens.
struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn run_oracle(oracle: &str, home: &Path, args: &[&str]) -> (i32, String) {
    let output = Command::new(oracle)
        .args(args)
        .env(HOME_VARIABLE, home)
        .env("SERMOFUR_NO_DAEMON", "1")
        .output()
        .expect("the oracle runs");
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr),
    )
}

#[test]
fn the_lock_of_a_daemon_excludes_the_other_tool() {
    let Some(oracle) = oracle() else { return };
    let scratch = Scratch::new("cross-lock");
    let home = scratch.0.join("home");
    let paths = home_paths(&home);
    // A daemon of the oracle holds the lock: the port sees it and is refused.
    let mut daemon = Daemon(
        Command::new(&oracle)
            .args(["daemon", "run"])
            .env(HOME_VARIABLE, &home)
            .spawn()
            .expect("the daemon starts"),
    );
    let started = Instant::now();
    while !is_held(&paths.lock_file()) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the daemon took no lock"
        );
        thread::sleep(Duration::from_millis(50));
    }
    let refused = acquire(&paths).expect_err("the lock is held by the oracle");
    assert_eq!(
        (refused.code.as_str(), refused.exit_code),
        ("daemon_already_running", 3)
    );
    let _ = daemon.0.kill();
    let _ = daemon.0.wait();
    drop(daemon);
    assert!(wait_for_release(
        &paths.lock_file(),
        Duration::from_secs(10)
    ));
    // The port holds the lock: a daemon of the oracle is refused.
    let held = acquire(&paths).expect("the lock is free");
    let (code, output) = run_oracle(&oracle, &home, &["daemon", "run"]);
    assert_eq!(code, 3, "{output}");
    assert!(
        output.contains("daemon_already_running: A Sermofur daemon already serves this user."),
        "{output}"
    );
    drop(held);
}

#[test]
fn each_tool_reads_and_changes_the_registry_of_the_other() {
    let Some(oracle) = oracle() else { return };
    let scratch = Scratch::new("cross-registry");
    let home = scratch.0.join("home");
    let first = scratch.instance("first");
    let second = scratch.instance("second");
    let (code, output) = run_oracle(
        &oracle,
        &home,
        &["daemon", "register", "--path", &first.to_string_lossy()],
    );
    assert_eq!(code, 0, "{output}");
    let registry = InstanceRegistry::new(&home_paths(&home).registry_file());
    assert_eq!(roots(&registry), [first.to_string_lossy().into_owned()]);
    registry
        .register(&second, now())
        .expect("registered by the port");
    let (code, output) = run_oracle(&oracle, &home, &["daemon", "instances"]);
    assert_eq!(code, 0, "{output}");
    let listed: serde_json::Value = serde_json::from_str(&output).expect("a JSON list");
    let listed: Vec<&str> = listed
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["root"].as_str().expect("a root"))
        .collect();
    assert_eq!(listed, [first.to_string_lossy(), second.to_string_lossy()]);
    let (code, output) = run_oracle(
        &oracle,
        &home,
        &["daemon", "unregister", "--path", &second.to_string_lossy()],
    );
    assert_eq!(code, 0, "{output}");
    registry
        .unregister(&first)
        .expect("unregistered by the port");
    assert!(registry.read().expect("readable").is_empty());
}
