//! Differential test of lot 3.4: `status`, `scope`, `claim`, `evidence`, `retex` and `export`.
//!
//! Each scenario runs a list of steps in a fresh folder under the temporary directory, almost
//! all of them commands of the program under test (`init`, then `scope add`, `claim add`…), and
//! records every answer. The expected answers are golden files recorded from the .NET `smf`
//! (the oracle). Runtime values are replaced by placeholders after their format is checked:
//! the folder (`<root>`), each GUID by its order of first appearance (`<g1>`, `<g2>`…), each
//! instant (`<date>`). Records listed in the order of their random identifiers are sorted
//! after normalization. Line breaks are compared as `\n`, also inside nested escaped JSON. The
//! entries left in the folder and the text of every projection are recorded too. To record:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test --locked -p smf --test golden_cli_3_4
//! ```
//!
//! Sources cannot be added by the command line before lot 5.2: the steps that need one write
//! it with `sermofur_core::store::Store`, the same way for both programs. With `SMF_ORACLE`
//! set, the cross-read test also runs: each tool reads what the other one wrote.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use serde_json::{Value, json};
use sermofur_core::instance::read_configuration;
use sermofur_core::records::{
    ActorKind, KnowledgeStatus, MemoryRecord, Provenance, RecordKind, SourceContent, SourceStatus,
    write,
};
use sermofur_core::store::Store;
use uuid::Uuid;

/// What a process printed and returned, with line endings normalized to `\n`.
struct Captured {
    exit_code: i32,
    stdout: String,
    stderr: String,
}

fn run(program: &str, arguments: &[String], cwd: &Path) -> Captured {
    let output = Command::new(program)
        .args(arguments)
        .env("SERMOFUR_NO_DAEMON", "1")
        .current_dir(cwd)
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

/// The temporary directory; on macOS `/var` is a link, so it is canonicalized everywhere but
/// on Windows (where `canonicalize` adds the `\\?\` prefix).
fn temp_base() -> &'static Path {
    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        let dir = std::env::temp_dir();
        if cfg!(windows) {
            dir
        } else {
            fs::canonicalize(dir).expect("the temporary directory resolves")
        }
    })
}

/// A fresh folder, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let root = temp_base().join(format!("sermofur-3-4-{}", Uuid::new_v4().simple()));
        fs::create_dir_all(&root).expect("scratch");
        Self(root)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One step of a scenario.
