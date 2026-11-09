//! Differential test of lot 6.1, on the harness of `common`: `smf migrate` from the command line
//! on a copy of the format 1 fixture of `MigrationTests` (answers, refusals and their exit codes,
//! files written, backup), the other commands refused on format 1 without touching it, and the
//! extended path carried from the review of lot 5.3 (`Path.GetFullPath` returns a `\\?\` path as
//! typed, `.` and `..` included, and the discovery refuses it as a network path). The migration
//! itself (tables, `sqlite_master`, backup, interruptions, in-process race) is compared in
//! `sermofur-core/tests/migration_golden.rs`. Also a race between real processes and the
//! cross-read of an instance migrated by the other tool. To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_6_1
//! ```

mod common;

use std::process::{Command, Stdio};

use common::{Act, Captured, Scenario, Scratch, arguments, copy_legacy, r, rin, run};
use serde_json::Value;

const OTHER_ID: &str = r#"{"schemaVersion":1,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;
const FIXTURE_ID: &str = r#"{"schemaVersion":1,"instanceId":"d0c43e10-ec6f-45c3-a15b-4b1413b8cfab","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;
const VERSION_3: &str = r#"{"schemaVersion":3,"instanceId":"d0c43e10-ec6f-45c3-a15b-4b1413b8cfab","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;

fn scenarios() -> Vec<Scenario> {
    let s = |name, windows_only, acts| Scenario {
        name,
        windows_only,
        acts,
    };
    vec![
        // OtherCommandsRefuseFormatOneWithoutTouchingIt (the doctor part waits for lot 6.2),
        // MigrateCommandReportsTheResult and MigratedInstanceAcceptsNewObjectsAndMigrateIsIdempotent.
        s(
            "migrate_legacy",
            false,
            vec![
                Act::Legacy,
                Act::Changes,
                r(&["--json", "status"]),
                r(&["status"]),
                r(&["--json", "claim", "add", "x", "--origin", "user"]),
                rin("client-a", &["claim", "list"]),
                r(&["root"]),
                r(&["--json", "root"]),
                Act::Changes,
                r(&["--json", "migrate"]),
                Act::Changes,
                r(&["migrate"]),
                r(&["--json", "migrate"]),
                Act::Changes,
                r(&["status"]),
                r(&["scope", "list"]),
                rin("client-a", &["claim", "list"]),
                rin("client-a", &["recall", "PostgreSQL"]),
                r(&["recall", "cache"]),
                r(&["retex", "list"]),
                rin("client-a", &["evidence", "list"]),
                rin(
                    "client-a",
                    &["claim", "add", "after migration", "--origin", "user"],
                ),
                rin("client-a/project-x", &["claim", "list"]),
                r(&["migrate"]),
                Act::Changes,
            ],
        ),
        s(
            "migrate_refusals",
            false,
            vec![
                r(&["migrate"]),
                r(&["--json", "migrate"]),
                r(&["init"]),
                r(&["migrate"]),
                r(&["migrate", "extra"]),
                r(&["migrate", "--origin", "user"]),
                r(&["--json", "migrate", "--limit"]),
                Act::Mkdir("sub"),
                rin("sub", &["migrate"]),
                Act::File(".sermofur/instance.json", VERSION_3),
                r(&["migrate"]),
                Act::Changes,
            ],
        ),
        s(
            "migrate_refusals_legacy",
            false,
            vec![
                Act::Legacy,
                Act::Changes,
                Act::File(".sermofur/backups", "a file in place of the folder"),
                r(&["--json", "migrate"]),
                Act::Remove(".sermofur/backups"),
                Act::File(".sermofur/instance.json", OTHER_ID),
                r(&["--json", "migrate"]),
                Act::File(".sermofur/instance.json", VERSION_3),
                r(&["migrate"]),
                Act::File(".sermofur/instance.json", FIXTURE_ID),
                Act::Changes,
                Act::Link(".sermofur/backups", "client-b"),
                r(&["--json", "migrate"]),
                Act::Bytes(".sermofur/memory.db", user_version_beyond_int32),
                r(&["--json", "migrate"]),
                Act::Remove(".sermofur/memory.db"),
                r(&["--json", "migrate"]),
                r(&["status"]),
                Act::Changes,
            ],
        ),
        s(
            "context_extended_path_windows",
            true,
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                Act::File("b/doc.md", "contenu"),
                r(&["--path", r"\\?\<root>\a\..\b", "status"]),
                r(&["--path", r"\\?\<root>\a\..\b", "--json", "root"]),
                r(&["--path", r"\\?\<root>\b\.", "status"]),
                r(&[
                    "source",
                    "add",
                    r"\\?\<root>\a\..\b\doc.md",
                    "--origin",
                    "user",
                ]),
                Act::Changes,
            ],
        ),
    ]
}

/// The fixture database with a `user_version` of 3 000 000 000, above `Int32.MaxValue`, in its
/// header (offset 60, 4 bytes big-endian, where `PRAGMA user_version` writes it): SQLite keeps
/// 32 bits, so both tools read -1 294 967 296, never a value that `Convert.ToInt32` refuses.
fn user_version_beyond_int32() -> Vec<u8> {
    let mut bytes = std::fs::read(common::legacy_fixture().join("memory.db")).expect("fixture");
    bytes[60..64].copy_from_slice(&3_000_000_000_u32.to_be_bytes());
    bytes
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-6-1", scenarios());
}

fn json_of(out: &Captured) -> Value {
    serde_json::from_str(&out.stdout).expect("JSON")
}

