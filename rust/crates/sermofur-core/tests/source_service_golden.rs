//! Differential test of lot 5.2 against `rust/tests/golden/source-service.json` (Windows) and
//! `source-service-unix.json` (Linux), from `rust/tools/source-service-oracle`: the narrowest
//! scope of a stored path (`SourcePlacement.NarrowerScope`) and the table of transitions of a
//! source (`SourceService.Transition`). Then the port of `SourceTests` and of the source part of
//! `HardeningTests` (`SpecialFileIsNotReadAsASource`) on a real instance. Through the command
//! line: `smf/tests/golden_cli_5_2.rs`.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use serde_json::Value;
use sermofur_core::discovery::expand_short_names;
use sermofur_core::error::Result;
use sermofur_core::format::{DateTimeOffset, parse_json_text};
use sermofur_core::init::initialize;
use sermofur_core::memory::{MemoryContext, MemoryService};
use sermofur_core::records::{
    ActorKind, ClaimContent, DotNetEnum, EvidenceContent, EvidenceKind, EvidenceRelation,
    KnowledgeStatus, MemoryCategory, MemoryRecord, Provenance, RecordKind, Scope, ScopeKind,
    SourceContent, SourceRejection, SourceStatus, Volatility, read, write,
};
use sermofur_core::source_service::{
    ReindexOutcome, SYSTEM_ACTOR, SourceReader, SourceService, content, file_reader,
    narrower_scope, transition,
};
use sermofur_core::sources::SourceSnapshot;
use sermofur_core::store::{Store, StoredScope};
use uuid::Uuid;

fn golden() -> Value {
    let name = if cfg!(windows) {
        "source-service.json"
    } else {
        "source-service-unix.json"
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name);
    serde_json::from_str(&fs::read_to_string(path).expect("golden file")).expect("JSON")
}

fn member<E: DotNetEnum>(json_name: &str) -> E {
    E::NAMES
        .iter()
        .find(|(name, json, _)| *json == json_name || *name == json_name)
        .map(|(_, _, member)| *member)
        .unwrap_or_else(|| panic!("member {json_name}"))
}

fn kind_index(json_name: &str) -> i64 {
    let index = ScopeKind::NAMES
        .iter()
        .position(|(_, json, _)| *json == json_name)
        .expect("kind");
    i64::try_from(index).expect("small")
}

#[test]
fn the_narrowest_scope_is_the_one_of_the_oracle() {
    let golden = golden();
    let mut count = 0;
    for placement in golden["placements"].as_array().expect("placements") {
        let scopes: Vec<StoredScope> = placement["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .map(|scope| StoredScope {
                id: scope["id"].as_str().expect("id").to_owned(),
                kind: kind_index(scope["kind"].as_str().expect("kind")),
                parent_id: scope["parentId"].as_str().map(str::to_owned),
                relative_path: scope["relativePath"].as_str().map(str::to_owned),
            })
            .collect();
        for case in placement["cases"].as_array().expect("cases") {
            let path = case["path"].as_str().expect("path");
            let owner = case["owner"].as_str().expect("owner");
            let found = narrower_scope(path, owner, &scopes).expect("valid tree");
            assert_eq!(
                found.map(|scope| scope.id.as_str()),
                case["narrower"].as_str(),
                "{}: {path} from {owner}",
                placement["name"]
            );
            count += 1;
        }
    }
    assert!(count > 3000, "{count} placements");
}

fn stamp() -> DateTimeOffset {
    parse_json_text("1999-01-01T00:00:00+00:00", |_| 0).expect("date")
}

