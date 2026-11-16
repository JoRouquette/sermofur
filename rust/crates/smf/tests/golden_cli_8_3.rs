//! `smf mcp install` and `uninstall`, and the `mcp` check of `doctor` (lot 8.3). Ports of the
//! .NET tests that pass through the CLI; with `SMF_ORACLE` (the `smf` of .NET 0.4.3), the same
//! steps are run by both and their outputs and written files compared (lecture croisée).
//! Every run gets its own daemon home, Codex home and working folder: the profile of the user
//! is never written.

mod common;

use std::fs;
use std::path::Path;
use std::process::Command;

use common::Scratch;

const SMF: &str = env!("CARGO_BIN_EXE_smf");

struct Out {
    code: i32,
    stdout: String,
    stderr: String,
}

/// One run with the homes of `scratch` (`daemon`, `codex`) and `instance` as working folder.
fn smf(program: &str, scratch: &Path, arguments: &[&str]) -> Out {
    let output = Command::new(program)
        .args(arguments)
        .env("SERMOFUR_NO_DAEMON", "1")
        .env("SERMOFUR_DAEMON_HOME", scratch.join("daemon"))
        .env("CODEX_HOME", scratch.join("codex"))
        // The folder of the program is never on this PATH: the launcher names it in full.
        .env("PATH", scratch.join("daemon"))
        .current_dir(scratch.join("instance"))
        .output()
        .expect("the program starts");
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    Out {
        code: output.status.code().expect("exit code"),
        stdout: text(&output.stdout),
        stderr: text(&output.stderr),
    }
}

/// A fresh scratch with an initialized instance.
fn instance(program: &str) -> Scratch {
    let scratch = Scratch::new();
    fs::create_dir_all(scratch.0.join("instance")).expect("instance folder");
    assert_eq!(smf(program, &scratch.0, &["init"]).code, 0);
    scratch
}

fn mcp_check(program: &str, scratch: &Path) -> String {
    let doctor = smf(program, scratch, &["doctor", "--json"]);
    let report: serde_json::Value = serde_json::from_str(&doctor.stdout).expect("doctor JSON");
    report["checks"]
        .as_array()
        .expect("checks")
        .iter()
        .find(|check| check["name"] == "mcp")
        .map(|check| format!("{}: {}", check["status"], check["detail"]))
        .expect("an mcp check")
}

/// Port of `McpConfigFileTests.DoctorReportsTheDeclarationAndCliCommandsWork`.
#[test]
fn doctor_reports_the_declaration_and_cli_commands_work() {
    let scratch = instance(SMF);
    let root = &scratch.0;
    assert!(mcp_check(SMF, root).starts_with("\"warning\": \"Not declared"));
    let installed = smf(SMF, root, &["mcp", "install", "--json"]);
    assert_eq!(installed.code, 0, "{}", installed.stderr);
    let report: serde_json::Value = serde_json::from_str(&installed.stdout).expect("JSON");
    assert_eq!(report["changed"], true);
    assert!(
        report["next"]
            .as_array()
            .expect("next")
            .iter()
            .any(|step| step.as_str().expect("text").contains("smf daemon"))
    );
    assert_eq!(
        mcp_check(SMF, root),
        "\"ok\": \"Declared to claude-code (.mcp.json).\""
    );
    assert_eq!(smf(SMF, root, &["mcp", "uninstall"]).code, 0);
    assert!(mcp_check(SMF, root).starts_with("\"warning\""));
    assert_eq!(smf(SMF, root, &["mcp", "nope"]).code, 1);
}

