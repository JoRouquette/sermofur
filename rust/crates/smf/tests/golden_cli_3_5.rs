//! Differential test of lot 3.5: input bounds, soft enumerations, non-empty GUID and write
//! detection, against `BoundaryTests` and the argument cases of `CliTests`, on the harness of
//! `common`. The `Changes` steps record which files under `.sermofur` the steps in between
//! added, changed or removed: a read or a refused command must leave them as they were. To
//! record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_3_5
//! ```
//!
//! An argument holding NUL cannot reach a process: that case is a unit test of `runner.rs`.

mod common;

use std::collections::HashSet;
use std::path::Path;

use common::{Act, Scenario, Scratch, UNKNOWN, arguments, printed_id, r, rin, run};
use serde_json::Value;

/// `text` repeated `count` times, for the life of the test.
fn long(text: &str, count: usize) -> &'static str {
    text.repeat(count).leak()
}

/// `count` arguments: `prefix` then as many `x` as needed.
fn padded(prefix: &[&'static str], count: usize) -> Vec<&'static str> {
    let mut values = prefix.to_vec();
    values.extend(std::iter::repeat_n("x", count - prefix.len()));
    values
}

/// The arguments of `BoundaryTests.ParserFuzzFailsWithoutUnhandledExceptionsOrWrites`: 100
/// texts of 1 to 39 printable ASCII characters, from a fixed seed. `{` is left out: it opens a
/// step reference in the harness.
fn fuzz_texts() -> Vec<&'static str> {
    let mut state: u64 = 17;
    let mut next = |bound: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % bound
    };
    (0..100)
        .map(|_| {
            let length = 1 + next(39);
            let text: String = (0..length)
                .map(|_| char::from(b'!' + u8::try_from(next(94)).expect("small")))
                .map(|c| if c == '{' { '[' } else { c })
                .collect();
            &*text.leak()
        })
        .collect()
}

/// Every spelling of an empty GUID that `Guid.TryParse` accepts in some format.
const EMPTY_GUIDS: [&str; 10] = [
    "00000000-0000-0000-0000-000000000000",
    "00000000000000000000000000000000",
    "{00000000-0000-0000-0000-000000000000}",
    "(00000000-0000-0000-0000-000000000000)",
    "{0x00000000,0x0000,0x0000,{0x00,0x00,0x00,0x00,0x00,0x00,0x00,0x00}}",
    "{0x0,0x0,0x0,{0x0,0x0,0x0,0x0,0x0,0x0,0x0,0x0}}",
    " { 0X0 , 0X0 , 0X0 , { 0X0 , 0X0 , 0X0 , 0X0 , 0X0 , 0X0 , 0X0 , 0X0 } } ",
    "+0000000-+000-+000-+000-000000000000",
    "0x000000-0x00-0x00-0x00-000000000000",
    "\t00000000-0000-0000-0000-000000000000\n",
];