#[test]
fn the_transitions_are_the_table_of_the_oracle() {
    let golden = golden();
    let transitions = golden["transitions"].as_array().expect("transitions");
    assert_eq!(transitions.len(), 540);
    for case in transitions {
        let current: MemoryRecord =
            read(case["current"].as_str().expect("current"), &|_| 0).expect("record");
        let shot = &case["snapshot"];
        let status: SourceStatus = member(shot["status"].as_str().expect("status"));
        let snapshot = SourceSnapshot {
            status,
            hash: shot["hash"].as_str().expect("hash").to_owned(),
            size: shot["size"].as_i64().expect("size"),
            passages: if status == SourceStatus::Indexed {
                vec![(0, "un".to_owned()), (1, "deux".to_owned())]
            } else {
                Vec::new()
            },
            rejection: shot["rejection"].as_str().map(member::<SourceRejection>),
        };
        let (change, outcome) = transition(&current, &snapshot, "tester").expect("transition");
        let context = format!("{} with {shot}", case["current"]);
        assert_eq!(
            Some(format!("{outcome:?}").as_str()),
            case["outcome"].as_str(),
            "{context}"
        );
        let Some(change) = change else {
            assert!(case["change"].is_null(), "{context}");
            continue;
        };
        let mut next = change.next.clone();
        if matches!(outcome, ReindexOutcome::Modified | ReindexOutcome::Restored) {
            let mut indexed = content(&next).expect("content");
            let at = indexed.indexed_at.expect("stamped");
            let age = DateTimeOffset::now_utc().utc_ticks() - at.utc_ticks();
            assert!((0..600_000_000).contains(&age), "{context}: stamped now");
            indexed.indexed_at = Some(stamp());
            next.content_json = Some(write(&indexed).expect("JSON"));
        }
        let expected = &case["change"];
        assert_eq!(
            write(&next).expect("JSON").replace("\r\n", "\n"),
            expected["next"].as_str().expect("next"),
            "{context}"
        );
        assert_eq!(Some(change.reason.as_str()), expected["reason"].as_str());
        assert_eq!(Some(change.actor.as_str()), expected["actor"].as_str());
    }
}

// Port of `SourceTests`.

/// An instance under the temporary directory, removed on drop: the store is declared first,
/// so its connection closes before the folder goes.
struct Fixture {
    store: Store,
    root: PathBuf,
    _folder: Folder,
}

/// A folder under the temporary directory, removed on drop.
struct Folder(PathBuf);

impl Drop for Folder {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn temp_base() -> PathBuf {
    let temp = std::env::temp_dir();
    // The discovery compares long names: the 8.3 names of the folder are expanded.
    if cfg!(windows) {
        expand_short_names(&temp)
    } else {
        fs::canonicalize(temp).expect("temp")
    }
}

fn fixture() -> Fixture {
    let root = temp_base().join(format!("sermofur-source-unit-{}", Uuid::new_v4().simple()));
    fs::create_dir_all(&root).expect("root");
    let initialized = initialize(&root).expect("init");
    let folder = Folder(root.clone());
    let store = Store::open(&root, initialized.configuration.instance_id, false).expect("store");
    Fixture {
        store,
        root,
        _folder: folder,
    }
}

impl Fixture {
    fn database(&self) -> PathBuf {
        self.root.join(".sermofur").join("memory.db")
    }

    /// A client scope mapped on a folder of its name.
    fn client(&mut self, id: &str) {
        fs::create_dir_all(self.root.join(id)).expect("client folder");
        let scope = Scope {
            id: id.to_owned(),
            kind: ScopeKind::Client,
            parent_id: Some("workspace".to_owned()),
            relative_path: Some(id.to_owned()),
        };
        self.store.add_scope(&scope, |_| Ok(())).expect("scope");
    }

    fn write_file(&self, relative: &str, text: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("folders");
        fs::write(&path, text).expect("file");
        path
    }

    fn context(&self, scope: &str) -> MemoryContext {
        MemoryContext {
            root: self.root.clone(),
            scope_id: scope.to_owned(),
            visible: HashSet::new(),
        }
    }

    fn sources(&mut self, scope: &str) -> SourceService<'_> {
        let context = self.context(scope);
        SourceService::new(&mut self.store, &file_reader, &context).expect("service")
    }

    fn add(&mut self, scope: &str, file: &Path) -> Result<MemoryRecord> {
        self.sources(scope).add(file, user())
    }

    fn scalar(&self, sql: &str) -> i64 {
        rusqlite::Connection::open(self.database())
            .expect("database")
            .query_row(sql, [], |row| row.get(0))
            .expect("scalar")
    }

    fn tamper(&self, sql: &str) {
        let changed = rusqlite::Connection::open(self.database())
            .expect("database")
            .execute(sql, [])
            .expect("tamper");
        assert!(changed > 0);
    }

    fn recall(&self, scope: &str, query: &str) -> usize {
        let context = self.context(scope).checked(&self.store).expect("context");
        sermofur_core::recall::recall(&self.store, &context, query, 3, DateTimeOffset::now_utc())
            .expect("recall")
            .results
            .len()
    }
}

fn user() -> Provenance {
    Provenance {
        origin: ActorKind::User,
        actor: Some("local-user".to_owned()),
        timestamp: DateTimeOffset::now_utc(),
    }
}