/// Port of `CodexConfigFileTests.CodexDeclarationThroughTheCli`, with the `mcp` check of the
/// user configuration.
#[test]
fn codex_declaration_through_the_cli() {
    let scratch = instance(SMF);
    let root = &scratch.0;
    let installed = smf(SMF, root, &["mcp", "install", "--host", "codex", "--json"]);
    assert_eq!(installed.code, 0, "{}", installed.stderr);
    let report: serde_json::Value = serde_json::from_str(&installed.stdout).expect("JSON");
    assert_eq!(report["host"], "codex");
    assert!(installed.stdout.contains("trust"));
    let project = root.join("instance").join(".codex").join("config.toml");
    assert!(project.is_file());
    let doctor = smf(SMF, root, &["doctor", "--json"]);
    assert!(doctor.stdout.contains("codex (.codex/config.toml)"));
    let uninstall = smf(SMF, root, &["mcp", "uninstall", "--host", "codex"]);
    assert_eq!(uninstall.code, 0);
    assert_eq!(fs::read_to_string(&project).expect("kept"), "");
    let user = smf(
        SMF,
        root,
        &[
            "mcp", "install", "--host", "codex", "--scope", "user", "--json",
        ],
    );
    let report: serde_json::Value = serde_json::from_str(&user.stdout).expect("JSON");
    let file = report["file"].as_str().expect("file");
    assert!(file.starts_with(&root.join("codex").to_string_lossy().into_owned()));
    assert_eq!(
        mcp_check(SMF, root),
        "\"ok\": \"Declared to codex (user configuration).\""
    );
    let removed = smf(
        SMF,
        root,
        &["mcp", "uninstall", "--host", "codex", "--scope", "user"],
    );
    assert_eq!(removed.code, 0);
    let refused = smf(SMF, root, &["mcp", "install", "--scope", "user", "--json"]);
    assert_eq!(refused.code, 1);
    assert!(refused.stderr.contains("claude mcp add --scope user"));
}

/// Port of `ServiceHardeningTests.CodexDeclarationKeepsOrCreatesAPrivateFile`, through the CLI.
#[cfg(unix)]
#[test]
fn codex_declaration_keeps_or_creates_a_private_file() {
    use std::os::unix::fs::PermissionsExt;
    let mode = |file: &Path| fs::metadata(file).expect("file").permissions().mode() & 0o777;
    let existing = instance(SMF);
    let file = existing
        .0
        .join("instance")
        .join(".codex")
        .join("config.toml");
    fs::create_dir_all(file.parent().expect("folder")).expect("folder");
    fs::write(&file, "model = \"x\"\n").expect("file");
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("mode");
    assert_eq!(
        smf(SMF, &existing.0, &["mcp", "install", "--host", "codex"]).code,
        0
    );
    assert_eq!(mode(&file), 0o600);
    assert_eq!(
        smf(SMF, &existing.0, &["mcp", "uninstall", "--host", "codex"]).code,
        0
    );
    assert_eq!(mode(&file), 0o600);
    let fresh = instance(SMF);
    assert_eq!(
        smf(SMF, &fresh.0, &["mcp", "install", "--host", "codex"]).code,
        0
    );
    let created = fresh.0.join("instance").join(".codex").join("config.toml");
    assert_eq!(mode(&created), 0o600);
}

/// Port of `ConfigurationWriteTests.TheWriterOfAHostConfigurationFollowsTheLinkAndNamesWhereItWrote`,
/// through the CLI; skipped where a file link cannot be created (`FileLinkFact`).
#[test]
fn the_writer_of_a_host_configuration_follows_the_link_and_names_where_it_wrote() {
    let scratch = instance(SMF);
    let folder = scratch.0.join("instance");
    let target = folder.join("dotfiles-mcp.json");
    fs::write(&target, "{}").expect("target");
    let link = folder.join(".mcp.json");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(&target, &link);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(&target, &link);
    if linked.is_err() {
        return;
    }
    let installed = smf(SMF, &scratch.0, &["mcp", "install"]);
    assert_eq!(installed.code, 0, "{}", installed.stderr);
    let report: serde_json::Value = serde_json::from_str(&installed.stdout).expect("JSON");
    assert_eq!(report["changed"], true);
    assert!(
        fs::read_to_string(&target)
            .expect("target")
            .contains("sermofur")
    );
    assert!(
        fs::symlink_metadata(&link)
            .expect("link")
            .file_type()
            .is_symlink()
    );
    // The CLI names its folder in the long form of the working directory, as .NET does.
    let replaced = report["replaced"].as_str().expect("replaced");
    assert!(replaced.ends_with("dotfiles-mcp.json"));
    assert!(
        report["file"]
            .as_str()
            .expect("file")
            .ends_with(".mcp.json")
    );
    assert!(report["next"][0].as_str().expect("next").contains(replaced));
}

