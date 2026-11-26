//! Lot 7.6: ports of the six `RegistryTests` cases that are not about the gate (those are in
//! `serving_gate_tests.rs`). The registry file lives in a fresh folder under the temporary
//! directory, as the `Home` of .NET.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use sermofur_core::error::SermofurError;
use sermofur_core::format::DateTimeOffset;
use sermofur_core::ownership::{EntryStatus, FileOwnership};
use sermofur_daemon::registry::{DEFAULT_LOCK_TIMEOUT, InstanceRegistry, MAX_BYTES};

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
            "smfy-{name}-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder).expect("the scratch folder");
        Self(folder)
    }

    /// `TestInstance`.
    fn instance(&self, name: &str) -> PathBuf {
        let root = self.0.join(name);
        fs::create_dir_all(&root).expect("root");
        sermofur_core::init::initialize(&root).expect("an instance");
        root
    }

    /// `Home.Registry`.
    fn file(&self) -> PathBuf {
        self.0.join("home").join("config").join("instances.json")
    }

    fn registry(&self) -> InstanceRegistry {
        InstanceRegistry::new(&self.file())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn now() -> DateTimeOffset {
    DateTimeOffset::now_local()
}

fn code(error: SermofurError) -> (String, i32) {
    (error.code, error.exit_code)
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `ForeignOwnership`: every folder and file belongs to another account.
struct Foreign;

impl FileOwnership for Foreign {
    fn inspect(&self, path: &Path) -> io::Result<Option<EntryStatus>> {
        Ok(if path.is_dir() {
            Some(EntryStatus {
                is_regular_file: false,
                is_directory: true,
                is_owned_by_current_user: false,
            })
        } else if path.is_file() {
            Some(EntryStatus {
                is_regular_file: true,
                is_directory: false,
                is_owned_by_current_user: false,
            })
        } else {
            None
        })
    }
}

/// Port of `RegisteringIsIdempotentAndListed`.
#[test]
fn registering_is_idempotent_and_listed() {
    let scratch = Scratch::new("idempotent");
    let root = scratch.instance("instance");
    let registry = scratch.registry();
    let first = registry.register(&root, now()).expect("registered");
    let deeper = root.join("deep").join("er");
    fs::create_dir_all(&deeper).expect("a deeper folder");
    let later = DateTimeOffset::new(now().utc_ticks() + 36_000_000_000, now().offset_minutes())
        .expect("an hour later");
    let again = registry.register(&deeper, later).expect("registered again");
    assert_eq!(first, again);
    let listed = registry.list().expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].root.clone(), listed[0].missing),
        (text(&root), false)
    );
}

/// Port of `RefusalsAreThoseOfTheDirectCli`.
#[test]
fn refusals_are_those_of_the_direct_cli() {
    let scratch = Scratch::new("refusals");
    let legacy = scratch.0.join("legacy");
    copy_folder(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/Sermofur.Tests/Fixtures/instance-v1/marker"),
        &legacy.join(".sermofur"),
    );
    let registry = scratch.registry();
    assert_eq!(
        registry.register(&legacy, now()).expect_err("refused").code,
        "migration_required"
    );
    let root = scratch.instance("instance");
    let foreign = InstanceRegistry::with(&scratch.file(), Box::new(Foreign), DEFAULT_LOCK_TIMEOUT);
    assert_eq!(
        code(foreign.register(&root, now()).expect_err("refused")),
        ("foreign_owner".to_owned(), 4)
    );
    let outside = scratch.0.join("outside");
    fs::create_dir_all(&outside).expect("a folder outside any instance");
    assert_eq!(
        registry
            .register(&outside, now())
            .expect_err("refused")
            .code,
        "no_instance"
    );
    assert!(!scratch.file().exists());
}

fn copy_folder(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("folder");
    for entry in fs::read_dir(source).expect("listing") {
        let path = entry.expect("entry").path();
        let to = target.join(path.file_name().expect("name"));
        if path.is_dir() {
            copy_folder(&path, &to);
        } else {
            fs::copy(&path, &to).expect("copy");
        }
    }
}

/// Port of `UnusableRegistryIsReportedAndLeftUntouched` (four cases).
#[test]
fn an_unusable_registry_is_reported_and_left_untouched() {
    for content in [
        "not json",
        "{\"version\":2,\"instances\":[]}",
        "{\"version\":1}",
        "{\"version\":1,\"instances\":[{\"root\":\"relative\",\"registeredAt\":\"x\"}]}",
    ] {
        let scratch = Scratch::new("unusable");
        let root = scratch.instance("instance");
        fs::create_dir_all(scratch.file().parent().expect("a folder")).expect("the folder");
        fs::write(scratch.file(), content).expect("written");
        let registry = scratch.registry();
        assert_eq!(
            code(registry.read().expect_err("refused")),
            ("invalid_registry".to_owned(), 3),
            "{content}"
        );
        assert!(registry.register(&root, now()).is_err(), "{content}");
        assert_eq!(fs::read_to_string(scratch.file()).expect("kept"), content);
    }
}

/// Port of `OversizedRegistryIsRefused`.
#[test]
fn an_oversized_registry_is_refused() {
    let scratch = Scratch::new("oversized");
    fs::create_dir_all(scratch.file().parent().expect("a folder")).expect("the folder");
    let size = usize::try_from(MAX_BYTES).expect("a size") + 1;
    fs::write(scratch.file(), " ".repeat(size)).expect("written");
    assert_eq!(
        scratch.registry().read().expect_err("refused").code,
        "invalid_registry"
    );
}

/// Port of `MovedInstanceIsMissingAndRemovableByItsRegisteredPath`.
#[test]
fn a_moved_instance_is_missing_and_removable_by_its_registered_path() {
    let scratch = Scratch::new("moved");
    let registry = scratch.registry();
    let instance = scratch.instance("instance");
    let root = registry
        .register(&instance, now())
        .expect("registered")
        .root
        .expect("a root");
    fs::remove_dir_all(&instance).expect("the instance is gone");
    assert!(registry.list().expect("a list")[0].missing);
    assert_eq!(
        registry.unregister(Path::new(&root)).expect("removed").root,
        Some(root.clone())
    );
    assert!(registry.read().expect("read").is_empty());
    assert_eq!(
        code(registry.unregister(Path::new(&root)).expect_err("refused")),
        ("not_registered".to_owned(), 1)
    );
}

/// Port of `UnregisteringFromInsideTheInstanceFindsIt`.
#[test]
fn unregistering_from_inside_the_instance_finds_it() {
    let scratch = Scratch::new("inside");
    let root = scratch.instance("instance");
    let registry = scratch.registry();
    registry.register(&root, now()).expect("registered");
    let sub = root.join("sub");
    fs::create_dir_all(&sub).expect("a subfolder");
    registry.unregister(&sub).expect("removed");
    assert!(registry.read().expect("read").is_empty());
}