fn code(result: Result<impl std::fmt::Debug>) -> String {
    result.expect_err("refused").code
}

fn sha256(bytes: &[u8]) -> String {
    sermofur_core::store::sha256_upper(bytes).to_ascii_lowercase()
}

#[test]
fn add_indexes_the_file_and_is_idempotent() {
    let mut f = fixture();
    f.client("a");
    let file = f.write_file(
        "a/notes.md",
        "Le cache est préchauffé.\n\nDeuxième paragraphe.",
    );
    let source = f.add("a", &file).expect("added");
    let indexed = content(&source).expect("content");
    assert_eq!(source.scope_id.as_deref(), Some("a"));
    assert_eq!(indexed.relative_path.as_deref(), Some("a/notes.md"));
    assert_eq!(indexed.status, SourceStatus::Indexed);
    assert_eq!(indexed.hash, Some(sha256(&fs::read(&file).expect("bytes"))));
    assert_eq!(indexed.passages, 1);
    let again = f.add("a", &file).expect("again");
    assert_eq!(again, source);
    assert_eq!(
        f.sources("a").show(source.id).expect("show").history.len(),
        1
    );
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM search WHERE object_id='{}'",
            source.id
        )),
        1
    );
}

#[test]
fn file_outside_the_current_scope_or_in_a_narrower_scope_is_refused() {
    let mut f = fixture();
    f.client("a");
    f.client("b");
    let in_a = f.write_file("a/notes.md", "text a");
    let in_b = f.write_file("b/notes.md", "text b");
    let outside = Folder(temp_base().join(format!("sermofur-outside-{}", Uuid::new_v4().simple())));
    fs::create_dir_all(&outside.0).expect("outside");
    let outside_file = outside.0.join("outside.md");
    fs::write(&outside_file, "outside").expect("file");
    // From the workspace, a file of client a would become visible to client b.
    assert_eq!(code(f.add("workspace", &in_a)), "scope_boundary");
    assert_eq!(code(f.add("a", &in_b)), "scope_boundary");
    assert_eq!(code(f.add("a", &outside_file)), "scope_boundary");
    let own = f.root.join(".sermofur").join("instance.json");
    assert_eq!(code(f.add("workspace", &own)), "unsafe_path");
    assert!(f.sources("a").list().expect("list").is_empty());
    assert!(f.sources("b").list().expect("list").is_empty());
}

#[test]
fn unindexable_files_are_refused_with_a_stable_code() {
    for (kind, expected) in [
        ("empty", "source_rejected"),
        ("binary", "source_rejected"),
        ("latin1", "source_rejected"),
        ("large", "source_rejected"),
        ("missing", "invalid_path"),
        ("directory", "invalid_path"),
    ] {
        let mut f = fixture();
        let path = f.root.join("file.txt");
        match kind {
            "empty" => fs::write(&path, "  \n ").expect("file"),
            "binary" => fs::write(&path, [0x41, 0x00, 0x42]).expect("file"),
            "latin1" => fs::write(&path, b"caf\xE9").expect("file"),
            "large" => fs::write(&path, "a".repeat(1024 * 1024 + 1)).expect("file"),
            "directory" => fs::create_dir(&path).expect("folder"),
            _ => {}
        }
        assert_eq!(code(f.add("workspace", &path)), expected, "{kind}");
        assert!(f.sources("workspace").list().expect("list").is_empty());
    }
}

