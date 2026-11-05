//! Sermofur core library.
//!
//! The Rust port is verified against the .NET `smf` 0.4.3 used as an oracle.

pub mod error;
pub mod json;
pub mod limits;

mod dotnet_escape_table;

/// The line break of the platform, as `Environment.NewLine` and `TextWriter.WriteLine` give it
/// in .NET: `\r\n` on Windows, `\n` elsewhere. The JSON writer of the .NET tool uses it between
/// its lines, so the port does too.
pub const NEW_LINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// Version of the port, taken from the crate manifest.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_the_development_version_of_the_oracle() {
        assert_eq!(VERSION, "0.0.0-dev");
    }
}
