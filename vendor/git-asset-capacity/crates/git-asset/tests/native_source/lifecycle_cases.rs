use super::*;
use git_asset::source::native::lifecycle;
use git_asset_core::identity;
use git_asset_model::{revisions, schema::Dialect};
use git_asset_store::existing::Layout;
use std::collections::BTreeMap;
#[path = "placement_cases.rs"]
mod placement_cases;

struct HistoryFixture {
    source: Fixture,
    cache: TempDir,
    layout: Layout,
    objects: Vec<Object>,
    revision: Object,
    root: Object,
    commit: GitId,
}
fn cache_put(root: &Path, layout: &Layout, value: Value, objects: &mut Vec<Object>) -> Object {
    let raw = serde_json::to_vec(&value).unwrap();
    let object = Object::from_bytes(&raw);
    let path = layout.object_path(root, &object.sha256);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, raw).unwrap();
    objects.push(object.clone());
    object
}
impl HistoryFixture {
    fn new(layout: Layout, primary: bool) -> Self {
        let source = Fixture::new();
        let cache = tempfile::tempdir().unwrap();
        source.record(
            "history",
            "private-asset-id",
            primary,
            json!([]),
            Some(b"payload not present in store"),
        );
        let mut objects = Vec::new();
        let mut put = |value| cache_put(cache.path(), &layout, value, &mut objects);
        let payload = Object::from_bytes(b"payload not present in store");
        let revision = put(
            json!({"schema":"tig.asset-revision.experimental-1","asset":"private-asset-id","revision":"take-private",
            "parents":[],"representations":[{"key":"main","object":payload}],
            "provenance":{"kind":"imported","source":Object::from_bytes(b"opaque historical witness")}}),
        );
        let revision_root = put(
            json!({"schema":"tig.revision-history.experimental-1","asset":"private-asset-id","coverage":"complete","parents":[],"revisions":[revision]}),
        );
        let target = Object::from_bytes(&identity::canonical_json(&json!({"schema":"tig.declared-publication-target.experimental-1",
            "provider":"synthetic-service","scope":"private-scope","path":"private-delivery/item.bin"})).unwrap()).sha256;
        let profile = Object::from_bytes(b"opaque rendition profile");
        let policy = put(
            json!({"schema":"tig.delivery-policy.experimental-1","targets":[{"key":"private-target-key","identity":target,
            "destination":Object::from_bytes(b"opaque destination"),"representation":"main","profile":profile,"gates":Object::from_bytes(b"opaque gate")}]}),
        );
        let mapping = put(
            json!({"schema":"tig.receipt-mapping.experimental-1","limits":{"bytes":1048576,"nodes":8192,"depth":32},
            "rules":[{"kind":"array-rows","root":"/rows","fields":{"sha256":"/sha256","bytes":"/bytes","target":"/path"}}]}),
        );
        let receipt = put(
            json!({"rows":[{"sha256":payload.sha256,"bytes":payload.bytes,"path":"private-delivery/item.bin"}],"duration":0.125}),
        );
        let binding = put(
            json!({"schema":"tig.receipt-binding.experimental-1","subject":payload,"receipt":receipt,"provider":"synthetic-service",
            "scope":"private-scope","selector":"/rows/0","mapping":mapping}),
        );
        let event = put(
            json!({"schema":"tig.delivery-event.experimental-1","asset":"private-asset-id","event":"private-event","operation":"private-op",
            "parents":[],"policy":policy,"target":"private-target-key","revision":revision,"representation":"main","subject":payload,"profile":profile,
            "stage":"acknowledged","evidence":{"kind":"receipt","binding":binding}}),
        );
        let root = put(
            json!({"schema":"tig.delivery-history.experimental-1","asset":"private-asset-id","coverage":"complete","parents":[],"events":[event]}),
        );
        let path = source.root().join("records/history.asset");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["metadata"]["revisions"] = json!({"schema":"tig.revision-state.experimental-1","history_slot":"revision-history","selection":{"slot":"selected-revision","representation":"main"}});
        record["metadata"]["delivery"] = json!({"schema":"tig.delivery-state.experimental-1","policy_slot":"delivery-policy","history_slot":"delivery-history"});
        for (slot, role, object) in [
            ("revision-history", "history/revisions", revision_root),
            ("selected-revision", "revision/selected", revision.clone()),
            ("delivery-policy", "policy/delivery", policy),
            ("delivery-history", "history/delivery", root.clone()),
        ] {
            record["streams"]
                .as_array_mut()
                .unwrap()
                .push(json!({"slot":slot,"role":role,"group":"evidence","object":object}));
        }
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let commit = commit(source.root());
        Self {
            source,
            cache,
            layout,
            objects,
            revision,
            root,
            commit,
        }
    }
    fn profile(&self) -> Value {
        json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":self.source.root(),
            "commit":self.commit.as_str(),"records":["records"],"definitions":["definitions"],
            "byte_source":{"places":[{"root":self.cache.path(),"layout":self.layout}],
                "required":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"}}})
    }
    fn run(&self, profile: &Value) -> std::process::Output {
        let local = self.source.root().join("history-profile.json");
        fs::write(&local, serde_json::to_vec(profile).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
            .args(["--history"])
            .arg(&local)
            .args(["private-asset-id", "2020-12"])
            .output()
            .unwrap()
    }
    fn summary(&self) -> Value {
        let result = self.run(&self.profile());
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output = String::from_utf8(result.stdout).unwrap();
        for secret in [
            "PRIVATE AUTHORING TEXT",
            "DO NOT PRINT",
            "private-asset-id",
            "private-target-key",
            "private-delivery",
            "private-scope",
            "take-private",
        ] {
            assert!(!output.contains(secret));
        }
        serde_json::from_str(&output).unwrap()
    }
    fn edit_record(&mut self, edit: impl FnOnce(&mut Value)) {
        let path = self.source.root().join("records/history.asset");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut record);
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        self.commit = commit(self.source.root());
    }
}

#[test]
fn history_cli_validates_two_native_types_and_existing_cas_lfs_layouts_without_media_or_writes() {
    for (layout, primary) in [(Layout::ReelCas, true), (Layout::GitLfs, false)] {
        let f = HistoryFixture::new(layout, primary);
        let before: BTreeMap<_, _> = f
            .objects
            .iter()
            .map(|o| {
                (
                    o.clone(),
                    fs::read(f.layout.object_path(f.cache.path(), &o.sha256)).unwrap(),
                )
            })
            .collect();
        let record_before = fs::read(f.source.root().join("records/history.asset")).unwrap();
        let summary = f.summary();
        assert_eq!(
            summary["schema"],
            "tig.asset-history-inspection.experimental-1"
        );
        assert_eq!(summary["authored_schema_validated"], true);
        assert_eq!(summary["revisions"], 1);
        assert_eq!(summary["delivery_events"], 1);
        assert_eq!(summary["targets"]["claims"], 1);
        assert_eq!(summary["receipt_bindings"], 1);
        assert_eq!(summary["metadata_objects"], 7);
        assert_eq!(summary["receipt_objects"], 1);
        assert_eq!(summary["opaque_witnesses"], 4);
        for field in [
            "local_contracts_validated",
            "payload_verified",
            "current_remote_content_verified",
            "independent_custody_verified",
            "authorizes_eviction",
            "complete_semantic_coverage",
        ] {
            assert_eq!(summary[field], false);
        }
        assert_eq!(
            fs::read(f.source.root().join("records/history.asset")).unwrap(),
            record_before
        );
        for (object, raw) in before {
            assert_eq!(
                fs::read(f.layout.object_path(f.cache.path(), &object.sha256)).unwrap(),
                raw
            );
        }
        assert_eq!(head(f.source.root()), f.commit);
    }
}

#[test]
fn history_cli_ignores_dirty_edits_and_preserves_old_commit_lookup() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let path = f.source.root().join("records/history.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["metadata"]["delivery"]["history_slot"] = json!("absent-slot");
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert_eq!(f.summary()["delivery_events"], 1);
    let changed_commit = commit(f.source.root());
    assert_eq!(f.summary()["delivery_events"], 1);
    let mut changed = f.profile();
    changed["commit"] = json!(changed_commit.as_str());
    let failed = f.run(&changed);
    assert!(!failed.status.success() && failed.stdout.is_empty());
    assert!(
        !String::from_utf8(failed.stderr)
            .unwrap()
            .contains("absent-slot")
    );
}

struct MemoryReader {
    objects: BTreeMap<Object, Vec<u8>>,
    calls: usize,
}
impl revisions::Reader for MemoryReader {
    fn read(&mut self, object: &Object, maximum: usize) -> Result<Vec<u8>, revisions::Error> {
        assert_eq!(maximum as u64, object.bytes);
        self.calls += 1;
        self.objects
            .get(object)
            .cloned()
            .ok_or(revisions::Error::Missing)
    }
}

#[test]
fn captured_history_sdk_works_after_source_git_is_unavailable_and_bounds_reads() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let repo = reader(f.source.root());
    let source = capture(&repo, f.commit.clone());
    let mut bytes = MemoryReader {
        objects: f
            .objects
            .iter()
            .map(|o| {
                (
                    o.clone(),
                    fs::read(f.layout.object_path(f.cache.path(), &o.sha256)).unwrap(),
                )
            })
            .collect(),
        calls: 0,
    };
    fs::rename(
        f.source.root().join(".git"),
        f.source.root().join("unavailable.git"),
    )
    .unwrap();
    let inspection = source
        .inspect_history(
            "private-asset-id",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut bytes,
        )
        .unwrap();
    assert_eq!(
        inspection.revisions().unwrap().selected_revision(),
        Some(&f.revision)
    );
    assert_eq!(inspection.delivery().unwrap().receipt_binding_count(), 1);
    assert_eq!(bytes.calls, 8);
    let mut limited = MemoryReader {
        objects: bytes.objects.clone(),
        calls: 0,
    };
    assert!(matches!(
        source.inspect_history(
            "private-asset-id",
            Dialect::Draft202012,
            revisions::Limits {
                documents: 1,
                ..revisions::Limits::default()
            },
            &mut limited
        ),
        Err(lifecycle::Error::Delivery(_))
    ));
    assert_eq!(limited.calls, 1);
    assert!(matches!(
        source.inspect_history(
            "other-private-id",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut limited
        ),
        Err(lifecycle::Error::Missing)
    ));
}