#[test]
fn reindex_reports_modified_missing_and_restored_sources_with_history() {
    let mut f = fixture();
    let changing = f.write_file("changing.md", "première version");
    let leaving = f.write_file("leaving.md", "bientôt supprimé");
    let first = f.add("workspace", &changing).expect("first");
    let second = f.add("workspace", &leaving).expect("second");
    let first_hash = content(&first).expect("content").hash;
    let entries = f
        .sources("workspace")
        .reindex(None, "tester")
        .expect("reindex");
    assert!(
        entries
            .iter()
            .all(|e| e.outcome == ReindexOutcome::Unchanged)
    );
    fs::write(&changing, "seconde version").expect("changed");
    fs::remove_file(&leaving).expect("removed");
    let entries = f
        .sources("workspace")
        .reindex(None, "tester")
        .expect("reindex");
    let of = |id: Uuid| {
        entries
            .iter()
            .find(|e| e.id == id.hyphenated().to_string())
            .expect("entry")
    };
    assert_eq!(of(first.id).outcome, ReindexOutcome::Modified);
    assert_eq!(of(first.id).previous_hash, first_hash);
    assert_ne!(of(first.id).hash, first_hash);
    assert_eq!(of(second.id).outcome, ReindexOutcome::Missing);
    assert_eq!(
        f.scalar(&format!(
            "SELECT count(*) FROM search WHERE object_id='{}'",
            second.id
        )),
        0
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM search WHERE search MATCH '\"seconde\"'"),
        1
    );
    assert_eq!(
        f.scalar("SELECT count(*) FROM search WHERE search MATCH '\"premiere\"'"),
        0
    );
    let history = f.sources("workspace").show(first.id).expect("show").history;
    let reasons: Vec<&str> = history.iter().filter_map(|e| e.reason.as_deref()).collect();
    assert_eq!(reasons, ["create", "modified"]);
    let before: MemoryRecord = read(
        history[1].previous_state.as_deref().expect("previous"),
        &|_| 0,
    )
    .expect("record");
    assert_eq!(content(&before).expect("content").hash, first_hash);
    fs::write(&leaving, "de retour").expect("back");
    let entries = f
        .sources("workspace")
        .reindex(Some(second.id), "tester")
        .expect("reindex");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].outcome, ReindexOutcome::Restored);
    // The `search_index` check of `doctor` is compared in `smf/tests/golden_cli_6_2.rs`.
}

#[test]
fn reindex_reads_only_declared_sources_of_the_current_scope() {
    let mut f = fixture();
    f.client("a");
    let root_file = f.write_file("root.md", "racine");
    f.add("workspace", &root_file).expect("root source");
    let a_file = f.write_file("a/a.md", "client a");
    let child = f.add("a", &a_file).expect("a source").id;
    f.write_file("undeclared.md", "jamais lu");
    let paths = RefCell::new(Vec::new());
    let recording = |root: &Path, relative: &str| {
        paths.borrow_mut().push(relative.to_owned());
        file_reader(root, relative)
    };
    let context = f.context("workspace");
    let mut service = SourceService::new(&mut f.store, &recording, &context).expect("service");
    service.reindex(None, "tester").expect("reindex");
    assert_eq!(*paths.borrow(), ["root.md"]);
    assert_eq!(code(service.reindex(Some(child), "tester")), "not_found");
}

fn claim(text: &str) -> ClaimContent {
    ClaimContent {
        text: Some(text.to_owned()),
        category: MemoryCategory::Semantic,
        volatility: Volatility::Evolving,
        last_verified: None,
        review_after: None,
    }
}

#[test]
fn evidence_freezes_the_hash_of_its_source_and_refuses_an_unavailable_one() {
    let mut f = fixture();
    let file = f.write_file("doc.md", "le service démarre en 2 s");
    let source = f.add("workspace", &file).expect("source");
    let context = f.context("workspace");
    let cited = |lineage: &str, hash: Option<&str>| EvidenceContent {
        claim_id: Uuid::nil(),
        kind: EvidenceKind::LocalDocumentation,
        reference: Some("doc.md".to_owned()),
        lineage_id: Some(lineage.to_owned()),
        relation: EvidenceRelation::Supports,
        source_id: Some(source.id),
        source_hash: hash.map(str::to_owned),
    };
    let claim_id = MemoryService::new(&mut f.store, &context)
        .expect("memory")
        .create_claim(&claim("démarrage rapide"), user(), None)
        .expect("claim")
        .id;
    let evidence = MemoryService::new(&mut f.store, &context)
        .expect("memory")
        .create_evidence(
            &EvidenceContent {
                claim_id,
                ..cited("doc", Some("forged"))
            },
            user(),
            None,
        )
        .expect("evidence");
    let frozen: EvidenceContent =
        read(evidence.content_json.as_deref().expect("json"), &|_| 0).expect("content");
    assert_eq!(frozen.source_hash, content(&source).expect("content").hash);
    fs::remove_file(&file).expect("removed");
    f.sources("workspace")
        .reindex(None, "tester")
        .expect("reindex");
    let refused = MemoryService::new(&mut f.store, &context)
        .expect("memory")
        .create_evidence(
            &EvidenceContent {
                claim_id,
                ..cited("doc2", None)
            },
            user(),
            None,
        );
    assert_eq!(code(refused), "source_unavailable");
}

