//! Runs one command line, ported from `CommandRunner.Run`.
//!
//! Order of the checks, as in the .NET tool: help, then version, then parsing, then the command.
//! Ported commands: `init` and `root` (lot 1b.4), `status`, `scope`, `export`, `claim`,
//! `evidence` and `retex` (lot 3.4), `recall` (lot 4.2), `challenge` (lot 4.3), `source`
//! (lot 5.2), `index` (lot 5.3), `migrate` (lot 6.1); the others answer `not_ported`.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sermofur_core::discovery::{discover, expand_short_names, get_full_path};
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::init::{Initialized, initialize};
use sermofur_core::instance::{InstanceConfiguration, read_configuration};
use sermofur_core::memory::resolve_context;
use sermofur_core::migration::migrate;
use sermofur_core::ownership::SystemOwnership;
use sermofur_core::store::Store;
use sermofur_core::{NEW_LINE, VERSION, json};

use crate::args::{
    CommandArguments, HELP, HELP_SHORTCUT, VERSION as VERSION_OPTION, VERSION_SHORTCUT,
};
use crate::help;
use crate::memory_commands::{dispatch, writes};

/// Exit code of a command that exists in the .NET tool but is not ported yet.
const NOT_PORTED_EXIT_CODE: i32 = 70;

#[derive(Serialize)]
struct VersionBody<'a> {
    version: &'a str,
}

#[derive(Serialize)]
struct RootBody {
    root: String,
}

/// The answer of `init`, as the anonymous object `{ root, configuration, created }` of .NET.
#[derive(Serialize)]
struct InitBody {
    root: String,
    configuration: ConfigurationBody,
    created: bool,
}

/// `InstanceConfiguration` as `RecordJson` writes it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigurationBody {
    schema_version: i32,
    instance_id: String,
    created_at: sermofur_core::format::DateTimeOffset,
}

impl From<&InstanceConfiguration> for ConfigurationBody {
    fn from(value: &InstanceConfiguration) -> Self {
        ConfigurationBody {
            schema_version: value.schema_version,
            instance_id: value.instance_id.hyphenated().to_string(),
            created_at: value.created_at,
        }
    }
}

impl From<Initialized> for InitBody {
    fn from(value: Initialized) -> Self {
        InitBody {
            root: text(&value.root),
            configuration: ConfigurationBody::from(&value.configuration),
            created: value.created,
        }
    }
}

/// `Path.GetFullPath(path, basePath)` before normalization: a full path stays; on Windows a
/// path relative to a drive (`D:sub`) joins the base on the same drive, and takes the root of
/// its drive otherwise, without the hidden current folder of that drive; an NT path `\??\...`
/// stays as typed; anything else joins the base (`PathBuf::join` already keeps the drive of the
/// base for `\sub`).
pub(crate) fn combine(path: &str, base: &Path) -> PathBuf {
    // An NT path `\??\...` is fully qualified for .NET: kept as typed.
    if cfg!(windows) && path.starts_with(r"\??\") {
        return PathBuf::from(path);
    }
    let bytes = path.as_bytes();
    if cfg!(windows) && bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let rooted = bytes.get(2).is_some_and(|c| *c == b'\\' || *c == b'/');
        if !rooted {
            let same_drive = base
                .to_str()
                .and_then(|b| b.as_bytes().first())
                .is_some_and(|drive| drive.eq_ignore_ascii_case(&bytes[0]))
                && base
                    .to_str()
                    .is_some_and(|b| b.as_bytes().get(1) == Some(&b':'));
            return if same_drive {
                base.join(&path[2..])
            } else {
                PathBuf::from(format!(r"{}\{}", &path[..2], &path[2..]))
            };
        }
    }
    base.join(path)
}

fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    code: &'a str,
    message: &'a str,
}

/// Runs the command line and returns the exit code of the process.
///
/// Results go to `out`, errors to `err`: with `--json`, an error is a JSON object on `err` and
/// `out` stays empty.
pub fn run(arguments: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    // Read ahead: a parsing error must already honour the requested format.
    let json = CommandArguments::has_flag(arguments, "json");
    match execute(arguments, json, out) {
        Ok(code) => code,
        Err(error) => {
            write_error(err, &error, json);
            error.exit_code
        }
    }
}

