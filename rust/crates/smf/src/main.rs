//! `smf`, the Sermofur command line (Rust port, work in progress).

mod args;
mod help;
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
    let code = runner::run(
        &arguments,
        &mut io::stdout().lock(),
        &mut io::stderr().lock(),
    );
    ExitCode::from(u8::try_from(code).unwrap_or(70))
}
