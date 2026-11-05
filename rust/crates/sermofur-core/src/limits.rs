//! Bounds of text inputs, shared by the command line and the services (`InputLimits` in .NET).

/// Maximum length of a text or an argument, in UTF-16 code units.
pub const MAX_TEXT_LENGTH: usize = 16_384;

/// Maximum number of arguments on a command line.
pub const MAX_ARGUMENTS: usize = 128;

/// Message for a command line that exceeds the number of arguments.
pub const ARGUMENT_COUNT_MESSAGE: &str = "Too many arguments: 128 at most.";

/// Message for an out-of-bounds command-line argument.
pub const ARGUMENT_MESSAGE: &str = "Argument limited to 16,384 characters without NUL.";

/// True if the value is longer than [`MAX_TEXT_LENGTH`] or contains a NUL character.
///
/// The .NET code counts `string.Length`, that is UTF-16 code units, not characters and not
/// bytes: an emoji counts for two. The port counts the same way so that both tools accept and
/// refuse exactly the same arguments.
pub fn is_out_of_bounds(value: &str) -> bool {
    value.encode_utf16().count() > MAX_TEXT_LENGTH || value.contains('\0')
}

#[cfg(test)]
mod tests {
    use super::is_out_of_bounds;
    use super::{ARGUMENT_COUNT_MESSAGE, ARGUMENT_MESSAGE, MAX_ARGUMENTS, MAX_TEXT_LENGTH};

    #[test]
    fn the_limit_itself_is_accepted() {
        assert!(!is_out_of_bounds(&"a".repeat(MAX_TEXT_LENGTH)));
        assert!(is_out_of_bounds(&"a".repeat(MAX_TEXT_LENGTH + 1)));
    }

    #[test]
    fn length_counts_utf16_code_units() {
        // 8,192 emoji are 16,384 code units (accepted); one more makes 16,386.
        assert!(!is_out_of_bounds(&"😀".repeat(MAX_TEXT_LENGTH / 2)));
        assert!(is_out_of_bounds(&"😀".repeat(MAX_TEXT_LENGTH / 2 + 1)));
        // 'é' is a single code unit, although two bytes in UTF-8.
        assert!(!is_out_of_bounds(&"é".repeat(MAX_TEXT_LENGTH)));
    }

    #[test]
    fn nul_is_refused() {
        assert!(is_out_of_bounds("a\0b"));
    }

    #[test]
    fn messages_quote_the_limits() {
        assert_eq!(
            ARGUMENT_COUNT_MESSAGE,
            format!("Too many arguments: {MAX_ARGUMENTS} at most.")
        );
        assert!(ARGUMENT_MESSAGE.contains("16,384"));
    }
}
