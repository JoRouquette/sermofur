//! Sermofur daemon (layer 7 of the Rust port).
//!
//! Lot 7.1: frames and messages of protocol 1 ([`framing`]), checked against the .NET
//! `Sermofur.Daemon` used as an oracle.

pub mod framing;

mod relaxed_escape_table;
