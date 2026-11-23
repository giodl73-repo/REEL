#![cfg(feature = "assets")]
mod common;
#[cfg(feature = "schema")]
#[path = "native_source/lifecycle_cases.rs"]
mod lifecycle_cases;
#[path = "native_source/payload_consumption_cases.rs"]
mod payload_consumption_cases;
#[cfg(feature = "schema")]
#[path = "native_source/retirement_cases.rs"]
mod retirement_cases;
#[cfg(feature = "schema")]
#[path = "native_source/schema_cases.rs"]
mod schema_cases;
#[cfg(feature = "schema")]
#[path = "native_source/update_cases.rs"]
mod update_cases;
#[path = "native_source/variant_cases.rs"]
mod variant_cases;
#[path = "native_source/variant_retention_cases.rs"]
mod variant_retention_cases;
use common::{git, git_executable};
use git_asset::source::{
    Error, GitId, Limits as GitLimits, Repository, Scope,
    native::{AssetSnapshot, Limits},
};
use git_asset_core::{LogicalPath, Object};
use git_asset_model::{
    graph::{self, Binding, Collection, EdgeKind, NodeKind, Payloads, Profile, Query, Rule},
    resolve::{AssetResolver, Error as ResolveError},
};
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs, path::Path, process::Command};
use tempfile::TempDir;