#[test]
fn scope_created_on_the_folder_of_an_ancestor_source_takes_the_source() {
    let mut f = fixture();
    let file = f.write_file("a/notes.md", "secret de a");
    let source = f.add("workspace", &file).expect("source");
    f.client("a");
    f.client("b");
    let listed = f.sources("a").list().expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].id, listed[0].scope_id.as_deref()),
        (source.id, Some("a"))
    );
    let history = f.sources("a").show(source.id).expect("show").history;
    assert_eq!(
        history.last().and_then(|e| e.reason.as_deref()),
        Some("rescoped to a")
    );
    assert_eq!(f.recall("b", "secret"), 0);
    assert_eq!(f.recall("workspace", "secret"), 0);
    assert_eq!(f.recall("a", "secret"), 1);
    // The `source_scopes` and `search_index` checks of `doctor` are compared in `smf/tests/golden_cli_6_2.rs`.
}

#[test]
fn one_file_is_one_source_whatever_the_case_typed() {
    for (on_disk, typed) in [
        ("Doc.md", "doc.md"),
        ("Dossier/Été.md", "dossier/été.md"),
        // Decomposed on disk (NFD, as some macOS tools write it), composed when typed (NFC).
        ("Dossier/E\u{301}te\u{301}.md", "dossier/été.md"),
    ] {
        let mut f = fixture();
        let first = f
            .add("workspace", &f.write_file(on_disk, "contenu"))
            .expect("first");
        let other = f.root.join(typed);
        // The file system decides, not the operating system.
        if other.exists() {
            let again = f.add("workspace", &other).expect("again");
            assert_eq!(again.id, first.id, "{on_disk}");
            assert_eq!(
                content(&again).expect("content").relative_path.as_deref(),
                Some(on_disk)
            );
            assert_eq!(f.sources("workspace").list().expect("list").len(), 1);
            continue;
        }
        fs::create_dir_all(other.parent().expect("parent")).expect("folders");
        fs::write(&other, "autre contenu").expect("file");
        let second = f.add("workspace", &other).expect("second");
        assert_ne!(second.id, first.id);
        let stored = content(&second)
            .expect("content")
            .relative_path
            .expect("path");
        let name = typed.rsplit('/').next().expect("name");
        assert!(
            format!("/{stored}").ends_with(&format!("/{name}")),
            "{stored}"
        );
        let entries = f
            .sources("workspace")
            .reindex(None, "tester")
            .expect("reindex");
        assert!(
            entries
                .iter()
                .all(|e| e.outcome == ReindexOutcome::Unchanged)
        );
    }
}

fn register(f: &mut Fixture, scope: &str, candidate: Scope) -> Result<Scope> {
    let context = f.context(scope);
    sermofur_core::scopes::register(&mut f.store, &context, candidate)
}

fn client_scope(id: &str, mapping: &str) -> Scope {
    Scope {
        id: id.to_owned(),
        kind: ScopeKind::Client,
        parent_id: Some("workspace".to_owned()),
        relative_path: Some(mapping.to_owned()),
    }
}

#[test]
fn scope_mapping_typed_in_another_case_still_holds_its_folder() {
    let mut f = fixture();
    let file = f.write_file("client/notes.md", "secret du client");
    if !f.root.join("CLIENT").is_dir() {
        // Case-sensitive file system: CLIENT is another folder, nothing to compare.
        return;
    }
    let registered = register(&mut f, "workspace", client_scope("c", "CLIENT")).expect("scope");
    assert_eq!(registered.relative_path.as_deref(), Some("client"));
    assert_eq!(code(f.add("workspace", &file)), "scope_boundary");
    // The `scope_mappings` check of `doctor`, before and after a tampered mapping: `smf/tests/golden_cli_6_2.rs`.
}

#[test]
fn mappings_compare_as_named_on_disk_for_children_and_duplicates() {
    let mut f = fixture();
    fs::create_dir_all(f.root.join("client").join("sub")).expect("folders");
    if !f.root.join("CLIENT").is_dir() {
        return;
    }
    register(&mut f, "workspace", client_scope("c", "CLIENT")).expect("scope");
    // The parent as an earlier version stored it: under the typed spelling.
    f.tamper("UPDATE scopes SET relative_path='CLIENT' WHERE id='c'");
    // Context in the stored spelling, so that it resolves to c.
    let scopes = f.store.read_scopes().expect("scopes");
    let context = sermofur_core::memory::resolve_context(&f.root.join("CLIENT"), &f.root, &scopes)
        .expect("context");
    assert_eq!(context.scope_id, "c");
    let child = sermofur_core::scopes::register(
        &mut f.store,
        &context,
        Scope {
            id: "p".to_owned(),
            kind: ScopeKind::Project,
            parent_id: Some("c".to_owned()),
            relative_path: Some("client/sub".to_owned()),
        },
    )
    .expect("child");
    assert_eq!(child.relative_path.as_deref(), Some("client/sub"));
    // A mapping stored under another spelling by an earlier version still collides.
    let duplicate = register(&mut f, "workspace", client_scope("d", "client"));
    assert_eq!(code(duplicate), "duplicate_mapping");
}

