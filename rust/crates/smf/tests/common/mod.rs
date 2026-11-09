//! The harness of the differential tests of the commands (lots 3.4 and 3.5).
//!
//! Each scenario runs a list of steps in a fresh folder under the temporary directory, almost
//! all of them commands of the program under test, and records every answer. The expected
//! answers are golden files recorded from the .NET `smf` (the oracle). Runtime values are
//! replaced by placeholders after their format is checked: the folder (`<root>`), each GUID by
//! its order of first appearance (`<g1>`, `<g2>`…), each instant (`<date>`), and a run of 64
//! or more identical characters by `<c×N>`. The elements of arrays listed in the order of
//! random identifiers are sorted after normalization; other arrays keep their order. Line
//! breaks are compared as `\n`, also inside nested escaped JSON. The entries left in the folder
//! and the text of every projection are recorded too.

// Each test crate uses a part of the harness.
#![allow(dead_code)]

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
pub struct Captured {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub fn run(program: &str, arguments: &[String], cwd: &Path) -> Captured {
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
pub fn temp_base() -> &'static Path {
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
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new() -> Self {
        let root = temp_base().join(format!("sermofur-cli-{}", Uuid::new_v4().simple()));
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
pub enum Act {
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
    /// Writes a file of bytes that are not a literal text (invalid UTF-8, a large file).
    Bytes(&'static str, fn() -> Vec<u8>),
    /// Creates a link to a folder of the root: a junction on Windows (`mklink /J`, no privilege
    /// needed), a symbolic link elsewhere. Removed before the folder, never followed.
    Link(&'static str, &'static str),
    /// Writes a source of a scope through the store: its path and whether it is indexed.
    Source(&'static str, &'static str, bool),
    /// The history of the record printed by step N, read through the store.
    History(usize),
    /// The files under `.sermofur` added, changed or removed since the previous `Changes` (or
    /// since the start): what the steps in between wrote.
    Changes,
    /// Copies the format 1 instance of `MigrationTests` (`tests/Sermofur.Tests/Fixtures/
    /// instance-v1`, never modified) into the root, with the folders of its scopes, as
    /// `LegacyInstance` does.
    Legacy,
}

/// The format 1 fixture of `MigrationTests`.
pub fn legacy_fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../tests/Sermofur.Tests/Fixtures/instance-v1/marker")
}

fn copy_folder(source: &Path, target: &Path) {
    fs::create_dir_all(target).expect("folder");
    for entry in fs::read_dir(source).expect("listing") {
        let path = entry.expect("entry").path();
        let to = target.join(path.file_name().expect("name"));
        if path.is_dir() {
            copy_folder(&path, &to);
        } else {
            fs::copy(&path, &to).expect("copy");
        }
    }
}

/// Copies the format 1 fixture into `root` (see [`Act::Legacy`]).
pub fn copy_legacy(root: &Path) {
    fs::create_dir_all(root.join("client-a").join("project-x")).expect("scopes");
    fs::create_dir_all(root.join("client-b")).expect("scopes");
    copy_folder(&legacy_fixture(), &root.join(".sermofur"));
}

/// `memory-v1-<yyyyMMddTHHmmssZ>-<guid32>.db`, the name of a backup of `migrate`, becomes
/// `memory-v1-<stamp>-<id>.db` once its shape is checked.
pub fn backup_names(text: &str) -> String {
    const PREFIX: &str = "memory-v1-";
    let mut out = String::new();
    let mut rest = text;
    while let Some(at) = rest.find(PREFIX) {
        let tail = &rest[at + PREFIX.len()..];
        let shape = tail.as_bytes();
        let matches = shape.len() >= 52
            && shape[..8].iter().all(u8::is_ascii_digit)
            && shape[8] == b'T'
            && shape[9..15].iter().all(u8::is_ascii_digit)
            && shape[15] == b'Z'
            && shape[16] == b'-'
            && shape[17..49]
                .iter()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(c))
            && &shape[49..52] == b".db";
        out.push_str(&rest[..at + PREFIX.len()]);
        if matches {
            out.push_str("<stamp>-<id>.db");
            rest = &tail[52..];
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

pub fn r(arguments: &[&'static str]) -> Act {
    Act::Run("", arguments.to_vec())
}

pub fn rin(cwd: &'static str, arguments: &[&'static str]) -> Act {
    Act::Run(cwd, arguments.to_vec())
}

pub struct Scenario {
    pub name: &'static str,
    /// The answers depend on the case rules of the file system: Windows only.
    pub windows_only: bool,
    pub acts: Vec<Act>,
}

pub const UNKNOWN: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";

/// The identifier `id` in a form of `Guid.ToString` or a variation of it.
pub fn guid_form(id: &str, form: &str) -> String {
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
pub fn substitute(argument: &str, ids: &HashMap<usize, String>) -> String {
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
pub fn long_form(root: &Path) -> String {
    let canonical = fs::canonicalize(root).expect("the folder resolves");
    let text = canonical.to_str().expect("UTF-8 path");
    text.strip_prefix(r"\\?\").unwrap_or(text).to_owned()
}

/// `\r\n` inside nested escaped JSON (a run of backslashes, `r`, the same run, `n`) becomes
/// `\n`, at any depth of escaping.
pub fn collapse_line_breaks(text: &str) -> String {
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

pub fn is_guid_at(chars: &[char], at: usize) -> bool {
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
pub fn date_length(chars: &[char], at: usize) -> Option<usize> {
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

/// True when the elements of an array follow the order of random identifiers: records (an
/// object whose first property is a GUID `id`) or paths named after one.
fn ordered_by_random_ids(blocks: &[Vec<String>]) -> bool {
    !blocks.is_empty()
        && blocks.iter().all(|block| {
            let first = block[0].trim_start();
            let record = block
                .get(1)
                .is_some_and(|line| line.trim_start().starts_with("\"id\": \"<g"));
            (first == "{" && record) || (first.starts_with('"') && first.contains("<g"))
        })
}

/// The elements of the arrays listed in the order of random identifiers (records of a
/// top-level array, `evidence`, `files` and the `sources` of `source reindex`), sorted as text, so that both runs compare. Other
/// arrays (scopes, history, visible scopes) keep the order the program gave.
pub fn sort_arrays(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let opens = ["[", "\"evidence\": [", "\"files\": [", "\"sources\": ["].contains(&trimmed);
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
        if !ordered_by_random_ids(&blocks) {
            out.extend(blocks.into_iter().flatten());
            i = close;
            continue;
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
pub struct Names {
    guids: HashMap<String, String>,
}

impl Names {
    pub fn normalize(&mut self, text: &str, root: &Path) -> String {
        let long = long_form(root);
        let mut out = backup_names(&collapse_line_breaks(text))
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
        sort_arrays(&compress_runs(&result))
    }
}

/// A unit of 1 to 32 characters repeated 64 times or more becomes `<unit×N>`, so that texts
/// at the bounds (escaped emoji included) keep the golden files short; the count stays
/// checked.
pub fn compress_runs(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    'scan: while i < chars.len() {
        for width in 1..=32 {
            // Cheap filter first: a repeated unit starts again `width` characters later.
            if chars.get(i + width) != Some(&chars[i]) {
                continue;
            }
            let unit = match chars.get(i..i + width) {
                Some(unit) => unit,
                None => break,
            };
            let repeats = (0..)
                .take_while(|r| chars.get(i + r * width..i + (r + 1) * width) == Some(unit))
                .count();
            if repeats >= 64 {
                let unit: String = unit.iter().collect();
                out.push_str(&format!("<{unit}×{repeats}>"));
                i += repeats * width;
                continue 'scan;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

/// The content of every file under `.sermofur`, by path relative to the root.
fn files_of(root: &Path) -> BTreeMap<String, Vec<u8>> {
    if !root.join(".sermofur").is_dir() {
        return BTreeMap::new();
    }
    entries(&root.join(".sermofur"))
        .into_iter()
        .filter(|entry| !entry.ends_with('/'))
        .map(|entry| {
            let bytes = fs::read(root.join(".sermofur").join(&entry)).expect("file");
            (format!(".sermofur/{entry}"), bytes)
        })
        .collect()
}

/// The entries under `root`, sorted, with `/` separators and `/` after a folder.
pub fn entries(root: &Path) -> Vec<String> {
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

/// A link to a folder: a junction on Windows (`mklink /J`), a symbolic link elsewhere.
pub fn directory_link(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            // cmd reads a `/` as the start of an option.
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(target.to_string_lossy().replace('/', "\\"))
            .output()
            .expect("cmd");
        assert!(out.status.success(), "mklink /J failed: {out:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

/// Removes the link itself, never its target.
pub fn remove_link(link: &Path) {
    #[cfg(windows)]
    fs::remove_dir(link).expect("junction removed");
    #[cfg(unix)]
    fs::remove_file(link).expect("link removed");
}

pub fn open_store(root: &Path, read_only: bool) -> Store {
    let configuration = read_configuration(root, false, |_| 0).expect("configuration");
    Store::open(root, configuration.instance_id, read_only).expect("store")
}

/// Writes a source of `scope` through the store, as `source add` would leave it.
pub fn write_source(root: &Path, scope: &str, path: &str, indexed: bool) -> String {
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

pub fn read_history(root: &Path, id: &str) -> String {
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

pub fn record(program: &str, scenario: &Scenario) -> Value {
    let scratch = Scratch::new();
    let root = &scratch.0;
    let root_text = root.to_str().expect("UTF-8 path");
    let mut names = Names::default();
    let mut ids: HashMap<usize, String> = HashMap::new();
    let mut steps = Vec::new();
    let mut baseline: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut links = Vec::new();
    for (index, act) in scenario.acts.iter().enumerate() {
        let (label, output) = match act {
            Act::Run(cwd, arguments) => {
                let arguments: Vec<String> = arguments
                    .iter()
                    .map(|a| substitute(&a.replace("<root>", root_text), &ids))
                    .collect();
                let out = run(program, &arguments, &root.join(cwd));
                let shown: Vec<String> = scenario_arguments(act)
                    .iter()
                    .map(|a| compress_runs(a))
                    .collect();
                let label = json!({ "cwd": cwd, "arguments": shown });
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
            Act::Bytes(path, bytes) => {
                fs::write(root.join(path), bytes()).expect("file");
                (json!({ "bytes": path }), Value::Null)
            }
            Act::Link(link, target) => {
                directory_link(&root.join(link), &root.join(target));
                links.push(root.join(link));
                (json!({ "link": link, "target": target }), Value::Null)
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
            Act::Legacy => {
                copy_legacy(root);
                (json!({ "legacy": true }), Value::Null)
            }
            Act::Changes => {
                let now = files_of(root);
                let mut changes = Vec::new();
                for (path, bytes) in &now {
                    match baseline.get(path) {
                        None => changes.push(format!("added {path}")),
                        Some(before) if before != bytes => changes.push(format!("changed {path}")),
                        Some(_) => {}
                    }
                }
                for path in baseline.keys().filter(|path| !now.contains_key(*path)) {
                    changes.push(format!("removed {path}"));
                }
                baseline = now;
                // Sorted after normalization: file names follow random identifiers.
                let mut changes: Vec<String> = changes
                    .iter()
                    .map(|change| names.normalize(change, root))
                    .collect();
                changes.sort();
                (json!({ "changes": true }), json!(changes))
            }
        };
        steps.push(json!({ "step": label, "output": output }));
    }
    for link in links {
        remove_link(&link);
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

pub fn scenario_arguments(act: &Act) -> Vec<&'static str> {
    match act {
        Act::Run(_, arguments) => arguments.clone(),
        _ => Vec::new(),
    }
}

pub fn golden_path(folder: &str, name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(folder)
        .join(format!("{name}.json"))
}

/// Runs every scenario with the Rust binary and compares it with its golden file in
/// `rust/tests/golden/<folder>`; with `SMF_UPDATE_GOLDEN=1`, records the golden file first
/// from the program named by `SMF_ORACLE`.
pub fn compare_with_golden_files(folder: &str, scenarios: Vec<Scenario>) {
    let update = std::env::var("SMF_UPDATE_GOLDEN").is_ok_and(|value| value == "1");
    let oracle = std::env::var("SMF_ORACLE").ok();
    let rust = env!("CARGO_BIN_EXE_smf");
    let mut failures = Vec::new();
    let mut count = 0;
    let mut steps = 0;
    for scenario in scenarios {
        if scenario.windows_only && !cfg!(windows) {
            continue;
        }
        count += 1;
        steps += scenario.acts.len();
        let path = golden_path(folder, scenario.name);
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
    eprintln!("{folder}: {count} scenarios ({steps} steps) compared");
    assert!(failures.is_empty(), "differences:\n{}", failures.join("\n"));
}

pub fn arguments(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// The `id` printed by a command.
pub fn printed_id(out: &Captured) -> String {
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    let value: Value = serde_json::from_str(&out.stdout).expect("JSON");
    value["id"].as_str().expect("id").to_owned()
}
