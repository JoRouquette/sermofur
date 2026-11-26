//! `smf mcp serve` (lot 8.4) against `rust/tests/golden/mcp-serve.json`, recorded from the
//! oracle by `rust/tools/mcp-serve-oracle` (its header gives the sessions, the waits and the
//! normalization, reproduced here). Each session gets a fresh base folder with an instance made
//! by the program of the daemon, its setup steps, its daemon when it has one, then the bridge;
//! every case is sent once its previous answers are in, and its answers (sorted, the SDK answers
//! side by side) must equal the recorded ones, byte for byte after normalization. A last `ping`
//! proves no answer was left over; the exit code, stderr and the `mcp_*` events of the journal
//! are compared too.
//!
//! With `SMF_ORACLE` (the `smf` of .NET 0.4.3): the oracle replays its own golden file, and the
//! bridge of each tool talks to the daemon of the other (lecture croisée, on an instance made by
//! that other tool).

#[path = "../../../tests/support/scratch.rs"]
mod scratch;

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::thread;
use std::time::Duration;

use serde_json::Value;
use sermofur_core::json::quoted_basic_latin;

const SMF: &str = env!("CARGO_BIN_EXE_smf");
const HOME_VARIABLE: &str = "SERMOFUR_DAEMON_HOME";
const ANSWER: Duration = Duration::from_secs(30);
const SYNC: &str = r#"{"jsonrpc":"2.0","id":"sync","method":"ping"}"#;
const SYNCED: &str = r#"{"result":{},"id":"sync","jsonrpc":"2.0"}"#;

fn oracle() -> Option<String> {
    std::env::var("SMF_ORACLE").ok().filter(|v| !v.is_empty())
}

fn golden() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/mcp-serve.json");
    serde_json::from_str(&fs::read_to_string(&path).expect("the golden file")).expect("JSON")
}

fn session_named(name: &str) -> Value {
    golden()["sessions"]
        .as_array()
        .expect("sessions")
        .iter()
        .find(|session| session["name"] == name)
        .unwrap_or_else(|| panic!("no session {name}"))
        .clone()
}

/// Replays the session `name` with this bridge and this daemon (which also makes the instance).
pub fn replay(name: &str, bridge: &str, daemon: &str) {
    let session = session_named(name);
    if let Err(difference) = run(&session, bridge, daemon) {
        panic!("{name} (bridge {bridge}, daemon {daemon}): {difference}");
    }
}

/// A fresh base folder, removed on drop.
struct Base(PathBuf);

impl Base {
    fn new() -> Self {
        let temporary = std::env::temp_dir();
        let temporary = if cfg!(windows) {
            sermofur_core::discovery::expand_short_names(&temporary)
        } else {
            fs::canonicalize(temporary).expect("the temporary directory resolves")
        };
        let base = temporary.join(scratch::name("sp", "golden_mcp_serve"));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("proj é")).expect("root");
        fs::create_dir_all(base.join("outside")).expect("outside");
        Self(base)
    }
}

