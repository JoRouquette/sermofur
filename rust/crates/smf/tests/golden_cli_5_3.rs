//! Differential test of lot 5.3: `index rebuild` from the command line (counts of visible
//! objects, history under the system actor, search entries, projections, refusals and their
//! order, exit codes), against `SourceTests` (`IndexRebuildRestoresADesynchronizedIndex` and the
//! command-line form of the other rebuild cases) and `KnowledgeCommands.Index`, on the harness of
//! `common`. The rebuild itself is compared in `sermofur-core/tests/index_rebuild_golden.rs`.
//! Also the context folder normalized as `Path.GetFullPath` (`--path "sub "`) and an NT path
//! given to `source add`, both on Windows only. To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_5_3
//! ```

mod common;

use std::fs;

use common::{Act, Scenario, Scratch, arguments, printed_id, r, rin, run};

fn scenarios() -> Vec<Scenario> {
    let s = |name, windows_only, acts| Scenario {
        name,
        windows_only,
        acts,
    };
    vec![
        s(
            "index_rebuild",
            false,
            vec![
                r(&["init"]),
                r(&["index", "rebuild"]),
                r(&["claim", "add", "le cache chaud", "--origin", "user"]),
                Act::File("doc.md", "cache chaud"),
                r(&["source", "add", "doc.md", "--origin", "user"]),
                Act::Changes,
                r(&["index", "rebuild"]),
                r(&["--json", "index", "rebuild"]),
                Act::Changes,
                Act::File("doc.md", "cache tiède"),
                r(&["index", "rebuild"]),
                Act::Changes,
                Act::History(4),
                r(&["recall", "tiede"]),
                Act::Remove("doc.md"),
                r(&["index", "rebuild"]),
                r(&["recall", "cache"]),
                r(&["source", "show", "{4}"]),
                Act::File("doc.md", "cache revenu"),
                r(&["index", "rebuild"]),
                Act::History(4),
                r(&["status"]),
                Act::Changes,
            ],
        ),
        s(
            "index_arguments",
            false,
            vec![
                r(&["init"]),
                r(&["index"]),
                r(&["index", "--json"]),
                r(&["index", "frob"]),
                r(&["index", "REBUILD"]),
                r(&["index", "frob", "extra"]),
                r(&["index", "rebuild", "extra"]),
                r(&["index", "rebuild", "--actor", "x"]),
                r(&["index", "rebuild", "--origin"]),
                r(&["index", "rebuild", "--json", "--json"]),
                r(&["index", "--", "rebuild"]),
                r(&["index", "rebuild", "--path", "."]),
                Act::Changes,
            ],
        ),
        s(
            "index_scopes",
            false,
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                Act::File("a/a.md", "texte de a"),
                Act::File("b/b.md", "texte de b"),
                rin("a", &["source", "add", "a.md", "--origin", "user"]),
                rin("b", &["source", "add", "b.md", "--origin", "user"]),
                rin("b", &["claim", "add", "claim de b", "--origin", "user"]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user",
                ]),
                Act::File("b/b.md", "texte de b modifié"),
                rin("a", &["index", "rebuild"]),
                Act::Changes,
                Act::History(8),
                rin("b", &["recall", "modifie"]),
                rin("b", &["index", "rebuild"]),
                r(&["index", "rebuild"]),
                Act::File("a/a.md", "texte de a modifié"),
                rin("b", &["index", "rebuild", "--json"]),
                rin("a", &["source", "list"]),
                Act::Changes,
            ],
        ),
        s(
            "index_desynchronized",
            false,
            vec![
                r(&["init"]),
                Act::File("forged.md", "texte forgé"),
                // Written through the store: indexed, without search entries, another hash.
                Act::Source("workspace", "forged.md", true),
                // Missing and still missing: unchanged. One change only, so that the order of
                // the random identifiers never decides the history sequence.
                Act::Source("workspace", "absent.md", false),
                r(&["recall", "forge"]),
                r(&["index", "rebuild"]),
                Act::Changes,
                Act::History(2),
                Act::History(3),
                r(&["recall", "forge"]),
                r(&["source", "list"]),
            ],
        ),
        s(
            "index_without_instance",
            false,
            vec![
                r(&["index", "rebuild"]),
                r(&["index", "rebuild", "--json"]),
                r(&["index"]),
            ],
        ),
        // `Path.GetFullPath` trims the trailing space of a segment: the context folder is `sub`.
        s(
            "context_path_trailing_space_windows",
            true,
            vec![
                r(&["init"]),
                Act::Mkdir("sub"),
                Act::File("sub/doc.md", "dans sub"),
                r(&["--path", "sub ", "scope", "current"]),
                r(&["scope", "add", "s", "client", "workspace", "sub"]),
                r(&["--path", "sub ", "scope", "current"]),
                r(&[
                    "--path", "sub ", "source", "add", "doc.md", "--origin", "user",
                ]),
                r(&["--path", "sub. ", "source", "list"]),
                r(&["--path", "sub ", "status"]),
                r(&["--path", "sub ", "index", "rebuild"]),
                Act::Changes,
            ],
        ),
        // An NT path is fully qualified for .NET and kept as typed: outside the instance.
        s(
            "source_add_nt_path_windows",
            true,
            vec![
                r(&["init"]),
                Act::File("doc.md", "contenu"),
                r(&["source", "add", r"\??\<root>\doc.md", "--origin", "user"]),
                r(&[
                    "source",
                    "add",
                    r"\??\<root>\missing.md",
                    "--origin",
                    "user",
                ]),
                r(&["source", "add", r"\\?\<root>\doc.md", "--origin", "user"]),
                r(&["--path", r"\??\<root>", "status"]),
                r(&["--path", r"\??\<root>"]),
                r(&["source", "list"]),
                Act::Changes,
            ],
        ),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-5-3", scenarios());
}