fn scenarios() -> Vec<Scenario> {
    let s = |name, acts| Scenario {
        name,
        windows_only: false,
        acts,
    };
    let mut empty_guid = vec![
        r(&["init"]),
        r(&["claim", "add", "Fact", "--origin", "user"]),
        Act::Changes,
    ];
    for empty in EMPTY_GUIDS {
        empty_guid.extend([
            r(&["claim", "show", empty]),
            r(&["claim", "invalidate", empty, "--reason", "r", "--json"]),
            r(&[
                "evidence",
                "add",
                empty,
                "execution",
                "ref",
                "--lineage",
                "l",
                "--origin",
                "user",
            ]),
            r(&["evidence", "show", empty, "--json"]),
            r(&["retex", "show", empty]),
        ]);
    }
    empty_guid.extend([
        r(&[
            "evidence",
            "add",
            "{1}",
            "execution",
            "ref",
            "--lineage",
            "l",
            "--origin",
            "user",
            "--source",
            EMPTY_GUIDS[0],
        ]),
        r(&[
            "evidence",
            "add",
            "{1}",
            "execution",
            "ref",
            "--lineage",
            "l",
            "--origin",
            "user",
            "--source",
            EMPTY_GUIDS[5],
        ]),
        Act::Changes,
    ]);
    let mut fuzz = vec![r(&["init"]), Act::Changes];
    for text in fuzz_texts() {
        fuzz.push(r(&["claim", "show", text, "--json"]));
    }
    fuzz.push(Act::Changes);
    let mut enums = vec![r(&["init"]), Act::Mkdir("c"), Act::Changes];
    for origin in [
        "USER",
        "uSeR",
        "u_s-e_r",
        "_user_",
        "-user-",
        "Llm",
        "L-L-M",
        "uſer",
        "uѕer",
        "ｕｓｅｒ",
        "user ",
        " user",
        "us er",
        "user,llm",
        "user|llm",
        "user\t",
        "1",
        "0",
        "-1",
        "",
        "-",
        "_",
        "--",
        "robot",
        "usér",
        "Üser",
    ] {
        enums.push(r(&["claim", "add", "x", "--origin", origin]));
    }
    for category in [
        "SEMANTIC",
        "sem_antic",
        "Episodic",
        "pro-ce-dural",
        "facts",
        "3",
        "semantic,episodic",
        " semantic",
        "ſemantic",
    ] {
        enums.push(r(&[
            "claim",
            "add",
            "x",
            "--origin",
            "user",
            "--category",
            category,
        ]));
    }
    for volatility in [
        "STABLE",
        "vol_atile",
        "Evolving",
        "0",
        "fast",
        "stable,volatile",
    ] {
        enums.push(r(&[
            "claim",
            "add",
            "x",
            "--origin",
            "user",
            "--volatility",
            volatility,
        ]));
    }
    enums.push(r(&["claim", "add", "Target", "--origin", "user", "--json"]));
    let target = enums.len() - 1;
    let target_ref: &'static str = format!("{{{target}}}").leak();
    for kind in [
        "SOURCE-CODE",
        "sourcecode",
        "SourceCode",
        "source code",
        "ſource_code",
        "7",
        "LLM_assertion",
        "user-assertion",
        "",
    ] {
        enums.push(r(&[
            "evidence",
            "add",
            target_ref,
            kind,
            "ref",
            "--lineage",
            "l",
            "--origin",
            "user",
        ]));
    }
    for kind in [
        "CLIENT",
        "c-l-i-e-n-t",
        "clıent",
        "Client ",
        "1",
        "Workspace",
        "tas\u{212a}",
        "",
    ] {
        enums.push(r(&["scope", "add", "c", kind, "workspace", "c"]));
    }
    enums.extend([Act::Changes, r(&["claim", "list"]), r(&["scope", "list"])]);
    vec![
        s(
            "argument_count",
            vec![
                r(&["init"]),
                Act::Changes,
                // `ArgumentCountIsAcceptedAtTheLimitAndRejectedBeyond`: at 128 the line is
                // parsed (init refuses the extra positionals); at 129 it is not.
                Act::Run("", padded(&["--json", "init"], 128)),
                Act::Run("", padded(&["--json", "init"], 129)),
                Act::Run("", padded(&["init"], 128)),
                Act::Run("", padded(&["status", "--json"], 129)),
                Act::Run("", padded(&["claim", "add", "--origin", "user"], 128)),
                Act::Changes,
            ],
        ),
        s(
            "argument_length",
            vec![
                r(&["init"]),
                Act::Mkdir("c"),
                Act::Changes,
                // `ArgumentLengthIsRejectedByTheCliBeforeTheScopeService`.
                r(&[
                    "--json",
                    "scope",
                    "add",
                    "x",
                    "client",
                    "workspace",
                    long("a", 16_385),
                ]),
                r(&[
                    "scope",
                    "add",
                    long("a", 16_385),
                    "client",
                    "workspace",
                    "c",
                ]),
                r(&[
                    "scope",
                    "add",
                    long("a", 16_384),
                    "client",
                    "workspace",
                    "c",
                ]),
                r(&["scope", "add", "c", "client", long("w", 16_384), "c"]),
                r(&["claim", "add", long("a", 16_385), "--origin", "user"]),
                r(&["claim", "add", long("😀", 8_193), "--origin", "user"]),
                r(&[
                    "claim",
                    "add",
                    "x",
                    "--origin",
                    "user",
                    "--actor",
                    long("a", 16_385),
                ]),
                r(&[
                    "claim",
                    "add",
                    "x",
                    "--origin",
                    "user",
                    "--key",
                    long("k", 16_385),
                ]),
                r(&["claim", "add", "x", "--origin", long("u", 16_385)]),
                Act::Changes,
                r(&["claim", "add", long("a", 16_384), "--origin", "user"]),
                r(&[
                    "claim",
                    "add",
                    long("😀", 8_192),
                    "--origin",
                    "user",
                    "--json",
                ]),
                r(&["claim", "add", long("é", 16_384), "--origin", "user"]),
                r(&[
                    "claim",
                    "add",
                    "x",
                    "--origin",
                    "user",
                    "--actor",
                    long("a", 16_384),
                    "--key",
                    long("k", 15_000),
                ]),
                Act::Changes,
                r(&[
                    "evidence",
                    "add",
                    "{14}",
                    "source_code",
                    long("r", 16_385),
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{14}",
                    "source_code",
                    "r",
                    "--lineage",
                    long("l", 16_385),
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{14}",
                    "source_code",
                    long("r", 16_384),
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "retex",
                    "add",
                    "--event",
                    "e",
                    "--impact",
                    "i",
                    "--next",
                    long("n", 16_385),
                    "--origin",
                    "user",
                ]),
                r(&[
                    "retex",
                    "add",
                    "--event",
                    long("e", 16_384),
                    "--impact",
                    "i",
                    "--next",
                    "n",
                    "--origin",
                    "user",
                ]),
                r(&["claim", "invalidate", "{14}", "--reason", long("r", 16_385)]),
                r(&["claim", "invalidate", "{14}", "--reason", long("r", 16_384)]),
                r(&["claim", "show", "{14}"]),
                Act::Changes,
                r(&["status"]),
            ],
        ),
        s("soft_enums", enums),
        s("empty_guid", empty_guid),
        s(
            "writes",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Fact", "--origin", "user", "--key", "k"]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "source_code",
                    "src/a.rs",
                    "--lineage",
                    "repo",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user",
                ]),
                Act::Mkdir("c"),
                Act::Changes,
                // Reads open the store read-only and write nothing.
                r(&["root"]),
                r(&["status"]),
                r(&["status", "--json"]),
                r(&["scope", "current"]),
                r(&["scope", "list"]),
                r(&["scope", "tree"]),
                r(&["claim", "list"]),
                r(&["claim", "show", "{1}"]),
                r(&["evidence", "list"]),
                r(&["evidence", "show", "{2}"]),
                r(&["retex", "list"]),
                r(&["retex", "show", "{3}"]),
                rin("c", &["status"]),
                Act::Changes,
                // Refused writes, and lines that look like writes, write nothing either.
                r(&["claim", "add", "x"]),
                r(&["claim", "add", "x", "--origin", "robot"]),
                r(&["claim", "add", "", "--origin", "user"]),
                r(&["claim", "invalidate", "{1}"]),
                r(&["claim", "invalidate", "bad", "--reason", "r"]),
                r(&["claim", "invalidate", UNKNOWN, "--reason", "r"]),
                r(&[
                    "evidence",
                    "add",
                    "bad",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&["retex", "add", "--event", "e", "--origin", "user"]),
                r(&["scope", "add", "c", "planet", "workspace", "c"]),
                r(&["scope", "add", "c", "client", "workspace", "missing"]),
                r(&["frob", "add"]),
                r(&["frob", "invalidate", "x"]),
                r(&["claim", "frob"]),
                r(&["export", "x"]),
                r(&["status", "add"]),
                r(&["scope", "invalidate"]),
                Act::Changes,
                // A replayed key changes nothing; each real write changes the database.
                r(&["claim", "add", "Fact", "--origin", "user", "--key", "k"]),
                Act::Changes,
                r(&["claim", "add", "Other", "--origin", "llm"]),
                Act::Changes,
                r(&["claim", "invalidate", "{1}", "--reason", "old"]),
                Act::Changes,
                r(&["export"]),
                Act::Changes,
                Act::Remove(".sermofur/records/{1}.md"),
                Act::Changes,
                r(&["export", "--json"]),
                Act::Changes,
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                Act::Changes,
            ],
        ),
        s(
            "invalid_arguments_without_mutation",
            vec![
                r(&["init"]),
                Act::Changes,
                // The cases of `InvalidArgumentsAreRejectedWithoutMutation`.
                r(&[
                    "--json", "claim", "add", "x", "--origin", "user", "--scope", "someone",
                ]),
                r(&["--json", "status", "--unknown", "value"]),
                r(&["--json", "claim", "show", "not-an-id"]),
                r(&["--json", "scope", "add", "x", "99", "workspace", "x"]),
                r(&["--json", "scope", "add", "Bad", "client", "workspace", "x"]),
                r(&["--json", "claim", "add", "x"]),
                r(&["--json", "claim", "add", "--", "x", "--origin", "user"]),
                r(&["--json", "claim", "add", "x", "--origin", "machine"]),
                // `MissingOriginIsRejectedEvenForDecisionsAndWritesNothing`.
                r(&[
                    "--json",
                    "claim",
                    "add",
                    "always review before commit",
                    "--category",
                    "decisions",
                ]),
                Act::Changes,
            ],
        ),
        s(
            "arguments_as_text",
            vec![
                r(&["init"]),
                // `HelpWinsOverVersionAndShortcutsAreTextAfterTheSeparator`.
                r(&["--json", "claim", "add", "--origin", "user", "--", "-h"]),
                r(&["--json", "claim", "add", "--origin", "user", "--", "-v"]),
                // `DoubleDashMakesFollowingArgumentsPositional`.
                r(&[
                    "--json",
                    "claim",
                    "add",
                    "--origin",
                    "user",
                    "--",
                    "--text starting with two dashes",
                ]),
                r(&["--json", "claim", "add", "--origin", "user", "--", "--help"]),
                // `ShortcutGivenAsAnOptionValueStaysAValue`.
                r(&[
                    "--json", "claim", "add", "x", "--origin", "user", "--actor", "-v",
                ]),
                r(&[
                    "--json", "claim", "add", "x", "--origin", "user", "--key", "-v",
                ]),
                r(&[
                    "--json", "claim", "add", "y", "--origin", "user", "--key", "-h",
                ]),
                // `AccentsStayLiteralWhileBackticksCannotCloseTheJsonBlock`.
                r(&[
                    "--json",
                    "claim",
                    "add",
                    "hypothèse ``` <b>fin</b>",
                    "--origin",
                    "user",
                ]),
                // `RealProcessWritesUtf8StdoutWithNonAsciiTextAndUtf8StderrWithoutBom`.
                r(&[
                    "claim",
                    "add",
                    "œuvre 漢 hypothèse",
                    "--origin",
                    "user",
                    "--json",
                ]),
                r(&["claim", "show", "{9}", "--json"]),
                r(&[
                    "claim",
                    "add",
                    "œuvre 漢 hypothèse",
                    "--origin",
                    "user",
                    "--actor",
                    " ",
                    "--json",
                ]),
            ],
        ),
        s(
            "cross_client",
            vec![
                // `CrossClientErrorDoesNotRevealContentAndStatusIsFiltered`.
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                rin(
                    "a",
                    &["claim", "add", "secret client A", "--origin", "user"],
                ),
                Act::Changes,
                rin("b", &["claim", "show", "{5}", "--json"]),
                rin(
                    "b",
                    &["claim", "invalidate", "{5}", "--reason", "r", "--json"],
                ),
                rin(
                    "b",
                    &[
                        "evidence",
                        "add",
                        "{5}",
                        "execution",
                        "ref",
                        "--lineage",
                        "l",
                        "--origin",
                        "user",
                    ],
                ),
                rin("b", &["status", "--json"]),
                rin("b", &["scope", "list", "--json"]),
                rin("b", &["claim", "list"]),
                Act::Changes,
            ],
        ),
        s("parser_fuzz", fuzz),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-3-5", scenarios());
}

