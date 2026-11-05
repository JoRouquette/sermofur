//! The command-line grammar, ported from `CommandArguments.cs`.
//!
//! It is written by hand, not with `clap`, because the grammar has rules that a generic parser
//! does not reproduce: `--` ends the options, an option value cannot start with `--`, and
//! `-h`/`-v` are recognized anywhere before `--` except as the value of an option.

use std::collections::{HashMap, HashSet};

use sermofur_core::error::{Result, SermofurError};
use sermofur_core::limits::{
    ARGUMENT_COUNT_MESSAGE, ARGUMENT_MESSAGE, MAX_ARGUMENTS, is_out_of_bounds,
};

/// Separator after which every argument is positional.
pub const END_OF_OPTIONS: &str = "--";

/// Options that take no value.
const FLAGS: [&str; 3] = ["json", "contradicts", "supervise"];

/// Name of the help option (`--help`).
pub const HELP: &str = "help";
/// Shortcut of the help option.
pub const HELP_SHORTCUT: &str = "-h";
/// Name of the version option (`--version`).
pub const VERSION: &str = "version";
/// Shortcut of the version option.
pub const VERSION_SHORTCUT: &str = "-v";

/// A parsed command line: the positional arguments and the `--name [value]` options.
#[derive(Debug)]
pub struct CommandArguments {
    /// `None` for a flag, the value for an option that takes one.
    options: HashMap<String, Option<String>>,
    /// Options that a command has read: an option nobody read is unknown for that command.
    used: HashSet<String>,
    positionals: Vec<String>,
}

impl CommandArguments {
    /// Parses the arguments, after checking their bounds.
    pub fn parse(arguments: &[String]) -> Result<Self> {
        validate_bounds(arguments)?;

        let mut options: HashMap<String, Option<String>> = HashMap::new();
        let mut positionals: Vec<String> = Vec::new();
        let mut options_ended = false;
        let mut index = 0;
        while index < arguments.len() {
            let value = &arguments[index];
            index += 1;
            if options_ended || !value.starts_with("--") {
                positionals.push(value.clone());
                continue;
            }
            if value == END_OF_OPTIONS {
                options_ended = true;
                continue;
            }
            let name = &value[2..];
            if options.contains_key(name) {
                return Err(SermofurError::invalid_arguments("Duplicate option."));
            }
            if FLAGS.contains(&name) {
                options.insert(name.to_string(), None);
                continue;
            }
            // An option that is not a flag takes the next argument, which cannot start with `--`.
            match arguments.get(index) {
                Some(next) if !next.starts_with("--") => {
                    options.insert(name.to_string(), Some(next.clone()));
                    index += 1;
                }
                _ => return Err(SermofurError::invalid_arguments("Option value required.")),
            }
        }
        let used = HashSet::from(["json".to_string()]);
        Ok(Self {
            options,
            used,
            positionals,
        })
    }

    /// The positional arguments, in order: the command first.
    pub fn positionals(&self) -> &[String] {
        &self.positionals
    }

    /// The value of an option, or `None` when it is absent or is a flag. Reading an option marks
    /// it as used by the command.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "called by the commands, from step 1b")
    )]
    pub fn option(&mut self, name: &str) -> Option<&str> {
        self.used.insert(name.to_string());
        self.options.get(name).and_then(|value| value.as_deref())
    }

    /// True for an option that takes no value, `--help` and `--version` included.
    fn is_flag(name: &str) -> bool {
        FLAGS.contains(&name) || name == HELP || name == VERSION
    }

    /// Help or version asked before `--`, as `--NAME` or as its shortcut, without parsing the
    /// line: answered before the bounds check and before any instance lookup. The value of an
    /// option is never taken for a shortcut: `--actor -v` keeps the actor `-v`.
    pub fn asks(arguments: &[String], name: &str, shortcut: &str) -> bool {
        let long = format!("--{name}");
        let mut index = 0;
        while index < arguments.len() {
            let argument = arguments[index].as_str();
            if argument == END_OF_OPTIONS {
                return false;
            }
            if argument == shortcut || argument == long {
                return true;
            }
            if Self::takes_value(arguments, index) {
                // The next argument is this option's value, not a shortcut.
                index += 1;
            }
            index += 1;
        }
        false
    }

    /// True when the argument at `index` is an option whose value is the next argument, with the
    /// rule of the parser: an option that is not a flag, followed by an argument that does not
    /// start with `--`.
    fn takes_value(arguments: &[String], index: usize) -> bool {
        let argument = arguments[index].as_str();
        argument.starts_with("--")
            && argument != END_OF_OPTIONS
            && !Self::is_flag(&argument[2..])
            && arguments
                .get(index + 1)
                .is_some_and(|next| !next.starts_with("--"))
    }

    /// Presence of a flag before the `--` separator, without parsing the line: used to choose
    /// the error format before any validation.
    pub fn has_flag(arguments: &[String], name: &str) -> bool {
        let flag = format!("--{name}");
        for argument in arguments {
            if argument == END_OF_OPTIONS {
                return false;
            }
            if *argument == flag {
                return true;
            }
        }
        false
    }
}