#[test]
fn legacy_history_cli_needs_no_byte_profile_and_reports_unknown_not_empty_history() {
    let f = Fixture::new();
    f.record("old", "private-asset-id", true, json!([]), None);
    let commit = commit(f.root());
    let path = f.root().join("legacy-profile.json");
    fs::write(&path, serde_json::to_vec(&json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),
        "repository":f.root(),"commit":commit.as_str(),"records":["records"],"definitions":["definitions"]})).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
        .arg("--history")
        .arg(path)
        .args(["private-asset-id", "2020-12"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["revisions"], 0);
    assert_eq!(summary["metadata_objects"], 0);
    assert!(
        summary["revision_history_coverage"].is_null()
            && summary["delivery_history_coverage"].is_null()
    );
    assert_eq!(summary["delivery_configured"], false);
    assert_eq!(summary["complete_semantic_coverage"], false);
}

#[test]
fn missing_profile_object_corruption_and_cache_budgets_refuse_without_partial_stdout() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let mut no_profile = f.profile();
    no_profile.as_object_mut().unwrap().remove("byte_source");
    let missing = f.run(&no_profile);
    assert!(!missing.status.success() && missing.stdout.is_empty());
    assert!(
        String::from_utf8(missing.stderr)
            .unwrap()
            .contains("explicit byte_source")
    );
    let mut budget = f.profile();
    budget["byte_source"]["limits"] = json!({"requests":1});
    let failed = f.run(&budget);
    assert!(!failed.status.success() && failed.stdout.is_empty());
    assert!(String::from_utf8(failed.stderr).unwrap().contains("Budget"));
    let root_path = f.layout.object_path(f.cache.path(), &f.root.sha256);
    let root = fs::read(&root_path).unwrap();
    fs::remove_file(&root_path).unwrap();
    let failed = f.run(&f.profile());
    assert!(!failed.status.success() && failed.stdout.is_empty());
    assert!(
        String::from_utf8(failed.stderr)
            .unwrap()
            .contains("Missing")
    );
    fs::write(root_path, vec![b'x'; root.len()]).unwrap();
    let failed = f.run(&f.profile());
    assert!(!failed.status.success() && failed.stdout.is_empty());
    let error = String::from_utf8(failed.stderr).unwrap();
    assert!(error.contains("Pin") && !error.contains("private"));
}

