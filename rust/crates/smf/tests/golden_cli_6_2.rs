//! Differential test of lot 6.2, on the harness of `common`: `smf doctor` from the command line,
//! text and JSON, on healthy instances and on instances damaged in every way a check detects
//! (invalid `.sermofur` entries, configuration, schema, integrity, orphans, scope tree,
//! mappings, overlaps, memory, projections, columns, index, source scopes, format 1), each run
//! followed by the files it changed (none). The `sqlite` check names the version of the engine,
//! normalized by the harness (`sqlite_versions`). The daemon and the Codex home of the user point
//! to a folder that never exists (`common::run`). Also the cross-read: the doctor of each tool on
//! instances written, then damaged, by the other. To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_6_2
//! ```

mod common;

use common::{Act, Scenario, Scratch, arguments, r, rin, run, sqlite_versions, tamper};

const NIL_ID: &str = r#"{"schemaVersion":2,"instanceId":"00000000-0000-0000-0000-000000000000","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;
const OTHER_ID: &str = r#"{"schemaVersion":2,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;
const VERSION_3: &str = r#"{"schemaVersion":3,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;
/// The identity of the format 1 fixture, declared in format 2 over its format 1 database.
const FIXTURE_V2: &str = r#"{"schemaVersion":2,"instanceId":"d0c43e10-ec6f-45c3-a15b-4b1413b8cfab","createdAt":"2026-10-08T10:16:34.8807593+00:00"}"#;

/// `PRAGMA integrity_check` reports an index that does not match its table (as `migrate-oracle`).
const BROKEN_INDEX: &str = "CREATE TABLE t(a,b); INSERT INTO t VALUES(1,2); CREATE INDEX t_a ON t(a); PRAGMA writable_schema=ON; UPDATE sqlite_master SET sql='CREATE INDEX t_a ON t(b)' WHERE name='t_a'; PRAGMA writable_schema=OFF";

/// A claim, its evidence and a RETEX in the workspace: steps 0 (init) to 3.
fn basics() -> Vec<Act> {
    vec![
        r(&["init"]),
        r(&["claim", "add", "The cache is warm.", "--origin", "user"]),
        r(&[
            "evidence",
            "add",
            "{1}",
            "source_code",
            "src/cache.rs",
            "--lineage",
            "repo",
            "--origin",
            "user",
        ]),
        r(&[
            "retex", "add", "--event", "Deploy", "--impact", "Slow", "--next", "Warm up",
            "--origin", "llm",
        ]),
    ]
}

fn with(mut acts: Vec<Act>, more: Vec<Act>) -> Vec<Act> {
    acts.extend(more);
    acts
}

fn doctor() -> Act {
    r(&["doctor"])
}

/// A configuration file larger than 64 KiB.
fn oversized() -> Vec<u8> {
    let mut bytes = OTHER_ID.as_bytes().to_vec();
    bytes.resize(64 * 1024 + 1, b' ');
    bytes
}