fn write(root: &Path, path: &str, v: &Value) -> Object {
    let raw = serde_json::to_vec(v).unwrap();
    fs::write(root.join(path), &raw).unwrap();
    Object::from_bytes(&raw)
}
fn scope(name: &str) -> Scope {
    Scope::new([LogicalPath::parse(name).unwrap()]).unwrap()
}
fn reader(root: &Path) -> Repository {
    Repository::new(git_executable(), root.to_owned(), GitLimits::default()).unwrap()
}
fn head(root: &Path) -> GitId {
    GitId::parse(
        String::from_utf8(git(root, &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim(),
    )
    .unwrap()
}
fn commit(root: &Path) -> GitId {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fixture"]);
    head(root)
}
fn capture(r: &Repository, id: GitId) -> AssetSnapshot {
    r.native(
        &r.commit(id).unwrap(),
        scope("records"),
        scope("definitions"),
        Limits::default(),
    )
    .unwrap()
}
struct Fixture {
    temp: TempDir,
    primary: Value,
    output: Value,
    profiles: Vec<Profile>,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Fixture"]);
        git(root, &["config", "user.email", "fixture@example.invalid"]);
        fs::create_dir(root.join("records")).unwrap();
        fs::create_dir(root.join("definitions")).unwrap();
        fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
        let schema = write(root, "definitions/authored.json", &json!({"type":"object"}));
        let mut pins = Vec::new();
        let mut profiles = Vec::new();
        for (name, class) in [("primary", NodeKind::Primary), ("output", NodeKind::Output)] {
            let id = format!("types/{name}");
            let object = write(
                root,
                &format!("definitions/{name}.json"),
                &json!({
                "schema":"tig.definition.design-1","id":id,"authored_schema":schema,
                "authoring":{"properties":["deps","text"],"required":["deps"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
            );
            let mut pin = serde_json::to_value(&object).unwrap();
            pin["id"] = json!(id);
            pins.push(pin);
            profiles.push(Profile {
                definition_id: id,
                definition: object,
                class,
                covered: BTreeSet::from([EdgeKind::Production]),
                rules: vec![Rule {
                    pointer: "/deps".into(),
                    kind: EdgeKind::Production,
                    collection: Collection::Array,
                    optional: false,
                    binding: Binding::Either,
                }],
                payloads: if class == NodeKind::Primary {
                    Payloads::SelectedObject
                } else {
                    Payloads::None
                },
            });
        }
        Self {
            temp,
            primary: pins.remove(0),
            output: pins.remove(0),
            profiles,
        }
    }
    fn root(&self) -> &Path {
        self.temp.path()
    }
    fn record(
        &self,
        name: &str,
        id: &str,
        primary: bool,
        deps: Value,
        payload: Option<&[u8]>,
    ) -> Object {
        write(
            self.root(),
            &format!("records/{name}.asset"),
            &json!({"schema":"tig.instance.design-1","id":id,
            "definition":if primary {&self.primary}else{&self.output},"authored":{"deps":deps,"text":"PRIVATE AUTHORING TEXT"},
            "streams":[{"slot":"preview","role":"review","group":"draft","object":Object::from_bytes(b"preview")}],
                "selected":payload.map(Object::from_bytes),"status":{},"metadata":{"private":"DO NOT PRINT PRIVATE METADATA"}}),
        )
    }
}
fn query(s: &AssetSnapshot, id: &str) -> Query {
    Query {
        checkpoint: s.checkpoint().clone(),
        roots: BTreeSet::from([s.resolve_id(id).unwrap().reference().clone()]),
        kinds: BTreeSet::from([EdgeKind::Production]),
        limits: graph::Limits {
            nodes: 100,
            edges: 100,
            depth: 10,
            bytes: 1_000_000,
        },
    }
}

#[test]
fn renamed_asset_catalogue_reopens_legacy_record_suffix_without_rewriting_bytes() {
    let f = Fixture::new();
    let object = f.record(
        "item",
        "asset/item",
        true,
        json!([]),
        Some(b"legacy payload"),
    );
    fs::rename(
        f.root().join("records/item.asset"),
        f.root().join("records/item.asset"),
    )
    .unwrap();
    let repository = reader(f.root());
    let snapshot = capture(&repository, commit(f.root()));
    let resolved = snapshot.resolve_id("asset/item").unwrap();
    assert_eq!(resolved.reference().object, object);
}

#[test]
fn real_git_day_two_resolves_new_cel_without_parent_edits_and_restores_old_commit() {
    let f = Fixture::new();
    f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    f.record(
        "scene",
        "scene/1",
        false,
        json!([{"id":"cel/b"}]),
        Some(b"old render"),
    );
    let one = commit(f.root());
    let r = reader(f.root());
    let mut old = capture(&r, one.clone());
    let q = query(&old, "scene/1");
    let before = graph::resolve(&mut old, &q, &f.profiles).unwrap();
    f.record("cel", "cel/b", true, json!([]), Some(b"v2"));
    let two = commit(f.root());
    let changes = git(
        f.root(),
        &["diff", "--name-only", one.as_str(), two.as_str()],
    );
    assert_eq!(
        String::from_utf8(changes.stdout).unwrap().trim(),
        "records/cel.asset"
    );
    let mut current = capture(&r, two);
    let q = query(&current, "scene/1");
    let after = graph::resolve(&mut current, &q, &f.profiles).unwrap();
    assert!(after.complete_declared_coverage());
    assert_eq!(before.roots(), after.roots());
    assert_ne!(before.binding_digest(), after.binding_digest());
    assert_eq!(
        graph::primary_change(&before, &after).unwrap().added,
        BTreeSet::from([Object::from_bytes(b"v2")])
    );
    assert_eq!(
        capture(&r, one)
            .resolve_id("cel/b")
            .unwrap()
            .selected()
            .unwrap(),
        &Object::from_bytes(b"v1")
    );
}

#[test]
fn captured_index_ignores_working_files_and_later_index_changes() {
    let f = Fixture::new();
    f.record("cel", "cel/b", true, json!([]), Some(b"staged"));
    fs::write(
        f.root().join(".gitattributes"),
        "* text eol=lf\nrecords/*.asset filter=tripwire\n",
    )
    .unwrap();
    git(f.root(), &["add", "."]);
    let r = reader(f.root());
    let index = r.index().unwrap();
    fs::write(f.root().join("records/cel.asset"), b"broken unstaged JSON").unwrap();
    git(
        f.root(),
        &["config", "filter.tripwire.smudge", "echo changed-by-filter"],
    );
    let first = r
        .native(
            &index,
            scope("records"),
            scope("definitions"),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(
        first.resolve_id("cel/b").unwrap().selected().unwrap(),
        &Object::from_bytes(b"staged")
    );
    f.record("cel", "cel/b", true, json!([]), Some(b"new index"));
    git(f.root(), &["add", "."]);
    let old = r
        .native(
            &index,
            scope("records"),
            scope("definitions"),
            Limits::default(),
        )
        .unwrap();
    assert_eq!(old.checkpoint(), first.checkpoint());
    assert_ne!(
        old.checkpoint(),
        r.native(
            &r.index().unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        )
        .unwrap()
        .checkpoint()
    );
}

#[test]
fn real_git_map_replaces_only_one_cel_without_parent_churn_and_replays_old_view() {
    let mut f = Fixture::new();
    f.profiles
        .iter_mut()
        .find(|p| p.class == NodeKind::Output)
        .unwrap()
        .rules[0]
        .collection = Collection::ObjectValues;
    f.record("a", "cel/a", true, json!([]), Some(b"cel a"));
    f.record("b", "cel/b", true, json!([]), Some(b"cel b v1"));
    f.record("cue", "cue/shared", true, json!([]), Some(b"shared cue"));
    f.record(
        "scene",
        "scene/1",
        false,
        json!({"picture.a":{"id":"cel/a"},"picture.b":{"id":"cel/b"},"take":{"id":"cue/shared"}}),
        Some(b"last render"),
    );
    let one = commit(f.root());
    let r = reader(f.root());
    let mut old = capture(&r, one.clone());
    let q = query(&old, "scene/1");
    let before = graph::resolve(&mut old, &q, &f.profiles).unwrap();
    f.record("b", "cel/b", true, json!([]), Some(b"cel b v2"));
    let two = commit(f.root());
    let changes = git(
        f.root(),
        &["diff", "--name-only", one.as_str(), two.as_str()],
    );
    assert_eq!(
        String::from_utf8(changes.stdout).unwrap().trim(),
        "records/b.asset"
    );
    let mut current = capture(&r, two);
    let q = query(&current, "scene/1");
    let after = graph::resolve(&mut current, &q, &f.profiles).unwrap();
    assert!(after.complete_declared_coverage());
    assert_eq!(before.roots(), after.roots());
    assert_ne!(before.binding_digest(), after.binding_digest());
    let change = graph::primary_change(&before, &after).unwrap();
    assert_eq!(
        change.added,
        BTreeSet::from([Object::from_bytes(b"cel b v2")])
    );
    assert_eq!(
        change.unreferenced_by_new_query,
        BTreeSet::from([Object::from_bytes(b"cel b v1")])
    );
    assert_eq!(change.shared.len(), 2);
    let mut replay = capture(&r, one);
    let q = query(&replay, "scene/1");
    assert_eq!(
        graph::resolve(&mut replay, &q, &f.profiles)
            .unwrap()
            .binding_digest(),
        before.binding_digest()
    );
    assert!(!format!("{after:?}").contains("PRIVATE AUTHORING TEXT"));
}

#[test]
fn real_git_map_frozen_pin_requires_explicit_old_capture_and_never_changes_current_selection() {
    let mut f = Fixture::new();
    f.profiles
        .iter_mut()
        .find(|p| p.class == NodeKind::Output)
        .unwrap()
        .rules[0]
        .collection = Collection::ObjectValues;
    let old_pin = f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    f.record("scene","scene/1",false,json!({"current":{"id":"cel/b"},"frozen":{"id":"cel/b","sha256":old_pin.sha256,"bytes":old_pin.bytes}}),None);
    let one = commit(f.root());
    let r = reader(f.root());
    let mut old = capture(&r, one);
    let q = query(&old, "scene/1");
    let before = graph::resolve(&mut old, &q, &f.profiles).unwrap();
    f.record("cel", "cel/b", true, json!([]), Some(b"v2"));
    let mut current = capture(&r, commit(f.root()));
    let q = query(&current, "scene/1");
    assert!(
        !graph::resolve(&mut current, &q, &f.profiles)
            .unwrap()
            .complete_declared_coverage()
    );
    current.retain(old).unwrap();
    let after = graph::resolve(&mut current, &q, &f.profiles).unwrap();
    assert!(after.complete_declared_coverage());
    assert_eq!(
        after.edges()[0].target,
        current.resolve_id("cel/b").unwrap().reference().clone()
    );
    assert_eq!(after.edges()[1].target.object, old_pin);
    assert_eq!(
        current.resolve_id("cel/b").unwrap().selected().unwrap(),
        &Object::from_bytes(b"v2")
    );
    let change = graph::primary_change(&before, &after).unwrap();
    assert_eq!(change.added, BTreeSet::from([Object::from_bytes(b"v2")]));
    assert_eq!(change.shared, BTreeSet::from([Object::from_bytes(b"v1")]));
    assert!(change.unreferenced_by_new_query.is_empty());
}

#[test]
fn explicit_retained_snapshot_restores_frozen_pin_without_changing_current_id_selection() {
    let f = Fixture::new();
    let object = f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    f.record(
        "scene",
        "scene/1",
        false,
        json!([{"id":"cel/b","sha256":object.sha256,"bytes":object.bytes}]),
        Some(b"render"),
    );
    let one = commit(f.root());
    let r = reader(f.root());
    let old = capture(&r, one);
    f.record("cel", "cel/b", true, json!([]), Some(b"v2"));
    let two = commit(f.root());
    let mut current = capture(&r, two.clone());
    let q = query(&current, "scene/1");
    assert!(
        !graph::resolve(&mut current, &q, &f.profiles)
            .unwrap()
            .complete_declared_coverage()
    );
    let context = current.checkpoint().clone();
    current.retain(old).unwrap();
    assert_eq!(
        current.resolve_id("scene/1").unwrap().checkpoint(),
        &context
    );
    assert_eq!(current.checkpoint(), &context);
    let graph = graph::resolve(&mut current, &q, &f.profiles).unwrap();
    assert!(graph.complete_declared_coverage());
    assert_eq!(graph.edges()[0].target.object, object);
    let selected = current.resolve_id("cel/b").unwrap();
    assert_eq!(selected.checkpoint(), &context);
    assert_eq!(selected.selected().unwrap(), &Object::from_bytes(b"v2"));
    current.retain(capture(&r, two)).unwrap();
    assert_eq!(
        current.resolve_id("scene/1").unwrap().checkpoint(),
        &context
    );
}

#[test]
fn duplicate_asset_ids_refuse_namespace_even_when_files_are_identical() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    fs::copy(
        f.root().join("records/one.asset"),
        f.root().join("records/two.asset"),
    )
    .unwrap();
    let id = commit(f.root());
    let r = reader(f.root());
    assert!(matches!(
        r.native(
            &r.commit(id).unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Ambiguous)
    ));
}

#[test]
fn missing_definition_and_malformed_record_never_become_an_empty_namespace() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    let one = commit(f.root());
    let r = reader(f.root());
    let view = r.commit(one).unwrap();
    assert!(matches!(
        r.native(&view, scope("records"), scope("missing"), Limits::default()),
        Err(Error::Incomplete)
    ));
    fs::write(f.root().join("records/broken.asset"), b"{}").unwrap();
    let two = commit(f.root());
    assert!(matches!(
        r.native(
            &r.commit(two).unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Integrity)
    ));
}

#[test]
fn conflict_stages_and_foreign_repository_snapshots_refuse() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    let id = commit(f.root());
    let r = reader(f.root());
    let view = r.commit(id).unwrap();
    let other = reader(f.root());
    assert!(matches!(
        other.native(
            &view,
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Invalid)
    ));
    let oid =
        String::from_utf8(git(f.root(), &["rev-parse", ":records/one.asset"]).stdout).unwrap();
    let input = format!(
        "0 {}\trecords/one.asset\n100644 {} 1\trecords/one.asset\n100644 {} 2\trecords/one.asset\n",
        "0".repeat(40),
        oid.trim(),
        oid.trim()
    );
    let mut command = common::command(f.root(), &["update-index", "--index-info"]);
    use std::io::Write;
    let mut child = command.stdin(std::process::Stdio::piped()).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    assert!(child.wait().unwrap().success());
    assert!(matches!(
        r.native(
            &r.index().unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Incomplete)
    ));
}

#[test]
fn document_and_aggregate_budgets_refuse_before_any_partial_catalog_is_returned() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    let id = commit(f.root());
    let r = reader(f.root());
    let view = r.commit(id).unwrap();
    for limits in [
        Limits {
            documents: 1,
            ..Limits::default()
        },
        Limits {
            bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            r.native(&view, scope("records"), scope("definitions"), limits),
            Err(Error::Budget)
        ));
    }
    fs::write(f.root().join("records/oversize.asset"), vec![b' '; 65_537]).unwrap();
    let id = commit(f.root());
    assert!(matches!(
        r.native(
            &r.commit(id).unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Budget)
    ));
}

#[test]
fn resolver_port_reports_planned_missing_slot_and_definition_identity() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), None);
    let id = commit(f.root());
    let r = reader(f.root());
    let catalog = capture(&r, id);
    let resolver: &dyn AssetResolver = &catalog;
    let asset = resolver.resolve_id("cel/b").unwrap();
    assert_eq!(asset.authored()["text"], "PRIVATE AUTHORING TEXT");
    assert!(!format!("{asset:?}").contains("PRIVATE AUTHORING"));
    assert_eq!(asset.selected(), Err(ResolveError::Planned));
    assert_eq!(
        asset.stream("preview").unwrap(),
        &Object::from_bytes(b"preview")
    );
    assert_eq!(asset.stream("missing"), Err(ResolveError::MissingStream));
    assert_eq!(asset.definition().id, "types/primary");
    assert!(matches!(
        resolver.resolve_id("../../other"),
        Err(ResolveError::Missing)
    ));
    assert!(!asset.complete_semantic_coverage());
}

