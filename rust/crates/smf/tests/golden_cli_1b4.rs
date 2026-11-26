//! Differential test of lot 1b.4: `smf init` and `smf root`.
//!
//! The expected outputs are golden files recorded from the .NET `smf` (the oracle). Every run
//! happens in a fresh folder under the temporary directory; the root, the instance identity and
//! the creation date are normalized, and the entries left in the folder are recorded too. To
//! record them again, publish the .NET tool (`dotnet publish src/Sermofur.Cli -o
//! rust/target/oracle`) and run
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test -p smf --test golden_cli_1b4
//! ```
//!
//! With `SMF_ORACLE` set, the cross-read tests at the end also run: each tool reads the
//! instance that the other one created.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};
use uuid::Uuid;

/// What a process printed and returned, with line endings normalized to `\n`.
struct Captured {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn run(program: &str, arguments: &[String], cwd: &Path) -> Captured {
    let output = Command::new(program)
        .args(arguments)
        .env("SERMOFUR_NO_DAEMON", "1")
        .current_dir(cwd)
        .output()
        .expect("the program starts");
    let text = |bytes: &[u8]| {
        String::from_utf8(bytes.to_vec())
            .expect("the output is UTF-8")
            .replace("\r\n", "\n")
    };
    Captured {
        exit_code: output.status.code().expect("the process exits normally"),
        stdout: text(&output.stdout),
        stderr: text(&output.stderr),
    }
}

/// The temporary directory; on macOS `/var` is a link, so it is canonicalized everywhere but
/// on Windows (where `canonicalize` adds the `\\?\` prefix).
fn temp_base() -> &'static Path {
    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        let dir = std::env::temp_dir();
        if cfg!(windows) {
            dir
        } else {
            fs::canonicalize(dir).expect("the temporary directory resolves")
        }
    })
}

/// A fresh folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = temp_base().join(format!("sermofur-1b4-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&root).expect("scratch");
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const LEGACY: &str = r#"{"schemaVersion":1,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-01-02T03:04:05+00:00"}"#;
const VALID: &str = r#"{"schemaVersion":2,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-01-02T03:04:05+00:00"}"#;

/// A scenario: how to prepare the folder (with the program under test), the arguments where
/// `<root>` stands for the folder, and the working directory relative to it.
struct Scenario {
    name: &'static str,
    prepare: fn(&str, &Path),
    arguments: &'static [&'static str],
    cwd: &'static str,
}

fn nothing(_: &str, _: &Path) {}

fn init_there(program: &str, root: &Path) {
    let out = run(program, &["init".to_string()], root);
    assert_eq!(out.exit_code, 0, "init failed: {}", out.stderr);
}

fn scenarios() -> Vec<Scenario> {
    let s = |name, prepare, arguments, cwd| Scenario {
        name,
        prepare,
        arguments,
        cwd,
    };
    vec![
        s(
            "init_creates_json",
            nothing,
            &["init", "--json", "--path", "<root>"],
            "",
        ),
        s("init_creates_text", nothing, &["init"], ""),
        s("init_existing", init_there, &["init", "--json"], ""),
        s(
            "init_nested",
            |p, r| {
                init_there(p, r);
                fs::create_dir(r.join("sub")).expect("sub");
            },
            &["init", "--json", "--path", "sub"],
            "",
        ),
        s(
            "init_relative_path",
            |_, r| fs::create_dir(r.join("sub")).expect("sub"),
            &["init", "--path", "sub"],
            "",
        ),
        s(
            "init_missing_folder",
            nothing,
            &["init", "--path", "missing"],
            "",
        ),
        s("init_empty_path", nothing, &["init", "--path", ""], ""),
        s("init_extra_positional", nothing, &["init", "x"], ""),
        s(
            "init_unknown_option",
            nothing,
            &["init", "--actor", "a"],
            "",
        ),
        s(
            "init_foreign_marker",
            |_, r| fs::write(r.join(".sermofur"), "x").expect("file"),
            &["init"],
            "",
        ),
        s(
            "init_missing_database",
            |_, r| {
                fs::create_dir(r.join(".sermofur")).expect("marker");
                fs::write(r.join(".sermofur/instance.json"), VALID).expect("config");
            },
            &["init"],
            "",
        ),
        s(
            "init_format_1",
            |_, r| {
                fs::create_dir(r.join(".sermofur")).expect("marker");
                fs::write(r.join(".sermofur/instance.json"), LEGACY).expect("config");
            },
            &["init"],
            "",
        ),
        s(
            "init_leftover_staging",
            |_, r| fs::create_dir(r.join(".sermofur-init-0123")).expect("staging"),
            &["init", "--json"],
            "",
        ),
        s(
            "root_text_from_below",
            |p, r| {
                init_there(p, r);
                fs::create_dir_all(r.join("a/b")).expect("a/b");
            },
            &["root"],
            "a/b",
        ),
        s("root_json", init_there, &["root", "--json"], ""),
        s("root_no_instance", nothing, &["root"], ""),
        s(
            "root_extra_positional_no_instance",
            nothing,
            &["root", "x"],
            "",
        ),
        s("root_extra_positional", init_there, &["root", "x"], ""),
        s(
            "root_unknown_option",
            init_there,
            &["root", "--actor", "a"],
            "",
        ),
        s(
            "root_format_1",
            |_, r| {
                fs::create_dir(r.join(".sermofur")).expect("marker");
                fs::write(r.join(".sermofur/instance.json"), LEGACY).expect("config");
            },
            &["root", "--json"],
            "",
        ),
    ]
}