#[test]
fn pinned_authored_schema_failure_happens_before_history_io() {
    let mut f = Fixture::new();
    let reject = write(f.root(), "definitions/reject.json", &json!(false));
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/primary.json")).unwrap())
            .unwrap();
    definition["authored_schema"] = serde_json::to_value(reject).unwrap();
    let pin = write(f.root(), "definitions/primary.json", &definition);
    f.primary["sha256"] = serde_json::to_value(pin.sha256).unwrap();
    f.primary["bytes"] = json!(pin.bytes);
    f.record("invalid", "private-asset-id", true, json!([]), None);
    let commit = commit(f.root());
    let repo = reader(f.root());
    let source = capture(&repo, commit);
    let mut bytes = MemoryReader {
        objects: BTreeMap::new(),
        calls: 0,
    };
    assert!(matches!(
        source.inspect_history(
            "private-asset-id",
            Dialect::Draft202012,
            revisions::Limits::default(),
            &mut bytes
        ),
        Err(lifecycle::Error::Schema)
    ));
    assert_eq!(bytes.calls, 0);
}

#[test]
fn history_cli_separates_partial_ambiguous_and_untracked_target_counts() {
    for mode in ["partial", "ambiguous", "untracked", "absent"] {
        let mut f = HistoryFixture::new(Layout::ReelCas, true);
        let mut page: Value = serde_json::from_slice(
            &fs::read(f.layout.object_path(f.cache.path(), &f.root.sha256)).unwrap(),
        )
        .unwrap();
        if mode == "partial" {
            page["coverage"] = json!("partial");
        }
        if matches!(mode, "ambiguous" | "absent") {
            let first: Object = serde_json::from_value(page["events"][0].clone()).unwrap();
            let mut second: Value = serde_json::from_slice(
                &fs::read(f.layout.object_path(f.cache.path(), &first.sha256)).unwrap(),
            )
            .unwrap();
            second["event"] = json!("separate-real-event");
            second["operation"] = json!("separate-real-operation");
            if mode == "absent" {
                second["parents"] = json!([first]);
                second["stage"] = json!("absent");
                second["evidence"] = json!({"kind":"observation","object":Object::from_bytes(b"opaque absence observation")});
            }
            let second = cache_put(f.cache.path(), &f.layout, second, &mut f.objects);
            page["events"]
                .as_array_mut()
                .unwrap()
                .push(serde_json::to_value(second).unwrap());
        }
        let root = cache_put(f.cache.path(), &f.layout, page, &mut f.objects);
        f.edit_record(|record| {
            let stream = record["streams"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|s| s["slot"] == "delivery-history")
                .unwrap();
            stream["object"] = serde_json::to_value(root).unwrap();
            if mode == "untracked" {
                record["metadata"]["delivery"]
                    .as_object_mut()
                    .unwrap()
                    .remove("history_slot");
            }
        });
        let summary = f.summary();
        assert_eq!(summary["targets"]["claims"], 0);
        let field = match mode {
            "partial" => "unknown_coverage",
            "ambiguous" => "ambiguous",
            "absent" => "absent",
            _ => "untracked",
        };
        assert_eq!(summary["targets"][field], 1);
        assert_eq!(summary["complete_semantic_coverage"], false);
    }
}

