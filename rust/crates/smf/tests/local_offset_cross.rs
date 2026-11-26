//! Decision A5 end to end, against the .NET `smf` (`SMF_ORACLE`) under the same zone of the
//! machine: dates written without offset go through the production code of both tools.
//! - `migrate` rewrites an `instance.json` whose `createdAt` has no offset (lot 6.1): the
//!   rewritten files are compared byte for byte.
//! - `recall`, `claim list` and `claim show` read claims whose stored timestamp has no offset
//!   (`ReadSummaries` of lot 4.1, and the STJ reading of the record): the outputs are compared
//!   byte for byte. One timestamp is written with an offset and the other without, one hour and a
//!   half later in the local clock, so that their order depends on the local offset.

mod common;

use std::fs;

use common::{Scratch, arguments, copy_legacy, printed_id, run, tamper};

fn oracle() -> Option<String> {
    let oracle = std::env::var("SMF_ORACLE").ok();
    if oracle.is_none() {
        eprintln!("A5 cross-check skipped: SMF_ORACLE is not set");
    }
    oracle
}

#[test]
fn migrate_rewrites_a_created_at_without_offset_as_dotnet_does() {
    let Some(oracle) = oracle() else { return };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    for created_at in [
        "2026-07-01T13:30:00.1234567",
        "2026-01-15T08:00:00",
        "2026-03-29T02:30:00",
        "2026-10-25T02:30:00",
    ] {
        let mut rewritten = Vec::new();
        for program in [&rust, &oracle] {
            let scratch = Scratch::new();
            let root = &scratch.0;
            copy_legacy(root);
            let file = root.join(".sermofur").join("instance.json");
            let text = fs::read_to_string(&file).expect("configuration");
            let start = text.find("\"createdAt\": \"").expect("createdAt") + 14;
            let end = start + text[start..].find('"').expect("end of createdAt");
            fs::write(
                &file,
                format!("{}{created_at}{}", &text[..start], &text[end..]),
            )
            .expect("configuration without offset");
            let migrated = run(program, &arguments(&["--json", "migrate"]), root);
            assert_eq!(migrated.exit_code, 0, "{}", migrated.stderr);
            rewritten.push(fs::read(&file).expect("rewritten configuration"));
        }
        assert_eq!(
            String::from_utf8_lossy(&rewritten[0]),
            String::from_utf8_lossy(&rewritten[1]),
            "createdAt {created_at}"
        );
    }
}

#[test]
fn stored_timestamps_without_offset_are_read_as_dotnet_reads_them() {
    let Some(oracle) = oracle() else { return };
    let rust = env!("CARGO_BIN_EXE_smf").to_owned();
    let scratch = Scratch::new();
    let root = &scratch.0;
    let init = run(&rust, &arguments(&["init"]), root);
    assert_eq!(init.exit_code, 0, "{}", init.stderr);
    let add = |text: &str| {
        printed_id(&run(
            &rust,
            &arguments(&["--json", "claim", "add", text, "--origin", "user"]),
            root,
        ))
    };
    let summer = add("cache chaud");
    let winter = add("cache chaud");
    tamper(
        root,
        &format!(
            "UPDATE records SET payload=json_set(payload,'$.provenance.timestamp','2026-07-01T12:00:00+00:00') WHERE id='{summer}';
             UPDATE records SET payload=json_set(payload,'$.provenance.timestamp','2026-07-01T13:30:00') WHERE id='{winter}';"
        ),
    );
    for command in [
        vec!["--json", "recall", "cache"],
        vec!["recall", "cache"],
        vec!["--json", "claim", "list"],
        vec!["--json", "claim", "show", &summer],
        vec!["--json", "claim", "show", &winter],
    ] {
        let ours = run(&rust, &arguments(&command), root);
        let theirs = run(&oracle, &arguments(&command), root);
        assert_eq!(
            (ours.exit_code, &ours.stdout, &ours.stderr),
            (theirs.exit_code, &theirs.stdout, &theirs.stderr),
            "{command:?}"
        );
    }
}