/// The command of a report names the program that ran: the oracle and the port differ there.
fn normalized(text: &str, scratch: &Path, program: &str) -> String {
    let long = scratch.to_string_lossy().into_owned();
    let json = |text: &str| text.replace('\\', "\\\\");
    let full = common::long_form(Path::new(program));
    text.replace(&json(&full), "<smf>")
        .replace(&full, "<smf>")
        .replace(&json(program), "<smf>")
        .replace(program, "<smf>")
        .replace(&json(&long), "<scratch>")
        .replace(&long, "<scratch>")
        .replace(&json(&common::long_form(scratch)), "<scratch>")
        .replace(&common::long_form(scratch), "<scratch>")
}

/// Lecture croisée: the same steps by the oracle and the port write the same text, JSON and
/// files. The `sqlite` line of `doctor` is left out (native version of each build).
#[test]
fn the_oracle_and_the_port_declare_alike() {
    let Some(oracle) = std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty()) else {
        return;
    };
    let steps: &[&[&str]] = &[
        &["mcp", "install", "--json"],
        &["mcp", "install"],
        &["doctor"],
        &["mcp", "install", "--host", "codex", "--json"],
        &["doctor", "--json"],
        &["mcp", "uninstall"],
        &["mcp", "uninstall", "--host", "codex"],
        &["mcp", "install", "--host", "codex", "--scope", "user"],
        &["doctor"],
        &["mcp", "uninstall", "--host", "codex", "--scope", "user"],
        &["mcp", "install", "--scope", "user", "--json"],
        &["mcp", "install", "--host", "x"],
        &["mcp", "install", "--scope", "x"],
        &["mcp", "nope"],
        &["mcp"],
    ];
    let transcript = |program: &str| {
        let scratch = instance(program);
        let mut lines = Vec::new();
        for step in steps {
            let out = smf(program, &scratch.0, step);
            let text = format!(
                "{} -> {}\n{}{}",
                step.join(" "),
                out.code,
                out.stdout,
                out.stderr
            );
            lines.extend(
                normalized(&text, &scratch.0, program)
                    .lines()
                    .filter(|line| !line.contains("sqlite") && !line.contains("Native version"))
                    .map(str::to_owned),
            );
        }
        for file in [
            scratch.0.join("instance").join(".mcp.json"),
            scratch
                .0
                .join("instance")
                .join(".codex")
                .join("config.toml"),
            scratch.0.join("codex").join("config.toml"),
        ] {
            let content = fs::read(&file)
                .map(|b| normalized(&String::from_utf8_lossy(&b), &scratch.0, program));
            lines.push(format!("{content:?}"));
        }
        lines
    };
    let (expected, actual) = (transcript(&oracle), transcript(SMF));
    for (line, (left, right)) in expected.iter().zip(&actual).enumerate() {
        assert_eq!(left, right, "line {line}");
    }
    assert_eq!(expected.len(), actual.len());
}

/// Minor of lot 8.3, closed at lot 8.4: a declaration file present but unreadable (mode 000)
/// stops `doctor` with `invalid_storage_or_path`, exit 3, as the oracle; `mcp install` answers
/// `invalid_mcp_config`, exit 3 (named deviation of lot 8.3). Skipped for root, who reads it.
#[cfg(unix)]
#[test]
fn an_unreadable_declaration_stops_doctor_as_the_oracle() {
    use std::os::unix::fs::PermissionsExt;

    let mut programs = vec![SMF.to_owned()];
    programs.extend(std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty()));
    for program in programs {
        let scratch = instance(&program);
        let root = &scratch.0;
        let file = root.join("instance").join(".mcp.json");
        fs::write(&file, "{}").expect("written");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("mode");
        if fs::read(&file).is_ok() {
            eprintln!("this account reads a file of mode 000 (root): skipped");
            return;
        }
        let doctor = smf(&program, root, &["doctor", "--json"]);
        assert_eq!(doctor.code, 3, "{program}: {}", doctor.stderr);
        assert!(
            doctor.stderr.contains("invalid_storage_or_path"),
            "{}",
            doctor.stderr
        );
        if program == SMF {
            let install = smf(SMF, root, &["mcp", "install", "--json"]);
            assert_eq!(install.code, 3, "{}", install.stderr);
            assert!(
                install.stderr.contains("invalid_mcp_config"),
                "{}",
                install.stderr
            );
        }
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).expect("mode");
    }
}
