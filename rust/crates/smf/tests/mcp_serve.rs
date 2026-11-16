//! `smf mcp serve` through the real protocol, on the real daemon of the same program (lot 8.4):
//! ports of the `McpTests` that compare the bridge with the command line. With `SMF_ORACLE`,
//! each test runs on the oracle too (its bridge, its daemon, its command line).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};
use std::{fs, thread};

use serde_json::{Value, json};

const SMF: &str = env!("CARGO_BIN_EXE_smf");

fn programs() -> Vec<String> {
    let mut programs = vec![SMF.to_owned()];
    programs.extend(std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty()));
    programs
}

/// `TestInstance` and the home of `TestDaemon`, removed on drop; the daemon is killed on drop.
struct Fixture {
    program: String,
    base: PathBuf,
    root: PathBuf,
    home: PathBuf,
    daemon: Option<Child>,
}

impl Fixture {
    fn new(program: &str) -> Self {
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let base = temporary.join(format!(
            "smf-mcpt-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&base);
        let root = base.join("project");
        fs::create_dir_all(&root).expect("root");
        let fixture = Self {
            program: program.to_owned(),
            home: base.join("home"),
            base,
            root,
            daemon: None,
        };
        fixture.direct(&["init"]);
        fixture
    }

    fn command(&self, words: &[&str]) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(words)
            .current_dir(&self.root)
            .env("SERMOFUR_DAEMON_HOME", &self.home)
            .env_remove("CLAUDE_PROJECT_DIR");
        command
    }

    /// `DaemonTests.Direct`: the command line without the daemon; its stdout.
    fn direct(&self, words: &[&str]) -> String {
        let done = self
            .command(words)
            .env("SERMOFUR_NO_DAEMON", "1")
            .output()
            .expect("the program starts");
        assert!(done.status.success(), "{words:?}: {done:?}");
        String::from_utf8(done.stdout).expect("UTF-8")
    }

    /// `TestDaemon.Start` and `Registry.Register(root)`.
    fn serve(&mut self) {
        let mut child = self
            .command(&["daemon", "run"])
            .env_remove("SERMOFUR_NO_DAEMON")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the daemon starts");
        let mut reader = BufReader::new(child.stdout.take().expect("stdout"));
        let mut first = String::new();
        reader.read_line(&mut first).expect("a line");
        assert!(first.contains("listening on"), "{first}");
        thread::spawn(move || {
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        self.daemon = Some(child);
        self.direct(&["daemon", "register"]);
    }

    fn root_text(&self) -> String {
        self.root.to_string_lossy().into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(mut daemon) = self.daemon.take() {
            let _ = daemon.kill();
            let _ = daemon.wait();
        }
        for _ in 0..20 {
            if fs::remove_dir_all(&self.base).is_ok() || !self.base.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
}

/// `McpClient` over `smf mcp serve`, initialized with a client name.
struct Bridge {
    child: Child,
    input: ChildStdin,
    lines: Receiver<String>,
    next: i64,
}

impl Bridge {
    fn open(fixture: &Fixture, folder: &Path, home: &Path, host: &str) -> Self {
        let mut child = Command::new(&fixture.program)
            .args(["mcp", "serve"])
            .current_dir(folder)
            .env("SERMOFUR_DAEMON_HOME", home)
            .env_remove("SERMOFUR_NO_DAEMON")
            .env_remove("CLAUDE_PROJECT_DIR")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the bridge starts");
        let input = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let (sender, lines) = channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { return };
                if sender.send(line).is_err() {
                    return;
                }
            }
        });
        let mut bridge = Self {
            child,
            input,
            lines,
            next: 0,
        };
        bridge.request(
            "initialize",
            json!({"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": host, "version": "1.0"}}),
        );
        bridge.notify("notifications/initialized");
        bridge
    }

    fn notify(&mut self, method: &str) {
        let line = json!({"jsonrpc": "2.0", "method": method}).to_string();
        self.send(&line);
    }

    fn send(&mut self, line: &str) {
        self.input.write_all(line.as_bytes()).expect("sent");
        self.input.write_all(b"\n").expect("sent");
        self.input.flush().expect("flushed");
    }

    /// The `result` of a request.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        let line = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        self.send(&line.to_string());
        let answer: Value = serde_json::from_str(
            &self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("an answer"),
        )
        .expect("JSON");
        assert_eq!(answer["id"], id, "{answer}");
        answer["result"].clone()
    }

