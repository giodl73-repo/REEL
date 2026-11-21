use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn write(root: &Path, name: &str, value: &Value) -> Vec<u8> {
    let bytes = serde_json::to_vec_pretty(value).unwrap();
    fs::write(root.join(name), &bytes).unwrap();
    bytes
}

fn fixture(root: &Path, family: &str) -> Value {
    let schema = write(root, "schema.json", &json!({"type":"object"}));
    let definition = write(
        root,
        "definition.json",
        &json!({
        "schema":"tig.definition.design-1", "id":format!("type/{family}"),
        "authored_schema":Object::from_bytes(&schema),
        "authoring":{"properties":["text"],"required":["text"],"defaults":{}},
        "stream_rules":{},"semantic_rules":{}}),
    );
    let mut pin = serde_json::to_value(Object::from_bytes(&definition)).unwrap();
    pin["id"] = json!(format!("type/{family}"));
    let record = json!({"schema":"tig.instance.design-1", "id":format!("{family}/one"),
        "definition":pin, "authored":{"text":"PRIVATE TEXT NEVER PRINT"},
        "streams":[{"slot":"output","role":"delivery","group":"current",
            "object":Object::from_bytes(b"approved output"),
            "provenance":{"private":"DO NOT PRINT PROVENANCE"}}],
        "selected":Object::from_bytes(b"selected manifest"),
        "status":{"private":"DO NOT PRINT STATUS"},"metadata":{}});
    write(root, "item.asset", &record);
    record
}

fn invoke(root: &Path, command: &str, slot: Option<&str>) -> Output {
    let mut process = Command::new(env!("CARGO_BIN_EXE_git-asset-record"));
    process
        .current_dir(root)
        .args([command, "item.asset", "definition.json", "schema.json"]);
    if let Some(slot) = slot {
        process.arg(slot);
    }
    process.output().unwrap()
}

#[cfg(feature = "schema")]
#[test]
fn actual_cli_executes_pinned_schema_without_mutation_or_private_error_text() {
    for family in ["scene", "invoice"] {
        let root = tempfile::tempdir().unwrap();
        let mut record = fixture(root.path(), family);
        let original = fs::read(root.path().join("item.asset")).unwrap();
        let result = invoke(root.path(), "validate", Some("2020-12"));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let output: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(output["authored_schema_validated"], true);
        assert_eq!(output["complete_semantic_coverage"], false);
        assert_eq!(output["payload_verified"], false);
        assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE"));
        assert_eq!(fs::read(root.path().join("item.asset")).unwrap(), original);
        let schema = write(
            root.path(),
            "schema.json",
            &json!({"properties":{"text":{"type":"integer"}}}),
        );
        let mut definition: Value =
            serde_json::from_slice(&fs::read(root.path().join("definition.json")).unwrap())
                .unwrap();
        definition["authored_schema"] = json!(Object::from_bytes(&schema));
        let raw = write(root.path(), "definition.json", &definition);
        let pin = Object::from_bytes(&raw);
        record["definition"]["sha256"] = json!(pin.sha256);
        record["definition"]["bytes"] = json!(pin.bytes);
        write(root.path(), "item.asset", &record);
        let result = invoke(root.path(), "validate", Some("2020-12"));
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    }
}

#[cfg(not(feature = "schema"))]
#[test]
fn actual_cli_without_schema_feature_refuses_validation_before_input_reads() {
    let root = tempfile::tempdir().unwrap();
    let result = invoke(root.path(), "validate", Some("2020-12"));
    assert!(!result.status.success());
    assert_eq!(
        String::from_utf8_lossy(&result.stderr).trim(),
        "schema feature required"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    let contract = invoke(root.path(), "validate-contract", Some("2020-12"));
    assert_eq!(
        String::from_utf8_lossy(&contract.stderr).trim(),
        "schema feature required"
    );
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

#[cfg(feature = "schema")]
#[test]
fn actual_cli_checks_explicit_local_contract_and_refuses_legacy_without_private_output() {
    for family in ["scene", "invoice"] {
        let root = tempfile::tempdir().unwrap();
        let mut record = fixture(root.path(), family);
        assert!(
            !invoke(root.path(), "validate-contract", Some("2020-12"))
                .status
                .success()
        );
        assert!(
            invoke(root.path(), "validate", Some("2020-12"))
                .status
                .success()
        );
        let mut definition: Value =
            serde_json::from_slice(&fs::read(root.path().join("definition.json")).unwrap())
                .unwrap();
        definition["stream_rules"] = json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":{"minItems":1,"items":{"properties":{"role":{"const":"delivery"}}}}});
        definition["semantic_rules"] = json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":{"properties":{"selected":{"type":"object"}}}});
        let raw = write(root.path(), "definition.json", &definition);
        let pin = Object::from_bytes(&raw);
        record["definition"]["sha256"] = json!(pin.sha256);
        record["definition"]["bytes"] = json!(pin.bytes);
        let before = write(root.path(), "item.asset", &record);
        let result = invoke(root.path(), "validate-contract", Some("2020-12"));
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(value["local_contracts_validated"], true);
        assert_eq!(value["complete_semantic_coverage"], false);
        assert!(!String::from_utf8_lossy(&result.stdout).contains("PRIVATE"));
        assert_eq!(fs::read(root.path().join("item.asset")).unwrap(), before);
        record["streams"][0]["role"] = json!("PRIVATE WRONG ROLE");
        write(root.path(), "item.asset", &record);
        let result = invoke(root.path(), "validate-contract", Some("2020-12"));
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!String::from_utf8_lossy(&result.stderr).contains("PRIVATE"));
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    }
}

#[test]
fn actual_cli_inspects_media_and_nonmedia_consumers_without_private_text() {
    for family in ["scene", "invoice"] {
        let root = tempfile::tempdir().unwrap();
        let record = fixture(root.path(), family);
        let output = invoke(root.path(), "inspect", None);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["stream_count"], 1);
        assert_eq!(result["complete_semantic_coverage"], false);
        let raw = fs::read(root.path().join("item.asset")).unwrap();
        assert_eq!(
            result["instance"],
            serde_json::to_value(Object::from_bytes(&raw)).unwrap()
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("DO NOT PRINT"));
        let stream = invoke(root.path(), "stream", Some("output"));
        assert!(stream.status.success());
        let pin: Value = serde_json::from_slice(&stream.stdout).unwrap();
        assert_eq!(pin["object"], record["streams"][0]["object"]);
        assert_eq!(pin["payload_verified"], false);
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
    }
}

#[test]
fn actual_cli_refuses_missing_or_changed_inputs_and_never_falls_back() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "invoice");
    assert!(
        !invoke(root.path(), "stream", Some("missing"))
            .status
            .success()
    );
    fs::write(root.path().join("schema.json"), b"{}").unwrap();
    let failed = invoke(root.path(), "inspect", None);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    fs::remove_file(root.path().join("schema.json")).unwrap();
    assert!(!invoke(root.path(), "inspect", None).status.success());
}

#[test]
fn actual_cli_bounds_file_reads_and_argument_shape() {
    let root = tempfile::tempdir().unwrap();
    fixture(root.path(), "invoice");
    fs::write(root.path().join("item.asset"), vec![b' '; 65_537]).unwrap();
    let failed = invoke(root.path(), "inspect", None);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("byte budget"));
    assert!(
        !Command::new(env!("CARGO_BIN_EXE_git-asset-record"))
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(!invoke(root.path(), "unsupported", None).status.success());
}