/// The programs that race: eight Rust processes, or four Rust and four .NET processes when
/// `SMF_ORACLE` names the oracle.
fn racers() -> Vec<String> {
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    match std::env::var("SMF_ORACLE") {
        Ok(oracle) => [rust, oracle].iter().cycle().take(8).cloned().collect(),
        Err(_) => vec![rust; 8],
    }
}

/// Runs each command line with its program at the same time, from `root`.
fn race(root: &Path, lines: Vec<(String, Vec<String>)>) -> Vec<common::Captured> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = lines
            .iter()
            .map(|(program, line)| scope.spawn(move || run(program, line, root)))
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("the process ran"))
            .collect()
    })
}

fn init(root: &Path) {
    let out = run(env!("CARGO_BIN_EXE_smf"), &arguments(&["init"]), root);
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
}

fn count(root: &Path, values: &[&str]) -> usize {
    let out = run(env!("CARGO_BIN_EXE_smf"), &arguments(values), root);
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    serde_json::from_str::<Value>(&out.stdout)
        .expect("JSON")
        .as_array()
        .expect("array")
        .len()
}

/// Port of `ConcurrentHostProcessesPersistAllClaims`: eight processes add a claim each.
#[test]
fn concurrent_processes_persist_all_claims() {
    let scratch = Scratch::new();
    let root = &scratch.0;
    init(root);
    let lines = racers()
        .into_iter()
        .enumerate()
        .map(|(i, program)| {
            let text = format!("process {i}");
            let key = format!("process-{i}");
            let line = arguments(&[
                "claim", "add", &text, "--origin", "user", "--key", &key, "--json",
            ]);
            (program, line)
        })
        .collect();
    for out in race(root, lines) {
        assert_eq!(out.exit_code, 0, "{}", out.stderr);
    }
    assert_eq!(count(root, &["claim", "list"]), 8);
}

/// Port of `ConcurrentSameKeyIngestionProducesOneRecordAndOneHistoryEntry`, through processes.
#[test]
fn concurrent_same_key_gives_one_record_and_one_history_entry() {
    let scratch = Scratch::new();
    let root = &scratch.0;
    init(root);
    let line = arguments(&[
        "claim", "add", "same", "--origin", "user", "--key", "same-key", "--json",
    ]);
    let lines = racers()
        .into_iter()
        .map(|program| (program, line.clone()))
        .collect();
    let ids: HashSet<String> = race(root, lines).iter().map(printed_id).collect();
    assert_eq!(ids.len(), 1, "{ids:?}");
    assert_eq!(count(root, &["claim", "list"]), 1);
    let id = ids.into_iter().next().expect("one id");
    let shown = run(
        env!("CARGO_BIN_EXE_smf"),
        &arguments(&["claim", "show", &id]),
        root,
    );
    let history = serde_json::from_str::<Value>(&shown.stdout).expect("JSON")["history"]
        .as_array()
        .expect("history")
        .len();
    assert_eq!(history, 1);
}