enum Act {
    /// A command line of the program under test, from a folder relative to the root. In an
    /// argument, `{N}` stands for the identifier printed by step N, and `{N:F}` for it in the
    /// form F (see [`guid_form`]).
    Run(&'static str, Vec<&'static str>),
    /// Creates a folder.
    Mkdir(&'static str),
    /// Removes a folder and its content.
    Rmdir(&'static str),
    /// Writes a file.
    File(&'static str, &'static str),
    /// Removes a file; `{N}` as in [`Act::Run`].
    Remove(&'static str),
    /// Writes a source of a scope through the store: its path and whether it is indexed.
    Source(&'static str, &'static str, bool),
    /// The history of the record printed by step N, read through the store.
    History(usize),
}

fn r(arguments: &[&'static str]) -> Act {
    Act::Run("", arguments.to_vec())
}

fn rin(cwd: &'static str, arguments: &[&'static str]) -> Act {
    Act::Run(cwd, arguments.to_vec())
}

struct Scenario {
    name: &'static str,
    /// The answers depend on the case rules of the file system: Windows only.
    windows_only: bool,
    acts: Vec<Act>,
}

const UNKNOWN: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
const LEGACY: &str = r#"{"schemaVersion":1,"instanceId":"0f8fad5b-d9cb-469f-a165-70867728950e","createdAt":"2026-01-02T03:04:05+00:00"}"#;

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
        s(
            "status_fresh",
            vec![
                r(&["init"]),
                r(&["status"]),
                r(&["status", "--json"]),
                r(&["status", "x"]),
                r(&["status", "--actor", "a"]),
                r(&["status", "--json", "--key", "k"]),
                Act::Mkdir("a/b"),
                rin("a/b", &["status"]),
                r(&["status", "--path", "a"]),
            ],
        ),
        s(
            "status_errors",
            vec![
                r(&["status"]),
                r(&["status", "--json"]),
                r(&["frobnicate"]),
                Act::Mkdir(".sermofur"),
                Act::File(".sermofur/instance.json", LEGACY),
                r(&["status"]),
                r(&["claim", "list", "--json"]),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
            ],
        ),
        s(
            "status_missing_database",
            vec![
                r(&["init"]),
                Act::Remove(".sermofur/memory.db"),
                r(&["status"]),
                r(&["claim", "add", "x", "--origin", "user"]),
            ],
        ),
        s(
            "status_counts",
            with(
                basics(),
                vec![
                    r(&["claim", "add", "Second", "--origin", "llm"]),
                    r(&["status"]),
                    Act::Mkdir("c"),
                    r(&["scope", "add", "c", "client", "workspace", "c"]),
                    rin("c", &["claim", "add", "Inner", "--origin", "user"]),
                    rin("c", &["status"]),
                    r(&["status"]),
                    r(&["frobnicate"]),
                    r(&["frobnicate", "--json"]),
                ],
            ),
        ),
        s(
            "scope_read",
            vec![
                r(&["init"]),
                r(&["scope", "current"]),
                r(&["scope", "list"]),
                r(&["scope", "tree", "--json"]),
                r(&["scope"]),
                r(&["scope", "frob"]),
                r(&["scope", "current", "x"]),
                r(&["scope", "list", "--actor", "a"]),
                r(&["scope", "frob", "x"]),
                r(&["scope", "--json"]),
            ],
        ),
        s(
            "scope_chain",
            vec![
                r(&["init"]),
                Act::Mkdir("c/p/t"),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                rin("c", &["scope", "add", "p", "project", "c", "c/p"]),
                rin("c/p", &["scope", "add", "r", "repository", "p", "c/p"]),
                rin("c/p", &["scope", "current"]),
                rin("c/p", &["scope", "add", "t", "task", "r", "c/p/t"]),
                rin("c/p/t", &["scope", "current"]),
                rin("c/p/t", &["scope", "list"]),
                rin("c/p/t", &["scope", "tree"]),
                rin("c", &["scope", "list"]),
                r(&["scope", "list"]),
                r(&["scope", "current", "--path", "c/p/t"]),
                rin("c/p/t", &["status", "--json"]),
            ],
        ),
        s(
            "scope_add_arguments",
            vec![
                r(&["init"]),
                Act::Mkdir("c"),
                r(&["scope", "add", "c", "client", "workspace"]),
                r(&["scope", "add", "c", "client", "workspace", "c", "x"]),
                r(&[
                    "scope",
                    "add",
                    "c",
                    "client",
                    "workspace",
                    "c",
                    "--actor",
                    "a",
                ]),
                r(&["scope", "add", "c", "planet", "workspace", "c"]),
                r(&["scope", "add", "c", "client2", "workspace", "c"]),
                r(&["scope", "add", "c", "", "workspace", "c"]),
                r(&[
                    "scope",
                    "add",
                    "c",
                    "source_code",
                    "workspace",
                    "c",
                    "--json",
                ]),
                r(&["scope", "add", "c", "workspace", "workspace", "c"]),
                r(&["scope", "add", "c", "project", "workspace", "c"]),
                r(&["scope", "add", "c", "task", "workspace", "c"]),
                r(&["scope", "add", "Client", "client", "workspace", "c"]),
                r(&["scope", "add", "1c", "client", "workspace", "c"]),
                r(&["scope", "add", "c_d", "client", "workspace", "c"]),
                r(&["scope", "add", "", "client", "workspace", "c"]),
                r(&["scope", "add", "é", "client", "workspace", "c"]),
                r(&[
                    "scope",
                    "add",
                    "a1234567890123456789012345678901234567890123456789012345678901234",
                    "client",
                    "workspace",
                    "c",
                ]),
                r(&[
                    "scope",
                    "add",
                    "a123456789012345678901234567890123456789012345678901234567890123",
                    "client",
                    "workspace",
                    "c",
                ]),
                r(&["scope", "add", "c", "client", "Workspace", "c"]),
                r(&["scope", "add", "c", "client", "", "c"]),
                r(&["scope", "add", "c", "client", "other", "c"]),
                r(&["scope", "add", "c", "client", "workspace", ""]),
                r(&["scope", "add", "c", "client", "workspace", "  "]),
                r(&["scope", "add", "c", "client", "workspace", "."]),
                r(&["scope", "add", "c", "client", "workspace", "missing"]),
                r(&["scope", "add", "c", "client", "workspace", "../c"]),
                r(&["scope", "add", "c", "client", "workspace", "/c"]),
                r(&["scope", "add", "c", "client", "workspace", "c:d"]),
                Act::File("f.txt", "x"),
                r(&["scope", "add", "c", "client", "workspace", "f.txt"]),
                r(&["scope", "add", "c", "CLIENT", "workspace", "./c/"]),
                r(&["scope", "list"]),
            ],
        ),
        s(
            "scope_add_kinds_and_spellings",
            vec![
                r(&["init"]),
                Act::Mkdir("a/b"),
                Act::Mkdir("d"),
                Act::Mkdir("e/f"),
                r(&["scope", "add", "a", "Cli-ent", "workspace", "a"]),
                r(&["scope", "add", "d", "_client_", "workspace", "d\\"]),
                r(&["scope", "add", "e", "client", "workspace", "e/./f/.."]),
                rin("a", &["scope", "add", "b", "PROJECT", "a", "a/b"]),
                rin("a/b", &["scope", "add", "rb", "repository", "b", "a/b"]),
                rin("a/b", &["scope", "add", "rb2", "repository", "b", "a/b"]),
                rin("e", &["scope", "add", "f", "project", "e", "e/f"]),
                rin("e/f", &["scope", "add", "g", "project", "f", "e/f"]),
                rin("e/f", &["scope", "add", "fr", "repository", "f", "."]),
                r(&["scope", "list"]),
                rin("a/b", &["scope", "list"]),
                rin("e/f", &["scope", "list", "--json"]),
            ],
        ),
        s(
            "scope_add_conflicts",
            vec![
                r(&["init"]),
                Act::Mkdir("a/b"),
                Act::Mkdir("x/y"),
                Act::Mkdir("p/q"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "a", "client", "workspace", "x"]),
                r(&["scope", "add", "a2", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "a/b"]),
                r(&["scope", "add", "y", "client", "workspace", "x/y"]),
                r(&["scope", "add", "x", "client", "workspace", "x"]),
                rin("a", &["scope", "add", "c", "client", "workspace", "a/b"]),
                rin("a", &["scope", "add", "ab", "project", "a", "x"]),
                rin("a", &["scope", "add", "ab", "project", "a", "a"]),
                rin("a", &["scope", "add", "a", "project", "a", "a/b"]),
                rin("a", &["scope", "add", "ab", "project", "a", "a/b"]),
                rin("a", &["scope", "add", "ab2", "project", "a", "a/b"]),
                rin("a/b", &["scope", "add", "w", "project", "a", "a/b"]),
                r(&["scope", "add", "p", "client", "workspace", "p"]),
                rin("p", &["scope", "add", "q", "project", "p", "p/q"]),
                rin("p/q", &["scope", "add", "q", "repository", "q", "p/q"]),
                rin("p/q", &["scope", "add", "qr", "repository", "q", "p/q"]),
                rin("p/q", &["scope", "add", "qt", "task", "qr", "p/q"]),
                r(&["scope", "list"]),
            ],
        ),
        windows(
            "scope_add_case_windows",
            vec![
                r(&["init"]),
                Act::Mkdir("Docs/Inner"),
                r(&["scope", "add", "d", "client", "workspace", "docs"]),
                rin("Docs", &["scope", "add", "i", "project", "d", "DOCS/inner"]),
                r(&["scope", "add", "d2", "client", "workspace", "DOCS"]),
                r(&["scope", "list"]),
                rin("docs/inner", &["scope", "current"]),
            ],
        ),
        s(
            "scope_context",
            vec![
                r(&["init"]),
                Act::Mkdir("c/sub"),
                Act::Mkdir("gone"),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                r(&["scope", "add", "g", "client", "workspace", "gone"]),
                rin("c/sub", &["scope", "current"]),
                rin("c/sub", &["claim", "add", "In c", "--origin", "user"]),
                Act::Rmdir("gone"),
                r(&["scope", "current"]),
                r(&["status"]),
                rin("c", &["status"]),
                r(&["scope", "current", "--path", "gone"]),
                r(&["scope", "current", "--path", "c/sub"]),
                r(&["claim", "list", "--path", "c"]),
            ],
        ),
        s(
            "scope_add_moves_sources",
            vec![
                r(&["init"]),
                Act::Mkdir("c/p"),
                Act::Mkdir("cx"),
                Act::Mkdir("d"),
                Act::Source("workspace", "c/a.txt", true),
                Act::Source("workspace", "c/p/b.txt", true),
                Act::Source("workspace", "cx/c.txt", true),
                Act::Source("workspace", "c", false),
                Act::Source("workspace", "d/e.txt", true),
                r(&["status"]),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                r(&["status"]),
                rin("c", &["status"]),
                Act::History(4),
                Act::History(6),
                Act::History(7),
                rin("c", &["scope", "add", "p", "project", "c", "c/p"]),
                rin("c", &["status"]),
                rin("c/p", &["status"]),
                Act::History(5),
                rin("c/p", &["scope", "add", "pr", "repository", "p", "c/p"]),
                rin("c/p", &["status"]),
                Act::History(5),
                r(&["export"]),
                rin("c/p", &["export"]),
            ],
        ),
        s(
            "claim_add",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Plain", "--origin", "user", "--json"]),
                r(&[
                    "claim",
                    "add",
                    "Accents é à, quotes \" ' and <b>&</b> + `tick` \\ 😀",
                    "--origin",
                    "USER",
                    "--actor",
                    "john",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Episodic",
                    "--origin",
                    "llm",
                    "--category",
                    "episodic",
                    "--volatility",
                    "stable",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Procedural",
                    "--origin",
                    "Llm",
                    "--category",
                    "PROCEDURAL",
                    "--volatility",
                    "vol-atile",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Pref",
                    "--origin",
                    "user",
                    "--category",
                    "preferences",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Decision",
                    "--origin",
                    "user",
                    "--category",
                    "decisions",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Pref",
                    "--origin",
                    "llm",
                    "--category",
                    "preferences",
                ]),
                r(&[
                    "claim",
                    "add",
                    "Decision",
                    "--origin",
                    "llm",
                    "--category",
                    "decisions",
                    "--json",
                ]),
                r(&["claim", "add", "No origin"]),
                r(&["claim", "add", "--origin", "user"]),
                r(&["claim", "add", "a", "b", "--origin", "user"]),
                r(&["claim", "add", "Robot", "--origin", "robot"]),
                r(&["claim", "add", "Number", "--origin", "1"]),
                r(&[
                    "claim",
                    "add",
                    "x",
                    "--origin",
                    "user",
                    "--category",
                    "facts",
                ]),
                r(&[
                    "claim",
                    "add",
                    "x",
                    "--origin",
                    "user",
                    "--volatility",
                    "fast",
                ]),
                r(&["claim", "add", "x", "--origin", "user", "--lineage", "l"]),
                r(&["claim", "add", "", "--origin", "user"]),
                r(&["claim", "add", "   ", "--origin", "user"]),
                r(&["claim", "add", "\t\u{3000}\u{a0}", "--origin", "user"]),
                r(&["claim", "add", "\u{200b}", "--origin", "user"]),
                r(&["claim", "add", "x", "--origin", "user", "--actor", " "]),
                r(&["claim", "add", "x", "--origin", "llm", "--actor", " "]),
                r(&["claim", "add", "--origin", "user", "--", "--dashed text"]),
                r(&["claim", "add", "x", "--origin", "user", "--actor", "-v"]),
                r(&["claim", "list"]),
                r(&["claim", "list", "x"]),
                r(&["claim", "list", "--origin", "user"]),
                r(&["claim"]),
                r(&["claim", "frob"]),
                r(&["claim", "--json"]),
            ],
        ),
        s(
            "claim_keys",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", "k1"]),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", "k1"]),
                r(&["claim", "add", "Other", "--origin", "user", "--key", "k1"]),
                r(&[
                    "claim", "add", "Keyed", "--origin", "user", "--key", "k1", "--actor", "me",
                ]),
                r(&["claim", "add", "Keyed", "--origin", "llm", "--key", "k1"]),
                r(&[
                    "claim",
                    "add",
                    "Keyed",
                    "--origin",
                    "user",
                    "--key",
                    "k1",
                    "--category",
                    "episodic",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user", "--key", "k1",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user", "--key", "k1",
                ]),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", "K1"]),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", " "]),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", ""]),
                Act::Mkdir("c"),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                rin(
                    "c",
                    &["claim", "add", "Keyed", "--origin", "user", "--key", "k1"],
                ),
                rin(
                    "c",
                    &["claim", "add", "Keyed", "--origin", "user", "--key", "k1"],
                ),
                Act::Remove(".sermofur/records/{1}.md"),
                r(&["claim", "add", "Keyed", "--origin", "user", "--key", "k1"]),
                r(&["claim", "list"]),
                r(&["retex", "list"]),
                r(&["status"]),
            ],
        ),
        s(
            "claim_show_and_confidence",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Fact", "--origin", "user"]),
                r(&["claim", "show", "{1}"]),
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
                r(&["claim", "show", "{1}"]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "human_observation",
                    "seen",
                    "--lineage",
                    "repo",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "authoritative_documentation",
                    "docs",
                    "--lineage",
                    "vendor",
                    "--origin",
                    "llm",
                ]),
                r(&["claim", "show", "{1}"]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "run",
                    "--lineage",
                    "ci",
                    "--origin",
                    "llm",
                    "--contradicts",
                ]),
                r(&["claim", "show", "{1}"]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "project_decision",
                    "adr",
                    "--lineage",
                    "team",
                    "--origin",
                    "user",
                    "--contradicts",
                ]),
                r(&["claim", "show", "{1}", "--json"]),
                r(&[
                    "claim",
                    "add",
                    "Pref",
                    "--origin",
                    "user",
                    "--category",
                    "preferences",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{12}",
                    "user_assertion",
                    "me",
                    "--lineage",
                    "me",
                    "--origin",
                    "user",
                ]),
                r(&["claim", "show", "{12}"]),
                r(&["claim", "show", "{3}"]),
                r(&["claim", "show", UNKNOWN]),
                r(&["claim", "show", "not-a-guid"]),
                r(&["claim", "show"]),
                r(&["claim", "show", "{1}", "x"]),
                r(&["claim", "show", "{1}", "--actor", "a"]),
                r(&["claim", "list"]),
                r(&["evidence", "list"]),
            ],
        ),
        s(
            "claim_invalidate",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Old fact", "--origin", "user"]),
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
                r(&["claim", "invalidate", "{1}"]),
                r(&["claim", "invalidate", "{1}", "--reason", " "]),
                r(&[
                    "claim",
                    "invalidate",
                    "{1}",
                    "--reason",
                    "r",
                    "--actor",
                    "\t",
                ]),
                r(&["claim", "invalidate", "--reason", "r"]),
                r(&["claim", "invalidate", "{1}", "x", "--reason", "r"]),
                r(&[
                    "claim",
                    "invalidate",
                    "{1}",
                    "--reason",
                    "r",
                    "--origin",
                    "user",
                ]),
                r(&["claim", "invalidate", "bad", "--reason", "r"]),
                r(&["claim", "invalidate", UNKNOWN, "--reason", "r"]),
                r(&["claim", "invalidate", "{2}", "--reason", "r"]),
                r(&["evidence", "invalidate", "{2}", "--reason", "r"]),
                r(&[
                    "claim",
                    "invalidate",
                    "{1}",
                    "--reason",
                    "Superseded by v2",
                    "--actor",
                    "john",
                ]),
                r(&["claim", "invalidate", "{1}", "--reason", "Again"]),
                r(&["claim", "show", "{1}"]),
                Act::Mkdir("c"),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                r(&["claim", "add", "Root claim", "--origin", "user"]),
                rin("c", &["claim", "invalidate", "{18}", "--reason", "r"]),
                rin("c", &["claim", "add", "Inner claim", "--origin", "user"]),
                r(&["claim", "invalidate", "{20}", "--reason", "r"]),
                rin(
                    "c",
                    &["claim", "invalidate", "{20}", "--reason", "r", "--json"],
                ),
                rin("c", &["claim", "list"]),
                r(&["claim", "list"]),
            ],
        ),
        s(
            "evidence_add",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Fact", "--origin", "user"]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "SourceCode",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                    "--actor",
                    "john",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "source-code",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "LLM_ASSERTION",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "llm",
                    "--key",
                    "e1",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "LLM_ASSERTION",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "llm",
                    "--key",
                    "e1",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "local_documentation",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "llm",
                    "--key",
                    "e1",
                    "--contradicts",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "rumour",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    " ",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "ref",
                    "--lineage",
                    " ",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "ref",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                ]),
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
                r(&[
                    "evidence",
                    "add",
                    "bad",
                    "rumour",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                    "--source",
                    "bad",
                ]),
                r(&[
                    "evidence",
                    "add",
                    UNKNOWN,
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{2}",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{3}",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
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
                    "bad",
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
                    "{2}",
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
                    UNKNOWN,
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
                    "--frob",
                    "x",
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
                    "--contradicts",
                    "x",
                ]),
                r(&["evidence", "list"]),
                r(&["evidence", "show", "{3}"]),
                r(&["evidence", "show", "{1}"]),
                r(&["evidence", "show", "{3}", "--json", "--key", "k"]),
            ],
        ),
        s(
            "evidence_sources",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Fact", "--origin", "user"]),
                Act::Source("workspace", "doc.txt", true),
                Act::Source("workspace", "gone.txt", false),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "local_documentation",
                    "doc.txt",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                    "--source",
                    "{2}",
                ]),
                r(&[
                    "evidence",
                    "add",
                    "{1}",
                    "local_documentation",
                    "gone.txt",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                    "--source",
                    "{3}",
                ]),
                r(&["claim", "show", "{2}"]),
                r(&["evidence", "show", "{2}"]),
                r(&["claim", "show", "{1}"]),
                r(&["status"]),
            ],
        ),
        s(
            "evidence_scopes",
            vec![
                r(&["init"]),
                Act::Mkdir("a"),
                Act::Mkdir("b"),
                r(&["scope", "add", "a", "client", "workspace", "a"]),
                r(&["scope", "add", "b", "client", "workspace", "b"]),
                r(&["claim", "add", "Root", "--origin", "user"]),
                rin("a", &["claim", "add", "In a", "--origin", "user"]),
                rin(
                    "a",
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
                rin(
                    "b",
                    &[
                        "evidence",
                        "add",
                        "{6}",
                        "execution",
                        "ref",
                        "--lineage",
                        "l",
                        "--origin",
                        "user",
                    ],
                ),
                r(&[
                    "evidence",
                    "add",
                    "{6}",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                rin(
                    "a",
                    &[
                        "evidence",
                        "add",
                        "{6}",
                        "execution",
                        "ref",
                        "--lineage",
                        "l",
                        "--origin",
                        "user",
                    ],
                ),
                rin("b", &["claim", "show", "{6}"]),
                rin("a", &["claim", "show", "{5}"]),
                rin("a", &["claim", "show", "{6}"]),
                r(&["claim", "list"]),
                rin("a", &["claim", "list"]),
                rin("a", &["evidence", "list"]),
                rin("b", &["evidence", "list"]),
                rin("b", &["evidence", "show", "{10}"]),
            ],
        ),
        s(
            "retex",
            vec![
                r(&["init"]),
                r(&[
                    "retex",
                    "add",
                    "--event",
                    "Outage",
                    "--impact",
                    "Two hours",
                    "--next",
                    "Add alert",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "llm", "--actor", "bot", "--json",
                ]),
                r(&[
                    "retex", "add", "--impact", "i", "--next", "n", "--origin", "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--next", "n", "--origin", "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--origin", "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n",
                ]),
                r(&[
                    "retex", "add", "x", "--event", "e", "--impact", "i", "--next", "n",
                    "--origin", "user",
                ]),
                r(&[
                    "retex", "add", "--event", " ", "--impact", "i", "--next", "n", "--origin",
                    "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "\n", "--next", "n", "--origin",
                    "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "", "--origin",
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
                    "n",
                    "--origin",
                    "user",
                    "--category",
                    "episodic",
                ]),
                r(&["retex", "list"]),
                r(&["retex", "show", "{1}"]),
                r(&["retex", "show", "{2}", "--json"]),
                r(&["retex", "show", UNKNOWN]),
                r(&["retex", "invalidate", "{1}", "--reason", "r"]),
                r(&["claim", "show", "{1}"]),
                r(&["evidence", "show", "{1}"]),
                r(&["retex", "frob"]),
                r(&["retex"]),
                r(&["status"]),
            ],
        ),
        s(
            "export",
            vec![
                r(&["init"]),
                r(&["export"]),
                r(&["claim", "add", "Fact", "--origin", "user"]),
                r(&[
                    "evidence",
                    "add",
                    "{2}",
                    "execution",
                    "ref",
                    "--lineage",
                    "l",
                    "--origin",
                    "user",
                ]),
                r(&[
                    "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin",
                    "user",
                ]),
                Act::Mkdir("c"),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                rin("c", &["claim", "add", "Inner", "--origin", "user"]),
                Act::Remove(".sermofur/records/{2}.md"),
                Act::Remove(".sermofur/records/{7}.md"),
                r(&["export"]),
                r(&["export", "--json"]),
                rin("c", &["export"]),
                r(&["export", "x"]),
                r(&["export", "--actor", "a"]),
            ],
        ),
        s(
            "projection_pending",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Before", "--origin", "user"]),
                Act::Mkdir("c"),
                Act::Source("workspace", "c/a.txt", true),
                Act::Rmdir(".sermofur/records"),
                Act::File(".sermofur/records", "not a folder"),
                r(&["claim", "add", "After", "--origin", "user", "--key", "k"]),
                r(&["claim", "add", "After", "--origin", "user", "--key", "k"]),
                r(&["scope", "add", "c", "client", "workspace", "c"]),
                r(&["scope", "list"]),
                rin("c", &["status"]),
                r(&["status"]),
                r(&["claim", "invalidate", "{1}", "--reason", "r"]),
                r(&["claim", "list"]),
                Act::Remove(".sermofur/records"),
                Act::Mkdir(".sermofur/records"),
                r(&["export"]),
            ],
        ),
        // One record only: export stops at the first failure, in the order of identifiers.
        s(
            "export_pending",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Only", "--origin", "user"]),
                Act::Rmdir(".sermofur/records"),
                Act::File(".sermofur/records", "not a folder"),
                r(&["export"]),
                r(&["export", "--json"]),
                r(&["claim", "show", "{1}"]),
            ],
        ),
        windows(
            "scope_add_moves_sources_case_windows",
            vec![
                r(&["init"]),
                Act::Mkdir("Docs/Inner"),
                Act::Source("workspace", "DOCS/a.txt", true),
                Act::Source("workspace", "docs/inner/b.txt", true),
                Act::Source("workspace", "DOCSX/c.txt", true),
                r(&["scope", "add", "d", "client", "workspace", "docs"]),
                rin("Docs", &["status"]),
                rin("Docs", &["scope", "add", "i", "project", "d", "docs/INNER"]),
                rin("Docs/Inner", &["status"]),
                r(&["status"]),
                Act::History(3),
            ],
        ),
        s(
            "guid_forms",
            vec![
                r(&["init"]),
                r(&["claim", "add", "Fact", "--origin", "user"]),
                r(&["claim", "show", "{1:U}"]),
                r(&["claim", "show", "{1:N}"]),
                r(&["claim", "show", "{1:B}"]),
                r(&["claim", "show", "{1:P}"]),
                r(&["claim", "show", "{1:X}"]),
                r(&["claim", "show", "{1:XS}"]),
                r(&["claim", "show", "{1:XW}"]),
                r(&["claim", "show", " {1} "]),
                r(&["claim", "show", "\t{1}\n"]),
                r(&["claim", "show", "\u{a0}{1}"]),
                r(&["claim", "show", "\u{3000}{1}\u{2028}"]),
                r(&["claim", "show", "\u{85}{1}"]),
                r(&["claim", "show", "\u{200b}{1}"]),
                r(&["claim", "show", "{1:D+}"]),
                r(&["claim", "show", "{1:D0x}"]),
                r(&["claim", "show", "{1:D0X}"]),
                r(&["claim", "show", "{1:Dlast0x}"]),
                r(&["claim", "show", "{1:Dlast+}"]),
                r(&["claim", "show", "{1:B)}"]),
                r(&["claim", "show", "{1:N-}"]),
                r(&["claim", "show", "{1:Xbig}"]),
                r(&["claim", "show", "{1:Xlong}"]),
                r(&["claim", "show", "{1:Xbyte}"]),
                r(&["claim", "show", "{1:Xextra}"]),
                r(&["claim", "show", "00000000-0000-0000-0000-000000000000"]),
                r(&["claim", "show", "{00000000-0000-0000-0000-000000000000}"]),
                r(&["claim", "show", "00000000000000000000000000000000"]),
                r(&["claim", "show", ""]),
                r(&["claim", "show", "{1}x"]),
                r(&["claim", "show", "g0000000-0000-0000-0000-000000000000"]),
            ],
        ),
    ]
}

