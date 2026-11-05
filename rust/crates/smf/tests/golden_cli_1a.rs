//! Differential test of step 1a: help, version and command-line errors.
//!
//! The expected outputs are golden files recorded once from the .NET `smf` (the oracle), so the
//! test needs no .NET: `cargo test` compares the Rust binary with them. To record them again,
//! publish the .NET tool (`dotnet publish src/Sermofur.Cli -o rust/target/oracle`) and run
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test -p smf --test golden_cli_1a
//! ```
//!
//! The scenarios avoid anything that touches an instance (the commands themselves, per-command
//! help), which later steps cover.

use std::path::PathBuf;
use std::process::Command;

use serde_json::{Value, json};

/// What a process printed and returned, with line endings normalized to `\n`.
struct Captured {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn run(program: &str, arguments: &[String]) -> Captured {
    let output = Command::new(program)
        .args(arguments)
        .env("SERMOFUR_NO_DAEMON", "1")
        .current_dir(std::env::temp_dir())
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

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/cli-1a")
        .join(format!("{name}.json"))
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

/// Name and arguments of each scenario.
fn scenarios() -> Vec<(&'static str, Vec<String>)> {
    vec![
        ("no_arguments", args(&[])),
        ("help_long", args(&["--help"])),
        ("help_short", args(&["-h"])),
        ("help_with_json", args(&["--json", "-h"])),
        ("help_wins_over_version", args(&["--version", "--help"])),
        ("version_long", args(&["--version"])),
        ("version_short", args(&["-v"])),
        ("version_json", args(&["--version", "--json"])),
        ("version_json_first", args(&["--json", "-v"])),
        ("version_after_a_command", args(&["x", "-v"])),
        ("shortcut_as_option_value", args(&["--actor", "-v"])),
        ("command_required", args(&["--json"])),
        ("command_required_text", args(&["--path", "somewhere"])),
        ("duplicate_flag", args(&["--json", "--json"])),
        ("duplicate_option", args(&["--actor", "a", "--actor", "b"])),
        ("option_value_required", args(&["--path"])),
        ("option_value_required_json", args(&["--json", "--path"])),
        ("option_value_starts_with_dashes", args(&["--actor", "--x"])),
        ("too_many_arguments", vec!["x".to_string(); 129]),
        (
            "too_many_arguments_json",
            std::iter::once("--json".to_string())
                .chain(std::iter::repeat_n("x".to_string(), 128))
                .collect(),
        ),
        ("argument_too_long", vec!["a".repeat(16_385)]),
        (
            "argument_too_long_json",
            vec!["--json".to_string(), "a".repeat(16_385)],
        ),
        // 8,193 emoji are 16,386 UTF-16 code units but only 8,193 characters.
        ("emoji_count_as_two_units", vec!["😀".repeat(8_193)]),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    let update = std::env::var("SMF_UPDATE_GOLDEN").is_ok_and(|value| value == "1");
    let oracle = std::env::var("SMF_ORACLE").ok();
    let rust = env!("CARGO_BIN_EXE_smf");

    let mut failures: Vec<String> = Vec::new();
    for (name, arguments) in scenarios() {
        let path = golden_path(name);
        if update {
            let oracle = oracle
                .as_deref()
                .expect("SMF_ORACLE names the oracle program");
            let recorded = run(oracle, &arguments);
            let body = json!({
                "exitCode": recorded.exit_code,
                "stdout": recorded.stdout,
                "stderr": recorded.stderr,
            });
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
            std::fs::write(
                &path,
                serde_json::to_string_pretty(&body).expect("serializes") + "\n",
            )
            .expect("the golden file is written");
        }
        let golden: Value = serde_json::from_str(
            &std::fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("golden file missing: {}", path.display())),
        )
        .expect("the golden file is valid JSON");
        let actual = run(rust, &arguments);
        let expected = (
            golden["exitCode"].as_i64().expect("exit code"),
            golden["stdout"].as_str().expect("stdout"),
            golden["stderr"].as_str().expect("stderr"),
        );
        if (
            i64::from(actual.exit_code),
            actual.stdout.as_str(),
            actual.stderr.as_str(),
        ) != expected
        {
            failures.push(format!(
                "{name}: expected {expected:?}, got {:?}",
                (actual.exit_code, &actual.stdout, &actual.stderr)
            ));
        }
    }
    assert!(failures.is_empty(), "differences:\n{}", failures.join("\n"));
}

/// The comparison above normalizes line endings, so the golden files serve every platform; this
/// test checks them on the raw bytes. The .NET tool writes `Environment.NewLine` between the
/// lines of its JSON, and for the help text it keeps the line feeds of its source file and adds
/// the platform line break only at the end (`WriteLine`): observed on the oracle under Windows.
#[test]
fn line_endings_follow_the_platform() {
    let newline = sermofur_core::NEW_LINE;
    let rust = env!("CARGO_BIN_EXE_smf");

    let version = run_raw(rust, &args(&["--version", "--json"]));
    assert_eq!(
        version,
        format!("{{{newline}  \"version\": \"0.0.0-dev\"{newline}}}{newline}")
    );

    let help = run_raw(rust, &args(&["-h"]));
    let body = help
        .strip_suffix(newline)
        .expect("the help ends with the line break of the platform");
    assert!(
        !body.contains('\r'),
        "the lines inside the help keep a bare line feed"
    );
}

/// Standard output of a run, exactly as written.
fn run_raw(program: &str, arguments: &[String]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .env("SERMOFUR_NO_DAEMON", "1")
        .current_dir(std::env::temp_dir())
        .output()
        .expect("the program starts");
    String::from_utf8(output.stdout).expect("the output is UTF-8")
}
