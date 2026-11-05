//! The header and the general usage text, ported from `CommandHelp.cs` and `CommandRunner.cs`.
//!
//! Help for one command (`smf claim add -h`) is not ported yet: it comes after step 1c.

use sermofur_core::VERSION;

const REPOSITORY: &str = "https://github.com/JoRouquette/sermofur";

/// Header shared by every help output: `Sermofur <version>`.
pub fn header() -> String {
    format!("Sermofur {VERSION}")
}

/// CLI reference for `version`: the documentation of its release tag, or that of `main` for a
/// build that carries a pre-release label, such as a development build.
pub fn cli_documentation(version: &str) -> String {
    if version.contains('-') {
        format!("{REPOSITORY}/blob/main/docs/cli.md")
    } else {
        format!("{REPOSITORY}/blob/v{version}/docs/cli.md")
    }
}

/// Lines of the usage text between the header and the last sentence, as the .NET tool prints
/// them.
const USAGE_LINES: [&str; 24] = [
    "Global options: [--path DIRECTORY] [--json]",
    "smf init | root | status | doctor | export | migrate",
    "smf scope current | list | tree",
    "smf scope add ID KIND PARENT RELATIVE_PATH",
    "smf claim add TEXT --origin user|llm [--category CATEGORY] [--volatility VOLATILITY] [--actor NAME] [--key KEY]",
    "smf claim list | show ID | invalidate ID --reason TEXT [--actor NAME]",
    "smf evidence add CLAIM_ID KIND REFERENCE --lineage ORIGIN --origin user|llm [--contradicts] [--source SOURCE_ID] [--actor NAME] [--key KEY]",
    "smf evidence list | show ID",
    "smf retex add --event TEXT --impact TEXT --next TEXT --origin user|llm [--actor NAME] [--key KEY]",
    "smf retex list | show ID",
    "smf source add FILE --origin user|llm [--actor NAME]",
    "smf source list | show ID | reindex [ID] [--actor NAME]",
    "smf index rebuild",
    "smf recall QUESTION [--limit 1-3]",
    "smf challenge CLAIM_ID | challenge --text TEXT",
    "smf daemon install | uninstall | start | stop | restart | status",
    "smf daemon register | unregister | instances",
    "smf daemon run [--supervise]",
    "smf mcp install | uninstall [--host claude-code|codex] [--scope project|user]",
    "smf mcp serve",
    "smf COMMAND -h | smf GROUP -h       help of one command, or the subcommands of a group",
    "smf --version | -v                  version",
    "--help (-h) and --version (-v) are recognized anywhere before --, except as an option value; --help wins.",
    "-- ends options: every following argument is positional (text starting with --).",
];

/// The general usage text, printed for `smf`, `smf -h` and `smf --help`.
pub fn usage() -> String {
    let mut text = header();
    for line in USAGE_LINES {
        text.push('\n');
        text.push_str(line);
    }
    text.push_str(&format!(
        "\nAn option value cannot start with --; see {} for values and exit codes.",
        cli_documentation(VERSION)
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::{cli_documentation, header, usage};

    #[test]
    fn header_names_the_tool_and_its_version() {
        assert_eq!(header(), "Sermofur 0.0.0-dev");
    }

    #[test]
    fn a_prerelease_points_to_the_documentation_of_main() {
        assert_eq!(
            cli_documentation("0.0.0-dev"),
            "https://github.com/JoRouquette/sermofur/blob/main/docs/cli.md"
        );
        assert_eq!(
            cli_documentation("0.4.3"),
            "https://github.com/JoRouquette/sermofur/blob/v0.4.3/docs/cli.md"
        );
    }

    #[test]
    fn usage_starts_with_the_header_and_ends_with_the_documentation_link() {
        let text = usage();
        assert!(text.starts_with("Sermofur 0.0.0-dev\nGlobal options:"));
        assert!(text.ends_with("docs/cli.md for values and exit codes."));
    }
}