#[test]
fn consumer_status_is_snapshot_pinned_and_never_default_debug_output() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"unchanged payload"));
    let path = f.root().join("records/one.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["status"] = json!({"selection_state":"draft","private_reason":"PRIVATE STATUS REASON"});
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let one = commit(f.root());
    let r = reader(f.root());
    let old = capture(&r, one);
    record["status"]["selection_state"] = json!("selected");
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    let two = commit(f.root());
    let new = capture(&r, two);
    let before = old.resolve_id("cel/b").unwrap();
    let after = new.resolve_id("cel/b").unwrap();
    assert_eq!(before.status()["selection_state"], "draft");
    assert_eq!(after.status()["selection_state"], "selected");
    assert_eq!(before.selected(), after.selected());
    assert_ne!(before.reference(), after.reference());
    assert_eq!(after.status()["private_reason"], "PRIVATE STATUS REASON");
    assert!(!format!("{after:?}").contains("PRIVATE STATUS"));
    assert!(!format!("{after:?}").contains("selection_state"));
}

#[test]
fn actual_resolve_cli_uses_pinned_view_and_prints_no_authored_text() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    let id = commit(f.root());
    let profile = json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":f.root(),"commit":id.as_str(),"records":["records"],"definitions":["definitions"]});
    let path = f.root().join("local-profile.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg(&path)
        .arg("cel/b")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let data: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert!(!String::from_utf8_lossy(&result.stdout).contains("DO NOT PRINT"));
    assert_eq!(
        data["object"],
        serde_json::to_value(Object::from_bytes(b"v1")).unwrap()
    );
    assert_eq!(data["payload_verified"], false);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE AUTHORING"));
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg(&path)
        .args(["cel/b", "preview"])
        .output()
        .unwrap();
    assert!(result.status.success());
    let data: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        data["object"],
        serde_json::to_value(Object::from_bytes(b"preview")).unwrap()
    );
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
            .arg(&path)
            .arg("missing")
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[cfg(not(feature = "schema"))]
#[test]
fn history_mode_without_schema_feature_refuses_before_profile_io() {
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .args([
            "--history",
            "missing-private-profile.json",
            "private-asset",
            "2020-12",
        ])
        .output()
        .unwrap();
    assert!(!result.status.success() && result.stdout.is_empty());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("schema feature") && !error.contains("private"));
}