#[test]
fn sermofur_files_cannot_be_reached_through_a_case_variant() {
    for typed in [".SERMOFUR/instance.json", ".Sermofur/memory.db"] {
        let mut f = fixture();
        let path = f.root.join(typed);
        assert_eq!(code(f.add("workspace", &path)), "unsafe_path", "{typed}");
    }
}

/// Port of `ReindexSkipsASourceChangedByAnotherCommandMeanwhile`: another connection changes
/// the source between the listing and the write of the reindexing, as `RacingStore` does.
#[test]
fn reindex_skips_a_source_changed_by_another_command_meanwhile() {
    let mut f = fixture();
    let file = f.write_file("doc.md", "contenu");
    let source = f.add("workspace", &file).expect("source");
    fs::write(&file, "contenu modifié").expect("changed");
    let database = f.database();
    let racing = |root: &Path, relative: &str| {
        rusqlite::Connection::open(&database)
            .expect("rival")
            .execute(
                "UPDATE records SET revision=revision+1, payload=json_set(payload,'$.revision',revision+1) WHERE kind='Source'",
                [],
            )
            .expect("race");
        file_reader(root, relative)
    };
    let context = f.context("workspace");
    let entries = SourceService::new(&mut f.store, &racing, &context)
        .expect("service")
        .reindex(None, "tester")
        .expect("reindex");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].outcome, ReindexOutcome::Skipped);
    assert_eq!(entries[0].hash, content(&source).expect("content").hash);
    assert_eq!(
        f.sources("workspace")
            .show(source.id)
            .expect("show")
            .history
            .len(),
        1
    );
}

/// Port of `ScopeCreatedDuringASourceAddIsSeenUnderTheWriteLock`: the service read the scopes
/// before a rival scope is inserted by another connection, just before the write.
#[test]
fn scope_created_during_a_source_add_is_seen_under_the_write_lock() {
    let mut f = fixture();
    let file = f.write_file("a/notes.md", "secret de a");
    let database = f.database();
    let saw_rival = RefCell::new(false);
    let racing = |root: &Path, relative: &str| {
        rusqlite::Connection::open(&database)
            .expect("rival")
            .execute(
                "INSERT INTO scopes VALUES('rival','Client','workspace','a')",
                [],
            )
            .expect("race");
        *saw_rival.borrow_mut() = true;
        file_reader(root, relative)
    };
    let context = f.context("workspace");
    let refused = SourceService::new(&mut f.store, &racing as SourceReader<'_>, &context)
        .expect("service")
        .add(&file, user());
    assert_eq!(code(refused), "scope_boundary");
    assert!(*saw_rival.borrow());
    let all = HashSet::from(["workspace".to_owned(), "rival".to_owned()]);
    assert!(
        f.store
            .read_records(&all, Some(RecordKind::Source))
            .expect("records")
            .is_empty()
    );
}

/// A link to a folder: a junction on Windows (`mklink /J`, no privilege needed), else a
/// symbolic link.
fn directory_link(link: &Path, target: &Path) {
    #[cfg(windows)]
    {
        let out = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .expect("cmd");
        assert!(out.status.success(), "mklink /J failed: {out:?}");
    }
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).expect("symlink");
}

#[test]
fn file_reached_through_a_link_is_refused() {
    let mut f = fixture();
    let outside = Folder(temp_base().join(format!("sermofur-outside-{}", Uuid::new_v4().simple())));
    fs::create_dir_all(&outside.0).expect("outside");
    fs::write(outside.0.join("secret.md"), "hors instance").expect("file");
    let link = f.root.join("linked");
    directory_link(&link, &outside.0);
    let refused = f.add("workspace", &link.join("secret.md"));
    // The link goes before the fixture removes the instance: never through it.
    #[cfg(windows)]
    fs::remove_dir(&link).expect("junction removed");
    #[cfg(unix)]
    fs::remove_file(&link).expect("link removed");
    assert_eq!(code(refused), "unsafe_path");
    assert!(f.sources("workspace").list().expect("list").is_empty());
    assert!(outside.0.join("secret.md").exists());
}

