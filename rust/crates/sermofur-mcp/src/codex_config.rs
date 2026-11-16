//! The `[mcp_servers.sermofur]` table of a Codex configuration (TOML), changed in place as
//! `CodexConfigFile` of the oracle (lot 8.2): only the lines of that table and of its sub-tables
//! are replaced, added or removed, so the rest of the file, comments included, stays byte for
//! byte. There is no full TOML parser: table headers are found outside strings, and anything the
//! edit cannot handle safely is refused. `rust/tests/golden/codex-config.json`
//! (`rust/tools/codex-config-oracle`) is the truth.
//!
//! Every marker the scan looks for is ASCII, so byte offsets stand for the UTF-16 offsets of the
//! oracle; whitespace is Unicode on both sides (`char::is_whitespace`, `char.IsWhiteSpace` and
//! the `\s` of .NET regular expressions cover the same characters).

use std::path::{Path, PathBuf};

use sermofur_core::daemon_paths::{SpecialFolder, system_folder};
use sermofur_core::error::{Result, SermofurError};

pub const PROJECT_FILE: &str = ".codex/config.toml";
pub const HOME_VARIABLE: &str = "CODEX_HOME";

const TABLE: [&str; 2] = ["mcp_servers", "sermofur"];

/// The configuration of a project folder.
pub fn project_path(folder: &Path) -> PathBuf {
    folder.join(".codex").join("config.toml")
}

/// The configuration of the user: `CODEX_HOME` when set and not empty, else `.codex` in the
/// profile folder.
pub fn user_path(variable: impl Fn(&str) -> Option<String>) -> PathBuf {
    let home = variable(HOME_VARIABLE)
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(system_folder(SpecialFolder::UserProfile)).join(".codex"));
    home.join("config.toml")
}

/// The block that declares the bridge, with the given line ending.
pub fn block(command: &str, arguments: &[String], newline: &str) -> String {
    [
        "[mcp_servers.sermofur]".to_owned(),
        command_line(command),
        args_line(arguments),
    ]
    .join(newline)
        + newline
}

fn command_line(command: &str) -> String {
    format!("command = {}", literal(command))
}

fn args_line(arguments: &[String]) -> String {
    let items: Vec<String> = arguments.iter().map(|a| literal(a)).collect();
    format!("args = [{}]", items.join(", "))
}

/// Content with the block added or updated, or `None` when it is already the same. A `None`
/// input is an absent file.
pub fn with_block(
    content: Option<&str>,
    command: &str,
    arguments: &[String],
) -> Result<Option<String>> {
    let content = match content {
        Some(content) if !content.is_empty() => content,
        _ => return Ok(Some(block(command, arguments, "\n"))),
    };
    let newline = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if let Some((start, end)) = scan(content)? {
        // Only the command and args lines are ours: keys and sub-tables the user added stay.
        let current = &content[start..end];
        let updated = with_launch_lines(current, command, arguments, newline)?;
        return Ok((updated != current)
            .then(|| format!("{}{updated}{}", &content[..start], &content[end..])));
    }
    let ending = if content.ends_with('\n') { "" } else { newline };
    Ok(Some(format!(
        "{content}{ending}{newline}{}",
        block(command, arguments, newline)
    )))
}

/// The block with its `command` and `args` lines set, in its main table (before any
/// sub-table); missing lines are added right after the header, in that order.
fn with_launch_lines(
    block: &str,
    command: &str,
    arguments: &[String],
    newline: &str,
) -> Result<String> {
    let mut lines: Vec<String> = block.split('\n').map(str::to_owned).collect();
    let command_line = command_line(command);
    let args_line = args_line(arguments);
    let main_end = (1..lines.len())
        .find(|&i| header_name(lines[i].trim_start().trim_end_matches('\r')).is_some())
        .unwrap_or(lines.len());
    let carriage = if newline == "\r\n" { "\r" } else { "" };
    let mut command_index = None;
    let mut args_set = false;
    for (index, line) in lines.iter_mut().enumerate().take(main_end).skip(1) {
        let key = key_of(line.trim()).unwrap_or("").to_owned();
        if key == "command" && command_index.is_none() {
            *line = format!("{command_line}{carriage}");
            command_index = Some(index);
        } else if key == "args" && !args_set {
            let count = |c: char| line.chars().filter(|&x| x == c).count();
            if count('[') > count(']') {
                return Err(invalid(
                    "args of [mcp_servers.sermofur] spans several lines",
                ));
            }
            *line = format!("{args_line}{carriage}");
            args_set = true;
        }
    }
    let command_index = command_index.unwrap_or_else(|| {
        lines.insert(1, format!("{command_line}{carriage}"));
        1
    });
    if !args_set {
        lines.insert(command_index + 1, format!("{args_line}{carriage}"));
    }
    Ok(lines.join("\n"))
}