    /// `CallToolAsync`: whether it is an error, and its text.
    fn call(&mut self, tool: &str, arguments: Value) -> (bool, String) {
        let result = self.request("tools/call", json!({"name": tool, "arguments": arguments}));
        let text = result["content"][0]["text"]
            .as_str()
            .expect("a text")
            .to_owned();
        (result["isError"] == true, text)
    }

    fn ok(&mut self, tool: &str, arguments: Value) -> String {
        let (error, text) = self.call(tool, arguments);
        assert!(!error, "{tool}: {text}");
        text
    }

    fn error(&mut self, tool: &str, arguments: Value) -> String {
        let (error, text) = self.call(tool, arguments);
        assert!(error, "{tool}: {text}");
        text
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn parse(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|_| panic!("JSON: {text}"))
}

fn id_of(text: &str) -> String {
    parse(text)["id"].as_str().expect("an id").to_owned()
}

const TOOL_NAMES: [&str; 8] = [
    "sermofur_challenge",
    "sermofur_claim",
    "sermofur_context",
    "sermofur_evidence",
    "sermofur_feedback",
    "sermofur_recall",
    "sermofur_record_retex",
    "sermofur_status",
];

/// Port of `McpTests.RealProtocolListsTheEightToolsAndRunsThem`.
#[test]
fn the_real_protocol_lists_the_eight_tools_and_runs_them() {
    for program in programs() {
        let mut fixture = Fixture::new(&program);
        fixture.serve();
        let mut client = Bridge::open(&fixture, &fixture.root, &fixture.home, "Claude Code Test");
        let tools = client.request("tools/list", json!({}))["tools"].clone();
        let tools = tools.as_array().expect("tools");
        let mut names: Vec<&str> = tools
            .iter()
            .map(|t| t["name"].as_str().expect("name"))
            .collect();
        names.sort_unstable();
        assert_eq!(names, TOOL_NAMES);
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object");
            assert_eq!(tool["inputSchema"]["additionalProperties"], false);
            assert!(
                !tool["description"]
                    .as_str()
                    .expect("description")
                    .is_empty()
            );
        }
        client.ok(
            "sermofur_claim",
            json!({"text": "Billing paginates with offsets"}),
        );
        let recalled = client.ok("sermofur_recall", json!({"question": "billing pagination"}));
        let direct = fixture.direct(&["recall", "--json", "--", "billing pagination"]);
        assert_eq!(parse(&direct), parse(&recalled), "{program}");
        let listed = parse(&fixture.direct(&["claim", "list", "--json"]))[0].to_string();
        assert!(listed.to_lowercase().contains("\"llm\""), "{listed}");
        assert!(listed.contains("claude-code-test"), "{listed}");
    }
}

/// Port of `McpTests.WithoutDaemonTheBridgeAnswersWithTheRemedy`.
#[test]
fn without_daemon_the_bridge_answers_with_the_remedy() {
    for program in programs() {
        let fixture = Fixture::new(&program);
        let home = fixture.base.join("no-daemon");
        let mut client = Bridge::open(&fixture, &fixture.root, &home, "test host");
        let tools = client.request("tools/list", json!({}));
        assert_eq!(tools["tools"].as_array().expect("tools").len(), 8);
        let watch = Instant::now();
        let text = client.error("sermofur_status", json!({}));
        assert!(
            watch.elapsed() < Duration::from_secs(1),
            "{program}: {:?}",
            watch.elapsed()
        );
        assert!(text.starts_with("daemon_unavailable:"), "{text}");
        assert!(text.contains("smf daemon install"), "{text}");
        client.error("sermofur_status", json!({}));
    }
}

