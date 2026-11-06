//! The database written by Rust has the same `sqlite_master` as the one written by the .NET
//! `smf init` (the oracle). The golden file is recorded once from the oracle:
//!
//! ```text
//! SMF_UPDATE_GOLDEN=1 SMF_ORACLE=<path of Sermofur.Cli.exe> cargo test -p sermofur-core --test schema_golden
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;

use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use sermofur_core::schema;
use uuid::Uuid;

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/schema-v2.json")
}

fn scratch(name: &str) -> PathBuf {
    let directory = std::env::temp_dir().join(format!("sermofur-{name}-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&directory).expect("directory");
    directory
}

/// Rows of `sqlite_master` and the schema version, in creation order.
fn describe(database: &Path) -> Value {
    let connection = Connection::open_with_flags(database, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("the database opens");
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .expect("version");
    let mut statement = connection
        .prepare("SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY rowid")
        .expect("statement");
    let rows: Vec<Value> = statement
        .query_map([], |row| {
            Ok(json!({
                "type": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "table": row.get::<_, String>(2)?,
                "sql": row.get::<_, Option<String>>(3)?,
            }))
        })
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("every row reads");
    let scopes: Vec<String> = connection
        .prepare("SELECT id||'|'||kind||'|'||ifnull(parent_id,'')||'|'||ifnull(relative_path,'') FROM scopes")
        .expect("statement")
        .query_map([], |row| row.get(0))
        .expect("rows")
        .collect::<Result<_, _>>()
        .expect("scopes");
    json!({ "userVersion": version, "master": rows, "scopes": scopes })
}

#[test]
fn the_rust_schema_equals_the_dotnet_schema() {
    if std::env::var("SMF_UPDATE_GOLDEN").is_ok_and(|value| value == "1") {
        let oracle = std::env::var("SMF_ORACLE").expect("SMF_ORACLE names the oracle program");
        let root = scratch("oracle-init");
        let status = Command::new(oracle)
            .args(["init", "--path"])
            .arg(&root)
            .env("SERMOFUR_NO_DAEMON", "1")
            .current_dir(&root)
            .status()
            .expect("the oracle starts");
        assert!(status.success(), "the oracle initialized the instance");
        let recorded = describe(&root.join(".sermofur").join("memory.db"));
        std::fs::create_dir_all(golden_path().parent().expect("a parent")).expect("directory");
        std::fs::write(
            golden_path(),
            serde_json::to_string_pretty(&recorded).expect("serializes") + "\n",
        )
        .expect("the golden file is written");
    }

    let golden: Value = serde_json::from_str(
        &std::fs::read_to_string(golden_path()).expect("the golden file exists"),
    )
    .expect("the golden file is valid JSON");

    let database = scratch("rust-init").join("memory.db");
    schema::create_database(&database, Uuid::new_v4()).expect("created");
    assert_eq!(describe(&database), golden);
}