/// The entries under `root`, sorted, with `/` separators and `/` after a folder.
fn entries(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).expect("listing") {
            let path = entry.expect("entry").path();
            let relative = path
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                found.push(format!("{relative}/"));
                pending.push(path);
            } else {
                found.push(relative);
            }
        }
    }
    found.sort();
    found
}

/// The folder with every short 8.3 name expanded, without the `\\?\` prefix of `canonicalize`.
fn long_form(root: &Path) -> String {
    let canonical = fs::canonicalize(root).expect("the folder resolves");
    let text = canonical.to_str().expect("UTF-8 path");
    text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
}

/// Replaces the folder, the instance identity and the creation date by placeholders. The
/// replaced values are checked first: a GUID in format D and a date in UTC, its `+` written as
/// it is (the .NET writer of dates does not escape it).
fn normalize(text: &str, root: &Path) -> String {
    // The tools print the long form of the folder (8.3 names expanded on Windows); a short form
    // left in the output is a defect, so only the long one is replaced.
    let long = long_form(root);
    let mut out = text
        .replace(&long.replace('\\', "\\\\"), "<root>")
        .replace(&long, "<root>");
    // The separator after the folder, raw or JSON-escaped, so that one file serves every system.
    out = out
        .replace("<root>\\\\", "<root>/")
        .replace("<root>\\", "<root>/");
    for (key, placeholder) in [
        ("\"instanceId\": \"", "<guid>"),
        ("\"createdAt\": \"", "<date>"),
    ] {
        let mut from = 0;
        while let Some(found) = out[from..].find(key) {
            let start = from + found + key.len();
            let end = start + out[start..].find('"').expect("closing quote");
            let value = &out[start..end];
            if placeholder == "<guid>" {
                assert!(
                    Uuid::try_parse(value).is_ok() && value.len() == 36,
                    "GUID {value}"
                );
            } else {
                assert!(
                    value.ends_with("+00:00")
                        && sermofur_core::format::parse_json_text(value, |_| 0).is_some(),
                    "date {value}"
                );
            }
            out.replace_range(start..end, placeholder);
            from = start + placeholder.len();
        }
    }
    out
}