fn validate_bounds(arguments: &[String]) -> Result<()> {
    if arguments.len() > MAX_ARGUMENTS {
        return Err(SermofurError::invalid_arguments(ARGUMENT_COUNT_MESSAGE));
    }
    if arguments.iter().any(|argument| is_out_of_bounds(argument)) {
        return Err(SermofurError::invalid_arguments(ARGUMENT_MESSAGE));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::{CommandArguments, HELP, HELP_SHORTCUT, VERSION, VERSION_SHORTCUT};
    use crate::test_support::{arbitrary_line, command_line};

    /// The error that a command line must give, worked out without the parser: `None` when the
    /// line is valid. Written independently of `CommandArguments::parse` on purpose, so that a
    /// parser that rejects a valid line, or accepts an invalid one, is caught.
    fn expected_error(arguments: &[String]) -> Option<&'static str> {
        if arguments.len() > 128 {
            return Some("Too many arguments: 128 at most.");
        }
        let mut seen: HashSet<&str> = HashSet::new();
        let mut words = arguments.iter().map(String::as_str);
        while let Some(word) = words.next() {
            let Some(name) = word.strip_prefix("--") else {
                continue; // A positional argument.
            };
            if name.is_empty() {
                break; // The separator: everything after it is positional.
            }
            if !seen.insert(name) {
                return Some("Duplicate option.");
            }
            if !["json", "contradicts", "supervise"].contains(&name) {
                match words.next() {
                    Some(value) if !value.starts_with("--") => {}
                    _ => return Some("Option value required."),
                }
            }
        }
        None
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    fn error_message(values: &[&str]) -> String {
        CommandArguments::parse(&args(values))
            .expect_err("the line is invalid")
            .message
    }

    #[test]
    fn positionals_and_options_are_separated() {
        let parsed = CommandArguments::parse(&args(&["claim", "add", "--actor", "me", "text"]))
            .expect("valid");
        assert_eq!(parsed.positionals(), ["claim", "add", "text"]);
    }

    #[test]
    fn double_dash_makes_everything_positional() {
        let parsed =
            CommandArguments::parse(&args(&["claim", "--", "--text", "-h"])).expect("valid");
        assert_eq!(parsed.positionals(), ["claim", "--text", "-h"]);
    }

    #[test]
    fn a_repeated_option_is_refused() {
        assert_eq!(error_message(&["--json", "--json"]), "Duplicate option.");
        assert_eq!(
            error_message(&["--actor", "a", "--actor", "b"]),
            "Duplicate option."
        );
    }

    #[test]
    fn an_option_needs_a_value_that_does_not_start_with_double_dash() {
        assert_eq!(error_message(&["--path"]), "Option value required.");
        assert_eq!(error_message(&["--actor", "--x"]), "Option value required.");
    }

    #[test]
    fn too_many_or_too_long_arguments_are_refused() {
        let many = vec!["x".to_string(); 129];
        assert_eq!(
            CommandArguments::parse(&many)
                .expect_err("too many")
                .message,
            "Too many arguments: 128 at most."
        );
        let long = vec!["a".repeat(16_385)];
        assert_eq!(
            CommandArguments::parse(&long)
                .expect_err("too long")
                .message,
            "Argument limited to 16,384 characters without NUL."
        );
    }

    #[test]
    fn help_and_version_are_found_before_double_dash_only() {
        assert!(CommandArguments::asks(
            &args(&["claim", "-h"]),
            HELP,
            HELP_SHORTCUT
        ));
        assert!(CommandArguments::asks(
            &args(&["--version"]),
            VERSION,
            VERSION_SHORTCUT
        ));
        assert!(!CommandArguments::asks(
            &args(&["--", "-h"]),
            HELP,
            HELP_SHORTCUT
        ));
    }

    #[test]
    fn a_shortcut_is_not_found_as_the_value_of_an_option() {
        // `--actor -v` keeps the actor `-v`; but `--actor --version` cannot be a value.
        assert!(!CommandArguments::asks(
            &args(&["--actor", "-v"]),
            VERSION,
            VERSION_SHORTCUT
        ));
        assert!(CommandArguments::asks(
            &args(&["--actor", "--version"]),
            VERSION,
            VERSION_SHORTCUT
        ));
    }

    #[test]
    fn the_json_flag_is_found_before_double_dash_only() {
        assert!(CommandArguments::has_flag(&args(&["x", "--json"]), "json"));
        assert!(!CommandArguments::has_flag(
            &args(&["--", "--json"]),
            "json"
        ));
    }

    #[test]
    fn exactly_128_arguments_are_accepted() {
        let arguments = vec!["x".to_string(); 128];
        let parsed = CommandArguments::parse(&arguments).expect("128 arguments are within bounds");
        assert_eq!(parsed.positionals().len(), 128);
    }

    #[test]
    fn an_argument_of_exactly_16384_units_is_accepted() {
        let parsed = CommandArguments::parse(&["a".repeat(16_384)]).expect("within bounds");
        assert_eq!(parsed.positionals().len(), 1);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        // The parser refuses a line with exactly the message that the model predicts, and
        // accepts every line that the model finds valid.
        #[test]
        fn the_parser_agrees_with_an_independent_model(arguments in command_line()) {
            let actual = CommandArguments::parse(&arguments).err().map(|error| error.message);
            prop_assert_eq!(actual, expected_error(&arguments).map(str::to_string));
        }
    }

    proptest! {
        // Fewer cases: each one generates up to 130 arguments from regular expressions, which
        // is slow in a build without optimization.
        #![proptest_config(ProptestConfig::with_cases(300))]

        // Arbitrary text, up to 130 arguments: no panic anywhere, and the only failure is
        // `invalid_arguments`. Covers multi-byte text right after `--`, characters above the
        // Basic Multilingual Plane and the argument-count limit.
        #[test]
        fn arbitrary_text_never_panics(arguments in arbitrary_line()) {
            if let Err(error) = CommandArguments::parse(&arguments) {
                prop_assert_eq!(error.code, "invalid_arguments");
            }
            let _ = CommandArguments::asks(&arguments, HELP, HELP_SHORTCUT);
            let _ = CommandArguments::asks(&arguments, VERSION, VERSION_SHORTCUT);
            let _ = CommandArguments::has_flag(&arguments, "json");
        }
    }

    // Port of `CommandLineGrammarHoldsForAnyArguments` (FsCheck, 1,000 cases in .NET).
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(1000))]

        #[test]
        fn the_grammar_holds_for_any_arguments(arguments in command_line()) {
            match CommandArguments::parse(&arguments) {
                Err(error) => prop_assert_eq!(error.code, "invalid_arguments"),
                Ok(mut parsed) => {
                    // Everything after the first `--` is positional, unchanged.
                    let tail: &[String] = match arguments.iter().position(|a| a == "--") {
                        Some(separator) => &arguments[separator + 1..],
                        None => &[],
                    };
                    let positionals = parsed.positionals().to_vec();
                    prop_assert!(positionals.len() >= tail.len());
                    let split = positionals.len() - tail.len();
                    prop_assert_eq!(&positionals[split..], tail);
                    // Before it, no positional starts with `--`.
                    prop_assert!(positionals[..split].iter().all(|v| !v.starts_with("--")));
                    // An option value never starts with `--`.
                    for name in ["path", "actor", "text", "x"] {
                        if let Some(value) = parsed.option(name) {
                            prop_assert!(!value.starts_with("--"));
                        }
                    }
                }
            }
        }
    }
}
