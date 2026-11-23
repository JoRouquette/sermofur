//! Guard of the release version (lot 10). The release build injects the version of the tag
//! through the compile-time variable `SMF_BUILD_VERSION`; without it, the version stays the
//! development version of the oracle. `smf --version` must print exactly the value the build
//! received, so a release can never ship a binary that names another version.

use std::process::Command;

#[test]
fn version_is_the_injected_one() {
    let expected = option_env!("SMF_BUILD_VERSION").unwrap_or("0.0.0-dev");
    let output = Command::new(env!("CARGO_BIN_EXE_smf"))
        .arg("--version")
        .output()
        .expect("smf runs");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("UTF-8");
    assert_eq!(stdout.trim_end(), format!("Sermofur {expected}"));
}
