//! `smf`, the Sermofur command line (Rust port, work in progress).

mod args;
mod daemon_run;
mod help;
mod memory_commands;
mod router;
mod runner;
#[cfg(test)]
mod test_support;

use std::io;
use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os` never fails on an argument that is not valid Unicode; such an argument is
    // converted with replacement characters, as the .NET runtime does.
    let arguments: Vec<String> = std::env::args_os()
        .skip(1)
        .map(|argument| argument.to_string_lossy().into_owned())
        .collect();
    let code = router::route(
        &arguments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(70))
}
