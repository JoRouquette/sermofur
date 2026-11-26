//! Short names for the scratch folders of the tests that give a daemon a home. Test code only,
//! shared by `#[path]` between the crates.
//!
//! Under Unix the socket of a daemon is `<home>/run/daemon.sock`, refused past 103 bytes
//! (`invalid_path`, as .NET). A macOS runner has a TMPDIR of the form
//! `/private/var/folders/xx/<30 characters>/T`: 56 bytes once canonical. Every home that a test
//! builds with these names keeps [`SPARE_BYTES`] under the limit with such a TMPDIR.

use std::sync::atomic::{AtomicUsize, Ordering};

/// A canonical macOS runner TMPDIR, the longest the tests plan for.
pub const LONG_TMPDIR_BYTES: usize = 56;

/// Bytes kept free under the limit with that TMPDIR.
pub const SPARE_BYTES: usize = 15;

/// The folder of the daemon home inside a scratch folder.
pub const HOME: &str = "h";

/// The bytes of `<a TMPDIR of LONG_TMPDIR_BYTES>/<relative>/run/daemon.sock`.
pub fn socket_bytes(relative: &str) -> usize {
    LONG_TMPDIR_BYTES + "/".len() + relative.len() + "/run/daemon.sock".len()
}

/// Panics, naming `test`, unless the socket of the home `relative` keeps [`SPARE_BYTES`] under
/// the limit with a TMPDIR of [`LONG_TMPDIR_BYTES`] (see [`socket_bytes`]).
pub fn assert_fits(test: &str, relative: &str) {
    let bytes = socket_bytes(relative);
    let limit = sermofur_core::daemon_paths::MAX_SOCKET_PATH_BYTES;
    assert!(
        bytes + SPARE_BYTES <= limit,
        "test {test}: the daemon socket under {relative:?} would take {bytes} bytes with a \
         TMPDIR of {LONG_TMPDIR_BYTES} bytes (macOS runner); the budget is {} ({limit} minus \
         {SPARE_BYTES} spare). Shorten the scratch folder.",
        limit - SPARE_BYTES
    );
}

/// A fresh folder name `<tag><pid>-<n>`, unique in the process (one tag per helper of a crate)
/// and between processes; its home [`HOME`] fits the budget (see [`assert_fits`]). The guard
/// runs on every system, to show under Windows and Linux an overrun meant for macOS: a name fits
/// while the pid and the count take 10 digits together (a 7-digit pid and a 3-digit count).
pub fn name(tag: &str, test: &str) -> String {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    let name = format!(
        "{tag}{}-{}",
        std::process::id(),
        COUNT.fetch_add(1, Ordering::SeqCst)
    );
    assert_fits(test, &format!("{name}/{HOME}"));
    name
}

/// The budget refuses a folder of the old form that failed on a macOS runner, and admits the
/// longest name of this module: a 7-digit pid (Linux `pid_max`) and a 3-digit count.
#[test]
fn the_budget_refuses_the_folders_that_failed_on_macos() {
    let budget = sermofur_core::daemon_paths::MAX_SOCKET_PATH_BYTES - SPARE_BYTES;
    assert_eq!(socket_bytes("smfv-stop-waiting-read-4076-7/home"), 107);
    assert!(socket_bytes("smfv-stop-waiting-read-4076-7/home") > budget);
    assert_eq!(socket_bytes(&format!("su4194304-999/{HOME}")), budget);
    assert_fits("the longest name", &format!("su4194304-999/{HOME}"));
}

/// The guard fails, and says what to do, past the budget.
#[test]
#[should_panic(expected = "Shorten the scratch folder")]
fn the_guard_fails_past_the_budget() {
    assert_fits("an old name", "smfv-stop-waiting-read-4076-7/home");
}