#[test]
fn native_git_mode_checks_do_not_follow_a_symlink_shaped_record() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    commit(f.root());
    let oid =
        String::from_utf8(git(f.root(), &["rev-parse", ":records/one.asset"]).stdout).unwrap();
    git(
        f.root(),
        &[
            "update-index",
            "--cacheinfo",
            &format!("120000,{},records/one.asset", oid.trim()),
        ],
    );
    let r = reader(f.root());
    assert!(matches!(
        r.native(
            &r.index().unwrap(),
            scope("records"),
            scope("definitions"),
            Limits::default()
        ),
        Err(Error::Incomplete)
    ));
}

#[test]
fn retained_metadata_budget_failure_keeps_current_selection_and_checkpoint() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"v1"));
    let one = commit(f.root());
    let r = reader(f.root());
    let old = capture(&r, one);
    f.record("one", "cel/b", true, json!([]), Some(b"v2"));
    let two = commit(f.root());
    let mut current = r
        .native(
            &r.commit(two).unwrap(),
            scope("records"),
            scope("definitions"),
            Limits {
                documents: 4,
                ..Limits::default()
            },
        )
        .unwrap();
    let checkpoint = current.checkpoint().clone();
    let bytes = current.metadata_bytes();
    assert!(matches!(current.retain(old), Err(Error::Budget)));
    assert_eq!(current.checkpoint(), &checkpoint);
    assert_eq!(current.metadata_bytes(), bytes);
    assert_eq!(
        current.resolve_id("cel/b").unwrap().selected().unwrap(),
        &Object::from_bytes(b"v2")
    );
}

#[test]
fn thousand_real_git_records_use_multiple_bounded_batches_then_resolve_without_git_io() {
    let f = Fixture::new();
    for i in 0..1000 {
        f.record(
            &format!("record-{i}"),
            &format!("items/{i}"),
            true,
            json!([]),
            Some(b"shared bytes"),
        );
    }
    let id = commit(f.root());
    let r = reader(f.root());
    let catalog = capture(&r, id);
    assert_eq!(catalog.len(), 1000);
    fs::rename(
        f.root().join(".git"),
        f.root().join("detached-git-metadata"),
    )
    .unwrap();
    for i in [0, 255, 256, 999] {
        assert_eq!(
            catalog
                .resolve_id(&format!("items/{i}"))
                .unwrap()
                .selected()
                .unwrap(),
            &Object::from_bytes(b"shared bytes")
        );
    }
}

#[test]
fn generic_consumer_preserves_order_roles_and_groups_for_nonmedia_assets() {
    use git_asset_core::tree::{MemoryNodes, TreeLimits};
    let f = Fixture::new();
    f.record(
        "invoice",
        "invoice/42",
        false,
        json!([]),
        Some(b"invoice manifest"),
    );
    let path = f.root().join("records/invoice.asset");
    let mut v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    v["streams"] = json!([
        {"slot":"z-pdf","role":"document","group":"published","object":Object::from_bytes(b"pdf")},
        {"slot":"a-sheet","role":"spreadsheet","group":"working","object":Object::from_bytes(b"sheet")}]);
    fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
    let id = commit(f.root());
    let r = reader(f.root());
    let catalog = capture(&r, id);
    let consumer: &dyn AssetResolver = &catalog;
    let asset = consumer.resolve_id("invoice/42").unwrap();
    let streams = asset.stream_bindings();
    assert_eq!(streams[0].slot, "z-pdf");
    assert_eq!(streams[1].slot, "a-sheet");
    assert_eq!(streams[0].role, "document");
    assert_eq!(streams[0].group, "published");
    assert_eq!(
        asset.stream("a-sheet").unwrap(),
        &Object::from_bytes(b"sheet")
    );
    let mut store = MemoryNodes::default();
    let root = catalog
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let restored = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap()
    .resolve_id("invoice/42")
    .unwrap();
    assert_eq!(restored.reference(), asset.reference());
    assert_eq!(restored.authored(), asset.authored());
    assert_eq!(restored.status(), asset.status());
    assert_eq!(restored.stream_bindings()[0].slot, "z-pdf");
    assert_eq!(restored.stream_bindings()[1].slot, "a-sheet");
    assert_eq!(restored.stream_bindings()[0].role, "document");
    assert_eq!(restored.stream_bindings()[0].group, "published");
    assert_eq!(
        restored.stream("a-sheet").unwrap(),
        asset.stream("a-sheet").unwrap()
    );
}

#[test]
fn id_to_existing_byte_reader_and_explicit_cli_verification_use_exact_identity() {
    use git_asset_store::existing::{
        ByteSource, Layout, Limits as ByteLimits, Place as BytePlace, Profile as ByteProfile,
        ReadCache, RequiredProtection,
    };
    use std::io::Read;
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"pixels"));
    let id = commit(f.root());
    let r = reader(f.root());
    let catalog = capture(&r, id.clone());
    let cache_root = f.root().join("cache");
    let object = Object::from_bytes(b"pixels");
    let path = Layout::ReelCas.object_path(&cache_root, &object.sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"pixels").unwrap();
    let required = if cfg!(windows) {
        RequiredProtection::PreventMutation
    } else {
        RequiredProtection::CooperativeImmutable
    };
    let mut bytes = ReadCache::new(ByteProfile {
        places: vec![BytePlace {
            root: cache_root.clone(),
            layout: Layout::ReelCas,
        }],
        required,
        limits: ByteLimits::default(),
    })
    .unwrap();
    let asset = catalog.resolve_id("cel/b").unwrap();
    let mut input = bytes.open_verified(asset.selected().unwrap()).unwrap();
    let mut actual = Vec::new();
    input.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, b"pixels");
    drop(input);
    let profile = json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":f.root(),"commit":id.as_str(),"records":["records"],"definitions":["definitions"],
        "byte_source":{"places":[{"root":cache_root,"layout":"reel-cas"}],"required":if cfg!(windows) {"prevent-mutation"}else{"cooperative-immutable"}}});
    let local = f.root().join("local-read-profile.json");
    fs::write(&local, serde_json::to_vec(&profile).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg("--verify")
        .arg(&local)
        .arg("cel/b")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let result: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(result["payload_verified"], true);
    assert_eq!(result["verified_source_index"], 0);
    assert_eq!(result["complete_semantic_coverage"], false);
    fs::write(path, b"broken").unwrap();
    let failed = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg("--verify")
        .arg(&local)
        .arg("cel/b")
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
}