fn scenarios() -> Vec<Scenario> {
    let s = |name, acts| Scenario {
        name,
        windows_only: false,
        acts,
    };
    let windows = |name, acts| Scenario {
        name,
        windows_only: true,
        acts,
    };
    vec![
        // DoctorLeavesPersistentFilesUnchanged, CliTests (doctor --json from a subfolder),
        // RepositorySharingItsProjectPathIsNotAnOverlap,
        // MissingMappedDirectoryDoesNotBlockOtherContextsAndDoctorWarns, BoundaryTests.
        s(
            "doctor_healthy",
            with(
                basics(),
                vec![
                    Act::Mkdir("a"),
                    r(&["scope", "add", "a", "client", "workspace", "a"]),
                    Act::File("a/notes.md", "secret de a"),
                    rin("a", &["source", "add", "notes.md", "--origin", "user"]),
                    Act::Changes,
                    doctor(),
                    r(&["--json", "doctor"]),
                    rin("a", &["doctor", "--json"]),
                    r(&["doctor", "extra"]),
                    r(&["doctor", "--origin", "user"]),
                    r(&["--json", "doctor", "--limit"]),
                    Act::Changes,
                    Act::Mkdir("a/p"),
                    rin("a", &["scope", "add", "p", "project", "a", "a/p"]),
                    rin("a/p", &["scope", "add", "r", "repository", "p", "a/p"]),
                    doctor(),
                    Act::Mkdir("b"),
                    r(&["scope", "add", "b", "client", "workspace", "b"]),
                    Act::Rmdir("a"),
                    r(&["--json", "status"]),
                    rin("b", &["--json", "status"]),
                    doctor(),
                    Act::Changes,
                ],
            ),
        ),
        // No instance; LinkedMarkerWithoutConfigurationIsRejectedRatherThanSkipped (doctor part).
        s(
            "doctor_outside",
            vec![
                doctor(),
                r(&["--json", "doctor"]),
                r(&["doctor", "extra"]),
                Act::Mkdir("work"),
                Act::Mkdir("elsewhere"),
                Act::Link("work/.sermofur", "elsewhere"),
                rin("work", &["--json", "doctor"]),
                rin("work", &["doctor"]),
            ],
        ),
        // DoctorReportsTheCaseOfAnInvalidMarkerAbove (four layouts),
        // DoctorReportsADamagedInstanceInsteadOfAnsweringNoInstance (valid ancestor).
        s(
            "doctor_invalid_markers",
            vec![
                r(&["init"]),
                Act::Mkdir("child/below"),
                Act::Changes,
                Act::File("child/.sermofur", "another tool"),
                rin("child/below", &["--json", "doctor"]),
                rin("child/below", &["doctor"]),
                Act::Remove("child/.sermofur"),
                Act::Mkdir("child/.sermofur"),
                rin("child/below", &["--json", "doctor"]),
                Act::Mkdir("child/.sermofur/records"),
                rin("child/below", &["--json", "doctor"]),
                Act::Rmdir("child/.sermofur/records"),
                Act::File("child/.sermofur/memory.db", ""),
                rin("child/below", &["--json", "doctor"]),
                rin("child", &["doctor"]),
                Act::Changes,
            ],
        ),
        // OversizedConfigurationIsADamagedInstance, SchemaAndInstanceIdentityMismatchIsDiagnosed,
        // MissingAndCorruptDatabaseAreNotCreatedOrRepaired; every refusal of the configuration.
        s(
            "doctor_configuration",
            vec![
                r(&["init"]),
                Act::Changes,
                Act::File(".sermofur/instance.json", NIL_ID),
                doctor(),
                Act::File(".sermofur/instance.json", VERSION_3),
                doctor(),
                Act::File(".sermofur/instance.json", "{"),
                r(&["--json", "doctor"]),
                Act::Bytes(".sermofur/instance.json", oversized),
                doctor(),
                Act::Remove(".sermofur/instance.json"),
                Act::Mkdir(".sermofur/instance.json"),
                doctor(),
                Act::Rmdir(".sermofur/instance.json"),
                Act::File(".sermofur/instance.json", OTHER_ID),
                r(&["--json", "doctor"]),
                Act::Remove(".sermofur/memory.db"),
                doctor(),
                Act::File(".sermofur/memory.db", "not sqlite"),
                doctor(),
                Act::Changes,
                Act::Remove(".sermofur/instance.json"),
                Act::Mkdir("configuration"),
                Act::Link(".sermofur/instance.json", "configuration"),
                doctor(),
            ],
        ),
        // FutureSchemaIsRejectedWithoutMigration, identity removed from the database.
        s(
            "doctor_schema",
            vec![
                r(&["init"]),
                Act::Sql("PRAGMA user_version=999"),
                Act::Changes,
                r(&["--json", "doctor"]),
                Act::Changes,
                Act::Sql("PRAGMA user_version=1"),
                doctor(),
                Act::Sql("PRAGMA user_version=2; DELETE FROM metadata"),
                doctor(),
            ],
        ),
        s(
            "doctor_integrity_and_orphans",
            with(
                basics(),
                vec![
                    Act::Sql(BROKEN_INDEX),
                    Act::Changes,
                    doctor(),
                    Act::Changes,
                    Act::Sql("DROP TABLE t"),
                    Act::Sql(
                        "INSERT INTO idempotency VALUES('ghost','op','k','h','0f8fad5b-d9cb-469f-a165-70867728950e')",
                    ),
                    doctor(),
                    Act::Sql("DELETE FROM records WHERE kind='Claim'"),
                    r(&["--json", "doctor"]),
                ],
            ),
        ),
        // OverlappingScopeInsertedOutsideTheCliIsAnError, a backslash mapping stored by an
        // earlier version (ScopeTests), invalid trees, kinds and mappings.
        s(
            "doctor_scopes",
            vec![
                r(&["init"]),
                Act::Mkdir("a/b"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                Act::Sql("INSERT INTO scopes VALUES('nested','Client','workspace','a/b')"),
                Act::Changes,
                doctor(),
                Act::Changes,
                Act::Sql("DELETE FROM scopes WHERE id='nested'"),
                Act::Mkdir("x/y"),
                Act::Sql(r"INSERT INTO scopes VALUES('legacy','Client','workspace','x\y')"),
                doctor(),
                Act::Sql("INSERT INTO scopes VALUES('lost','Client','nowhere','lost')"),
                doctor(),
                Act::Sql("DELETE FROM scopes WHERE id='lost'"),
                Act::Sql("INSERT INTO scopes VALUES('odd','Galaxy','workspace',NULL)"),
                doctor(),
                Act::Sql("UPDATE scopes SET kind='7' WHERE id='odd'"),
                doctor(),
                Act::Sql("UPDATE scopes SET kind='Repository' WHERE id='odd'"),
                doctor(),
                Act::Sql("DELETE FROM scopes WHERE id='odd'"),
                Act::Sql("UPDATE scopes SET relative_path='../out' WHERE id='a'"),
                doctor(),
                Act::Sql("UPDATE scopes SET relative_path='C:/out' WHERE id='a'"),
                doctor(),
                Act::Sql("UPDATE scopes SET relative_path='a' WHERE id='a'"),
                Act::Rmdir("a"),
                Act::Mkdir("target"),
                Act::Link("a", "target"),
                doctor(),
            ],
        ),
        // ScopeMappingTypedInAnotherCaseStillHoldsItsFolder (Windows absorbs the case), and a
        // mapping stored decomposed (NFD) over a composed folder: missing, NTFS keeps the form.
        windows(
            "doctor_mappings_windows",
            vec![
                r(&["init"]),
                Act::Mkdir("client"),
                r(&["scope", "add", "c", "client", "workspace", "CLIENT"]),
                doctor(),
                Act::Sql("UPDATE scopes SET relative_path='CLIENT' WHERE id='c'"),
                doctor(),
                Act::Mkdir("\u{c9}t\u{e9}"),
                r(&["scope", "add", "e", "client", "workspace", "\u{c9}t\u{e9}"]),
                Act::Sql("UPDATE scopes SET relative_path='E\u{301}te\u{301}' WHERE id='e'"),
                doctor(),
                Act::Sql("DELETE FROM scopes WHERE id='e'"),
                // A short 8.3 name is expanded by `Path.GetFullPath`: the same folder, not misnamed.
                Act::Mkdir("long folder name"),
                r(&[
                    "scope",
                    "add",
                    "l",
                    "client",
                    "workspace",
                    "long folder name",
                ]),
                Act::Sql("UPDATE scopes SET relative_path='LONGFO~1' WHERE id='l'"),
                doctor(),
                r(&["--json", "doctor"]),
            ],
        ),
        // memory_consistency, record_columns: history, revision, scope and identity altered.
        s(
            "doctor_memory",
            with(
                basics(),
                vec![
                    Act::Sql(
                        "DELETE FROM history WHERE object_id IN (SELECT id FROM records WHERE kind='Retex')",
                    ),
                    doctor(),
                    Act::Sql(
                        "UPDATE records SET payload=json_set(payload,'$.revision',0) WHERE kind='Claim'",
                    ),
                    doctor(),
                    Act::Mkdir("a"),
                    r(&["scope", "add", "a", "client", "workspace", "a"]),
                    Act::Sql(
                        "UPDATE records SET scope_id='a', payload=json_set(payload,'$.scopeId','a') WHERE kind='Evidence'",
                    ),
                    doctor(),
                    Act::Sql(
                        "UPDATE records SET payload=json_set(payload,'$.scopeId','ghost') WHERE kind='Retex'",
                    ),
                    doctor(),
                    Act::Sql(
                        "UPDATE records SET payload=json_set(payload,'$.id','00000000-0000-0000-0000-000000000000') WHERE kind='Retex'",
                    ),
                    r(&["--json", "doctor"]),
                    Act::Sql(
                        "UPDATE records SET payload=json_set(payload,'$.contentJson','{') WHERE kind='Retex'",
                    ),
                    doctor(),
                ],
            ),
        ),
        // ProjectionLossDoesNotLoseCanonicalDataAndExportRepairs; records folder as a link.
        s(
            "doctor_projections",
            with(
                basics(),
                vec![
                    Act::Remove(".sermofur/records/{1}.md"),
                    doctor(),
                    Act::Remove(".sermofur/records/{3}.md"),
                    r(&["--json", "doctor"]),
                    r(&["export"]),
                    doctor(),
                    Act::Rmdir(".sermofur/records"),
                    Act::Mkdir("elsewhere"),
                    Act::Link(".sermofur/records", "elsewhere"),
                    doctor(),
                ],
            ),
        ),
        // DoctorFindsASourceLeftInABroaderScopeAndAMisplacedIndexEntry,
        // ScopeCreatedOnTheFolderOfAnAncestorSourceTakesTheSource.
        s(
            "doctor_source_scopes",
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::File("a/notes.md", "secret de a"),
                r(&["source", "add", "a/notes.md", "--origin", "user"]),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                doctor(),
                Act::Sql("UPDATE search SET scope_id='workspace'"),
                doctor(),
                Act::Sql(
                    "UPDATE records SET scope_id='workspace', payload=json_set(payload,'$.scopeId','workspace') WHERE kind='Source'",
                ),
                r(&["--json", "doctor"]),
            ],
        ),
        // IndexRebuildRestoresADesynchronizedIndex, IndexRebuildReadsEverySourceAgainAndCounts
        // OnlyVisibleObjects, IndexRebuildDropsALostSourceAndRollsBackOnFailure,
        // ReindexReportsModifiedMissingAndRestoredSourcesWithHistory (search_index part).
        s(
            "doctor_search_index",
            vec![
                r(&["init"]),
                r(&["claim", "add", "conservé", "--origin", "user"]),
                Act::File("doc.md", "texte perdu"),
                Act::File("kept.md", "texte gardé"),
                r(&["source", "add", "doc.md", "--origin", "user"]),
                r(&["source", "add", "kept.md", "--origin", "user"]),
                Act::Sql("DELETE FROM search"),
                Act::Changes,
                r(&["--json", "doctor"]),
                Act::Changes,
                r(&["index", "rebuild"]),
                doctor(),
                Act::File("kept.md", "texte révisé"),
                r(&["source", "reindex"]),
                doctor(),
                Act::Remove("doc.md"),
                r(&["index", "rebuild"]),
                doctor(),
                Act::File("doc.md", "de retour"),
                r(&["source", "reindex"]),
                doctor(),
                Act::Sql(
                    "INSERT INTO search(object_id,scope_id,kind,passage,length,text) VALUES('x','workspace','claim',0,1,'x')",
                ),
                doctor(),
                Act::Sql("UPDATE search SET kind=NULL WHERE object_id='x'"),
                doctor(),
            ],
        ),
        // OtherCommandsRefuseFormatOneWithoutTouchingIt (doctor part),
        // MigrationKeepsEveryObjectHistoryKeyAndProjection (no error, search_index ok).
        s(
            "doctor_legacy",
            vec![
                Act::Legacy,
                Act::Changes,
                doctor(),
                r(&["--json", "doctor"]),
                rin("client-a", &["doctor"]),
                Act::Changes,
                r(&["migrate"]),
                Act::Changes,
                doctor(),
                rin("client-a/project-x", &["--json", "doctor"]),
                Act::Changes,
            ],
        ),
        s(
            "doctor_legacy_database",
            vec![
                Act::Legacy,
                Act::File(".sermofur/instance.json", FIXTURE_V2),
                Act::Changes,
                r(&["--json", "doctor"]),
                Act::Changes,
            ],
        ),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-6-2", scenarios());
}

/// The `sqlite` check names the engine the port is built with (normalized in the golden files).
#[test]
fn doctor_prints_the_bundled_sqlite_version() {
    let scratch = Scratch::new();
    let rust = env!("CARGO_BIN_EXE_smf");
    assert_eq!(run(rust, &arguments(&["init"]), &scratch.0).exit_code, 0);
    let out = run(rust, &arguments(&["doctor"]), &scratch.0);
    assert!(
        out.stdout
            .contains("  ok: sqlite — Native version 3.53.2; journal delete.\n"),
        "{}",
        out.stdout
    );
}

/// Cross-read: the doctor of each tool on an instance written by the other, healthy then
/// damaged, gives the same report.
#[test]
fn each_tool_diagnoses_the_instance_written_by_the_other() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("cross-read skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    for (writer, reader) in [(&rust, &oracle), (&oracle, &rust)] {
        let scratch = Scratch::new();
        let root = &scratch.0;
        let step = |values: &[&str], cwd: &str| {
            let out = run(writer, &arguments(values), &root.join(cwd));
            assert_eq!(out.exit_code, 0, "{values:?}: {}", out.stderr);
        };
        step(&["init"], "");
        step(
            &["claim", "add", "The cache is warm.", "--origin", "user"],
            "",
        );
        std::fs::create_dir_all(root.join("a")).expect("folder");
        std::fs::write(root.join("a").join("notes.md"), "secret de a").expect("file");
        step(&["scope", "add", "a", "client", "workspace", "a"], "");
        step(&["source", "add", "notes.md", "--origin", "user"], "a");
        step(
            &[
                "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin", "llm",
            ],
            "a",
        );
        let compare = |state: &str| {
            for values in [vec!["doctor"], vec!["--json", "doctor"]] {
                let a = run(writer, &arguments(&values), root);
                let b = run(reader, &arguments(&values), root);
                assert_eq!(
                    (a.exit_code, sqlite_versions(&a.stdout), a.stderr.as_str()),
                    (b.exit_code, sqlite_versions(&b.stdout), b.stderr.as_str()),
                    "{values:?} on the {state} instance written by {writer}"
                );
            }
        };
        compare("healthy");
        tamper(
            root,
            "UPDATE search SET scope_id='workspace'; DELETE FROM history WHERE object_id IN (SELECT id FROM records WHERE kind='Retex'); INSERT INTO idempotency VALUES('ghost','op','k','h','x')",
        );
        std::fs::remove_dir_all(root.join(".sermofur").join("records")).expect("projections");
        compare("damaged");
        eprintln!("cross-read ok: {reader} diagnoses the instance written by {writer}");
    }
}

