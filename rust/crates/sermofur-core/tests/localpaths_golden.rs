//! `sermofur_core::local_paths` against the tables that the .NET code produced
//! (`rust/tests/golden/localpaths.json` on Windows, `localpaths-unix.json` on Linux), regenerated
//! by `rust/tools/localpaths-oracle`.

use std::fs;
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};
#[cfg(windows)]
use std::process::Command;

use serde_json::{Value, json};
use sermofur_core::error::SermofurError;
use sermofur_core::local_paths::{
    canonical_case, contains, normalize_mapping, relativize_mapping, resolve_mapping,
};
use uuid::Uuid;

/// The temporary folder as the oracle sees it: `Path.GetFullPath(Path.GetTempPath())`, so 8.3
/// names expanded on Windows; canonical elsewhere (on macOS `/var` is a link).
fn temp_base() -> PathBuf {
    let dir = std::env::temp_dir();
    if cfg!(windows) {
        sermofur_core::discovery::expand_short_names(&dir)
    } else {
        fs::canonicalize(dir).expect("the temporary directory resolves")
    }
}

/// The crafted folder of the oracle; drop restores the listing right and removes it.
struct Tree {
    root: PathBuf,
}

impl Drop for Tree {
    fn drop(&mut self) {
        let locked = self.root.join("Locked");
        #[cfg(windows)]
        {
            let user = std::env::var("USERNAME").unwrap_or_default();
            let _ = Command::new("icacls")
                .args([locked.to_str().unwrap_or_default(), "/remove:d", &user])
                .output();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&locked, fs::Permissions::from_mode(0o700));
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// The tree and the GUID in its name, which holds an "i" and an "s" as the oracle's does.
fn build_tree() -> (Tree, String) {
    let guid = Uuid::new_v4().simple().to_string();
    let root = temp_base().join(format!("sermofur-localpaths-oracle-is-{guid}"));
    let tree = Tree { root: root.clone() };
    let mkdir = |relative: &str| fs::create_dir_all(root.join(relative)).expect("folder");
    mkdir("Alpha/Beta");
    fs::write(root.join("Alpha/file.txt"), "x").expect("file");
    mkdir("caf\u{e9}");
    mkdir("nfd-e\u{301}");
    mkdir("\u{212A}elvin");
    mkdir("Long Folder Name");
    // A dangling link, typed in another case by the CanonicalCase rows.
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(root.join("nowhere"), root.join("Dangle"));
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(root.join("nowhere"), root.join("Dangle"));
    let allow_skip = std::env::var("SERMOFUR_ALLOW_SYMLINK_SKIP").is_ok_and(|v| v == "1");
    assert!(
        linked.is_ok() || allow_skip,
        "symlink creation refused (SERMOFUR_ALLOW_SYMLINK_SKIP=1 allows it): {linked:?}"
    );
    mkdir("\u{17F}tate");
    mkdir("\u{10428}x");
    mkdir("Locked/inner");
    #[cfg(windows)]
    {
        mkdir("dup-\u{e9}");
        mkdir("dup-e\u{301}");
        let user = std::env::var("USERNAME").expect("USERNAME");
        let out = Command::new("icacls")
            .args([
                root.join("Locked").to_str().expect("UTF-8"),
                "/deny",
                &format!("{user}:(RD)"),
            ])
            .output()
            .expect("icacls");
        assert!(out.status.success(), "icacls failed: {out:?}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root.join("Locked"), fs::Permissions::from_mode(0o300)).expect("mode");
    }
    (tree, guid)
}

/// The 8.3 alias of "Long Folder Name" (`%~sI` of cmd), or the long name without 8.3 names.
#[cfg(windows)]
fn short_name(path: &Path) -> String {
    use std::os::windows::process::CommandExt;
    // cmd does not read the quotes escaped by the standard library: the line is passed raw.
    let out = Command::new("cmd")
        .raw_arg(format!(
            "/C for %I in (\"{}\") do @echo %~sI",
            path.display()
        ))
        .output()
        .expect("cmd");
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    Path::new(&text).file_name().map_or_else(
        || "Long Folder Name".to_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(not(windows))]
fn short_name(_path: &Path) -> String {
    "Long Folder Name".to_owned()
}

/// Whether the tests run as root, for whom a folder that cannot be listed does not exist.
#[cfg(unix)]
fn runs_as_root() -> bool {
    use std::os::unix::fs::MetadataExt;
    let probe = temp_base().join(format!("sermofur-uid-{}", Uuid::new_v4()));
    fs::write(&probe, "").expect("probe");
    let uid = fs::metadata(&probe).expect("probe").uid();
    let _ = fs::remove_file(&probe);
    uid == 0
}

#[test]
fn local_paths_match_the_oracle() {
    let (file, os) = if cfg!(windows) {
        ("localpaths.json", "windows")
    } else {
        ("localpaths-unix.json", "unix")
    };
    let path = format!("{}/../../tests/golden/{file}", env!("CARGO_MANIFEST_DIR"));
    let golden: Value =
        serde_json::from_str(&fs::read_to_string(path).expect("golden file")).expect("JSON");
    assert_eq!(golden["os"], os);
    #[cfg(unix)]
    let root_user = runs_as_root();
    #[cfg(windows)]
    let root_user = false;
    let (tree, guid) = build_tree();
    let root = tree.root.clone();
    let root_text = root.to_str().expect("UTF-8").to_owned();
    let temp_text = temp_base().to_str().expect("UTF-8").to_owned();
    let volume = format!(
        "{}{MAIN_SEPARATOR_STR}",
        root_text.split(MAIN_SEPARATOR_STR).next().expect("volume")
    );
    let recorded_short = golden["shortName"].as_str().expect("shortName").to_owned();
    let short = short_name(&root.join("Long Folder Name"));
    // A path argument shown by the oracle, back to a real path of this machine.
    let real = |shown: &str| -> PathBuf {
        let text = shown
            .replace("<root>", &root_text)
            .replace("<temp>", &temp_text)
            .replace("<volume>", &volume)
            .replace("<guid>", &guid)
            .replace("<GUID>", &guid.to_uppercase());
        PathBuf::from(if cfg!(windows) {
            text.replace('/', "\\")
        } else {
            text
        })
    };
    let show = |text: &str| {
        text.replace(&root_text, "<root>")
            .replace(&temp_text, "<temp>")
            .replace(&guid, "<guid>")
            .replace(&guid.to_uppercase(), "<GUID>")
            .replace('\\', "/")
    };
    let error =
        |e: SermofurError| json!({"code": e.code, "message": e.message, "exitCode": e.exit_code});
    let mut mismatches = Vec::new();
    let mut skipped = Vec::new();
    for call in golden["calls"].as_array().expect("calls") {
        let function = call["function"].as_str().expect("function");
        let arguments = call["arguments"].as_array().expect("arguments");
        let shown_second = arguments[1].as_str().expect("argument");
        let second_text = shown_second
            .replace("<guid>", &guid)
            .replace("<GUID>", &guid.to_uppercase());
        let second = second_text.as_str();
        // The table of Linux (ext4) serves macOS too, but APFS ignores case and normalization:
        // the answers that depend on the file system, or on the case rule of GetRelativePath
        // (case-insensitive on macOS), are not compared there.
        if cfg!(target_os = "macos")
            && matches!(
                function,
                "ResolveMapping" | "CanonicalCase" | "RelativizeMapping"
            )
        {
            skipped.push(format!(
                "{function} {shown_second}: macOS has no table of its own"
            ));
            continue;
        }
        // The alias rows hold the short name of the machine that recorded them.
        if second.contains("LONGFO~1") && short != recorded_short {
            skipped.push(format!(
                "{function} {second}: no 8.3 name {recorded_short} here"
            ));
            continue;
        }
        if root_user && second.starts_with("locked") {
            skipped.push(format!("{function} {second}: root lists every folder"));
            continue;
        }
        let outcome: Value = match function {
            "Contains" => json!({"result": contains(
                &real(arguments[0].as_str().expect("parent")),
                &real(shown_second),
            )}),
            "RelativizeMapping" => match relativize_mapping(&root, &real(shown_second)) {
                Ok(text) => json!({"result": show(&text)}),
                Err(e) => error(e),
            },
            "NormalizeMapping" | "ResolveMapping" => {
                let result = if function == "NormalizeMapping" {
                    normalize_mapping(&root, second)
                } else {
                    resolve_mapping(&root, second)
                };
                match result {
                    Ok(path) => json!({"result": show(path.to_str().expect("UTF-8"))}),
                    Err(e) => error(e),
                }
            }
            "CanonicalCase" => match canonical_case(&root, second) {
                Ok(text) => json!({"result": text}),
                Err(e) => error(e),
            },
            other => panic!("unknown function {other}"),
        };
        let mut expected = call.clone();
        let expected = expected.as_object_mut().expect("object");
        expected.remove("function");
        expected.remove("arguments");
        if Value::Object(expected.clone()) != outcome {
            mismatches.push(format!(
                "{function} {second:?}: expected {}, got {outcome}",
                Value::Object(expected.clone())
            ));
        }
    }
    eprintln!("skipped: {skipped:?}");
    drop(tree);
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
}
