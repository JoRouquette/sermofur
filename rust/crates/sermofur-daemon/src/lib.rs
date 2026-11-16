//! Sermofur daemon (layer 7 of the Rust port).
//!
//! Lot 7.1: frames and messages of protocol 1 ([`framing`]). Lot 7.2: the shared files of the
//! daemon: its locations ([`daemon_paths`], from the core, where `doctor` reads them), its lock
//! files ([`file_turn`]), its journal ([`log`]) and its registry ([`registry`]). Lot 7.3: the local endpoint
//! ([`endpoint`]), a pipe or a socket, on tokio (decision A2). Each is checked against the .NET `Sermofur.Daemon` used as an oracle.

pub use sermofur_core::daemon_paths;

pub mod endpoint;
pub mod file_turn;
pub mod framing;
pub mod log;
pub mod registry;

mod relaxed_escape_table;
