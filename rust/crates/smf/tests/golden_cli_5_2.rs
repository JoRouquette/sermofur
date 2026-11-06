//! Differential test of lot 5.2: `source add/list/show/reindex` from the command line (stored
//! path, refusals and their order, transitions of `reindex`, history, search entries,
//! projections, exit codes), against `SourceTests` (`SourceCommandsWorkThroughTheCli` and the
//! command-line form of the other cases) and `KnowledgeCommands.Source`, on the harness of
//! `common`. The rules themselves are compared in `sermofur-core/tests/source_service_golden.rs`.
//! To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_5_2
//! ```

mod common;

use std::fs;

use common::{Act, Scenario, Scratch, UNKNOWN, arguments, printed_id, r, rin, run};

fn latin1() -> Vec<u8> {
    b"caf\xE9".to_vec()
}

fn too_large() -> Vec<u8> {
    vec![b'a'; 1024 * 1024 + 1]
}

fn largest() -> Vec<u8> {
    vec![b'a'; 1024 * 1024]
}

fn bom() -> Vec<u8> {
    b"\xEF\xBB\xBFavec BOM".to_vec()
}

fn scenarios() -> Vec<Scenario> {
    let s = |name, windows_only, acts| Scenario {
        name,
        windows_only,
        acts,
    };
    vec![
        s(
            "source_basics",
            false,
            vec![
                r(&["init"]),
                Act::File("doc.md", "contenu"),
                r(&["--json", "source", "add", "doc.md", "--origin", "user"]),
                Act::Changes,
                r(&["source", "add", "doc.md", "--origin", "user"]),
                Act::Changes,
                r(&["--json", "source", "add", "doc.md"]),
                r(&["source", "list"]),
                r(&["source", "show", "{2}"]),
                r(&["source", "reindex"]),
                r(&["--json", "source", "reindex", "--actor", "tester"]),
                Act::Changes,
                Act::File("doc.md", "contenu modifié"),
                r(&["source", "reindex", "--actor", "tester"]),
                Act::Changes,
                r(&["source", "show", "{2}"]),
                r(&["recall", "modifie"]),
                Act::Remove("doc.md"),
                r(&["source", "reindex", "{2}"]),
                r(&["source", "reindex", "{2:N}"]),
                r(&["recall", "modifie"]),
                Act::File("doc.md", "contenu modifié"),
                r(&["source", "reindex"]),
                Act::File("doc.md", "  \n "),
                r(&["source", "reindex"]),
                Act::File("doc.md", "A\u{0}B"),
                r(&["source", "reindex"]),
                Act::Bytes("doc.md", latin1),
                r(&["source", "reindex"]),
                Act::Bytes("doc.md", too_large),
                r(&["source", "reindex"]),
                r(&["source", "add", "doc.md", "--origin", "user"]),
                Act::Changes,
                Act::File("doc.md", "texte rendu"),
                r(&[
                    "source", "add", "doc.md", "--origin", "llm", "--actor", "claude",
                ]),
                r(&[
                    "source", "add", "doc.md", "--origin", "llm", "--actor", "claude",
                ]),
                Act::History(2),
                r(&["status"]),
                Act::Changes,
            ],
        ),
        s(
            "source_files",
            false,
            vec![
                r(&["init"]),
                Act::Mkdir("sub"),
                Act::File("empty.md", "  \n "),
                Act::File("binary.md", "A\u{0}B"),
                Act::Bytes("latin1.md", latin1),
                Act::Bytes("large.md", too_large),
                Act::Bytes("largest.md", largest),
                Act::Bytes("bom.md", bom),
                Act::File("sub/long.md", "mot\n\nautre paragraphe\r\n\r\nfin"),
                r(&["source", "add", "empty.md", "--origin", "user"]),
                r(&["source", "add", "binary.md", "--origin", "user"]),
                r(&["source", "add", "latin1.md", "--origin", "user", "--json"]),
                r(&["source", "add", "large.md", "--origin", "user"]),
                r(&["source", "add", "missing.md", "--origin", "user"]),
                r(&["source", "add", "sub", "--origin", "user"]),
                r(&["source", "add", ".", "--origin", "user"]),
                r(&["source", "add", "", "--origin", "user"]),
                r(&["source", "add", "  ", "--origin", "user"]),
                r(&["source", "add", "<root>", "--origin", "user"]),
                r(&["source", "add", "../outside.md", "--origin", "user"]),
                r(&[
                    "source",
                    "add",
                    ".sermofur/instance.json",
                    "--origin",
                    "user",
                ]),
                r(&["source", "add", ".sermofur", "--origin", "user"]),
                r(&[
                    "source",
                    "add",
                    ".SERMOFUR/instance.json",
                    "--origin",
                    "user",
                ]),
                r(&["source", "add", ".Sermofur/memory.db", "--origin", "user"]),
                r(&[
                    "source",
                    "add",
                    "sub/../.sermofur/memory.db",
                    "--origin",
                    "user",
                ]),
                Act::Changes,
                r(&["source", "add", "largest.md", "--origin", "user"]),
                r(&["source", "add", "bom.md", "--origin", "user"]),
                r(&["source", "add", "<root>/sub/long.md", "--origin", "user"]),
                rin(
                    "sub",
                    &["source", "add", "../sub/./long.md", "--origin", "user"],
                ),
                rin("sub", &["source", "add", "long.md", "--origin", "user"]),
                // Within the argument bound, over 16 384 units once combined with the root,
                // short once normalized: the bound applies to the full path, as `GetFullPath`.
                r(&[
                    "source",
                    "add",
                    Box::leak(format!("{}sub/long.md", "sub/../".repeat(2334)).into_boxed_str()),
                    "--origin",
                    "user",
                ]),
                r(&[
                    "--path", "sub", "source", "add", "long.md", "--origin", "user",
                ]),
                r(&["source", "list"]),
                r(&["recall", "paragraphe"]),
                Act::Changes,
            ],
        ),
        s(
            "source_arguments",
            false,
            vec![
                r(&["init"]),
                Act::File("doc.md", "contenu"),
                r(&["source", "add", "doc.md", "--origin", "user"]),
                r(&["claim", "add", "le contenu existe", "--origin", "user"]),
                Act::Changes,
                r(&["source"]),
                r(&["source", "--json"]),
                r(&["source", "frob"]),
                r(&["source", "add"]),
                r(&["source", "add", "--origin", "user"]),
                r(&["source", "add", "doc.md", "--origin"]),
                r(&["source", "add", "doc.md", "--origin", "robot"]),
                r(&["source", "add", "doc.md", "--origin", "U_s-er"]),
                r(&["source", "add", "a", "b", "--origin", "user"]),
                r(&["source", "add", "missing.md", "--origin", "robot"]),
                r(&["source", "add", "doc.md", "--origin", "user", "--key", "k"]),
                r(&["source", "add", "doc.md", "--origin", "user", "--actor", ""]),
                r(&[
                    "source", "add", "doc.md", "--origin", "user", "--actor", "  ",
                ]),
                r(&[
                    "source",
                    "add",
                    "missing.md",
                    "--origin",
                    "user",
                    "--actor",
                    " ",
                ]),
                r(&["source", "list", "extra"]),
                r(&["source", "list", "--actor", "x"]),
                r(&["source", "show"]),
                r(&["source", "show", "x"]),
                r(&["source", "show", "00000000-0000-0000-0000-000000000000"]),
                r(&["source", "show", UNKNOWN]),
                r(&["source", "show", "{2:B}"]),
                r(&["source", "show", "{3}"]),
                r(&["claim", "show", "{2}"]),
                r(&["source", "show", "{2}", "extra"]),
                r(&["source", "reindex", "a", "b"]),
                r(&["source", "reindex", "x"]),
                r(&["source", "reindex", UNKNOWN]),
                r(&["source", "reindex", "{3}"]),
                r(&["source", "reindex", "--actor", ""]),
                r(&["source", "reindex", "--actor", "   "]),
                r(&["source", "reindex", "--origin", "user"]),
                r(&["source", "reindex", "--json", "--actor"]),
                r(&["source", "reindex", "{2:X}", "--json"]),
                r(&["source", "--", "list"]),
                Act::Changes,
                r(&[
                    "evidence",
                    "add",
                    "{3}",
                    "local_documentation",
                    "doc.md",
                    "--lineage",
                    "doc",
                    "--origin",
                    "user",
                    "--source",
                    "{2}",
                ]),
                Act::Remove("doc.md"),
                r(&["source", "reindex"]),
                r(&[
                    "evidence",
                    "add",
                    "{3}",
                    "local_documentation",
                    "doc.md",
                    "--lineage",
                    "doc2",
                    "--origin",
                    "user",
                    "--source",
                    "{2}",
                ]),
                r(&["challenge", "{3}"]),
                Act::Changes,
            ],
        ),
        s(
            "source_scopes",
            false,
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                Act::Mkdir("c"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                Act::File("a/notes.md", "secret de a"),
                Act::File("b/notes.md", "secret de b"),
                r(&["source", "add", "a/notes.md", "--origin", "user"]),
                rin("a", &["source", "add", "../b/notes.md", "--origin", "user"]),
                rin("a", &["source", "add", "notes.md", "--origin", "user"]),
                Act::Changes,
                rin("a", &["source", "list"]),
                rin("b", &["source", "list"]),
                r(&["source", "list"]),
                rin("b", &["source", "show", "{10}"]),
                rin("b", &["source", "reindex", "{10}"]),
                r(&["source", "reindex", "{10}"]),
                r(&["source", "reindex"]),
                rin("a", &["source", "reindex"]),
                rin("b", &["recall", "secret"]),
                rin("a", &["recall", "secret"]),
                // A source of the workspace taken by a scope created on its folder.
                Act::File("c/plan.md", "le plan de c"),
                r(&["source", "add", "c/plan.md", "--origin", "user"]),
                r(&["source", "list"]),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                Act::Changes,
                rin("c", &["source", "show", "{23}"]),
                r(&["source", "list"]),
                r(&["source", "show", "{23}"]),
                r(&["source", "add", "c/plan.md", "--origin", "user"]),
                rin("c", &["source", "add", "plan.md", "--origin", "user"]),
                r(&["recall", "plan"]),
                rin("c", &["recall", "plan"]),
                Act::File("c/plan.md", "le plan de c, revu"),
                r(&["source", "reindex"]),
                rin("c", &["source", "reindex"]),
                Act::Changes,
                // A project inside a client: the client cannot declare its files.
                Act::Mkdir("a/p"),
                rin("a", &["scope", "add", "p", "project", "a", "a/p"]),
                Act::File("a/p/x.md", "projet"),
                rin("a", &["source", "add", "p/x.md", "--origin", "user"]),
                rin("a/p", &["source", "add", "x.md", "--origin", "user"]),
                rin("a/p", &["source", "list"]),
                rin("a", &["source", "list"]),
                Act::Changes,
            ],
        ),
        s(
            "source_link",
            false,
            vec![
                r(&["init"]),
                Act::Mkdir("real"),
                Act::File("real/secret.md", "derrière un lien"),
                Act::Link("linked", "real"),
                r(&["source", "add", "linked/secret.md", "--origin", "user"]),
                r(&["source", "add", "linked", "--origin", "user"]),
                r(&["source", "add", "real/secret.md", "--origin", "user"]),
                Act::Changes,
                r(&["source", "list"]),
            ],
        ),
        s(
            "source_case_windows",
            true,
            vec![
                r(&["init"]),
                Act::File("Doc.md", "contenu"),
                r(&["source", "add", "doc.md", "--origin", "user"]),
                r(&["source", "add", "DOC.MD", "--origin", "user"]),
                r(&["source", "add", "<root>/dOC.md", "--origin", "user"]),
                Act::Mkdir("Dossier"),
                Act::File("Dossier/Été.md", "accentué"),
                r(&["source", "add", "dossier/été.md", "--origin", "user"]),
                r(&["source", "add", "DOSSIER/ÉTÉ.md", "--origin", "user"]),
                Act::File("Dossier/E\u{301}te.md", "décomposé"),
                r(&["source", "add", "dossier/ete\u{301}.md", "--origin", "user"]),
                r(&["source", "add", "dossier/\u{e9}te.md", "--origin", "user"]),
                r(&["source", "list"]),
                r(&["source", "reindex"]),
                Act::Changes,
                Act::Mkdir("client"),
                Act::File("client/notes.md", "secret du client"),
                r(&["scope", "add", "c", "client", "workspace", "CLIENT"]),
                r(&["source", "add", "client/notes.md", "--origin", "user"]),
                r(&["source", "add", "CLIENT/notes.md", "--origin", "user"]),
                rin("client", &["source", "add", "NOTES.md", "--origin", "user"]),
                rin("client", &["source", "list"]),
                Act::Changes,
            ],
        ),
        s(
            "source_without_instance",
            false,
            vec![
                r(&["source", "list"]),
                r(&["source", "add", "x.md", "--origin", "user", "--json"]),
                r(&["source"]),
            ],
        ),
    ]
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    common::compare_with_golden_files("cli-5-2", scenarios());
}