/// The identifier `id` in a form of `Guid.ToString` or a variation of it.
fn guid_form(id: &str, form: &str) -> String {
    let n = id.replace('-', "");
    let x = |a: &str, b: &str| {
        let bytes: Vec<String> = (0..8)
            .map(|i| format!("0x{}", &n[16 + 2 * i..18 + 2 * i]))
            .collect();
        format!("{{0x{a},0x{b},0x{},{{{}}}}}", &n[12..16], bytes.join(","))
    };
    match form {
        "" => id.to_owned(),
        "U" => id.to_uppercase(),
        "N" => n.clone(),
        "B" => format!("{{{id}}}"),
        "P" => format!("({id})"),
        "X" => x(&n[..8], &n[8..12]),
        "XS" => x(&n[..8], &n[8..12]).replace(',', " , "),
        "XW" => format!("\t{}", x(&n[..8], &n[8..12]).replace("0x", "0X")),
        "Xbig" => x(&n[..8], &format!("1{}", &n[8..12])),
        "Xlong" => x(&format!("00{}", &n[..8]), &n[8..12]),
        "Xbyte" => x(&n[..8], &n[8..12]).replacen(",0x", ",0x1", 4),
        "Xextra" => format!("{}x", x(&n[..8], &n[8..12])),
        // Accepted forms whose group reads 0: another identifier (unless the group was 0).
        "D+" => format!("{}+000{}", &id[..9], &id[13..]),
        "D0x" => format!("{}0x00{}", &id[..9], &id[13..]),
        "D0X" => format!("{}+0X0{}", &id[..14], &id[18..]),
        "Dlast0x" => format!("{}0x00{}", &id[..24], &id[28..]),
        "Dlast+" => format!("{}+{}", &id[..28], &id[29..]),
        "B)" => format!("{{{id})"),
        "N-" => format!("{}-{}", &n[..9], &n[10..]),
        other => panic!("unknown form {other}"),
    }
}