fn execute(arguments: &[String], json: bool, out: &mut dyn Write) -> Result<i32> {
    if arguments.is_empty() || CommandArguments::asks(arguments, HELP, HELP_SHORTCUT) {
        write_line(out, &help::usage());
        return Ok(0);
    }
    if CommandArguments::asks(arguments, VERSION_OPTION, VERSION_SHORTCUT) {
        let text = if json {
            to_json(&VersionBody { version: VERSION })?
        } else {
            help::header()
        };
        write_line(out, &text);
        return Ok(0);
    }
    let mut parsed = CommandArguments::parse(arguments)?;
    if parsed
        .positionals()
        .first()
        .is_some_and(|command| command == "daemon" || command == "mcp")
    {
        return Err(not_ported());
    }
    // `Path.GetFullPath(args.Option("path", workingDirectory), workingDirectory)`: a relative
    // `--path` is taken from the working directory, then normalized as `GetFullPath` does
    // (`sub ` is `sub`), before any command; its `ArgumentException` is `invalid_storage_or_path`.
    // `Environment.CurrentDirectory` expands the short 8.3 names of the folder on Windows.
    let working_directory =
        expand_short_names(&std::env::current_dir().map_err(|_| invalid_storage())?);
    let path: PathBuf = match parsed.option("path") {
        Some(value) => get_full_path(&combine(value, &working_directory))?,
        None => working_directory,
    };
    if parsed.positionals().is_empty() {
        return Err(SermofurError::invalid_arguments("Command required."));
    }
    let command = parsed.positionals()[0].clone();
    if command == "init" {
        parsed.require_count(1)?;
        parsed.validate_used()?;
        let body = InitBody::from(initialize(&path)?);
        write_line(out, &to_json(&body)?);
        return Ok(0);
    }
    if command == "doctor" {
        return Err(not_ported());
    }
    let root = discover(&path, &SystemOwnership)?;
    if command == "root" {
        parsed.require_count(1)?;
        parsed.validate_used()?;
        let shown = if json {
            to_json(&RootBody { root: text(&root) })?
        } else {
            text(&root)
        };
        write_line(out, &shown);
        return Ok(0);
    }
    if command == "migrate" {
        parsed.require_count(1)?;
        parsed.validate_used()?;
        write_line(out, &to_json(&migrate(&root)?)?);
        return Ok(0);
    }
    let configuration = read_configuration(&root, false, local_offset)?;
    let mut store = Store::open(&root, configuration.instance_id, !writes(&parsed))?;
    let scopes = store.read_scopes()?;
    let context = resolve_context(&path, &root, &scopes)?;
    let body = dispatch(&mut parsed, &mut store, &context, &configuration, &path)?;
    write_line(out, &body);
    Ok(0)
}

/// The offset of local time, which reads a `createdAt` written without offset.
// ponytail: UTC until lot 7.2 ports the local offset (DateTimeOffset.Now).
fn local_offset(_utc_ticks: i64) -> i16 {
    0
}

/// A command of the .NET tool that the port does not run yet.
fn not_ported() -> SermofurError {
    SermofurError::new(
        "not_ported",
        "This command is not ported to Rust yet.",
        NOT_PORTED_EXIT_CODE,
    )
}

/// `invalid_storage_or_path`, exit 3: what `CommandRunner.Run` answers to an IO failure.
fn invalid_storage() -> SermofurError {
    SermofurError::new(
        "invalid_storage_or_path",
        "Invalid path or data; run doctor.",
        3,
    )
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    json::to_string(value).map_err(|error| {
        SermofurError::new("internal_error", error.to_string(), NOT_PORTED_EXIT_CODE)
    })
}

fn write_error(err: &mut dyn Write, error: &SermofurError, json: bool) {
    let text = if json {
        to_json(&ErrorBody {
            code: &error.code,
            message: &error.message,
        })
        .unwrap_or_else(|_| format!("smf: {}: {}", error.code, error.message))
    } else {
        format!("smf: {}: {}", error.code, error.message)
    };
    write_line(err, &text);
}