#[test]
fn verification_requires_explicit_byte_profile_but_metadata_lookup_does_not() {
    let f = Fixture::new();
    f.record("one", "cel/b", true, json!([]), Some(b"pixels"));
    let id = commit(f.root());
    let local = f.root().join("profile.json");
    fs::write(&local,serde_json::to_vec(&json!({
        "schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":f.root(),"commit":id.as_str(),"records":["records"],"definitions":["definitions"]})).unwrap()).unwrap();
    let run = |verify| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"));
        if verify {
            command.arg("--verify");
        }
        command.arg(&local).arg("cel/b").output().unwrap()
    };
    assert!(run(false).status.success());
    let result = run(true);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
}

#[test]
fn metadata_archive_reopens_without_git_and_preserves_frozen_and_current_selections() {
    use git_asset_core::tree::{NodeStore, TreeLimits};
    use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};
    let f = Fixture::new();
    let old_cel = f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    f.record(
        "scene",
        "scene/1",
        false,
        json!([
            {"id":"cel/b"},
            {"id":"cel/b","sha256":old_cel.sha256,"bytes":old_cel.bytes}
        ]),
        None,
    );
    let first = commit(f.root());
    let r = reader(f.root());
    let old = capture(&r, first);
    let old_reference = old.resolve_id("cel/b").unwrap().reference().clone();
    f.record("cel", "cel/b", true, json!([]), Some(b"v2"));
    let mut current = capture(&r, commit(f.root()));
    current.retain(old).unwrap();
    let before_query = query(&current, "scene/1");
    let before = graph::resolve(&mut current, &before_query, &f.profiles).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let location = temp.path().join("metadata");
    let mut store = MetadataStore::create(
        &location,
        StoreLimits {
            nodes: 4096,
            bytes: 4 * 1024 * 1024,
            database_bytes: 8 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let root = current
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let checkpoint = store.finish().unwrap();
    drop(store);
    drop(current);
    drop(r);
    fs::rename(f.root().join(".git"), f.root().join("offline-git")).unwrap();
    fs::rename(f.root().join("records"), f.root().join("offline-records")).unwrap();
    fs::rename(
        f.root().join("definitions"),
        f.root().join("offline-definitions"),
    )
    .unwrap();
    let mut store = MetadataReader::open(&location, &checkpoint).unwrap();
    let mut restored = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored.resolve_id("cel/b").unwrap().selected().unwrap(),
        &Object::from_bytes(b"v2")
    );
    let selected = graph::Source::select(&mut restored, "cel/b", 65536);
    assert!(matches!(selected.selection, graph::Selection::Found(ref r) if r.object != old_cel));
    let frozen = graph::Source::lookup(&mut restored, &old_reference, 65536);
    assert!(matches!(frozen.data, graph::Data::Record { .. }));
    let q = query(&restored, "scene/1");
    let after = graph::resolve(&mut restored, &q, &f.profiles).unwrap();
    assert_eq!(before.binding_digest(), after.binding_digest());
    assert_eq!(before.checkpoint(), after.checkpoint());
    assert_eq!(store.get(&Object::from_bytes(b"v1")).unwrap(), None);
    assert_eq!(store.get(&Object::from_bytes(b"v2")).unwrap(), None);
}

#[test]
fn native_metadata_archives_share_unchanged_documents_and_tree_nodes() {
    use git_asset_core::tree::{MemoryNodes, TreeLimits};
    let f = Fixture::new();
    for i in 0..100 {
        f.record(
            &format!("cel-{i}"),
            &format!("cel/{i}"),
            true,
            json!([]),
            Some(b"v1"),
        );
    }
    let r = reader(f.root());
    let first = capture(&r, commit(f.root()));
    let mut store = MemoryNodes::default();
    let one = first
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let initial_nodes = store.objects().count();
    let initial_bytes: usize = store.objects().map(|(_, b)| b.len()).sum();
    assert_eq!(
        first
            .archive_metadata(&mut store, TreeLimits::default())
            .unwrap(),
        one
    );
    assert_eq!(store.objects().count(), initial_nodes);
    f.record("cel-50", "cel/50", true, json!([]), Some(b"v2"));
    let second = capture(&r, commit(f.root()));
    let two = second
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    assert_ne!(one, two);
    let added_nodes = store.objects().count() - initial_nodes;
    let added_bytes = store.objects().map(|(_, b)| b.len()).sum::<usize>() - initial_bytes;
    assert!(
        added_nodes < initial_nodes / 2,
        "{added_nodes}/{initial_nodes}"
    );
    assert!(
        added_bytes < initial_bytes / 2,
        "{added_bytes}/{initial_bytes}"
    );
    for (root, expected) in [(&one, b"v1".as_slice()), (&two, b"v2".as_slice())] {
        let restored = AssetSnapshot::from_metadata_archive(
            &mut store,
            root,
            Limits::default(),
            TreeLimits::default(),
        )
        .unwrap();
        assert_eq!(
            restored.resolve_id("cel/50").unwrap().selected().unwrap(),
            &Object::from_bytes(expected)
        );
    }
}

