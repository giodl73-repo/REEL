use super::*;
use git_asset_core::tree::TreeLimits;
use git_asset_model::schema::Dialect;
use git_asset_store::metadata::{Limits as StoreLimits, MetadataReader, MetadataStore};

fn fixture() -> Fixture {
    let mut f = Fixture::new();
    let schema = write(
        f.root(),
        "definitions/authored.json",
        &json!({"type":"object","properties":{"deps":{"type":"array"},"text":{"type":"string"}},"required":["deps","text"],"additionalProperties":false}),
    );
    for (i, name) in ["primary", "output"].into_iter().enumerate() {
        let path = format!("definitions/{name}.json");
        let mut value: Value =
            serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
        value["authored_schema"] = json!(schema);
        let object = write(f.root(), &path, &value);
        f.profiles[i].definition = object.clone();
        let mut pin = json!(object);
        pin["id"] = json!(value["id"]);
        if i == 0 {
            f.primary = pin
        } else {
            f.output = pin
        }
    }
    f
}

#[test]
fn native_schema_validation_checks_all_records_and_reuses_exact_definitions() {
    let f = fixture();
    for i in 0..1000 {
        f.record(
            &format!("row-{i}"),
            &format!("rows/{i}"),
            true,
            json!([]),
            None,
        );
    }
    f.record("report", "outputs/report", false, json!([]), None);
    let source = capture(&reader(f.root()), commit(f.root()));
    let validated = source
        .validate_authored_schemas(Dialect::Draft202012)
        .unwrap();
    assert_eq!(validated.records_checked(), 1001);
    assert_eq!(validated.definitions_checked(), 2);
    assert_eq!(validated.checkpoint(), source.checkpoint());
    assert!(!validated.complete_semantic_coverage());
    assert!(!validated.local_contracts_validated());
    assert_eq!(
        validated.resolve_id("rows/999").unwrap().reference(),
        source.resolve_id("rows/999").unwrap().reference()
    );
    assert!(validated.policy().bytes > 0);
}