/// Writes a line. A write that fails (output closed by the reader, for example) cannot be
/// reported anywhere more useful, so it is ignored, as a closed pipe ends any command-line tool.
fn write_line(writer: &mut dyn Write, text: &str) {
    let _ = writer.write_all(text.as_bytes());
    let _ = writer.write_all(NEW_LINE.as_bytes());
    let _ = writer.flush();
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::run;
    use crate::test_support::command_line;

    fn run_capture(values: &[&str]) -> (i32, String, String) {
        let arguments: Vec<String> = values.iter().map(|value| (*value).to_string()).collect();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = run(&arguments, &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("UTF-8").replace("\r\n", "\n"),
            String::from_utf8(err).expect("UTF-8").replace("\r\n", "\n"),
        )
    }

    #[test]
    fn version_prints_the_header() {
        assert_eq!(
            run_capture(&["--version"]),
            (0, "Sermofur 0.0.0-dev\n".into(), String::new())
        );
    }

    #[test]
    fn version_as_json() {
        let (code, out, _) = run_capture(&["-v", "--json"]);
        assert_eq!(
            (code, out.as_str()),
            (0, "{\n  \"version\": \"0.0.0-dev\"\n}\n")
        );
    }

    #[test]
    fn help_wins_over_version() {
        let (code, out, _) = run_capture(&["--version", "--help"]);
        assert_eq!(code, 0);
        assert!(out.contains("Global options:"));
    }

    #[test]
    fn a_parsing_error_follows_the_requested_format() {
        let (code, out, err) = run_capture(&["--json", "--path"]);
        assert_eq!(code, 1);
        assert_eq!(out, "");
        assert_eq!(
            err,
            "{\n  \"code\": \"invalid_arguments\",\n  \"message\": \"Option value required.\"\n}\n"
        );
        let (_, _, text_err) = run_capture(&["--path"]);
        assert_eq!(text_err, "smf: invalid_arguments: Option value required.\n");
    }

    /// Port of `ArgumentContainingNulIsRejectedBeforeAnyCommand`: in process, since a NUL
    /// cannot reach the arguments of a real process. The bound is checked before the path is
    /// read, so the missing folder is never touched.
    #[test]
    fn an_argument_containing_nul_is_rejected_before_any_command() {
        let missing = std::env::temp_dir().join(format!(
            "sermofur-nul-{}",
            sermofur_core::guid::Uuid::new_v4().simple()
        ));
        let missing = missing.to_str().expect("UTF-8 path");
        let (code, out, err) = run_capture(&[
            "--path",
            missing,
            "--json",
            "claim",
            "add",
            "before\0after",
            "--origin",
            "user",
        ]);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "{\n  \"code\": \"invalid_arguments\",\n  \"message\": \"Argument limited to 16,384 characters without NUL.\"\n}\n"
        );
        assert!(!std::path::Path::new(missing).exists());
    }

    #[test]
    fn no_command_is_an_error_but_no_arguments_is_help() {
        let (code, _, err) = run_capture(&["--json"]);
        assert_eq!(code, 1);
        assert!(err.contains("Command required."));
        let (code, out, _) = run_capture(&[]);
        assert_eq!(code, 0);
        assert!(out.starts_with("Sermofur 0.0.0-dev\n"));
    }

    // Port of `HelpAndVersionAnswerWheneverAskedBeforeTheSeparator` (FsCheck, 500 cases).
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(500))]

        #[test]
        fn help_and_version_answer_whenever_asked_before_the_separator(
            arguments in command_line()
        ) {
            // Independent oracle: the words before `--` that are not the value of an option.
            let mut heads: Vec<&str> = Vec::new();
            let mut index = 0;
            while index < arguments.len() && arguments[index] != "--" {
                heads.push(&arguments[index]);
                let takes_value = ["--path", "--actor", "--text", "--x"]
                    .contains(&arguments[index].as_str())
                    && arguments.get(index + 1).is_some_and(|next| !next.starts_with("--"));
                if takes_value {
                    index += 1;
                }
                index += 1;
            }
            let help = arguments.is_empty() || heads.iter().any(|w| ["-h", "--help"].contains(w));
            let version = heads.iter().any(|w| ["-v", "--version"].contains(w));
            if help || version {
                let argument_refs: Vec<&str> = arguments.iter().map(String::as_str).collect();
                let (code, out, err) = run_capture(&argument_refs);
                let json = arguments.iter().take_while(|a| *a != "--").any(|a| a == "--json");
                prop_assert_eq!(code, 0);
                prop_assert_eq!(err, "");
                if help || !json {
                    prop_assert!(out.starts_with("Sermofur 0.0.0-dev"));
                } else {
                    prop_assert!(out.contains("\"version\": \"0.0.0-dev\""));
                }
            }
        }
    }

    /// The cases of `Path.GetFullPath(path, basePath)` in the .NET source (Path.Windows.cs).
    #[cfg(windows)]
    #[test]
    fn combine_follows_get_full_path_with_a_base() {
        use std::path::{Path, PathBuf};
        let base = Path::new(r"C:\Bar");
        let cases = [
            (r"C:Foo", r"C:\Bar\Foo"),
            (r"c:Foo", r"C:\Bar\Foo"),
            (r"C:", r"C:\Bar\"),
            (r"D:Foo", r"D:\Foo"),
            (r"D:", r"D:\"),
            (r"\Foo", r"C:\Foo"),
            (r"Foo", r"C:\Bar\Foo"),
            (r"D:\Foo", r"D:\Foo"),
            (r"\??\D:\Foo", r"\??\D:\Foo"),
        ];
        for (path, expected) in cases {
            assert_eq!(
                super::combine(path, base),
                PathBuf::from(expected),
                "{path}"
            );
        }
    }
}