#[test]
fn native_metadata_archive_refuses_missing_corrupt_and_over_budget_sources() {
    use git_asset_core::tree::{MemoryNodes, NodeStore, TreeLimits, TreeResult};
    struct Filter {
        inner: MemoryNodes,
        target: Object,
        corrupt: bool,
    }
    impl NodeStore for Filter {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            if object == &self.target {
                return Ok(if self.corrupt {
                    Some(b"bad".to_vec())
                } else {
                    None
                });
            }
            self.inner.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            self.inner.put(bytes)
        }
    }
    let f = Fixture::new();
    let cel = f.record("cel", "cel/b", true, json!([]), Some(b"no payload read"));
    let r = reader(f.root());
    let snapshot = capture(&r, commit(f.root()));
    let mut store = Filter {
        inner: MemoryNodes::default(),
        target: cel,
        corrupt: false,
    };
    let root = snapshot
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    assert!(matches!(
        AssetSnapshot::from_metadata_archive(
            &mut store,
            &root,
            Limits::default(),
            TreeLimits::default()
        ),
        Err(Error::Incomplete)
    ));
    store.corrupt = true;
    assert!(matches!(
        AssetSnapshot::from_metadata_archive(
            &mut store,
            &root,
            Limits::default(),
            TreeLimits::default()
        ),
        Err(Error::Integrity)
    ));
    let mut memory = store.inner;
    for limits in [
        Limits {
            documents: 1,
            ..Limits::default()
        },
        Limits {
            bytes: 1,
            ..Limits::default()
        },
    ] {
        assert!(matches!(
            AssetSnapshot::from_metadata_archive(&mut memory, &root, limits, TreeLimits::default()),
            Err(Error::Budget)
        ));
    }
    let tiny = TreeLimits {
        max_entries: 1,
        ..TreeLimits::default()
    };
    assert!(matches!(
        snapshot.archive_metadata(&mut memory, tiny),
        Err(Error::Budget)
    ));
    assert!(matches!(
        AssetSnapshot::from_metadata_archive(&mut memory, &root, Limits::default(), tiny),
        Err(Error::Budget)
    ));
}

#[test]
fn native_metadata_archive_rejects_header_counts_unknown_fields_and_index_substitution() {
    use git_asset_core::tree::{MemoryNodes, NodeStore, Tree, TreeLimits};
    let f = Fixture::new();
    f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    let r = reader(f.root());
    let snapshot = capture(&r, commit(f.root()));
    let mut store = MemoryNodes::default();
    let root = snapshot
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let header: Value = serde_json::from_slice(&store.get(&root).unwrap().unwrap()).unwrap();
    for edit in ["counts", "unknown", "selection"] {
        let mut value = header.clone();
        match edit {
            "counts" => value["records"] = json!(0),
            "unknown" => value["extension"] = json!(true),
            _ => {
                let index: Object = serde_json::from_value(value["tree"].clone()).unwrap();
                let (mut entries, _) = Tree::from_root(index)
                    .entries(&mut store, TreeLimits::default())
                    .unwrap();
                let selected = entries
                    .keys()
                    .find(|p| p.components().next() == Some("selected"))
                    .unwrap()
                    .clone();
                let object = entries.remove(&selected).unwrap();
                entries.insert(
                    LogicalPath::parse(&format!(
                        "selected/{}",
                        git_asset_core::Digest::blob(b"other ID").as_str()
                    ))
                    .unwrap(),
                    object,
                );
                let (tree, _) = Tree::build(&entries, &mut store, TreeLimits::default()).unwrap();
                value["tree"] = serde_json::to_value(tree.root()).unwrap();
            }
        }
        let substituted = store.put(&serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(
            AssetSnapshot::from_metadata_archive(
                &mut store,
                &substituted,
                Limits::default(),
                TreeLimits::default()
            )
            .is_err(),
            "{edit}"
        );
    }
}

#[test]
fn interrupted_native_archive_creation_does_not_replace_an_old_root() {
    use git_asset_core::tree::{MemoryNodes, NodeStore, TreeError, TreeLimits, TreeResult};
    struct Fail {
        inner: MemoryNodes,
        writes: usize,
    }
    impl NodeStore for Fail {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.inner.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            if self.writes == 0 {
                return Err(TreeError::Store("synthetic full".into()));
            }
            self.writes -= 1;
            self.inner.put(bytes)
        }
    }
    let f = Fixture::new();
    f.record("cel", "cel/b", true, json!([]), Some(b"v1"));
    let r = reader(f.root());
    let old = capture(&r, commit(f.root()));
    let mut store = Fail {
        inner: MemoryNodes::default(),
        writes: 1000,
    };
    let root = old
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    f.record("cel", "cel/b", true, json!([]), Some(b"v2"));
    let new = capture(&r, commit(f.root()));
    store.writes = 2;
    assert!(
        new.archive_metadata(&mut store, TreeLimits::default())
            .is_err()
    );
    let restored = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored.resolve_id("cel/b").unwrap().selected().unwrap(),
        &Object::from_bytes(b"v1")
    );
}

#[test]
fn native_archive_verifies_provider_acknowledgements_and_exact_root_identity() {
    use git_asset_core::tree::{MemoryNodes, NodeStore, TreeLimits, TreeResult};
    struct WrongAck(MemoryNodes);
    impl NodeStore for WrongAck {
        fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
            self.0.get(object)
        }
        fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
            self.0.put(bytes)?;
            Ok(Object::from_bytes(b"not the acknowledged document"))
        }
    }
    let f = Fixture::new();
    f.record("cel", "cel/b", true, json!([]), None);
    let r = reader(f.root());
    let snapshot = capture(&r, commit(f.root()));
    let mut wrong = WrongAck(MemoryNodes::default());
    assert!(matches!(
        snapshot.archive_metadata(&mut wrong, TreeLimits::default()),
        Err(Error::Integrity)
    ));
    let mut store = wrong.0;
    let root = snapshot
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let wrong_size = Object {
        bytes: root.bytes + 1,
        ..root.clone()
    };
    assert!(matches!(
        AssetSnapshot::from_metadata_archive(
            &mut store,
            &wrong_size,
            Limits::default(),
            TreeLimits::default()
        ),
        Err(Error::Integrity)
    ));
    let restored = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    assert_eq!(
        restored
            .archive_metadata(&mut store, TreeLimits::default())
            .unwrap(),
        root
    );
    assert!(matches!(
        restored.resolve_id("cel/b").unwrap().selected(),
        Err(ResolveError::Planned)
    ));
}