/// Content without the block and its sub-tables, or `None` when there is none.
pub fn without_block(content: &str) -> Result<Option<String>> {
    let Some((start, end)) = scan(content)? else {
        return Ok(None);
    };
    let mut before = &content[..start];
    let after = &content[end..];
    // In the middle, the span already holds the blank lines that follow the block. At the end,
    // the blank line written before the block goes with it.
    if !after.is_empty() {
        return Ok(Some(format!("{before}{after}")));
    }
    if before.ends_with("\r\n\r\n") {
        before = &before[..before.len() - 2];
    } else if before.ends_with("\n\n") {
        before = &before[..before.len() - 1];
    } else if before == "\n" || before == "\r\n" {
        before = "";
    }
    Ok(Some(before.to_owned()))
}

/// True when the content declares the bridge.
pub fn declares(content: &str) -> Result<bool> {
    Ok(scan(content)?.is_some())
}

/// The span of the block and its sub-tables, if any.
fn scan(content: &str) -> Result<Option<(usize, usize)>> {
    let mut headers: Vec<(usize, Vec<&str>)> = Vec::new();
    let mut root_keys_of_servers = false;
    let mut current: Option<Vec<&str>> = None;
    let mut index = 0;
    let mut multiline: Option<&'static [u8]> = None;
    while index < content.len() {
        let line_start = index;
        let (line_end, next) = match content[index..].find('\n') {
            Some(at) => (index + at, index + at + 1),
            None => (content.len(), content.len()),
        };
        let line = content[line_start..line_end].trim_end_matches('\r');
        if multiline.is_none() {
            let trimmed = line.trim_start();
            if let Some(name) = header_name(trimmed) {
                let name = names(name);
                headers.push((line_start, name.clone()));
                current = Some(name);
            } else if let Some(key) = key_of(trimmed) {
                let parts = names(key);
                let about_us = (current.is_none() && parts.len() >= 2 && parts[..2] == TABLE)
                    || (current.is_none() && key == "mcp_servers" && trimmed.contains("sermofur"))
                    || (current.as_deref() == Some(&["mcp_servers"][..]) && parts[0] == "sermofur");
                root_keys_of_servers |= about_us;
            }
        }
        multiline = strings(line.as_bytes(), multiline);
        index = next;
    }
    if multiline.is_some() {
        return Err(invalid("a multi-line string is not closed"));
    }
    if root_keys_of_servers {
        return Err(invalid(
            "mcp_servers.sermofur is written as a key, not as a table",
        ));
    }
    let mains: Vec<usize> = (0..headers.len())
        .filter(|&i| headers[i].1 == TABLE)
        .collect();
    let first = match mains[..] {
        [] => return Ok(None),
        [first] => first,
        _ => return Err(invalid("[mcp_servers.sermofur] appears more than once")),
    };
    let mut last = first;
    while last + 1 < headers.len()
        && headers[last + 1].1.len() > 2
        && headers[last + 1].1[..2] == TABLE
    {
        last += 1;
    }
    let start = headers[first].0;
    let end = match headers.get(last + 1) {
        Some(&(next, _)) => before_attached_comments(content, start, next),
        None => content.len(),
    };
    Ok(Some((start, end)))
}

/// End of the block when a table follows: comment lines written just above that table, and
/// what separates them from it, belong to it. The block keeps its last line of content and the
/// blank lines right after it.
fn before_attached_comments(content: &str, start: usize, end: usize) -> usize {
    let mut lines: Vec<(usize, &str)> = Vec::new();
    let mut index = start;
    while index < end {
        let next = content[index..end]
            .find('\n')
            .map_or(end, |at| index + at + 1);
        lines.push((index, content[index..next].trim()));
        index = next;
    }
    let last_content = (0..lines.len())
        .rev()
        .find(|&i| !lines[i].1.is_empty() && !lines[i].1.starts_with('#'))
        .unwrap_or(0);
    lines[last_content + 1..]
        .iter()
        .find(|(_, text)| text.starts_with('#'))
        .map_or(end, |&(at, _)| at)
}