#[test]
fn revision_only_history_avoids_delivery_reads_and_planned_assets_need_no_selection() {
    let mut f = HistoryFixture::new(Layout::ReelCas, true);
    f.edit_record(|record| {
        record["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("delivery");
        record["selected"] = Value::Null;
        record["metadata"]["revisions"]
            .as_object_mut()
            .unwrap()
            .remove("selection");
    });
    let mut profile = f.profile();
    profile["byte_source"]["limits"] = json!({"requests":2});
    let result = f.run(&profile);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["metadata_objects"], 2);
    assert_eq!(summary["revisions"], 1);
    assert_eq!(summary["delivery_configured"], false);
    assert!(summary["selected_revision"].is_null());
}

#[test]
fn history_cli_rejects_argument_and_dialect_mismatches_without_profile_access() {
    for args in [
        vec!["--history", "missing-private-profile", "private-id"],
        vec![
            "--history",
            "missing-private-profile",
            "private-id",
            "unsupported-dialect",
        ],
        vec![
            "--history",
            "missing-private-profile",
            "private-id",
            "2020-12",
            "extra",
        ],
    ] {
        let result = Command::new(env!("CARGO_BIN_EXE_git-asset-resolve"))
            .args(args)
            .output()
            .unwrap();
        assert!(!result.status.success() && result.stdout.is_empty());
        let error = String::from_utf8(result.stderr).unwrap();
        assert!(
            !error.contains("cannot open")
                && !error.contains("missing-private-profile")
                && !error.contains("private-id")
        );
    }
}

#[cfg(windows)]
#[test]
fn history_cli_keeps_denied_reads_distinct_from_absence_without_leaking_paths() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let _held = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(f.layout.object_path(f.cache.path(), &f.root.sha256))
        .unwrap();
    let result = f.run(&f.profile());
    assert!(!result.status.success() && result.stdout.is_empty());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("Denied") || error.contains("Busy"));
    assert!(
        !error.contains("Missing")
            && !error.contains("private")
            && !error.contains(f.cache.path().to_str().unwrap())
    );
}

#[cfg(not(windows))]
#[test]
fn unsupported_history_protection_is_not_reported_as_missing_bytes() {
    let f = HistoryFixture::new(Layout::ReelCas, true);
    let mut profile = f.profile();
    profile["byte_source"]["required"] = json!("prevent-mutation");
    let result = f.run(&profile);
    assert!(!result.status.success() && result.stdout.is_empty());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(
        error.contains("Unsupported") && !error.contains("Missing") && !error.contains("private")
    );
}
