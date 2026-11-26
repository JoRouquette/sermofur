//! `discover` and `discover_for_diagnosis` against the tables that the .NET code produced
//! (`rust/tests/golden/discovery.json` on Windows, `discovery-unix.json` on Linux), regenerated
//! by `rust/tools/discovery-oracle`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};
use sermofur_core::discovery::{discover, discover_for_diagnosis};
use sermofur_core::error::SermofurError;
use sermofur_core::ownership::{EntryStatus, FileOwnership, SystemOwnership};
use uuid::Uuid;

const VALID: &str = r#"{"schemaVersion":2,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-01-02T03:04:05+00:00"}"#;

/// Ownership of a test, as `FakeOwnership` of the oracle.
enum Fake {
    Foreign,
    Throwing,
    Absent,
}

impl FileOwnership for Fake {
    fn inspect(&self, _path: &Path) -> io::Result<Option<EntryStatus>> {
        match self {
            Fake::Throwing => Err(io::Error::other("owner unreadable")),
            Fake::Absent => Ok(None),
            Fake::Foreign => Ok(Some(EntryStatus {
                is_regular_file: false,
                is_directory: true,
                is_owned_by_current_user: false,
            })),
        }
    }
}

/// The temporary directory, as the oracle sees it; on macOS `/var` is a link, so it is canonicalized everywhere but
/// on Windows (where `canonicalize` adds the `\\?\` prefix).
fn temp_base() -> &'static Path {
    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        let dir = std::env::temp_dir();
        if cfg!(windows) {
            // `Path.GetTempPath` of the oracle gives the long form: 8.3 names expanded.
            sermofur_core::discovery::expand_short_names(&dir)
        } else {
            fs::canonicalize(dir).expect("the temporary directory resolves")
        }
    })
}

/// A scratch folder; drop undoes the permission changes, removes links, then the folder.
struct Scratch {
    root: PathBuf,
    restore: Vec<PathBuf>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        for folder in &self.restore {
            restore(folder);
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn marker(root: &Path, entries: &[&str]) {
    let marker = root.join(".sermofur");
    fs::create_dir_all(&marker).expect("marker");
    for entry in entries {
        if let Some(folder) = entry.strip_suffix('/') {
            fs::create_dir(marker.join(folder)).expect("folder");
        } else {
            let text = if *entry == "instance.json" {
                VALID
            } else {
                "x"
            };
            fs::write(marker.join(entry), text).expect("file");
        }
    }
}

fn instance_in(root: &Path, relative: &str) {
    fs::create_dir_all(root.join(relative)).expect("folder");
    marker(&root.join(relative), &["instance.json"]);
}

/// A junction on Windows (`mklink /J`, no privilege needed), a symbolic link elsewhere.
fn directory_link(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("cmd runs");
        assert!(out.status.success(), "mklink /J failed: {out:?}");
    }
    #[cfg(not(windows))]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[cfg(windows)]
fn icacls(args: &[&str]) {
    let out = Command::new("icacls").args(args).output().expect("icacls");
    assert!(out.status.success(), "icacls {args:?} failed: {out:?}");
}

/// The listing of the folder is denied: to the user on Windows, every right on Unix.
fn unlistable(folder: &Path) {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").expect("USERNAME");
        icacls(&[
            folder.to_str().expect("UTF-8"),
            "/deny",
            &format!("{user}:(RD)"),
        ]);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(folder, fs::Permissions::from_mode(0o000)).expect("mode");
    }
}

/// Unix: the folder can be listed but not searched.
#[cfg(unix)]
fn unsearchable(folder: &Path) {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(folder, fs::Permissions::from_mode(0o600)).expect("mode");
}

fn restore(folder: &Path) {
    #[cfg(windows)]
    {
        let user = std::env::var("USERNAME").unwrap_or_default();
        let _ = Command::new("icacls")
            .args([folder.to_str().unwrap_or_default(), "/remove:d", &user])
            .output();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(folder, fs::Permissions::from_mode(0o700));
    }
}

/// Prepares the scenario like the oracle does; returns the folders to restore.
fn prepare(name: &str, r: &Path) -> Vec<PathBuf> {
    let mkdir = |relative: &str| fs::create_dir_all(r.join(relative)).expect("folder");
    match name {
        "instance_at_start" | "foreign_owner" | "owner_unreadable" | "owner_unknown"
        | "nt_path_instance" => {
            marker(r, &["instance.json"]);
        }
        "instance_above" => {
            marker(r, &["instance.json"]);
            mkdir("a/b");
        }
        "start_with_dot_dot" | "start_with_trailing_separator" => {
            marker(r, &["instance.json"]);
            mkdir("a");
        }
        "no_instance" | "start_missing" | "unc_path" | "device_path" | "nt_path_missing" => {}
        "start_is_file" => fs::write(r.join("file"), "x").expect("file"),
        "marker_is_file" => fs::write(r.join(".sermofur"), "x").expect("file"),
        "marker_empty" => marker(r, &[]),
        "marker_unrelated_entry" => marker(r, &["notes.txt"]),
        "damaged_database_only" => marker(r, &["memory.db"]),
        "damaged_records_only" => marker(r, &["records/"]),
        "damaged_backups_only" => marker(r, &["backups/"]),
        "configuration_upper_case" => marker(r, &["INSTANCE.JSON"]),
        "configuration_dotless_i" => marker(r, &["\u{131}nstance.json"]),
        "backups_long_s" => marker(r, &["backup\u{17F}/"]),
        "configuration_is_folder" => marker(r, &["instance.json/"]),
        "nearest_marker_wins" => {
            marker(r, &["instance.json"]);
            mkdir("a");
            marker(&r.join("a"), &[]);
        }
        "marker_link" => {
            instance_in(r, "real");
            directory_link(&r.join(".sermofur"), &r.join("real").join(".sermofur"));
        }
        "start_through_link" => {
            instance_in(r, "real");
            directory_link(&r.join("link"), &r.join("real"));
        }
        "marker_unlistable" => {
            marker(r, &["instance.json"]);
            unlistable(&r.join(".sermofur"));
            return vec![r.join(".sermofur")];
        }
        #[cfg(unix)]
        "marker_attributes_unreadable" => {
            instance_in(r, "locked");
            unsearchable(&r.join("locked"));
            return vec![r.join("locked")];
        }
        other => panic!("unknown scenario {other}"),
    }
    Vec::new()
}

fn shown(root: &Path, result: Result<PathBuf, SermofurError>) -> Value {
    let root = root.to_str().expect("UTF-8 path");
    let show = |text: &str| text.replace(root, "<root>").replace('\\', "/");
    match result {
        Ok(path) => json!({ "root": show(path.to_str().expect("UTF-8 path")) }),
        Err(error) => json!({
            "code": error.code,
            "message": show(&error.message),
            "exitCode": error.exit_code,
        }),
    }
}

/// Whether the tests run as root, read from the owner of a fresh file.
#[cfg(unix)]
fn runs_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    let probe = temp_base().join(format!("sermofur-uid-{}", Uuid::new_v4()));
    fs::write(&probe, "").expect("probe");
    let uid = fs::metadata(&probe).expect("probe").uid();
    let _ = fs::remove_file(&probe);
    uid == 0
}

