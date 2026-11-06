//! Sermofur core library.
//!
//! The Rust port is verified against the .NET `smf` 0.4.3 used as an oracle.

pub mod atomic;
mod casing;
pub mod confidence;
pub mod discovery;
pub mod error;
pub mod format;
pub mod guid;
pub mod init;
pub mod instance;
pub mod json;
pub mod limits;
pub mod local_paths;
pub mod memory;
pub mod ownership;
pub mod projection;
pub mod records;
pub mod schema;
pub mod scope_policy;
pub mod scopes;
mod stj;
pub mod store;

mod dotnet_escape_table;
#[cfg(windows)]
mod windows;

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