/// Replaces `{N}` and `{N:F}` by the identifier printed by step N.
fn substitute(argument: &str, ids: &HashMap<usize, String>) -> String {
    let mut out = String::new();
    let mut rest = argument;
    while let Some(start) = rest.find('{') {
        let Some(length) = rest[start..].find('}') else {
            break;
        };
        let inner = &rest[start + 1..start + length];
        let (step, form) = inner.split_once(':').unwrap_or((inner, ""));
        match step.parse::<usize>() {
            Ok(step) => {
                out.push_str(&rest[..start]);
                let id = ids
                    .get(&step)
                    .unwrap_or_else(|| panic!("step {step} printed no identifier"));
                out.push_str(&guid_form(id, form));
                rest = &rest[start + length + 1..];
            }
            Err(_) => {
                out.push_str(&rest[..=start]);
                rest = &rest[start + 1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The folder with every short 8.3 name expanded, without the `\\?\` prefix of `canonicalize`.
fn long_form(root: &Path) -> String {
    let canonical = fs::canonicalize(root).expect("the folder resolves");
    let text = canonical.to_str().expect("UTF-8 path");
    text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
}

/// `\r\n` inside nested escaped JSON (a run of backslashes, `r`, the same run, `n`) becomes
/// `\n`, at any depth of escaping.
fn collapse_line_breaks(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let run = chars[i..].iter().take_while(|c| **c == '\\').count();
        let tail = i + run;
        let crlf = run > 0
            && chars.get(tail) == Some(&'r')
            && chars
                .get(tail + 1..tail + 1 + run)
                .is_some_and(|s| s.iter().all(|c| *c == '\\'))
            && chars.get(tail + 1 + run) == Some(&'n');
        if crlf {
            i = tail + 1;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_guid_at(chars: &[char], at: usize) -> bool {
    chars.len() >= at + 36
        && (0..36).all(|i| {
            let c = chars[at + i];
            if [8, 13, 18, 23].contains(&i) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
        && !chars
            .get(at + 36)
            .is_some_and(|c| c.is_ascii_hexdigit() || *c == '-')
}

/// A date of `DateTimeOffset.UtcNow` as the tools write it: `yyyy-MM-ddTHH:mm:ss`, an
/// optional fraction of 1 to 7 digits, then `+00:00`. Returns its length.
fn date_length(chars: &[char], at: usize) -> Option<usize> {
    let pattern = "dddd-dd-ddTdd:dd:dd";
    let fixed = pattern.chars().enumerate().all(|(i, p)| {
        chars.get(at + i).is_some_and(|c| {
            if p == 'd' {
                c.is_ascii_digit()
            } else {
                *c == p
            }
        })
    });
    if !fixed {
        return None;
    }
    let mut end = at + pattern.len();
    if chars.get(end) == Some(&'.') {
        let digits = chars[end + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        assert!((1..=7).contains(&digits), "fraction of a date");
        end += 1 + digits;
    }
    // The `+` is escaped as `+` when the date sits in a nested JSON text.
    let slashes = chars[end..].iter().take_while(|c| **c == '\\').count();
    let sign = if slashes > 0 { slashes + 5 } else { 1 };
    let offset: String = chars
        .get(end..end + sign + 5)
        .map(|s| s.iter().collect())
        .unwrap_or_default();
    let expected = if slashes > 0 {
        format!("{}u002B00:00", "\\".repeat(slashes))
    } else {
        "+00:00".to_owned()
    };
    assert_eq!(offset, expected, "a date in UTC");
    Some(end + sign + 5 - at)
}

/// The elements of the arrays listed in the order of random identifiers (a top-level array,
/// `evidence` and `files`), sorted as text, so that both runs compare.
fn sort_arrays(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let opens = trimmed == "[" || trimmed == "\"evidence\": [" || trimmed == "\"files\": [";
        out.push(line.to_owned());
        i += 1;
        if !opens {
            continue;
        }
        let close = (i..lines.len())
            .find(|&j| {
                let t = lines[j].trim_start();
                lines[j].len() - t.len() == indent && t.starts_with(']')
            })
            .expect("closing bracket");
        let mut blocks: Vec<Vec<String>> = Vec::new();
        for element in &lines[i..close] {
            let t = element.trim_start();
            let starts =
                element.len() - t.len() == indent + 2 && !t.starts_with('}') && !t.starts_with(']');
            if starts || blocks.is_empty() {
                blocks.push(Vec::new());
            }
            blocks
                .last_mut()
                .expect("a block")
                .push((*element).to_owned());
        }
        for block in &mut blocks {
            let last = block.last_mut().expect("a line");
            if last.ends_with(',') {
                last.pop();
            }
        }
        blocks.sort();
        let count = blocks.len();
        for (index, mut block) in blocks.into_iter().enumerate() {
            if index + 1 < count {
                block.last_mut().expect("a line").push(',');
            }
            out.extend(block);
        }
        i = close;
    }
    out.join("\n")
}

/// The placeholders of one scenario: each GUID gets the next number at its first appearance.
#[derive(Default)]
struct Names {
    guids: HashMap<String, String>,
}

impl Names {
    fn normalize(&mut self, text: &str, root: &Path) -> String {
        let long = long_form(root);
        let mut out = collapse_line_breaks(text)
            .replace(&long.replace('\\', "\\\\"), "<root>")
            .replace(&long, "<root>");
        out = out
            .replace(
                "<root>\\\\.sermofur\\\\records\\\\",
                "<root>/.sermofur/records/",
            )
            .replace("<root>\\.sermofur\\records\\", "<root>/.sermofur/records/")
            .replace("<root>\\\\", "<root>/")
            .replace("<root>\\", "<root>/");
        let chars: Vec<char> = out.chars().collect();
        let mut result = String::with_capacity(out.len());
        let mut i = 0;
        while i < chars.len() {
            if is_guid_at(&chars, i) {
                let guid: String = chars[i..i + 36].iter().collect::<String>().to_lowercase();
                let next = format!("<g{}>", self.guids.len() + 1);
                result.push_str(self.guids.entry(guid).or_insert(next));
                i += 36;
            } else if let Some(length) = date_length(&chars, i) {
                result.push_str("<date>");
                i += length;
            } else {
                result.push(chars[i]);
                i += 1;
            }
        }
        sort_arrays(&result)
    }
}

/// The entries under `root`, sorted, with `/` separators and `/` after a folder.
fn entries(root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(folder) = pending.pop() {
        for entry in fs::read_dir(&folder).expect("listing") {
            let path = entry.expect("entry").path();
            let relative = path
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            if path.is_dir() {
                found.push(format!("{relative}/"));
                pending.push(path);
            } else {
                found.push(relative);
            }
        }
    }
    found
}

fn open_store(root: &Path, read_only: bool) -> Store {
    let configuration = read_configuration(root, false, |_| 0).expect("configuration");
    Store::open(root, configuration.instance_id, read_only).expect("store")
}

/// Writes a source of `scope` through the store, as `source add` would leave it.
fn write_source(root: &Path, scope: &str, path: &str, indexed: bool) -> String {
    let content = SourceContent {
        relative_path: Some(path.to_owned()),
        status: if indexed {
            SourceStatus::Indexed
        } else {
            SourceStatus::Missing
        },
        hash: Some("9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08".into()),
        size: 4,
        indexed_at: None,
        passages: 1,
        rejection: None,
    };
    let record = MemoryRecord {
        id: Uuid::new_v4(),
        scope_id: Some(scope.to_owned()),
        kind: RecordKind::Source,
        status: KnowledgeStatus::Active,
        revision: 1,
        content_json: Some(write(&content).expect("JSON")),
        provenance: Some(Provenance {
            origin: ActorKind::User,
            actor: Some("john".to_owned()),
            timestamp: sermofur_core::format::parse_json_text("2026-03-04T05:06:07+00:00", |_| 0)
                .expect("date"),
        }),
    };
    let created = open_store(root, false)
        .create_record(&record, None)
        .expect("source written");
    write(&created).expect("JSON")
}

fn read_history(root: &Path, id: &str) -> String {
    let store = open_store(root, true);
    let all: HashSet<String> = store
        .read_scopes()
        .expect("scopes")
        .into_iter()
        .map(|scope| scope.id)
        .collect();
    let id = Uuid::parse_str(id).expect("GUID");
    write(&store.read_history(id, &all).expect("history")).expect("JSON")
}

fn record(program: &str, scenario: &Scenario) -> Value {
    let scratch = Scratch::new();
    let root = &scratch.0;
    let root_text = root.to_str().expect("UTF-8 path");
    let mut names = Names::default();
    let mut ids: HashMap<usize, String> = HashMap::new();
    let mut steps = Vec::new();
    for (index, act) in scenario.acts.iter().enumerate() {
        let (label, output) = match act {
            Act::Run(cwd, arguments) => {
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|a| substitute(&a.replace("<root>", root_text), &ids))
                    .collect();
                let out = run(program, &arguments, &root.join(cwd));
                let label = json!({ "cwd": cwd, "arguments": scenario_arguments(act) });
                if let Ok(value) = serde_json::from_str::<Value>(&out.stdout)
                    && let Some(id) = value["id"].as_str()
                {
                    ids.insert(index, id.to_owned());
                }
                let output = json!({
                    "exitCode": out.exit_code,
                    "stdout": names.normalize(&out.stdout, root),
                    "stderr": names.normalize(&out.stderr, root),
                });
                (label, output)
            }
            Act::Mkdir(path) => {
                fs::create_dir_all(root.join(path)).expect("mkdir");
                (json!({ "mkdir": path }), Value::Null)
            }
            Act::Rmdir(path) => {
                fs::remove_dir_all(root.join(path)).expect("rmdir");
                (json!({ "rmdir": path }), Value::Null)
            }
            Act::File(path, text) => {
                fs::write(root.join(path), text).expect("file");
                (json!({ "file": path }), Value::Null)
            }
            Act::Remove(path) => {
                fs::remove_file(root.join(substitute(path, &ids))).expect("remove");
                (json!({ "remove": path }), Value::Null)
            }
            Act::Source(scope, path, indexed) => {
                let text = write_source(root, scope, path, *indexed);
                let value: Value = serde_json::from_str(&text).expect("JSON");
                ids.insert(index, value["id"].as_str().expect("id").to_owned());
                (
                    json!({ "source": path, "scope": scope, "indexed": indexed }),
                    Value::String(names.normalize(&text.replace("\r\n", "\n"), root)),
                )
            }
            Act::History(step) => {
                let text = read_history(root, &ids[step]);
                (
                    json!({ "history": step }),
                    Value::String(names.normalize(&text.replace("\r\n", "\n"), root)),
                )
            }
        };
        steps.push(json!({ "step": label, "output": output }));
    }
    let mut listed: Vec<String> = entries(root)
        .iter()
        .map(|entry| names.normalize(entry, root))
        .collect();
    listed.sort();
    let mut projections = BTreeMap::new();
    let records = root.join(".sermofur").join("records");
    if let Ok(files) = fs::read_dir(&records) {
        for file in files {
            let path = file.expect("entry").path();
            let name = path
                .file_name()
                .expect("name")
                .to_string_lossy()
                .into_owned();
            let text = fs::read_to_string(&path)
                .expect("projection")
                .replace("\r\n", "\n");
            projections.insert(names.normalize(&name, root), names.normalize(&text, root));
        }
    }
    json!({
        "steps": steps,
        "entries": listed,
        "projections": projections,
    })
}

fn scenario_arguments(act: &Act) -> Vec<&'static str> {
    match act {
        Act::Run(_, arguments) => arguments.clone(),
        _ => Vec::new(),
    }
}

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden/cli-3-4")
        .join(format!("{name}.json"))
}

#[test]
fn the_rust_binary_answers_like_the_dotnet_oracle() {
    let update = std::env::var("SMF_UPDATE_GOLDEN").is_ok_and(|value| value == "1");
    let oracle = std::env::var("SMF_ORACLE").ok();
    let rust = env!("CARGO_BIN_EXE_smf");
    let mut failures = Vec::new();
    let mut count = 0;
    for scenario in scenarios() {
        if scenario.windows_only && !cfg!(windows) {
            continue;
        }
        count += 1;
        let path = golden_path(scenario.name);
        if update {
            let oracle = oracle
                .as_deref()
                .expect("SMF_ORACLE names the oracle program");
            let body = record(oracle, &scenario);
            fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
            fs::write(
                &path,
                serde_json::to_string_pretty(&body).expect("JSON") + "\n",
            )
            .expect("the golden file is written");
        }
        let golden: Value = serde_json::from_str(
            &fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("golden file missing: {}", path.display())),
        )
        .expect("valid JSON");
        let actual = record(rust, &scenario);
        if actual != golden {
            let (Value::Array(expected), Value::Array(got)) = (&golden["steps"], &actual["steps"])
            else {
                panic!("steps");
            };
            let first = expected
                .iter()
                .zip(got)
                .position(|(e, g)| e != g)
                .map_or_else(
                    || {
                        format!(
                            "entries or projections:\n  expected {}\n  got      {}",
                            json!([&golden["entries"], &golden["projections"]]),
                            json!([&actual["entries"], &actual["projections"]])
                        )
                    },
                    |i| {
                        format!(
                            "step {i}:\n  expected {}\n  got      {}",
                            expected[i], got[i]
                        )
                    },
                );
            failures.push(format!("{}: {first}", scenario.name));
        }
    }
    eprintln!("{count} scenarios compared");
    assert!(failures.is_empty(), "differences:\n{}", failures.join("\n"));
}

fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// The `id` printed by a command.
fn printed_id(out: &Captured) -> String {
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let value: Value = serde_json::from_str(&out.stdout).expect("JSON");
    value["id"].as_str().expect("id").to_owned()
}

/// Cross-read: one tool writes scopes, claims, evidence, an invalidation and a RETEX; both
/// tools then answer the same read commands with the same bytes.
#[test]
fn each_tool_reads_the_memory_of_the_other() {
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
        fs::create_dir_all(root.join("c/p")).expect("folders");
        assert_eq!(
            w(&["scope", "add", "c", "client", "workspace", "c"], "").exit_code,
            0
        );
        assert_eq!(
            w(&["scope", "add", "p", "project", "c", "c/p"], "c").exit_code,
            0
        );
        let claim = printed_id(&w(&["claim", "add", "Root é <b>", "--origin", "user"], ""));
        let inner = printed_id(&w(
            &[
                "claim",
                "add",
                "Inner",
                "--origin",
                "llm",
                "--category",
                "episodic",
                "--key",
                "k",
            ],
            "c/p",
        ));
        let evidence = printed_id(&w(
            &[
                "evidence",
                "add",
                &inner,
                "source_code",
                "a.rs",
                "--lineage",
                "l",
                "--origin",
                "user",
            ],
            "c/p",
        ));
        let contra = printed_id(&w(
            &[
                "evidence",
                "add",
                &inner,
                "execution",
                "run",
                "--lineage",
                "m",
                "--origin",
                "user",
                "--contradicts",
            ],
            "c/p",
        ));
        let retex = printed_id(&w(
            &[
                "retex", "add", "--event", "e", "--impact", "i", "--next", "n", "--origin", "user",
            ],
            "c",
        ));
        assert_eq!(
            w(&["claim", "invalidate", &claim, "--reason", "old"], "").exit_code,
            0
        );
        let reads: Vec<(Vec<&str>, &str)> = vec![
            (vec!["status"], ""),
            (vec!["status"], "c/p"),
            (vec!["scope", "list"], "c/p"),
            (vec!["scope", "current"], "c/p"),
            (vec!["claim", "list"], "c/p"),
            (vec!["claim", "show", &claim], "c/p"),
            (vec!["claim", "show", &inner], "c/p"),
            (vec!["evidence", "list"], "c/p"),
            (vec!["evidence", "show", &evidence], "c/p"),
            (vec!["evidence", "show", &contra], "c/p"),
            (vec!["retex", "list"], "c/p"),
            (vec!["retex", "show", &retex], "c"),
            (vec!["retex", "show", &retex], ""),
            (
                vec![
                    "claim",
                    "add",
                    "Inner",
                    "--origin",
                    "llm",
                    "--category",
                    "episodic",
                    "--key",
                    "k",
                ],
                "c/p",
            ),
        ];
        for (values, cwd) in reads {
            let by_writer = w(&values, cwd);
            let by_reader = run(reader, &arguments(&values), &root.join(cwd));
            assert_eq!(
                (by_reader.exit_code, &by_reader.stdout, &by_reader.stderr),
                (by_writer.exit_code, &by_writer.stdout, &by_writer.stderr),
                "{reader} reads {values:?} written by {writer}"
            );
        }
        // The writer's projections are rewritten identically by the reader.
        let before = fs::read_dir(root.join(".sermofur/records"))
            .expect("records")
            .map(|e| fs::read(e.expect("entry").path()).expect("bytes"))
            .collect::<Vec<_>>();
        assert_eq!(run(reader, &arguments(&["export"]), root).exit_code, 0);
        let after = fs::read_dir(root.join(".sermofur/records"))
            .expect("records")
            .map(|e| fs::read(e.expect("entry").path()).expect("bytes"))
            .collect::<Vec<_>>();
        assert_eq!(before, after, "{reader} exports what {writer} projected");
        eprintln!("cross-read ok: {reader} reads the memory written by {writer}");
    }
}
