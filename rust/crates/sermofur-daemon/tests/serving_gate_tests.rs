//! Ports of the five `ServingGate` cases left by lot 7.2: `GateServesOnlyRegisteredRoots...`,
//! `GateRefusesANestedInstance...`, `GateClosesIdleAndUnregisteredInstances` (`RegistryTests`),
//! `TwoRewritesWithTheSameDateAndSizeAreBothSeen` and
//! `RegistryDatedInTheFutureIsReadOnceThenByItsStamp` (`SharedFilesTests`).

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::{EntryStatus, FileOwnership, SystemOwnership};
use sermofur_daemon::registry::InstanceRegistry;
use sermofur_daemon::server::ServingGate;

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
            "smfg-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("the scratch folder");
        Self(folder)
    }

    fn instance(&self, name: &str) -> String {
        let root = self.0.join(name);
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        root.to_string_lossy().into_owned()
    }

    fn registry(&self) -> InstanceRegistry {
        InstanceRegistry::new(&self.0.join("home").join("config").join("instances.json"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn now() -> DateTimeOffset {
    DateTimeOffset::now_utc()
}

fn register(registry: &InstanceRegistry, root: &str) {
    registry
        .register(Path::new(root), now())
        .expect("registered");
}

fn unregister(registry: &InstanceRegistry, root: &str) {
    registry.unregister(Path::new(root)).expect("unregistered");
}

fn set_written(file: &Path, at: SystemTime) {
    File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_modified(at))
        .expect("date set");
}

fn written(file: &Path) -> SystemTime {
    fs::metadata(file)
        .and_then(|m| m.modified())
        .expect("a date")
}

/// Fails the test if the gate looks at the file system.
struct NoReadOwnership;

impl FileOwnership for NoReadOwnership {
    fn inspect(&self, path: &Path) -> io::Result<Option<EntryStatus>> {
        panic!("the gate read {}", path.display());
    }
}

#[test]
fn gate_serves_only_registered_roots_without_reading_outside() {
    let scratch = Scratch::new("outside");
    let root = scratch.instance("instance");
    let registry = scratch.registry();
    register(&registry, &root);
    let gate = ServingGate::with(
        registry,
        Box::new(NoReadOwnership),
        Duration::from_secs(600),
        Box::new(SystemTime::now),
        Box::new(|_| {}),
    );
    let outside = scratch.0.join("smf-outside").to_string_lossy().into_owned();
    assert_eq!(gate.resolve(&outside), None);
    assert_eq!(gate.resolve(&format!("{root}-sibling")), None);
    assert_eq!(gate.open_count(), 0);
}

#[test]
fn gate_refuses_a_nested_instance_that_is_not_registered() {
    let scratch = Scratch::new("nested");
    let root = scratch.instance("instance");
    let registry = scratch.registry();
    register(&registry, &root);
    let nested = Path::new(&root).join("nested");
    // Init refuses nesting: build the entry by hand, as a copied folder would.
    fs::create_dir_all(nested.join(".sermofur")).expect("marker");
    fs::copy(
        Path::new(&root).join(".sermofur").join("instance.json"),
        nested.join(".sermofur").join("instance.json"),
    )
    .expect("copied");
    let refusals = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&refusals);
    let gate = ServingGate::new(registry).refusing(Box::new(move |code| {
        seen.lock().expect("lock").push(code.to_owned());
    }));
    assert_eq!(gate.resolve(&root).as_deref(), Some(root.as_str()));
    assert_eq!(gate.resolve(&nested.to_string_lossy()), None);
    assert_eq!(
        *refusals.lock().expect("lock"),
        ["nested_instance_not_registered"]
    );
}

#[test]
fn gate_closes_idle_and_unregistered_instances() {
    let scratch = Scratch::new("idle");
    let root = scratch.instance("instance");
    let registry = scratch.registry();
    let file = registry.file().to_path_buf();
    register(&registry, &root);
    let clock = Arc::new(Mutex::new(SystemTime::now()));
    let read = Arc::clone(&clock);
    let gate = ServingGate::with(
        scratch.registry(),
        Box::new(SystemOwnership),
        Duration::from_secs(600),
        Box::new(move || *read.lock().expect("lock")),
        Box::new(|_| {}),
    );
    assert_eq!(gate.resolve(&root).as_deref(), Some(root.as_str()));
    assert_eq!(gate.open_count(), 1);
    *clock.lock().expect("lock") += Duration::from_secs(11 * 60);
    assert_eq!(gate.open_count(), 0);
    assert_eq!(gate.resolve(&root).as_deref(), Some(root.as_str()));
    unregister(&registry, &root);
    set_written(&file, SystemTime::now() + Duration::from_secs(5));
    assert_eq!(gate.resolve(&root), None);
    assert_eq!(gate.open_count(), 0);
}

#[test]
fn two_rewrites_with_the_same_date_and_size_are_both_seen() {
    let scratch = Scratch::new("rewrites");
    let first = scratch.instance("first");
    let second = scratch.instance("secnd");
    let registry = scratch.registry();
    register(&registry, &first);
    let gate = ServingGate::new(scratch.registry());
    assert_eq!(gate.resolve(&first).as_deref(), Some(first.as_str()));
    let before = registry.stamp();
    // Same length of root, same date written back: only the content changed.
    unregister(&registry, &first);
    register(&registry, &second);
    set_written(registry.file(), before.expect("a stamp").0);
    assert_eq!(registry.stamp(), before);
    assert_eq!(gate.resolve(&first), None);
    assert_eq!(gate.resolve(&second).as_deref(), Some(second.as_str()));
}

#[test]
fn registry_dated_in_the_future_is_read_once_then_by_its_stamp() {
    let scratch = Scratch::new("future");
    let first = scratch.instance("first");
    let second = scratch.instance("secnd");
    let registry = scratch.registry();
    register(&registry, &first);
    let future = SystemTime::now() + Duration::from_secs(3600);
    set_written(registry.file(), future);
    let gate = ServingGate::new(scratch.registry());
    assert_eq!(gate.resolve(&first).as_deref(), Some(first.as_str()));
    // Rewritten with the same date and size: a future date is not "recent", so only a change
    // of stamp makes the gate read it again.
    unregister(&registry, &first);
    register(&registry, &second);
    set_written(registry.file(), future);
    assert_eq!(written(registry.file()), future);
    assert_eq!(gate.resolve(&first).as_deref(), Some(first.as_str()));
    set_written(registry.file(), future + Duration::from_secs(1));
    assert_eq!(gate.resolve(&second).as_deref(), Some(second.as_str()));
}