fn record(program: &str, scenario: &Scenario) -> Value {
    let scratch = Scratch::new();
    let root = &scratch.0;
    (scenario.prepare)(program, root);
    let root_text = root.to_str().expect("UTF-8 path");
    let arguments: Vec<String> = scenario
        .arguments
        .iter()
        .map(|a| a.replace("<root>", root_text))
        .collect();
    let out = run(program, &arguments, &root.join(scenario.cwd));
    // The bytes of the configuration file each tool leaves, line breaks and values normalized.
    let configuration = fs::read_to_string(root.join(".sermofur").join("instance.json"))
        .map(|text| normalize(&text.replace("\r\n", "\n"), root))
        .ok();
    json!({
        "exitCode": out.exit_code,
        "stdout": normalize(&out.stdout, root),
        "stderr": normalize(&out.stderr, root),
        "instanceJson": configuration,
        "entries": entries(root),
    })
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/cli-1b4")
        .join(format!("{name}.json"))
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    let update = std::env::var("SMF_UPDATE_GOLDEN").is_ok_and(|value| value == "1");
    let oracle = std::env::var("SMF_ORACLE").ok();
    let rust = env!("CARGO_BIN_EXE_smf");
    let mut failures = Vec::new();
    for scenario in scenarios() {
        let path = golden_path(scenario.name);
        if update {
            let oracle = oracle
                .as_deref()
                .expect("SMF_ORACLE names the oracle program");
            let body = record(oracle, &scenario);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
            fs::write(
                &path,
                serde_json::to_string_pretty(&body).expect("JSON") + "\n",
            )
            .expect("the golden file is written");
        }
        let golden: Value = serde_json::from_str(
            &fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("golden file missing: {}", path.display())),
        )
        .expect("valid JSON");
        let actual = record(rust, &scenario);
        if actual != golden {
            failures.push(format!(
                "{}:\n  expected {golden}\n  got      {actual}",
                scenario.name
            ));
        }
    }
    assert!(failures.is_empty(), "differences:\n{}", failures.join("\n"));
}

/// The instance identity printed by `init --json`.
fn instance_id(out: &Captured) -> String {
    let value: Value = serde_json::from_str(&out.stdout).expect("JSON");
    value["configuration"]["instanceId"]
        .as_str()
        .expect("instanceId")
        .to_owned()
}

/// Cross-read: an instance created by one tool is found, read and validated by the other one
/// (`init` on an existing instance reads `instance.json` and checks the database identity).
#[test]
fn each_tool_reads_the_instance_of_the_other() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("cross-read skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    let init = vec!["init".to_string(), "--json".to_string()];
    let root_json = vec!["root".to_string(), "--json".to_string()];
    for (writer, reader) in [(&rust, &oracle), (&oracle, &rust)] {
        let scratch = Scratch::new();
        let created = run(writer, &init, &scratch.0);
        assert_eq!(created.exit_code, 0, "{writer}: {}", created.stderr);
        let read = run(reader, &init, &scratch.0);
        assert_eq!(
            read.exit_code, 0,
            "{reader} reads {writer}: {}",
            read.stderr
        );
        let value: Value = serde_json::from_str(&read.stdout).expect("JSON");
        assert_eq!(value["created"], false, "{reader} must not create again");
        assert_eq!(instance_id(&read), instance_id(&created));
        let below = scratch.0.join("a");
        fs::create_dir(&below).expect("a");
        let root = run(reader, &root_json, &below);
        assert_eq!(root.exit_code, 0, "{reader} root: {}", root.stderr);
        let value: Value = serde_json::from_str(&root.stdout).expect("JSON");
        assert_eq!(value["root"], long_form(&scratch.0).as_str());
        eprintln!("cross-read ok: {reader} reads the instance of {writer}");
    }
}

/// Port of the concurrent `init` of `CliTests`: several processes race on one folder; all
/// succeed, exactly one creates, all report the same instance, and no staging folder is left.
#[test]
fn concurrent_inits_agree_on_one_instance() {
    let rust = env!("CARGO_BIN_EXE_smf");
    let scratch = Scratch::new();
    let children: Vec<_> = (0..8)
        .map(|_| {
            Command::new(rust)
                .args(["init", "--json"])
                .env("SERMOFUR_NO_DAEMON", "1")
                .current_dir(&scratch.0)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("starts")
        })
        .collect();
    let mut ids = Vec::new();
    let mut created = 0;
    for child in children {
        let output = child.wait_with_output().expect("ends");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).expect("JSON");
        created += usize::from(value["created"] == true);
        ids.push(value["configuration"]["instanceId"].clone());
    }
    assert_eq!(created, 1, "exactly one process creates the instance");
    assert!(ids.windows(2).all(|pair| pair[0] == pair[1]));
    assert_eq!(
        entries(&scratch.0),
        [
            ".sermofur/",
            ".sermofur/instance.json",
            ".sermofur/memory.db",
            ".sermofur/records/"
        ]
    );
}