/// Port of `SpecialFileIsNotReadAsASource` (`HardeningTests`): a FIFO is refused without being
/// opened, which would block.
#[cfg(unix)]
#[test]
fn special_file_is_not_read_as_a_source() {
    let mut f = fixture();
    let fifo = f.root.join("pipe");
    let status = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("mkfifo");
    assert!(status.success());
    assert_eq!(code(f.add("workspace", &fifo)), "source_rejected");
}

/// A source declared by another scope at the same stored path (base written by an earlier
/// version, or a repository sharing its project's folder): `source add` refuses it.
#[test]
fn a_file_declared_by_another_scope_is_refused() {
    let mut f = fixture();
    let file = f.write_file("doc.md", "contenu");
    let source = f.add("workspace", &file).expect("source");
    fs::create_dir_all(f.root.join("x")).expect("folder");
    f.store
        .add_scope(&client_scope("x", "x"), |_| Ok(()))
        .expect("scope");
    f.tamper(&format!(
        "UPDATE records SET scope_id='x', payload=json_set(payload,'$.scopeId','x') WHERE id='{}'",
        source.id
    ));
    // Code only, as the tests of the oracle: no golden file reaches this altered base.
    assert_eq!(code(f.add("workspace", &file)), "scope_boundary");
}

/// Port of `IndexRebuildRestoresADesynchronizedIndex` (service part; the command line is in
/// `smf/tests/golden_cli_5_3.rs`, the `search_index` check of `doctor` is compared in `golden_cli_6_2.rs`).
#[test]
fn index_rebuild_restores_a_desynchronized_index() {
    let mut f = fixture();
    let context = f.context("workspace");
    MemoryService::new(&mut f.store, &context)
        .expect("memory")
        .create_claim(&claim("cache"), user(), None)
        .expect("claim");
    let file = f.write_file("doc.md", "cache chaud");
    f.add("workspace", &file).expect("source");
    f.tamper("DELETE FROM search");
    assert_eq!(f.recall("workspace", "cache"), 0);
    let rebuild = f.sources("workspace").rebuild_index().expect("rebuild");
    assert_eq!(
        (rebuild.indexed, rebuild.changed_sources),
        (2, 0),
        "{rebuild:?}"
    );
    assert_eq!(f.recall("workspace", "cache"), 2);
}

/// Port of `IndexRebuildReadsEverySourceAgainAndCountsOnlyVisibleObjects`.
#[test]
fn index_rebuild_reads_every_source_again_and_counts_only_visible_objects() {
    let mut f = fixture();
    f.client("a");
    f.client("b");
    let in_a = f.write_file("a/a.md", "texte de a");
    f.add("a", &in_a).expect("source of a");
    let in_b = f.write_file("b/b.md", "texte de b");
    let source_b = f.add("b", &in_b).expect("source of b");
    let context = f.context("b");
    MemoryService::new(&mut f.store, &context)
        .expect("memory")
        .create_claim(&claim("claim de b"), user(), None)
        .expect("claim");
    fs::write(&in_b, "texte de b modifié").expect("changed");
    let rebuild = f.sources("a").rebuild_index().expect("rebuild");
    assert_eq!((rebuild.indexed, rebuild.changed_sources), (1, 0));
    let history = f.sources("b").show(source_b.id).expect("show").history;
    let reasons: Vec<&str> = history.iter().filter_map(|e| e.reason.as_deref()).collect();
    assert_eq!(reasons, ["create", "modified"]);
    // An instance operation: recorded under the system actor, not a user of one scope.
    assert_eq!(history[1].actor.as_deref(), Some(SYSTEM_ACTOR));
    assert_eq!(f.recall("b", "modifie"), 1);
    let rebuild = f.sources("b").rebuild_index().expect("rebuild");
    assert_eq!((rebuild.indexed, rebuild.changed_sources), (2, 0));
}