/// Cross-read: one tool declares a claim and sources, then a file changes and another goes;
/// the other tool rebuilds the index, and both tools then answer `recall`, `source list`,
/// `source show` and `status` with the same bytes.
#[test]
fn each_tool_reads_the_index_rebuilt_by_the_other() {
    let Ok(oracle) = std::env::var("SMF_ORACLE") else {
        eprintln!("cross-read skipped: SMF_ORACLE is not set");
        return;
    };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    for (writer, rebuilder) in [(&rust, &oracle), (&oracle, &rust)] {
        let scratch = Scratch::new();
        let root = &scratch.0;
        let w = |values: &[&str], cwd: &str| run(writer, &arguments(values), &root.join(cwd));
        assert_eq!(w(&["init"], "").exit_code, 0);
        fs::create_dir_all(root.join("c")).expect("folder");
        assert_eq!(
            w(&["scope", "add", "c", "client", "workspace", "c"], "").exit_code,
            0
        );
        printed_id(&w(
            &[
                "claim",
                "add",
                "le build passe en 3 min",
                "--origin",
                "user",
            ],
            "c",
        ));
        let mut ids = Vec::new();
        for name in ["adr.md", "gone.md", "kept.md"] {
            fs::write(root.join("c").join(name), "le build passe en 3 min").expect("source");
            ids.push(printed_id(&w(
                &["source", "add", name, "--origin", "user"],
                "c",
            )));
        }
        fs::write(root.join("c").join("adr.md"), "le build passe en 9 min").expect("changed");
        fs::remove_file(root.join("c").join("gone.md")).expect("removed");
        let rebuilt = run(
            rebuilder,
            &arguments(&["index", "rebuild"]),
            &root.join("c"),
        );
        assert_eq!(rebuilt.exit_code, 0, "{}", rebuilt.stderr);
        assert!(
            rebuilt.stdout.contains("\"changedSources\": 2"),
            "{}",
            rebuilt.stdout
        );
        let mut reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["recall", "build"], "c"),
            (vec!["recall", "9 min"], "c"),
            (vec!["recall", "3 min"], ""),
            (vec!["source", "list"], "c"),
            (vec!["status"], "c"),
        ];
        for id in &ids {
            reads.push((vec!["source", "show", id], "c"));
        }
        for (values, cwd) in &reads {
            let by_writer = w(values, cwd);
            let by_rebuilder = run(rebuilder, &arguments(values), &root.join(cwd));
            assert_eq!(
                (by_writer.exit_code, &by_writer.stdout, &by_writer.stderr),
                (
                    by_rebuilder.exit_code,
                    &by_rebuilder.stdout,
                    &by_rebuilder.stderr
                ),
                "{writer} answers {values:?} on the index rebuilt by {rebuilder}"
            );
        }
        // The writer rebuilds again what the other tool rebuilt: nothing changes.
        let again = w(&["index", "rebuild"], "c");
        assert!(
            again.stdout.contains("\"changedSources\": 0"),
            "{}",
            again.stdout
        );
        eprintln!("cross-read ok: {writer} reads the index rebuilt by {rebuilder}");
    }
}
