//! Differential test of lot 4.3: `challenge` from the command line (`--text`, identifier forms,
//! order of the refusals, output, exit codes, no write), against `ChallengeTests`
//! (`ChallengeAndEvidenceOptionsWorkThroughTheCli`,
//! `ClaimOfAnotherClientAnswersLikeAnUnknownIdentifier`,
//! `EvidenceCitingASourceMovedToANarrowerScopeIsUnavailableWithoutItsPath`) and
//! `KnowledgeCommands.Challenge`, on the harness of `common`. The signals themselves are compared
//! in `sermofur-core/tests/challenge_golden.rs`. To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_4_3
//! ```

mod common;

use std::fs;

use common::{Act, Scenario, Scratch, UNKNOWN, arguments, printed_id, r, rin, run};

fn scenarios() -> Vec<Scenario> {
    let s = |name, acts| Scenario {
        name,
        windows_only: false,
        acts,
    };
    vec![
        s(
            "challenge_basics",
            vec![
                r(&["init"]),
                r(&["claim", "add", "le port est 8080", "--origin", "user"]),
                r(&[
                    "--json",
                    "evidence",
                    "add",
                    "{1}",
                    "human_observation",
                    "netstat",
                    "--lineage",
                    "obs",
                    "--origin",
                    "user",
                    "--contradicts",
                ]),
                r(&[
                    "claim",
                    "add",
                    "le port 8080 est ouvert",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{3}",
                    "llm_assertion",
                    "le port est \"9090\"",
                    "--lineage",
                    "llm",
                    "--origin",
                    "llm",
                    "--actor",
                    "claude",
                    "--contradicts",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "source_code",
                    "app.json",
                    "--lineage",
                    "code",
                    "--origin",
                    "user",
                ]),
                // One contradiction per claim: evidence is listed by random identifier.
                Act::Changes,
                r(&["--json", "challenge", "{1}"]),
                r(&["challenge", "{1}"]),
                r(&["challenge", "{3}"]),
                r(&["--json", "challenge", "--text", "port 8080"]),
                r(&["challenge", "--text", "port 8080"]),
                r(&["challenge", "--text", "introuvable"]),
                r(&["challenge", "--text", "\"port\" OR *"]),
                r(&["claim", "invalidate", "{3}", "--reason", "fermé"]),
                Act::Changes,
                r(&["challenge", "{3}"]),
                r(&["challenge", "{1}"]),
                r(&["challenge", "--text", "port ouvert"]),
                r(&["claim", "add", "!? \" --", "--origin", "user"]),
                Act::Changes,
                r(&["challenge", "{19}"]),
                r(&["challenge", "{19}", "--json"]),
                Act::Changes,
            ],
        ),
        s(
            "challenge_arguments",
            vec![
                r(&["init"]),
                r(&["claim", "add", "cache chaud", "--origin", "user"]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "bench",
                    "--lineage",
                    "b",
                    "--origin",
                    "user",
                ]),
                Act::Changes,
                r(&["challenge"]),
                r(&["challenge", "--json"]),
                r(&["challenge", "x"]),
                r(&["challenge", "x", "--json"]),
                r(&["challenge", "00000000-0000-0000-0000-000000000000"]),
                r(&["challenge", UNKNOWN]),
                r(&["challenge", "{1:N}"]),
                r(&["challenge", "{1:B}"]),
                r(&["challenge", "{1:U}"]),
                r(&["challenge", "{1:X}"]),
                r(&["challenge", "{2}"]),
                r(&["challenge", "{1}", "extra"]),
                r(&["challenge", "x", "extra"]),
                r(&["challenge", "{1}", "--limit", "1"]),
                r(&["challenge", "x", "--limit", "1"]),
                r(&["challenge", "{1}", "--origin", "user"]),
                r(&["challenge", "{1}", "--contradicts"]),
                r(&["challenge", "--text"]),
                r(&["challenge", "--text", "--json"]),
                r(&["challenge", "--text", ""]),
                r(&["challenge", "--text", "", "--json"]),
                r(&["challenge", "--text", "   "]),
                r(&["challenge", "--text", "!?"]),
                r(&["challenge", "--text", "!?", "--json"]),
                r(&["challenge", "--text", "cache", "{1}"]),
                r(&["challenge", "{1}", "--text", "cache"]),
                r(&["challenge", "--text", "", "extra"]),
                r(&["challenge", "--text", "", "--limit", "1"]),
                r(&["challenge", "--text", "a", "--text", "b"]),
                r(&["challenge", "--", "--text"]),
                r(&["challenge", "--", "{1}"]),
                r(&["challenge", "--text", "-h"]),
                r(&["challenge", "--text", "--", "cache"]),
                Act::Changes,
            ],
        ),
        s(
            "challenge_scopes",
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                Act::Mkdir("c"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                rin("a", &["claim", "add", "secret de a", "--origin", "user"]),
                rin("b", &["challenge", "{6}"]),
                rin("b", &["challenge", UNKNOWN]),
                rin("b", &["challenge", "--text", "secret"]),
                rin("a", &["challenge", "--text", "secret"]),
                r(&["challenge", "{6}"]),
                rin("a", &["challenge", "{6}"]),
                // A cited source moved to a narrower scope by `scope add`.
                Act::Source("workspace", "c/plan.md", true),
                r(&["claim", "add", "le plan existe", "--origin", "user"]),
                r(&[
                    "evidence",
                    "add",
                    "{14}",
                    "project_decision",
                    "plan",
                    "--lineage",
                    "plan",
                    "--origin",
                    "user",
                    "--source",
                    "{13}",
                ]),
                r(&["challenge", "{14}"]),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                Act::Changes,
                r(&["challenge", "{14}"]),
                rin("c", &["challenge", "{14}"]),
                rin("c", &["challenge", "{13}"]),
                rin("c", &["challenge", "--text", "plan"]),
                Act::Changes,
            ],
        ),
        s(
            "challenge_without_instance",
            vec![
                r(&["challenge", UNKNOWN]),
                r(&["challenge", "--text", "x", "--json"]),
                r(&["challenge"]),
            ],
        ),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-4-3", scenarios());
}