/// Port of `IndexRebuildDropsALostSourceAndRollsBackOnFailure`: the reader fails on `lost.md`
/// in the middle of the transaction; whatever the order, the change of `kept.md` is rolled back.
#[test]
fn index_rebuild_drops_a_lost_source_and_rolls_back_on_failure() {
    let mut f = fixture();
    let kept = f.write_file("kept.md", "texte conservé");
    let lost = f.write_file("lost.md", "texte perdu");
    let kept_source = f.add("workspace", &kept).expect("kept");
    let lost_source = f.add("workspace", &lost).expect("lost");
    fs::write(&kept, "texte conservé et révisé").expect("revised");
    let failing = |root: &Path, relative: &str| {
        if relative == "lost.md" {
            return Err(sermofur_core::error::SermofurError::new(
                "invalid_storage_or_path",
                "simulated read failure",
                3,
            ));
        }
        file_reader(root, relative)
    };
    let context = f.context("workspace");
    let refused = SourceService::new(&mut f.store, &failing, &context)
        .expect("service")
        .rebuild_index();
    assert_eq!(
        refused.expect_err("failed").message,
        "simulated read failure"
    );
    assert_eq!(
        f.sources("workspace")
            .show(kept_source.id)
            .expect("show")
            .history
            .len(),
        1
    );
    assert_eq!(f.recall("workspace", "révisé"), 0);
    assert_eq!(f.recall("workspace", "conservé"), 1);
    assert_eq!(f.recall("workspace", "perdu"), 1);

    fs::remove_file(&lost).expect("lost");
    let rebuild = f.sources("workspace").rebuild_index().expect("rebuild");
    assert_eq!((rebuild.indexed, rebuild.changed_sources), (1, 2));
    assert_eq!(f.recall("workspace", "perdu"), 0);
    assert_eq!(f.recall("workspace", "révisé"), 1);
    let history = f
        .sources("workspace")
        .show(lost_source.id)
        .expect("show")
        .history;
    let entry = history.last().expect("entry");
    assert_eq!(
        (entry.reason.as_deref(), entry.actor.as_deref()),
        (Some("missing"), Some(SYSTEM_ACTOR))
    );
    // The `search_index` check of `doctor` is compared in `smf/tests/golden_cli_6_2.rs`.
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// The transitions on any pair of states: no change exactly when nothing changed; a change
    /// takes the next revision, keeps the path, and is indexed exactly when the snapshot is.
    #[test]
    fn a_transition_changes_exactly_what_changed(
        before in 0usize..7, after in 0usize..7, same_hash in any::<bool>(),
    ) {
        let states = [
            (SourceStatus::Indexed, None),
            (SourceStatus::Missing, None),
            (SourceStatus::Unreadable, None),
            (SourceStatus::Rejected, Some(SourceRejection::TooLarge)),
            (SourceStatus::Rejected, Some(SourceRejection::Binary)),
            (SourceStatus::Rejected, Some(SourceRejection::Empty)),
            (SourceStatus::Rejected, Some(SourceRejection::UnsafePath)),
        ];
        let previous = SourceContent {
            relative_path: Some("d/x.md".to_owned()),
            status: states[before].0,
            hash: Some("aa".to_owned()),
            size: 2,
            indexed_at: None,
            passages: 1,
            rejection: states[before].1,
        };
        let current = MemoryRecord {
            id: Uuid::new_v4(),
            scope_id: Some("workspace".to_owned()),
            kind: RecordKind::Source,
            status: KnowledgeStatus::Active,
            revision: 4,
            content_json: Some(write(&previous).expect("JSON")),
            provenance: Some(user()),
        };
        let snapshot = SourceSnapshot {
            status: states[after].0,
            hash: if same_hash { "aa" } else { "bb" }.to_owned(),
            size: 2,
            passages: vec![(0, "x".to_owned())],
            rejection: states[after].1,
        };
        let (change, outcome) = transition(&current, &snapshot, "t").expect("transition");
        let unchanged = if states[after].0 == SourceStatus::Indexed {
            before == 0 && same_hash
        } else {
            before == after
        };
        prop_assert_eq!(change.is_none(), unchanged);
        prop_assert_eq!(outcome == ReindexOutcome::Unchanged, unchanged);
        if let Some(change) = change {
            prop_assert_eq!(change.next.revision, 5);
            let next = content(&change.next).expect("content");
            prop_assert_eq!(next.relative_path.as_deref(), Some("d/x.md"));
            prop_assert_eq!(next.status, states[after].0);
            prop_assert_eq!(next.rejection, states[after].1);
            prop_assert_eq!(change.reason, format!("{outcome:?}").to_lowercase());
        }
    }
}
