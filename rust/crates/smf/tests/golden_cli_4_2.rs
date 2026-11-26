//! Differential test of lot 4.2: `recall` from the command line (options, output, exit codes,
//! no write), against `RecallTests` (`LimitIsBetweenOneAndThree`,
//! `RecallRunsOnTheReadOnlyStoreOfARealProcess`) and `KnowledgeCommands.Recall`, on the harness
//! of `common`. The ranking itself is compared in `sermofur-core/tests/recall_golden.rs`. To
//! record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_4_2
//! ```

mod common;

use std::fs;

use common::{Act, Scenario, Scratch, arguments, printed_id, r, rin, run};

fn scenarios() -> Vec<Scenario> {
    let s = |name, acts| Scenario {
        name,
        windows_only: false,
        acts,
    };
    let mut basics = vec![r(&["init"])];
    for n in 0..5 {
        let text: &'static str = format!("le cache {n} se vide au déploiement").leak();
        basics.push(r(&["claim", "add", text, "--origin", "user"]));
    }
    basics.extend([
        r(&[
            "retex",
            "add",
            "--event",
            "Cache vidé",
            "--impact",
            "Lenteur",
            "--next",
            "Préchauffer le cache",
            "--origin",
            "user",
        ]),
        Act::Changes,
        r(&["recall", "CACHE déploiement"]),
        r(&["recall", "CACHE déploiement", "--json"]),
        r(&["recall", "cache", "--limit", "1"]),
        r(&["--json", "recall", "--limit", "2", "cache"]),
        r(&["recall", "préchauffer"]),
        r(&["recall", "introuvable"]),
        r(&["recall", "\"cache\" OR *"]),
        r(&["recall", "NEAR(cache, x) AND col:text"]),
        Act::Changes,
    ]);
    let mut limits = vec![
        r(&["init"]),
        r(&["claim", "add", "cache chaud", "--origin", "user"]),
        Act::Changes,
    ];
    for limit in [
        "0",
        "4",
        "x",
        "-1",
        "+1",
        " 1",
        "1 ",
        "01",
        "0003",
        "2",
        "1.0",
        "１",
        "2147483647",
        "2147483648",
        "99999999999999999999",
        "",
    ] {
        limits.push(r(&["recall", "cache", "--limit", limit]));
        limits.push(r(&["recall", "cache", "--limit", limit, "--json"]));
    }
    limits.extend([
        r(&["recall", "cache", "--limit"]),
        r(&["recall", "cache", "--limit", "1", "--limit", "2"]),
        Act::Changes,
    ]);
    vec![
        s("recall_basics", basics),
        s("recall_limits", limits),
        s(
            "recall_arguments",
            vec![
                r(&["init"]),
                r(&["claim", "add", "cache chaud", "--origin", "user"]),
                Act::Changes,
                r(&["recall"]),
                r(&["recall", "--json"]),
                r(&["recall", "cache", "chaud"]),
                r(&["recall", "cache", "--text", "x"]),
                r(&["recall", "cache", "--origin", "user", "--json"]),
                r(&["recall", "cache", "--contradicts"]),
                r(&["recall", ""]),
                r(&["recall", "", "--json"]),
                r(&["recall", "   "]),
                r(&["recall", "!? \" --"]),
                r(&["recall", "!? \" --", "--json"]),
                r(&["recall", "", "--limit", "x"]),
                r(&["recall", "", "--limit", "0"]),
                r(&["recall", "!?", "--limit", "0"]),
                r(&["recall", "!?", "--limit", "x"]),
                r(&["recall", "--", "--limit"]),
                r(&["recall", "add"]),
                r(&["recall", "show", "x"]),
                Act::Changes,
            ],
        ),
        s(
            "recall_scopes",
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                Act::Mkdir("a/p"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                rin("a", &["scope", "add", "p", "project", "a", "a/p"]),
                rin(
                    "a",
                    &[
                        "claim",
                        "add",
                        "migration de la base postgres",
                        "--origin",
                        "user",
                    ],
                ),
                rin(
                    "a",
                    &[
                        "claim",
                        "add",
                        "postgres tourne en version 16",
                        "--origin",
                        "user",
                    ],
                ),
                rin(
                    "a/p",
                    &[
                        "claim",
                        "add",
                        "la migration se fait la nuit",
                        "--origin",
                        "user",
                    ],
                ),
                r(&["claim", "add", "postgres partout", "--origin", "user"]),
                rin("a", &["recall", "migration postgres"]),
                rin(
                    "b",
                    &[
                        "claim",
                        "add",
                        "postgres postgres migration",
                        "--origin",
                        "user",
                    ],
                ),
                rin("a", &["recall", "migration postgres"]),
                rin("b", &["recall", "migration postgres"]),
                rin("a/p", &["recall", "migration postgres"]),
                r(&["recall", "migration postgres"]),
                rin(
                    "a",
                    &[
                        "evidence",
                        "add",
                        "{8}",
                        "source_code",
                        "db.cs",
                        "--lineage",
                        "code",
                        "--origin",
                        "user",
                    ],
                ),
                rin("a", &["recall", "migration postgres"]),
                rin("a", &["claim", "invalidate", "{7}", "--reason", "migrée"]),
                rin("a", &["recall", "migration postgres"]),
                rin("a", &["recall", "migration postgres", "--limit", "1"]),
                rin("a", &["recall", "base", "--limit", "1"]),
                Act::Changes,
            ],
        ),
        s(
            "recall_without_instance",
            vec![r(&["recall", "cache"]), r(&["recall", "cache", "--json"])],
        ),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-4-2", scenarios());
}

/// Cross-read: one tool writes scopes, claims, evidence, an invalidation and a RETEX (and, for
/// the .NET tool, a source through `source add`, lot 5); both tools then answer the same
/// recalls with the same bytes.
#[test]
fn each_tool_recalls_the_memory_of_the_other() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("cross-read skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    for (writer, reader) in [(&rust, &oracle), (&oracle, &rust)] {
        let scratch = Scratch::new();
        let root = &scratch.0;
        let w = |values: &[&str], cwd: &str| run(writer, &arguments(values), &root.join(cwd));
        assert_eq!(w(&["init"], "").exit_code, 0);
        fs::create_dir_all(root.join("c")).expect("folder");
        assert_eq!(
            w(&["scope", "add", "c", "client", "workspace", "c"], "").exit_code,
            0
        );
        let stale = printed_id(&w(
            &["claim", "add", "le cache redis redis", "--origin", "user"],
            "",
        ));
        let lifted = printed_id(&w(
            &[
                "claim",
                "add",
                "le cache se vide au redémarrage",
                "--origin",
                "user",
            ],
            "c",
        ));
        printed_id(&w(
            &["claim", "add", "le cache se vide", "--origin", "llm"],
            "c",
        ));
        printed_id(&w(
            &[
                "evidence",
                "add",
                &lifted,
                "source_code",
                "cache.rs",
                "--lineage",
                "code",
                "--origin",
                "user",
            ],
            "c",
        ));
        printed_id(&w(
            &[
                "retex",
                "add",
                "--event",
                "Cache vidé",
                "--impact",
                "Lenteur",
                "--next",
                "Préchauffer",
                "--origin",
                "user",
            ],
            "c",
        ));
        assert_eq!(
            w(
                &["claim", "invalidate", &stale, "--reason", "redis retiré"],
                ""
            )
            .exit_code,
            0
        );
        if *writer == oracle {
            let filler = vec!["x".repeat(1500); 3].join("\n\n");
            fs::write(
                root.join("c").join("guide.md"),
                format!("{filler}\n\nLe préchauffage du cache prend dix minutes.\n\n{filler}"),
            )
            .expect("source file");
            printed_id(&w(&["source", "add", "guide.md", "--origin", "user"], "c"));
        }
        let reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["recall", "cache vide"], "c"),
            (vec!["recall", "cache vide", "--limit", "1"], "c"),
            (vec!["recall", "cache"], ""),
            (vec!["recall", "redis", "--json"], ""),
            (vec!["recall", "redis", "--limit", "1"], "c"),
            (vec!["recall", "prechauffage"], "c"),
            (vec!["recall", "préchauffage", "--limit", "2"], ""),
            (vec!["recall", "!?"], "c"),
        ];
        for (values, cwd) in reads {
            let by_writer = w(&values, cwd);
            let by_reader = run(reader, &arguments(&values), &root.join(cwd));
            assert_eq!(
                (by_reader.exit_code, &by_reader.stdout, &by_reader.stderr),
                (by_writer.exit_code, &by_writer.stdout, &by_writer.stderr),
                "{reader} recalls {values:?} written by {writer}"
            );
        }
        eprintln!("cross-read ok: {reader} recalls the memory written by {writer}");
    }
}