#[test]
fn native_schema_validation_rejects_envelope_valid_authored_values_including_retained_history() {
    let f = fixture();
    f.record("row", "rows/1", true, json!([]), None);
    let repository = reader(f.root());
    let mut valid = capture(&repository, commit(f.root()));
    assert!(
        valid
            .validate_authored_schemas(Dialect::Draft202012)
            .is_ok()
    );
    let path = f.root().join("records/row.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["authored"]["text"] = json!(12);
    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
    let invalid = capture(&repository, commit(f.root()));
    assert!(invalid.resolve_id("rows/1").is_ok()); // Envelope intake is deliberately separate.
    assert!(
        invalid
            .validate_authored_schemas(Dialect::Draft202012)
            .is_err()
    );
    valid.retain(invalid).unwrap();
    assert!(valid.resolve_id("rows/1").unwrap().authored()["text"].is_string());
    assert!(
        valid
            .validate_authored_schemas(Dialect::Draft202012)
            .is_err()
    );
}

#[test]
fn native_schema_validation_reopens_archived_metadata_without_git_or_media_io() {
    let f = fixture();
    f.record("row", "rows/1", true, json!([]), None);
    let source = capture(&reader(f.root()), commit(f.root()));
    let runtime = tempfile::tempdir().unwrap();
    let path = runtime.path().join("metadata");
    let mut store = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let root = source
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let source = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let validated = source
        .validate_authored_schemas(Dialect::Draft202012)
        .unwrap();
    assert_eq!(validated.records_checked(), 1);
    assert!(validated.resolve_id("rows/1").is_ok());
    assert!(!f.root().join(".git").exists());
}

fn contract_fixture() -> Fixture {
    let mut f = fixture();
    for (i, name) in ["primary", "output"].into_iter().enumerate() {
        let path = format!("definitions/{name}.json");
        let mut value: Value =
            serde_json::from_slice(&fs::read(f.root().join(&path)).unwrap()).unwrap();
        value["stream_rules"] = json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":{"type":"array","minItems":1,"maxItems":2,"items":{"properties":{"role":{"const":"review"},"group":{"const":"draft"}}}}});
        value["semantic_rules"] = json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":{"properties":{"authored":{"properties":{"text":{"minLength":1}}}}}});
        let object = write(f.root(), &path, &value);
        f.profiles[i].definition = object.clone();
        let mut pin = json!(object);
        pin["id"] = json!(value["id"]);
        if i == 0 {
            f.primary = pin
        } else {
            f.output = pin
        }
    }
    f
}

#[test]
fn native_local_contracts_refuse_legacy_and_preserve_authored_only_distinction() {
    let f = fixture();
    f.record("row", "rows/1", true, json!([]), None);
    let source = capture(&reader(f.root()), commit(f.root()));
    assert!(
        !source
            .validate_authored_schemas(Dialect::Draft202012)
            .unwrap()
            .local_contracts_validated()
    );
    assert!(
        source
            .validate_local_contracts(Dialect::Draft202012)
            .is_err()
    );
}

#[test]
fn native_local_contracts_validate_current_and_retained_stream_and_record_assertions() {
    let f = contract_fixture();
    f.record("row", "rows/1", true, json!([]), None);
    let repository = reader(f.root());
    let mut good = capture(&repository, commit(f.root()));
    let validated = good.validate_local_contracts(Dialect::Draft202012).unwrap();
    assert!(validated.local_contracts_validated());
    assert_eq!(validated.records_checked(), 1);
    assert_eq!(
        validated.resolve_id("rows/1").unwrap().reference(),
        good.resolve_id("rows/1").unwrap().reference()
    );
    assert!(!validated.complete_semantic_coverage());
    let path = f.root().join("records/row.asset");
    let mut value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["streams"][0]["role"] = json!("unadmitted");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let bad = capture(&repository, commit(f.root()));
    assert!(bad.validate_authored_schemas(Dialect::Draft202012).is_ok());
    assert!(bad.validate_local_contracts(Dialect::Draft202012).is_err());
    good.retain(bad).unwrap();
    assert!(good.validate_local_contracts(Dialect::Draft202012).is_err());
    value["streams"][0]["role"] = json!("review");
    value["authored"]["text"] = json!("");
    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
    let bad = capture(&repository, commit(f.root()));
    assert!(bad.validate_authored_schemas(Dialect::Draft202012).is_ok());
    assert!(bad.validate_local_contracts(Dialect::Draft202012).is_err());
}

#[test]
fn native_local_contracts_replay_after_metadata_reopen_with_git_offline() {
    let f = contract_fixture();
    f.record("row", "rows/1", true, json!([]), None);
    let source = capture(&reader(f.root()), commit(f.root()));
    let runtime = tempfile::tempdir().unwrap();
    let path = runtime.path().join("metadata");
    let mut store = MetadataStore::create(
        &path,
        StoreLimits {
            nodes: 8192,
            bytes: 8 * 1024 * 1024,
            database_bytes: 16 * 1024 * 1024,
            min_free_bytes: 65536,
        },
    )
    .unwrap();
    let root = source
        .archive_metadata(&mut store, TreeLimits::default())
        .unwrap();
    let witness = store.finish().unwrap();
    drop(store);
    drop(source);
    for name in [".git", "records", "definitions"] {
        fs::rename(
            f.root().join(name),
            f.root().join(format!("offline-{name}")),
        )
        .unwrap();
    }
    let mut store = MetadataReader::open(&path, &witness).unwrap();
    let source = AssetSnapshot::from_metadata_archive(
        &mut store,
        &root,
        Limits::default(),
        TreeLimits::default(),
    )
    .unwrap();
    let validated = source
        .validate_local_contracts(Dialect::Draft202012)
        .unwrap();
    assert!(validated.local_contracts_validated());
    assert!(validated.resolve_id("rows/1").is_ok());
    assert!(!f.root().join(".git").exists());
}