/// Cross-read: one tool declares sources through `source add` (one cited by evidence), then
/// changes one and removes another before `source reindex`; the other tool then answers
/// `source list`, `source show`, `recall` and `challenge` with the same bytes, and reindexes
/// the memory of the first one: `recall` and `challenge` again agree.
#[test]
fn each_tool_reads_the_sources_of_the_other() {
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
        let mut ids = Vec::new();
        for name in ["adr.md", "gone.md", "kept.md"] {
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
                    name,
                    "--origin",
                    "user",
                    "--source",
                    &source,
                ],
                "c",
            ));
            ids.push(source);
        }
        fs::write(root.join("c").join("adr.md"), "le build passe en 9 min").expect("source");
        fs::remove_file(root.join("c").join("gone.md")).expect("removed");
        assert_eq!(w(&["source", "reindex"], "c").exit_code, 0);
        let mut reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["source", "list"], "c"),
            (vec!["source", "list"], ""),
            (vec!["recall", "build"], "c"),
            (vec!["recall", "9 min"], "c"),
            (vec!["challenge", &claim], "c"),
            (vec!["challenge", &claim, "--json"], "c"),
            (vec!["status"], "c"),
        ];
        for id in &ids {
            reads.push((vec!["source", "show", id], "c"));
        }
        let compare = |what: &str| {
            for (values, cwd) in &reads {
                let by_writer = w(values, cwd);
                let by_reader = run(reader, &arguments(values), &root.join(cwd));
                assert_eq!(
                    (by_reader.exit_code, &by_reader.stdout, &by_reader.stderr),
                    (by_writer.exit_code, &by_writer.stdout, &by_writer.stderr),
                    "{reader} answers {values:?} on sources {what} {writer}"
                );
            }
        };
        let by_writer = w(&["challenge", &claim], "c");
        for signal in ["source_changed", "source_unavailable"] {
            assert!(by_writer.stdout.contains(signal), "{signal}");
        }
        compare("written by");
        // The reader reindexes what the writer declared: kept, then restored and changed.
        fs::write(root.join("c").join("gone.md"), "le build passe en 3 min").expect("back");
        fs::write(root.join("c").join("kept.md"), "le build passe en 5 min").expect("changed");
        let reindexed = run(reader, &arguments(&["source", "reindex"]), &root.join("c"));
        assert_eq!(reindexed.exit_code, 0, "{}", reindexed.stderr);
        for outcome in ["\"unchanged\"", "\"restored\"", "\"modified\""] {
            assert!(reindexed.stdout.contains(outcome), "{outcome}");
        }
        compare("reindexed by the reader of");
        eprintln!("cross-read ok: {reader} reads the sources written by {writer}");
    }
}