/// Cross-read: one tool writes scopes, claims, contradicting evidence (one of LLM origin), an
/// invalidation and, for the .NET tool, a source through `source add`, cited by evidence, then
/// changed and reindexed, and another one removed and reindexed (lot 5); both tools then answer
/// the same challenges with the same bytes.
#[test]
fn each_tool_challenges_the_memory_of_the_other() {
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
        let claim = printed_id(&w(
            &[
                "claim",
                "add",
                "le build passe en 3 min",
                "--origin",
                "user",
            ],
            "c",
        ));
        let other = printed_id(&w(
            &["claim", "add", "le build passe vite", "--origin", "user"],
            "c",
        ));
        printed_id(&w(
            &["claim", "add", "le build est lent", "--origin", "user"],
            "",
        ));
        for (kind, reference, origin) in [
            ("source_code", "pipeline.yml", "user"),
            ("human_observation", "run 42 : 9 min", "user"),
            ("llm_assertion", "il prend \"10 min\"", "llm"),
        ] {
            let mut values = vec![
                "evidence",
                "add",
                &claim,
                kind,
                reference,
                "--lineage",
                kind,
                "--origin",
                origin,
            ];
            if kind != "source_code" {
                values.push("--contradicts");
            }
            printed_id(&w(&values, "c"));
        }
        assert_eq!(
            w(&["claim", "invalidate", &other, "--reason", "mesuré"], "c").exit_code,
            0
        );
        if *writer == oracle {
            for name in ["adr.md", "gone.md"] {
                fs::write(root.join("c").join(name), "le build passe en 3 min").expect("source");
                let source = printed_id(&w(&["source", "add", name, "--origin", "user"], "c"));
                printed_id(&w(
                    &[
                        "evidence",
                        "add",
                        &claim,
                        "project_decision",
                        name,
                        "--lineage",
                        "adr",
                        "--origin",
                        "user",
                        "--source",
                        &source,
                    ],
                    "c",
                ));
            }
            fs::write(root.join("c").join("adr.md"), "le build passe en 9 min").expect("source");
            fs::remove_file(root.join("c").join("gone.md")).expect("removed");
            assert_eq!(w(&["source", "reindex"], "c").exit_code, 0);
        }
        let reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["challenge", &claim], "c"),
            (vec!["challenge", &claim, "--json"], "c"),
            (vec!["challenge", &other], "c"),
            (vec!["challenge", &claim], ""),
            (vec!["challenge", "--text", "build lent"], "c"),
            (vec!["challenge", "--text", "build"], ""),
            (vec!["challenge", "--text", "!?"], "c"),
        ];
        for (values, cwd) in reads {
            let by_writer = w(&values, cwd);
            if *writer == oracle && values == ["challenge", claim.as_str()] && cwd == "c" {
                for signal in ["contradiction", "source_changed", "source_unavailable"] {
                    assert!(by_writer.stdout.contains(signal), "{signal}");
                }
            }
            let by_reader = run(reader, &arguments(&values), &root.join(cwd));
            assert_eq!(
                (by_reader.exit_code, &by_reader.stdout, &by_reader.stderr),
                (by_writer.exit_code, &by_writer.stdout, &by_writer.stderr),
                "{reader} challenges {values:?} written by {writer}"
            );
        }
        eprintln!("cross-read ok: {reader} challenges the memory written by {writer}");
    }
}