impl Drop for Base {
    fn drop(&mut self) {
        for _ in 0..20 {
            if fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() {
                return;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
}

/// A process killed on drop.
struct Killed(Child);

impl Drop for Killed {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command(program: &str, cwd: &Path, home: &Path, words: &[&str]) -> Command {
    let mut command = Command::new(program);
    command
        .args(words)
        .current_dir(cwd)
        .env(HOME_VARIABLE, home)
        .env_remove("SERMOFUR_NO_DAEMON")
        .env_remove("CLAUDE_PROJECT_DIR");
    command
}

/// Runs a command to its end; its stdout.
fn output(program: &str, cwd: &Path, home: &Path, words: &[&str]) -> String {
    let done = command(program, cwd, home, words)
        .output()
        .expect("the program starts");
    assert!(
        done.status.success(),
        "{words:?}: {}",
        String::from_utf8_lossy(&done.stderr)
    );
    String::from_utf8(done.stdout).expect("UTF-8")
}

fn run(session: &Value, bridge: &str, daemon_program: &str) -> Result<(), String> {
    let base = Base::new();
    let root = base.0.join("proj é");
    let home = base.0.join(scratch::HOME);
    output(daemon_program, &root, &home, &["init"]);
    let mut ids: Vec<String> = Vec::new();
    for step in session["setup"].as_array().expect("setup") {
        if let Some(folder) = step["mkdir"].as_str() {
            fs::create_dir_all(root.join(folder)).expect("mkdir");
            ids.push(String::new());
            continue;
        }
        let cwd = step["cwd"]
            .as_str()
            .map_or(root.clone(), |sub| root.join(sub));
        let words: Vec<&str> = step["run"]
            .as_array()
            .expect("run")
            .iter()
            .map(|word| word.as_str().expect("word"))
            .collect();
        let printed: Value =
            serde_json::from_str(&output(daemon_program, &cwd, &home, &words)).expect("JSON");
        ids.push(printed["id"].as_str().unwrap_or_default().to_owned());
    }
    let with_daemon = session["daemon"] == true;
    let _daemon = with_daemon.then(|| {
        let mut child = command(daemon_program, &root, &home, &["daemon", "run"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("the daemon starts");
        let mut first = String::new();
        let stdout = child.stdout.take().expect("stdout");
        let mut reader = BufReader::new(stdout);
        reader.read_line(&mut first).expect("a line");
        assert!(first.contains("listening on"), "{first}");
        // Keeps the pipe open (and drained) for the life of the daemon.
        thread::spawn(move || {
            let _ = std::io::copy(&mut reader, &mut std::io::sink());
        });
        Killed(child)
    });
    if session["registered"] == true {
        output(daemon_program, &root, &home, &["daemon", "register"]);
    }
    let folder = session["folder"].as_str().expect("folder");
    let working = match folder {
        "root" => root.clone(),
        "outside" => base.0.join("outside"),
        other => root.join(other.trim_start_matches("root/")),
    };
    let served_home = if with_daemon {
        home.clone()
    } else {
        base.0.join("a")
    };
    let mut serve = command(bridge, &working, &served_home, &["mcp", "serve"]);
    match session["projectDirectory"].as_str() {
        Some("root") => {
            serve.env("CLAUDE_PROJECT_DIR", &root);
        }
        Some("relative") => {
            serve.env("CLAUDE_PROJECT_DIR", "proj é");
        }
        _ => {}
    }
    let mut child = Killed(
        serve
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the bridge starts"),
    );
    let mut input = child.0.stdin.take().expect("stdin");
    let lines = lines_of(child.0.stdout.take().expect("stdout"));
    let mut stderr = child.0.stderr.take().expect("stderr");
    let errors = thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let mut names = Names::new(&root, &base.0);
    for id in ids.iter().filter(|id| !id.is_empty()) {
        names.normalize(id);
    }
    for (index, case) in session["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .enumerate()
    {
        let template = case["send"].as_str().expect("send");
        let send = expand(template, &ids);
        write(&mut input, &send);
        let mut expected: Vec<String> = case["answers"]
            .as_array()
            .expect("answers")
            .iter()
            .map(|answer| answer.as_str().expect("answer").to_owned())
            .collect();
        let mut got = Vec::new();
        for _ in 0..expected.len() {
            match lines.recv_timeout(ANSWER) {
                Ok(line) => got.push(names.normalize(&line)),
                Err(_) => break,
            }
        }
        expected.sort();
        got.sort();
        if got != expected {
            return Err(format!(
                "case {index}: {}\n  expected {expected:?}\n  got      {got:?}",
                shorten(template)
            ));
        }
    }
    write(&mut input, SYNC);
    match lines.recv_timeout(ANSWER) {
        Ok(line) if line == SYNCED => {}
        other => return Err(format!("an answer was left over: {other:?}")),
    }
    drop(input);
    let exit = wait(&mut child.0);
    if Some(exit) != session["exit"].as_i64() {
        return Err(format!("exit {exit}, expected {}", session["exit"]));
    }
    let stderr = errors.join().expect("stderr");
    if stderr != session["stderr"].as_str().unwrap_or_default() {
        return Err(format!("stderr: {stderr}"));
    }
    if with_daemon {
        let events = events(&home.join("state").join("daemon.log"));
        let expected: Vec<String> = session["log"]
            .as_array()
            .expect("log")
            .iter()
            .map(|event| event.as_str().expect("event").to_owned())
            .collect();
        if events != expected {
            return Err(format!(
                "journal:\n  expected {expected:?}\n  got      {events:?}"
            ));
        }
    }
    Ok(())
}

fn shorten(text: &str) -> String {
    text.chars().take(160).collect()
}

fn write(input: &mut ChildStdin, line: &str) {
    input.write_all(line.as_bytes()).expect("written");
    input.write_all(b"\n").expect("written");
    input.flush().expect("flushed");
}

fn lines_of(stdout: impl Read + Send + 'static) -> Receiver<String> {
    let (sender, receiver) = channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { return };
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    receiver
}

fn wait(child: &mut Child) -> i64 {
    for _ in 0..300 {
        if let Some(status) = child.try_wait().expect("status") {
            return i64::from(status.code().unwrap_or(-1));
        }
        thread::sleep(Duration::from_millis(100));
    }
    panic!("the bridge did not stop at the end of stdin");
}

/// The `mcp_*` events of the journal, with their code.
fn events(file: &Path) -> Vec<String> {
    let Ok(text) = fs::read_to_string(file) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|entry| {
            let event = entry["event"].as_str()?.to_owned();
            event
                .starts_with("mcp_")
                .then(|| match entry["code"].as_str() {
                    Some(code) => format!("{event} {code}"),
                    None => event,
                })
        })
        .collect()
}

/// `@N@` is the id printed by setup step N; then `@N*TEXT@` is TEXT repeated N times.
fn expand(template: &str, ids: &[String]) -> String {
    let mut text = template.to_owned();
    for (index, id) in ids.iter().enumerate() {
        text = text.replace(&format!("@{index}@"), id);
    }
    let mut out = String::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find('@') {
        out.push_str(&rest[..at]);
        let after = &rest[at + 1..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        let repeated = (digits > 0 && after[digits..].starts_with('*'))
            .then(|| after[digits + 1..].find('@').map(|end| (digits, end)))
            .flatten();
        match repeated {
            Some((digits, end)) => {
                let count: usize = after[..digits].parse().expect("a count");
                out.push_str(&after[digits + 1..digits + 1 + end].repeat(count));
                rest = &after[digits + 1 + end + 1..];
            }
            None => {
                out.push('@');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The placeholders of one session, as the oracle tool writes them.
struct Names {
    forms: Vec<(String, &'static str)>,
    guids: HashMap<String, String>,
}

impl Names {
    fn new(root: &Path, base: &Path) -> Self {
        let unquoted = |text: &str| {
            let quoted = quoted_basic_latin(text);
            quoted[1..quoted.len() - 1].to_owned()
        };
        let mut forms = Vec::new();
        for (path, placeholder) in [(root, "<root>"), (base, "<base>")] {
            let raw = path.to_string_lossy().into_owned();
            let once = unquoted(&raw);
            let twice = unquoted(&once);
            forms.extend([
                (twice, placeholder),
                (once, placeholder),
                (raw, placeholder),
            ]);
        }
        Self {
            forms,
            guids: HashMap::new(),
        }
    }

    fn normalize(&mut self, line: &str) -> String {
        let mut text = line.to_owned();
        for (form, placeholder) in &self.forms {
            text = text.replace(form.as_str(), placeholder);
        }
        let chars: Vec<char> = text.chars().collect();
        let mut out = String::with_capacity(text.len());
        let mut i = 0;
        while i < chars.len() {
            if is_guid(&chars[i..]) {
                let key: String = chars[i..i + 36].iter().collect::<String>().to_lowercase();
                let next = format!("<g{}>", self.guids.len() + 1);
                out.push_str(self.guids.entry(key).or_insert(next));
                i += 36;
            } else if let Some(length) = instant(&chars[i..]) {
                out.push_str("<date>");
                i += length;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        }
        out.replace(r"\\r\\n", r"\\n").replace(r"\r\n", r"\n")
    }
}

fn is_guid(chars: &[char]) -> bool {
    chars.len() >= 36
        && chars[..36].iter().enumerate().all(|(index, c)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}

/// `\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,7})?`: its length.
fn instant(chars: &[char]) -> Option<usize> {
    const SHAPE: &str = "dddd-dd-ddTdd:dd:dd";
    if chars.len() < SHAPE.len()
        || !SHAPE.chars().zip(chars).all(|(shape, c)| match shape {
            'd' => c.is_ascii_digit(),
            other => other == *c,
        })
    {
        return None;
    }
    let mut length = SHAPE.len();
    if chars.get(length) == Some(&'.') {
        let fraction = chars[length + 1..]
            .iter()
            .take(7)
            .take_while(|c| c.is_ascii_digit())
            .count();
        if fraction > 0 {
            length += 1 + fraction;
        }
    }
    Some(length)
}

#[test]
fn the_protocol_is_answered_as_the_sdk_answers_it() {
    replay("protocol", SMF, SMF);
}

#[test]
fn lines_are_split_and_read_as_the_sdk_reads_them() {
    replay("framing", SMF, SMF);
}

/// Port of `McpTests.NotRegisteredAndOutsideAnInstanceAreNamed`.
#[test]
fn not_registered_and_outside_an_instance_are_named() {
    replay("not_registered", SMF, SMF);
    replay("outside_an_instance", SMF, SMF);
}

#[test]
fn the_eight_tools_answer_as_the_oracle() {
    replay("tools", SMF, SMF);
}

/// Port of `McpTests.SiblingObjectsStayInvisible`.
#[test]
fn sibling_objects_stay_invisible() {
    replay("sibling_scope", SMF, SMF);
}

#[test]
fn the_launch_folder_comes_from_claude_project_dir_when_absolute() {
    replay("project_directory", SMF, SMF);
    replay("relative_project_directory", SMF, SMF);
}

/// With `SMF_ORACLE`: the oracle answers as its golden file (the file is not stale, and the
/// waits of the harness suit it).
#[test]
fn the_oracle_replays_its_golden_file() {
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: the oracle is not replayed");
        return;
    };
    for session in golden()["sessions"].as_array().expect("sessions") {
        replay(session["name"].as_str().expect("name"), &oracle, &oracle);
    }
}

/// With `SMF_ORACLE`: the bridge of each tool on the daemon and the instance of the other.
#[test]
fn each_bridge_works_with_the_daemon_of_the_other() {
    let Some(oracle) = oracle() else {
        eprintln!("SMF_ORACLE not set: no lecture croisée");
        return;
    };
    for name in ["tools", "sibling_scope", "not_registered"] {
        replay(name, SMF, &oracle);
        replay(name, &oracle, SMF);
    }
    eprintln!("cross-read ok: mcp serve");
}

#[test]
fn templates_expand_as_the_oracle_tool_expands_them() {
    let ids = vec!["id0".to_owned(), String::new()];
    assert_eq!(expand("a@0@b@3*xy@c@", &ids), "aid0bxyxyxyc@");
    assert_eq!(expand("@1@@2*é@", &ids), "éé");
}
