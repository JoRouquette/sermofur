//! The error type of every Sermofur operation.
//!
//! It mirrors `SermofurException` of the .NET code: a stable machine code, a short English
//! message and the exit code of the process. Codes and exit codes are part of the contract
//! (`docs/cli.md`); messages may change.

use thiserror::Error;

/// A failure that the command line reports as `code: message` and an exit code.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{code}: {message}")]
pub struct SermofurError {
    /// Stable machine-readable code, such as `invalid_arguments`.
    pub code: String,
    /// Short English text for people.
    pub message: String,
    /// Exit code of the process.
    pub exit_code: i32,
}

/// Result of a Sermofur operation.
pub type Result<T> = std::result::Result<T, SermofurError>;

impl SermofurError {
    /// Builds an error from its three parts.
    pub fn new(code: impl Into<String>, message: impl Into<String>, exit_code: i32) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            exit_code,
        }
    }

    /// An invalid command line: `invalid_arguments`, exit code 1.
    pub fn invalid_arguments(message: impl Into<String>) -> Self {
        Self::new("invalid_arguments", message, 1)
    }
}

#[cfg(test)]
mod tests {
    use super::SermofurError;

    #[test]
    fn invalid_arguments_exits_with_one() {
        let error = SermofurError::invalid_arguments("Duplicate option.");
        assert_eq!(error.code, "invalid_arguments");
        assert_eq!(error.exit_code, 1);
        assert_eq!(error.to_string(), "invalid_arguments: Duplicate option.");
    }
}