/// Port of `McpTests.ReadToolsGiveWhatTheCliGives`.
#[test]
fn read_tools_give_what_the_cli_gives() {
    for program in programs() {
        let mut fixture = Fixture::new(&program);
        let claim = id_of(&fixture.direct(&[
            "claim",
            "add",
            "cache invalidation is hard",
            "--origin",
            "user",
            "--json",
        ]));
        fixture.serve();
        let mut client = Bridge::open(&fixture, &fixture.root, &fixture.home, "test host");
        let status = fixture
            .direct(&["status", "--json"])
            .replace("\"direct\"", "\"daemon\"");
        assert_eq!(
            parse(&status),
            parse(&client.ok("sermofur_status", json!({})))
        );
        assert_eq!(
            parse(&fixture.direct(&["challenge", &claim, "--json"])),
            parse(&client.ok("sermofur_challenge", json!({"claimId": claim})))
        );
        assert_eq!(
            parse(&fixture.direct(&["recall", "--limit", "1", "--json", "--", "cache"])),
            parse(&client.ok("sermofur_recall", json!({"question": "cache", "limit": 1})))
        );
        let context = parse(&client.ok("sermofur_context", json!({})));
        assert_eq!(context["root"], fixture.root_text());
        assert_eq!(context["counts"]["claims"], 1);
    }
}

/// Port of `McpTests.WritesComeFromTheHostAndReplaysAreIdempotent`.
#[test]
fn writes_come_from_the_host_and_replays_are_idempotent() {
    for program in programs() {
        let mut fixture = Fixture::new(&program);
        fixture.serve();
        let mut client = Bridge::open(&fixture, &fixture.root, &fixture.home, "Claude Code");
        let replayed = json!({"text": "-- starts with dashes", "key": "k1"});
        let first = id_of(&client.ok("sermofur_claim", replayed.clone()));
        assert_eq!(first, id_of(&client.ok("sermofur_claim", replayed)));
        let help_text = id_of(&client.ok("sermofur_claim", json!({"text": "-h"})));
        assert_ne!(first, help_text);
        client.ok(
            "sermofur_evidence",
            json!({"claimId": first, "kind": "source_code", "reference": "--src/a.cs", "lineage": "repo"}),
        );
        client.ok(
            "sermofur_record_retex",
            json!({"event": "e", "impact": "i", "next": "n"}),
        );
        for (tool, arguments) in [
            (
                "sermofur_record_retex",
                json!({"event": "--e", "impact": "i", "next": "n"}),
            ),
            ("sermofur_claim", json!({"text": "x", "origin": "user"})),
            ("sermofur_claim", json!({"text": "x", "actor": "someone"})),
            (
                "sermofur_claim",
                json!({"text": "x", "category": "decisions"}),
            ),
        ] {
            let text = client.error(tool, arguments);
            assert!(text.starts_with("invalid_input:"), "{text}");
        }
        let shown = fixture.direct(&["claim", "show", &first, "--json"]);
        assert!(shown.contains("claude-code"), "{shown}");
        assert!(!shown.contains("\"high\""), "{shown}");
        let claim = &parse(&shown)["claim"];
        assert_eq!(claim["provenance"]["origin"], "llm");
        assert_eq!(claim["provenance"]["actor"], "claude-code");
        let content = parse(claim["contentJson"].as_str().expect("content"));
        assert_eq!(content["text"], "-- starts with dashes");
    }
}

/// Port of `McpTests.FeedbackIsAMarkedDraftRetexWithoutEffect`.
#[test]
fn feedback_is_a_marked_draft_retex_without_effect() {
    for program in programs() {
        let mut fixture = Fixture::new(&program);
        let claim = id_of(&fixture.direct(&[
            "claim",
            "add",
            "queues retry three times",
            "--origin",
            "user",
            "--json",
        ]));
        let before = parse(&fixture.direct(&["recall", "--json", "--", "queues retry"]));
        fixture.serve();
        let mut client = Bridge::open(&fixture, &fixture.root, &fixture.home, "test host");
        let retex = client.ok(
            "sermofur_feedback",
            json!({"targetId": claim, "verdict": "wrong", "comment": "it retries five times"}),
        );
        assert!(
            retex.contains(&format!("feedback wrong on claim {claim}")),
            "{retex}"
        );
        let refused = client.error(
            "sermofur_feedback",
            json!({"targetId": claim, "verdict": "great"}),
        );
        assert!(refused.starts_with("invalid_input:"), "{refused}");
        let after = parse(&fixture.direct(&["recall", "--json", "--", "queues retry"]));
        // Adding any document changes BM25 statistics, so scores move; rank and confidence do not.
        let (first_before, first_after) = (&before["results"][0], &after["results"][0]);
        assert!(first_before.to_string().contains(&claim));
        assert!(first_after.to_string().contains(&claim));
        assert_eq!(first_before["confidence"], first_after["confidence"]);
    }
}