/// The daemon check of each tool with `SERMOFUR_DAEMON_HOME` set to `home`.
fn daemon_line(program: &str, root: &std::path::Path, home: &std::path::Path) -> String {
    let output = std::process::Command::new(program)
        .arg("doctor")
        .env("SERMOFUR_DAEMON_HOME", home)
        .env("CODEX_HOME", home)
        .current_dir(root)
        .output()
        .expect("the program starts");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find(|line| line.contains(" daemon — "))
        .expect("a daemon check")
        .to_owned()
}

/// Kills the daemon of the oracle when the test ends, whatever happens.
struct Daemon(std::process::Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The endpoint of a daemon of the oracle, started under its own `SERMOFUR_DAEMON_HOME`, is
/// found by the port under the same name (hash of the folder, listing of the pipes or the
/// socket): both tools connect and report it running with the same line (lot 7.5); once the
/// daemon is killed, both report it absent (Windows) or not answering (a socket left behind).
#[test]
fn the_endpoint_of_a_running_daemon_is_found() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("daemon endpoint skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf");
    let scratch = Scratch::new();
    let root = scratch.0.join("instance");
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&root).expect("folder");
    assert_eq!(run(rust, &arguments(&["init"]), &root).exit_code, 0);
    let mut daemon = Daemon(
        std::process::Command::new(&oracle)
            .args(["daemon", "run"])
            .env("SERMOFUR_DAEMON_HOME", &home)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the daemon starts"),
    );
    let started = std::time::Instant::now();
    while !daemon_line(&oracle, &root, &home).contains("running") {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(30),
            "the daemon of the oracle does not answer"
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    assert_eq!(
        daemon_line(rust, &root, &home),
        daemon_line(&oracle, &root, &home)
    );
    let _ = daemon.0.kill();
    let _ = daemon.0.wait();
    drop(daemon);
    if cfg!(windows) {
        // The pipe disappears with the process: both tools report it absent.
        assert_eq!(
            daemon_line(rust, &root, &home),
            daemon_line(&oracle, &root, &home)
        );
        assert!(daemon_line(rust, &root, &home).contains("absent"));
    } else {
        // A killed daemon leaves its socket: both tools try it and report it not answering.
        assert!(daemon_line(&oracle, &root, &home).contains("not_answering"));
        assert_eq!(
            daemon_line(rust, &root, &home),
            daemon_line(&oracle, &root, &home)
        );
    }
}