/// Multi-line string still open at the end of the line, given the one open at its start.
fn strings(line: &[u8], mut open: Option<&'static [u8]>) -> Option<&'static [u8]> {
    let mut index = 0;
    while index < line.len() {
        if let Some(quotes) = open {
            let Some(close) = line[index..].windows(3).position(|w| w == quotes) else {
                return open;
            };
            index += close + 3;
            open = None;
            continue;
        }
        match line[index] {
            b'#' => return None,
            b'"' if line[index..].starts_with(b"\"\"\"") => {
                open = Some(b"\"\"\"");
                index += 3;
            }
            b'\'' if line[index..].starts_with(b"'''") => {
                open = Some(b"'''");
                index += 3;
            }
            b'"' | b'\'' => index = skip_string(line, index),
            _ => index += 1,
        }
    }
    open
}

fn skip_string(line: &[u8], start: usize) -> usize {
    let quote = line[start];
    let mut index = start + 1;
    while index < line.len() {
        if quote == b'"' && line[index] == b'\\' {
            index += 2;
            continue;
        }
        if line[index] == quote {
            return index + 1;
        }
        index += 1;
    }
    index
}

/// The name of a table header alone on its line, comment allowed: `[a.b]`, `[[a]]`,
/// `[a."b c"]`. An array value such as `["x", "y"],` does not match. Same language as the
/// regular expression of the oracle,
/// `^\[\[?(?<name>\s*SEG(?:\s*\.\s*SEG)*\s*)\]\]?\s*(?:#.*)?$`, which never backtracks into a
/// different match.
fn header_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix('[')?;
    let rest = rest.strip_prefix('[').unwrap_or(rest);
    let mut at = skip_spaces(rest, 0);
    at = segment(rest, at)?;
    loop {
        let dot = skip_spaces(rest, at);
        if !rest[dot..].starts_with('.') {
            break;
        }
        match segment(rest, skip_spaces(rest, dot + 1)) {
            Some(end) => at = end,
            None => break,
        }
    }
    at = skip_spaces(rest, at);
    let (name, tail) = rest.split_at(at);
    let tail = tail.strip_prefix(']')?;
    let tail = tail.strip_prefix(']').unwrap_or(tail).trim_start();
    (tail.is_empty() || tail.starts_with('#')).then_some(name)
}

fn skip_spaces(text: &str, at: usize) -> usize {
    text[at..]
        .find(|c: char| !c.is_whitespace())
        .map_or(text.len(), |offset| at + offset)
}

/// End of a bare (`[A-Za-z0-9_-]+`) or quoted (`"..."`, `'...'`, no line break) key.
fn segment(text: &str, at: usize) -> Option<usize> {
    let bytes = &text.as_bytes()[at..];
    let bare = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-';
    match bytes.first()? {
        quote @ (b'"' | b'\'') => bytes[1..]
            .iter()
            .position(|b| b == quote || *b == b'\r' || *b == b'\n')
            .filter(|&i| bytes[1 + i] == *quote)
            .map(|i| at + i + 2),
        first if bare(first) => {
            Some(at + bytes.iter().position(|b| !bare(b)).unwrap_or(bytes.len()))
        }
        _ => None,
    }
}

fn key_of(trimmed: &str) -> Option<&str> {
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    match trimmed.find('=') {
        Some(equals) if equals > 0 => Some(trimmed[..equals].trim()),
        _ => None,
    }
}

fn names(dotted: &str) -> Vec<&str> {
    dotted
        .split('.')
        .map(|part| part.trim().trim_matches(|c| c == '"' || c == '\''))
        .collect()
}

/// A TOML literal string when possible, so that Windows backslashes stay as they are.
fn literal(value: &str) -> String {
    if value.contains('\'') || value.contains('\n') {
        let escaped = value
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n");
        format!("\"{escaped}\"")
    } else {
        format!("'{value}'")
    }
}

fn invalid(reason: &str) -> SermofurError {
    SermofurError::new(
        "invalid_mcp_config",
        format!(
            "The Codex configuration cannot be changed safely ({reason}); it is left untouched."
        ),
        3,
    )
}

/// Ports of `CodexConfigFileTests` (the facts; the properties are in
/// `crates/smf/tests/codex_config_golden.rs`, the CLI case comes with lot 8.3).
#[cfg(test)]
mod tests {
    use super::*;

    const EXISTING: &str = "# personal Codex configuration\nmodel = \"gpt-5\"   # comment kept\n\n[profiles.work]\napproval_policy = \"on-request\"\n\n[mcp_servers.github]\ncommand = \"npx\"\nargs = [\"-y\", \"@modelcontextprotocol/server-github\"]\nnotes = '''\n[mcp_servers.sermofur]\nnot a header, inside a multi-line string\n'''\n\n[mcp_servers.github.env]\nTOKEN = \"x\"\n\n[[hooks]]\nname = \"a\"";

    fn serve() -> Vec<String> {
        vec!["mcp".to_owned(), "serve".to_owned()]
    }

    fn put(content: &str, command: &str) -> Option<String> {
        with_block(Some(content), command, &serve()).expect("accepted")
    }

    fn take(content: &str) -> String {
        without_block(content).expect("accepted").expect("declared")
    }

    #[test]
    fn absent_file_gets_only_the_block() {
        let created = with_block(None, "smf", &serve()).unwrap().unwrap();
        assert_eq!(
            created,
            "[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n"
        );
        assert!(declares(&created).unwrap());
        assert_eq!(put(&created, "smf"), None);
        assert_eq!(take(&created), "");
    }