#[test]
fn native_job_obligations_preserve_old_primary_controls_and_source_metadata_after_main_changes() {
    use git_asset_core::tree::{NodeStore, Tree, TreeLimits};
    use git_asset_store::{
        Key,
        metadata::{Limits as StoreLimits, MetadataReader, MetadataStore},
        source_claims::{Command, SourceJournal},
    };
    let mut f = Fixture::new();
    let schema = Object::from_bytes(&fs::read(f.root().join("definitions/authored.json")).unwrap());
    let definition = write(
        f.root(),
        "definitions/scene.json",
        &json!({
            "schema":"tig.definition.design-1","id":"types/scene","authored_schema":schema,
            "authoring":{"properties":["deps","text"],"required":["deps"],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}
        }),
    );
    f.output = json!({"id":"types/scene","sha256":definition.sha256,"bytes":definition.bytes});
    f.profiles.push(Profile {
        definition_id: "types/scene".into(),
        definition,
        class: NodeKind::Container,
        covered: BTreeSet::from([EdgeKind::Production]),
        rules: vec![Rule {
            pointer: "/deps".into(),
            kind: EdgeKind::Production,
            collection: Collection::Array,
            optional: false,
            binding: Binding::SnapshotSelected,
        }],
        payloads: Payloads::Named {
            selected: true,
            slots: BTreeSet::from(["catalog".into(), "profile".into()]),
        },
    });
    f.record("cel", "cel/b", true, json!([]), Some(b"old primary"));
    f.record(
        "scene",
        "scene/1",
        false,
        json!([{"id":"cel/b"}]),
        Some(b"scene document"),
    );
    let scene_path = f.root().join("records/scene.asset");
    let mut scene: Value = serde_json::from_slice(&fs::read(&scene_path).unwrap()).unwrap();
    scene["streams"] = json!([
        {"slot":"catalog","role":"control","group":"current","object":Object::from_bytes(b"catalog")},
        {"slot":"profile","role":"control","group":"current","object":Object::from_bytes(b"profile")}
    ]);
    fs::write(scene_path, serde_json::to_vec(&scene).unwrap()).unwrap();
    let repository = reader(f.root());
    let mut old = capture(&repository, commit(f.root()));
    let old_query = query(&old, "scene/1");
    let old_graph = graph::resolve(&mut old, &old_query, &f.profiles).unwrap();
    let old_requirements = old_graph.production_requirements().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let metadata_path = runtime.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &metadata_path,
        StoreLimits {
            nodes: 4096,
            bytes: 4 * 1024 * 1024,
            database_bytes: 8 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let descriptor =
        br#"{"schema":"synthetic.render-job","language":"es","producer":"pinned-test-producer"}"#;
    let old_context = old
        .freeze_production_context(
            &old_query,
            &f.profiles,
            descriptor,
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let old_selection = old_context.selection().clone();
    f.record("cel", "cel/b", true, json!([]), Some(b"new primary"));
    let mut new = capture(&repository, commit(f.root()));
    let new_query = query(&new, "scene/1");
    let new_graph = graph::resolve(&mut new, &new_query, &f.profiles).unwrap();
    let new_requirements = new_graph.production_requirements().unwrap();
    assert_eq!(old_requirements.controls(), new_requirements.controls());
    assert_ne!(old_requirements.primary(), new_requirements.primary());
    let new_context = new
        .freeze_production_context(
            &new_query,
            &f.profiles,
            descriptor,
            &mut nodes,
            TreeLimits::default(),
        )
        .unwrap();
    let new_selection = new_context.selection().clone();
    let metadata_checkpoint = nodes.finish().unwrap();
    drop(nodes);
    drop(old);
    drop(new);
    drop(repository);
    fs::rename(f.root().join(".git"), f.root().join("offline-git")).unwrap();
    fs::rename(f.root().join("records"), f.root().join("offline-records")).unwrap();
    fs::rename(
        f.root().join("definitions"),
        f.root().join("offline-definitions"),
    )
    .unwrap();
    let key = |s: &str| Key::parse(s).unwrap();
    let journal_path = runtime.path().join("claims");
    // Finish/reopen/read back both candidates before recording owner obligations.
    let mut readback = MetadataReader::open(&metadata_path, &metadata_checkpoint).unwrap();
    for selection in [&old_selection, &new_selection] {
        AssetSnapshot::from_production_context(
            selection,
            &f.profiles,
            &mut readback,
            Limits::default(),
            TreeLimits::default(),
            old_query.limits,
        )
        .unwrap();
    }
    drop(readback);
    let mut journal = SourceJournal::create(&journal_path, key("project"), key("epoch")).unwrap();
    for source in ["main", "render-job"] {
        let prepared = journal
            .apply(
                key(&format!("prepare-{source}")),
                Command::Prepare {
                    source: key(source),
                    owner: key("owner"),
                    expected_revision: 0,
                    selection: old_selection.clone(),
                },
            )
            .unwrap();
        journal
            .apply(
                key(&format!("adopt-{source}")),
                Command::Adopt {
                    source: key(source),
                    owner: key("owner"),
                    expected_revision: 1,
                    prepared: prepared.reference,
                },
            )
            .unwrap();
    }
    let prepared = journal
        .apply(
            key("prepare-new-main"),
            Command::Prepare {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 2,
                selection: new_selection.clone(),
            },
        )
        .unwrap();
    let witness = journal.checkpoint();
    drop(journal);
    let mut journal = SourceJournal::open(&journal_path, &witness).unwrap();
    assert_eq!(journal.obligations(10).unwrap().len(), 3);
    journal
        .apply(
            key("adopt-new-main"),
            Command::Adopt {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 3,
                prepared: prepared.reference,
            },
        )
        .unwrap();
    journal
        .apply(
            key("retire-old-main"),
            Command::Retire {
                source: key("main"),
                owner: key("owner"),
                expected_revision: 4,
                predecessor_revision: 2,
            },
        )
        .unwrap();
    let obligations = journal.obligations(10).unwrap();
    assert_eq!(obligations.len(), 2);
    assert!(!obligations.complete_claim_coverage());
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("render-job") && o.selection == old_selection)
    );
    assert!(
        obligations
            .iter()
            .any(|o| o.source == key("main") && o.selection == new_selection)
    );
    let mut nodes = MetadataReader::open(&metadata_path, &metadata_checkpoint).unwrap();
    let (_, replay) = AssetSnapshot::from_production_context(
        &old_selection,
        &f.profiles,
        &mut nodes,
        Limits::default(),
        TreeLimits::default(),
        old_query.limits,
    )
    .unwrap();
    assert_eq!(replay.requirements(), &old_requirements);
    assert_eq!(replay.descriptor(), descriptor);
    let tree = Tree::from_root(old_selection.root);
    for (namespace, objects) in [
        ("primary", old_requirements.primary()),
        ("controls", old_requirements.controls()),
        ("interpretation", old_requirements.metadata()),
    ] {
        for object in objects {
            let entry = tree
                .get(
                    &LogicalPath::parse(&format!("{namespace}/{}", object.sha256.as_str()))
                        .unwrap(),
                    &mut nodes,
                    TreeLimits::default(),
                )
                .unwrap()
                .0;
            assert_eq!(entry, Some(object.clone()));
        }
    }
    // Claim/root replay preserves declared pins, not proof of payload presence.
    assert!(
        nodes
            .get(&Object::from_bytes(b"old primary"))
            .unwrap()
            .is_none()
    );
}

