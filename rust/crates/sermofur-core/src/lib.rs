//! Sermofur core library.
//!
//! The Rust port is verified against the .NET `smf` 0.4.3 used as an oracle.

pub mod atomic;
mod casing;
pub mod challenge;
pub mod collation;
pub mod confidence;
pub mod daemon_paths;
pub mod discovery;
pub mod doctor;
pub mod error;
pub mod format;
pub mod guid;
pub mod init;
pub mod instance;
pub mod json;
pub mod limits;
pub mod local_paths;
pub mod memory;
pub mod migration;
pub mod ownership;
pub mod projection;
pub mod recall;
pub mod records;
pub mod schema;
pub mod scope_policy;
pub mod scopes;
pub mod search;
pub mod source_service;
pub mod sources;
pub mod stj;
pub mod store;

mod dotnet_escape_table;
#[cfg(windows)]
mod windows;

/// The line break of the platform, as `Environment.NewLine` and `TextWriter.WriteLine` give it
/// in .NET: `\r\n` on Windows, `\n` elsewhere. The JSON writer of the .NET tool uses it between
/// its lines, so the port does too.
pub const NEW_LINE: &str = if cfg!(windows) { "\r\n" } else { "\n" };

/// Version of the port: the release build injects the version of the tag through the
/// compile-time variable `SMF_BUILD_VERSION` (lot 10, `.github/scripts/package-rust.sh`), so
/// `Cargo.toml` keeps the development version of the oracle. Without it, the crate manifest
/// gives the version.
pub const VERSION: &str = match option_env!("SMF_BUILD_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_the_development_version_of_the_oracle() {
        assert_eq!(VERSION, "0.0.0-dev");
    }
}
