//! Help of each command and group (lot 9) against `rust/tests/golden/help.json` (Windows) or
//! `help-unix.json` (elsewhere), recorded from the oracle by `rust/tools/help-oracle`: every
//! topic and group of `CommandHelp`, then the edge cases of `CommandHelp.Find`. Each case runs
//! in a fresh empty folder, without daemon, and must give the recorded stdout and stderr byte
//! for byte (line breaks of the platform included), the exit code, and leave the folder empty.
//!
//! With `SMF_ORACLE` (the `smf` of .NET 0.4.3), the oracle replays the same file.

#[path = "../../../tests/support/scratch.rs"]
#[allow(dead_code)] // only the guard of a fixed home is used here
mod scratch;

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::Value;

const SMF: &str = env!("CARGO_BIN_EXE_smf");

fn golden() -> Vec<Value> {
    let name = if cfg!(windows) {
        "help.json"
    } else {
        "help-unix.json"
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    let file: Value =
        serde_json::from_str(&fs::read_to_string(&path).expect("the golden file")).expect("JSON");
    file["cases"].as_array().expect("cases").clone()
}

/// The differences between what `program` prints and the golden file, one line per case.
fn replay(program: &str) -> Vec<String> {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    scratch::assert_fits("the help golden", "smf-nh");
    let elsewhere = std::env::temp_dir().join("smf-nh");
    let mut differences = Vec::new();
    for case in golden() {
        let name = case["name"].as_str().expect("name");
        let arguments: Vec<&str> = case["arguments"]
            .as_array()
            .expect("arguments")
            .iter()
            .map(|a| a.as_str().expect("an argument"))
            .collect();
        let folder = std::env::temp_dir().join(format!(
            "sermofur-help-golden-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&folder).expect("the folder");
        let output = Command::new(program)
            .args(&arguments)
            .env("SERMOFUR_NO_DAEMON", "1")
            .env("SERMOFUR_DAEMON_HOME", &elsewhere)
            .env("CODEX_HOME", &elsewhere)
            .current_dir(&folder)
            .output()
            .expect("the program starts");
        let untouched = fs::read_dir(&folder).expect("listable").next().is_none();
        fs::remove_dir_all(&folder).expect("removed");
        let actual = (
            output.status.code().map(i64::from),
            String::from_utf8_lossy(&output.stdout).into_owned(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        );
        let expected = (
            case["exit_code"].as_i64(),
            case["stdout"].as_str().expect("stdout").to_string(),
            case["stderr"].as_str().expect("stderr").to_string(),
        );
        if actual != expected || !untouched {
            differences.push(format!(
                "{name} {arguments:?}: expected {expected:?}, got {actual:?}, folder untouched: {untouched}"
            ));
        }
    }
    differences
}

#[test]
fn every_help_is_the_one_of_the_oracle() {
    let differences = replay(SMF);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}

#[test]
fn the_golden_file_covers_every_topic_and_group() {
    let cases = golden();
    let count = |prefix: &str| {
        cases
            .iter()
            .filter(|c| c["name"].as_str().is_some_and(|n| n.starts_with(prefix)))
            .count()
    };
    assert_eq!((count("topic "), count("group ")), (40, 8));
}

/// With `SMF_ORACLE`: the oracle prints its golden file (the file is not stale).
#[test]
fn the_oracle_replays_its_golden_file() {
    let Some(oracle) = std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty()) else {
        eprintln!("SMF_ORACLE not set: the oracle is not replayed");
        return;
    };
    let differences = replay(&oracle);
    assert!(differences.is_empty(), "{}", differences.join("\n"));
}
