//! Runs one command line, ported from `CommandRunner.Run`.
//!
//! Order of the checks, as in the .NET tool: help, then version, then parsing, then the command.
//! Step 1a stops there: no command is ported yet.

use std::io::Write;

use serde::Serialize;
use sermofur_core::error::{Result, SermofurError};
use sermofur_core::{NEW_LINE, VERSION, json};

use crate::args::{
    CommandArguments, HELP, HELP_SHORTCUT, VERSION as VERSION_OPTION, VERSION_SHORTCUT,
};
use crate::help;

/// Exit code of a command that exists in the .NET tool but is not ported yet.
const NOT_PORTED_EXIT_CODE: i32 = 70;

#[derive(Serialize)]
struct VersionBody<'a> {
    version: &'a str,
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
    let parsed = CommandArguments::parse(arguments)?;
    if parsed.positionals().is_empty() {
        return Err(SermofurError::invalid_arguments("Command required."));
    }
    Err(SermofurError::new(
        "not_ported",
        "This command is not ported to Rust yet.",
        NOT_PORTED_EXIT_CODE,
    ))
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
}