/// Eight real processes migrate the same format 1 instance at once. The `IMMEDIATE`
/// transaction lets one rebuild the database; the others find format 2 under the lock, delete
/// their own backup and answer without one (and `migrated: true` when they read the
/// configuration before it was rewritten: E1). A process may also meet a lock of another one
/// (`storage_busy`: the backup step never waits, as `BackupDatabase`) or a configuration being
/// replaced (`invalid_storage_or_path`); in every case the instance ends migrated once, with at
/// most one backup reported, and readable.
fn race_between_processes(program: &str) {
    let scratch = Scratch::new();
    let root = &scratch.0;
    copy_legacy(root);
    let children: Vec<_> = (0..8)
        .map(|_| {
            Command::new(program)
                .args(["--json", "migrate"])
                .env("SERMOFUR_NO_DAEMON", "1")
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("the program starts")
        })
        .collect();
    let mut reported = Vec::new();
    let (mut migrated, mut unchanged, mut refused) = (0, 0, 0);
    for child in children {
        let output = child.wait_with_output().expect("the process ends");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        match output.status.code() {
            Some(0) => {
                let answer: Value = serde_json::from_str(&stdout).expect("JSON");
                assert_eq!(answer["to"], 2, "{stdout}");
                if answer["migrated"] == true {
                    assert_eq!(answer["from"], 1, "{stdout}");
                    migrated += 1;
                } else {
                    assert_eq!(answer["from"], 2, "{stdout}");
                    unchanged += 1;
                }
                if let Some(backup) = answer["backup"].as_str() {
                    reported.push(backup.to_owned());
                }
            }
            Some(3) => {
                assert!(
                    stderr.contains("storage_busy") || stderr.contains("invalid_storage_or_path"),
                    "{stderr}"
                );
                refused += 1;
            }
            other => panic!("exit {other:?}: {stderr}"),
        }
    }
    assert!(reported.len() <= 1, "one backup at most: {reported:?}");
    for backup in &reported {
        assert!(root.join(backup).is_file(), "{backup} is kept");
    }
    let again = run(program, &arguments(&["--json", "migrate"]), root);
    assert_eq!(again.exit_code, 0, "{}", again.stderr);
    assert_eq!(json_of(&again)["migrated"], false);
    let claims = run(
        program,
        &arguments(&["--json", "claim", "list"]),
        &root.join("client-a"),
    );
    assert_eq!(claims.exit_code, 0, "{}", claims.stderr);
    assert_eq!(json_of(&claims).as_array().expect("claims").len(), 2);
    eprintln!(
        "race of {program}: {migrated} migrated, {unchanged} unchanged, {refused} refused, {} backup reported",
        reported.len()
    );
}

#[test]
fn concurrent_processes_migrate_the_instance_once() {
    race_between_processes(env!("CARGO_BIN_EXE_smf"));
    match std::env::var("SMF_ORACLE") {
        Ok(oracle) => race_between_processes(&oracle),
        Err(_) => eprintln!("race of the oracle skipped: SMF_ORACLE is not set"),
    }
}

/// Cross-read: one tool migrates the fixture, the other reads it with the same bytes, adds a
/// claim, and the first reads that claim; a second migration by the other tool changes nothing.
#[test]
fn each_tool_reads_the_instance_migrated_by_the_other() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("cross-read skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    for (migrator, reader) in [(&rust, &oracle), (&oracle, &rust)] {
        let scratch = Scratch::new();
        let root = &scratch.0;
        copy_legacy(root);
        let migrated = run(migrator, &arguments(&["--json", "migrate"]), root);
        assert_eq!(migrated.exit_code, 0, "{}", migrated.stderr);
        assert_eq!(json_of(&migrated)["migrated"], true);
        let reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["status"], ""),
            (vec!["scope", "list"], ""),
            (vec!["claim", "list"], "client-a"),
            (vec!["claim", "list"], "client-b"),
            (vec!["recall", "PostgreSQL"], "client-a"),
            (vec!["recall", "cache"], ""),
            (vec!["retex", "list"], ""),
            (vec!["evidence", "list"], "client-a"),
            (vec!["export"], "client-a"),
        ];
        let compare = |first: &String, second: &String| {
            for (values, cwd) in &reads {
                let a = run(first, &arguments(values), &root.join(cwd));
                let b = run(second, &arguments(values), &root.join(cwd));
                assert_eq!(
                    (a.exit_code, &a.stdout, &a.stderr),
                    (b.exit_code, &b.stdout, &b.stderr),
                    "{values:?} on the instance migrated by {migrator}"
                );
            }
        };
        compare(migrator, reader);
        let added = run(
            reader,
            &arguments(&[
                "--json",
                "claim",
                "add",
                "après migration",
                "--origin",
                "user",
            ]),
            &root.join("client-a"),
        );
        let id = common::printed_id(&added);
        let shown = run(
            migrator,
            &arguments(&["claim", "show", &id]),
            &root.join("client-a"),
        );
        assert_eq!(shown.exit_code, 0, "{}", shown.stderr);
        assert!(shown.stdout.contains("après migration"), "{}", shown.stdout);
        compare(reader, migrator);
        let again = run(reader, &arguments(&["--json", "migrate"]), root);
        assert_eq!(again.exit_code, 0, "{}", again.stderr);
        let answer = json_of(&again);
        assert_eq!(answer["migrated"], false);
        assert_eq!(answer["from"], 2);
        assert!(answer["backup"].is_null());
        eprintln!("cross-read ok: {reader} reads the instance migrated by {migrator}");
    }
}