#[path = "native_source/context_cases.rs"]
mod context_cases;
#[path = "native_source/job_cases.rs"]
mod job_cases;
#[path = "native_source/produced_cases.rs"]
mod produced_cases;

#[test]
fn native_recipe_replays_metadata_and_consumed_changes_from_archives_without_git() {
    use git_asset_core::{
        Digest,
        tree::{MemoryNodes, TreeLimits},
    };
    use git_asset_model::graph::recipe::{self, Consumption, Operation};
    let f = Fixture::new();
    f.record("row", "rows/1", true, json!([]), Some(b"immutable rows"));
    let profiles = f.profiles.clone();
    let consumption = profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: if p.class == NodeKind::Primary {
                BTreeSet::from(["text".into()])
            } else {
                BTreeSet::new()
            },
        })
        .collect::<Vec<_>>();
    let operation = Operation {
        name: "report".into(),
        semantics: Digest::blob(b"report semantics"),
        producer: Object::from_bytes(b"synthetic producer"),
        parameters: Object::from_bytes(b"{}"),
    };
    let mut nodes = MemoryNodes::default();
    let mut expected = Vec::new();
    let mut archives = Vec::new();
    let repository = reader(f.root());
    let calculate = |source: &mut AssetSnapshot| {
        let q = query(source, "rows/1");
        let graph = graph::resolve(source, &q, &profiles).unwrap();
        let plan = recipe::plan(
            &graph,
            source,
            &profiles,
            &consumption,
            &operation,
            recipe::Limits {
                nodes: 100,
                edges: 100,
                bytes: 1_000_000,
            },
        )
        .unwrap();
        (
            plan.fingerprint().clone(),
            plan.binding().clone(),
            plan.checkpoint().clone(),
        )
    };
    for version in 0..4 {
        let path = f.root().join("records/row.asset");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match version {
            1 => {
                record["metadata"] = json!({"annotation":"changed"});
                record["status"] = json!({"review":"draft"});
            }
            2 => record["authored"]["text"] = json!("changed consumed text"),
            3 => record["selected"] = json!(Object::from_bytes(b"replaced rows")),
            _ => {}
        }
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        let mut source = capture(&repository, commit(f.root()));
        expected.push(calculate(&mut source));
        archives.push(
            source
                .archive_metadata(&mut nodes, TreeLimits::default())
                .unwrap(),
        );
    }
    assert_eq!(expected[0].0, expected[1].0);
    assert_ne!(expected[0].1, expected[1].1);
    assert_ne!(expected[1].0, expected[2].0);
    assert_ne!(expected[2].0, expected[3].0);
    drop(repository);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    for (archive, expected) in archives.iter().zip(expected) {
        let mut source = AssetSnapshot::from_metadata_archive(
            &mut nodes,
            archive,
            Limits::default(),
            TreeLimits::default(),
        )
        .unwrap();
        assert_eq!(calculate(&mut source), expected);
    }
}

#[test]
fn native_frozen_descriptors_bind_language_parameters_without_changing_input_closure() {
    use git_asset_core::{Digest, tree::TreeLimits};
    use git_asset_model::graph::recipe::{self, Consumption, Operation};
    use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};
    let f = Fixture::new();
    f.record(
        "bilingual",
        "inputs/bilingual",
        true,
        json!([]),
        Some(b"one shared bilingual input"),
    );
    let repository = reader(f.root());
    let mut source = capture(&repository, commit(f.root()));
    let q = query(&source, "inputs/bilingual");
    let profiles = f.profiles.clone();
    let consumption = profiles
        .iter()
        .map(|p| Consumption {
            definition_id: p.definition_id.clone(),
            definition: p.definition.clone(),
            fields: BTreeSet::new(),
        })
        .collect::<Vec<_>>();
    let runtime = tempfile::tempdir().unwrap();
    let path = runtime.path().join("metadata");
    let mut nodes = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let mut contexts = Vec::new();
    for language in ["es", "en"] {
        let descriptor = serde_json::to_vec(
            &json!({"schema":"synthetic.scene-parameters-1","language":language,"width":1280}),
        )
        .unwrap();
        contexts.push(
            source
                .freeze_production_context(
                    &q,
                    &profiles,
                    &descriptor,
                    &mut nodes,
                    TreeLimits::default(),
                )
                .unwrap(),
        );
    }
    assert_eq!(contexts[0].requirements(), contexts[1].requirements());
    let checkpoint = nodes.finish().unwrap();
    drop(nodes);
    drop(source);
    drop(repository);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut nodes = MetadataReader::open(&path, &checkpoint).unwrap();
    let mut plans = Vec::new();
    for context in contexts {
        let (mut source, replay) = AssetSnapshot::from_production_context(
            context.selection(),
            &profiles,
            &mut nodes,
            Limits::default(),
            TreeLimits::default(),
            q.limits,
        )
        .unwrap();
        assert_eq!(context.descriptor(), replay.descriptor());
        let operation = Operation {
            name: "scene".into(),
            semantics: Digest::blob(b"synthetic render contract"),
            producer: Object::from_bytes(b"synthetic producer"),
            parameters: Object::from_bytes(replay.descriptor()),
        };
        let graph = graph::resolve(&mut source, &q, &profiles).unwrap();
        let plan = recipe::plan(
            &graph,
            &mut source,
            &profiles,
            &consumption,
            &operation,
            recipe::Limits {
                nodes: 100,
                edges: 100,
                bytes: 1_000_000,
            },
        )
        .unwrap();
        assert_eq!(plan.operation(), &operation);
        plans.push(plan);
    }
    assert_eq!(plans[0].nodes(), plans[1].nodes());
    assert_eq!(plans[0].binding(), plans[1].binding());
    assert_eq!(plans[0].checkpoint(), plans[1].checkpoint());
    assert_ne!(plans[0].fingerprint(), plans[1].fingerprint());
}