    #[test]
    fn everything_else_stays_byte_for_byte() {
        let original = format!("{EXISTING}\n");
        assert!(!declares(&original).unwrap());
        let installed = put(&original, "smf").unwrap();
        assert!(installed.starts_with(&original));
        assert_eq!(put(&installed, "smf"), None);
        let updated = put(&installed, r"C:\Users\me\.dotnet\tools\smf.exe").unwrap();
        assert!(updated.contains(r"command = 'C:\Users\me\.dotnet\tools\smf.exe'"));
        assert!(updated.starts_with(&original));
        assert_eq!(take(&updated), original);
    }

    #[test]
    fn block_in_the_middle_is_replaced_and_removed_with_its_sub_tables() {
        let content = "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'old'\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n";
        let updated = put(content, "smf").unwrap();
        assert_eq!(
            updated,
            "[a]\nx = 1\n\n[mcp_servers.sermofur]\ncommand = 'smf'\nargs = ['mcp', 'serve']\n\n[mcp_servers.sermofur.env]\nK = 'v'\n\n[b]\ny = 2\n"
        );
        let with_approval = updated.replace(
            "args = ['mcp', 'serve']\n",
            "args = ['mcp', 'serve']\ndefault_tools_approval_mode = \"writes\"\n",
        );
        assert_eq!(put(&with_approval, "smf"), None);
        assert!(
            put(&with_approval, "/opt/smf")
                .unwrap()
                .contains("default_tools_approval_mode = \"writes\"")
        );
        let refusal = with_block(
            Some("[mcp_servers.sermofur]\nargs = [\n  'x',\n]\n"),
            "smf",
            &serve(),
        )
        .unwrap_err();
        assert_eq!(refusal.code, "invalid_mcp_config");
        assert_eq!(take(&updated), "[a]\nx = 1\n\n[b]\ny = 2\n");
    }

    #[test]
    fn crlf_files_stay_crlf_and_quotes_are_escaped() {
        let content = "model = \"x\"\r\n";
        let installed = put(content, "C:\\it's\\smf.exe").unwrap();
        assert_eq!(
            installed,
            "model = \"x\"\r\n\r\n[mcp_servers.sermofur]\r\ncommand = \"C:\\\\it's\\\\smf.exe\"\r\nargs = ['mcp', 'serve']\r\n"
        );
        assert_eq!(take(&installed), content);
    }

    #[test]
    fn unsafe_files_are_refused() {
        for content in [
            "[mcp_servers]\nsermofur = { command = \"smf\" }\n",
            "mcp_servers.sermofur.command = \"smf\"\n",
            "mcp_servers = { sermofur = { command = \"smf\" } }\n",
            "notes = \"\"\"\nnever closed\n",
            "[mcp_servers.sermofur]\ncommand = 'a'\n[mcp_servers.sermofur]\ncommand = 'b'\n",
        ] {
            let refusal = with_block(Some(content), "smf", &serve()).unwrap_err();
            assert_eq!(
                (refusal.code.as_str(), refusal.exit_code),
                ("invalid_mcp_config", 3)
            );
        }
    }

    #[test]
    fn array_values_are_not_table_headers() {
        let content = "matrix = [\n  [\"a\", \"b\"],\n  [\"c\"]\n]\n";
        assert_eq!(take(&put(content, "smf").unwrap()), content);
    }

    #[test]
    fn comments_above_the_next_table_stay_on_removal() {
        for nl in ["\n", "\r\n"] {
            let before = format!("[a]{nl}x = 1{nl}{nl}");
            let after = format!(
                "# docs server{nl}# second line{nl}[mcp_servers.docs]{nl}command = 'd'{nl}"
            );
            let block = format!(
                "[mcp_servers.sermofur]{nl}# ours{nl}command = 'smf'{nl}args = ['mcp', 'serve']{nl}{nl}"
            );
            let whole = format!("{before}{block}{after}");
            assert_eq!(take(&whole), format!("{before}{after}"));
            let updated = put(&whole, "/opt/smf").unwrap();
            assert!(updated.ends_with(&after));
            assert!(updated.contains("# ours"));
        }
    }

    #[test]
    fn missing_final_newline_is_the_only_byte_added_for_good() {
        assert_eq!(take(&put("a = 1", "smf").unwrap()), "a = 1\n");
    }

    #[test]
    fn user_path_prefers_codex_home() {
        let path = user_path(|name| (name == HOME_VARIABLE).then(|| "home".to_owned()));
        assert_eq!(path, Path::new("home").join("config.toml"));
        let empty = user_path(|_| Some(String::new()));
        assert!(empty.ends_with(Path::new(".codex").join("config.toml")));
        assert_eq!(
            project_path(Path::new("p")),
            Path::new("p").join(".codex").join("config.toml")
        );
    }
}