#[test]
fn discovery_matches_the_oracle() {
    let (file, os) = if cfg!(windows) {
        ("discovery.json", "windows")
    } else {
        ("discovery-unix.json", "unix")
    };
    let path = format!("{}/../../tests/golden/{file}", env!("CARGO_MANIFEST_DIR"));
    let golden: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("golden file")).expect("JSON");
    assert_eq!(golden["os"], os);
    // Permissions do not hold for root, for whom the unreadable cases do not apply.
    #[cfg(unix)]
    let root_user = runs_as_root();
    #[cfg(windows)]
    let root_user = false;
    let base = temp_base().join(format!("sermofur-discovery-{}", Uuid::new_v4()));
    let mut mismatches = Vec::new();
    let scenarios = golden["scenarios"].as_array().expect("scenarios");
    for row in scenarios {
        let name = row["name"].as_str().expect("name");
        if root_user && (name == "marker_unlistable" || name == "marker_attributes_unreadable") {
            continue;
        }
        // The search walks up to the volume root: a marker above the temporary folder (an
        // instance in the home folder, say) changes the answer, so the scenario is skipped.
        if name == "no_instance"
            && let Some(above) = base
                .ancestors()
                .map(|folder| folder.join(".sermofur"))
                .find(|marker| fs::symlink_metadata(marker).is_ok())
        {
            eprintln!("no_instance skipped: {} exists", above.display());
            continue;
        }
        let root = base.join(name);
        fs::create_dir_all(&root).expect("root");
        let restore = prepare(name, &root);
        let scratch = Scratch {
            root: root.clone(),
            restore,
        };
        let start = row["start"].as_str().expect("start");
        let path = if start.starts_with('\\') {
            PathBuf::from(start.replace("<root>", root.to_str().expect("UTF-8")))
        } else if start.is_empty() {
            root.clone()
        } else {
            PathBuf::from(format!(
                "{}{}{}",
                root.to_str().expect("UTF-8"),
                std::path::MAIN_SEPARATOR,
                start.replace('/', std::path::MAIN_SEPARATOR_STR)
            ))
        };
        let owners: Box<dyn FileOwnership> = match row["owners"].as_str() {
            Some("foreign") => Box::new(Fake::Foreign),
            Some("throwing") => Box::new(Fake::Throwing),
            Some("absent") => Box::new(Fake::Absent),
            _ => Box::new(SystemOwnership),
        };
        let actual = json!({
            "name": name,
            "start": start,
            "owners": row["owners"],
            "discover": shown(&root, discover(&path, owners.as_ref())),
            "diagnosis": shown(&root, discover_for_diagnosis(&path, owners.as_ref())),
        });
        // A .NET exception that is not a Sermofur refusal: the CLI maps ArgumentException to
        // invalid_storage_or_path, exit 3 (CommandRunner.Run), which the port returns directly.
        let mut expected = row.clone();
        for call in ["discover", "diagnosis"] {
            if expected[call]["exception"] == "System.ArgumentException" {
                expected[call] = json!({
                    "code": "invalid_storage_or_path",
                    "message": "Invalid path or data; run doctor.",
                    "exitCode": 3,
                });
            }
        }
        if actual != expected {
            mismatches.push(format!("{name}:\n  expected {row}\n  got      {actual}"));
        }
        drop(scratch);
    }
    let _ = fs::remove_dir_all(&base);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
