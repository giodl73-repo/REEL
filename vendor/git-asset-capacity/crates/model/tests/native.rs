use git_asset_core::Object;
use git_asset_model::native::{Definition, Instance, Pin};
use serde_json::{Value, json};

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn pin(payload: &[u8]) -> Value {
    serde_json::to_value(Object::from_bytes(payload)).unwrap()
}

fn fixture() -> (Value, Vec<u8>, Vec<u8>) {
    let schema = bytes(&json!({"type":"object", "properties":{"title":{"type":"string"}}}));
    let definition = bytes(
        &json!({"schema":"tig.definition.design-1", "id":"type/chapter",
        "authored_schema":pin(&schema),
        "authoring":{"properties":["title"],"required":["title"],"defaults":{}},
        "stream_rules":{},"semantic_rules":{"admitted_adapter":"example"}}),
    );
    let mut definition_pin = pin(&definition);
    definition_pin["id"] = json!("type/chapter");
    let record = json!({"schema":"tig.instance.design-1", "id":"chapter/04",
        "definition":definition_pin, "authored":{"title":"A chapter"},
        "streams":[{"slot":"cel-b","role":"input/image","group":"primary","object":pin(b"image v1")},
                   {"slot":"cue-1","role":"input/audio","group":"primary","object":pin(b"audio v1")}],
        "selected":pin(b"result manifest"), "status":{"review":"golden"},"metadata":{}});
    (record, definition, schema)
}

#[test]
fn separate_exact_definition_and_schema_bind_without_claiming_semantics() {
    let (record, definition, schema) = fixture();
    let raw = bytes(&record);
    let parsed = Instance::parse(&raw).unwrap();
    let bound = parsed.bind(&definition, &schema).unwrap();
    assert_eq!(bound.instance().object(), &Object::from_bytes(&raw));
    assert_eq!(bound.instance().value().status["review"], "golden");
    assert!(!bound.complete_semantic_coverage());
}

#[test]
fn raw_whitespace_changes_revision_but_not_parsed_value() {
    let (record, _, _) = fixture();
    let compact = Instance::parse(&bytes(&record)).unwrap();
    let pretty = Instance::parse(&serde_json::to_vec_pretty(&record).unwrap()).unwrap();
    assert_eq!(compact.value(), pretty.value());
    assert_ne!(compact.object(), pretty.object());
}

#[test]
fn binding_rejects_changed_definition_schema_and_id() {
    let (record, definition, schema) = fixture();
    let parsed = Instance::parse(&bytes(&record)).unwrap();
    let mut changed = definition.clone();
    changed.push(b' ');
    assert!(parsed.bind(&changed, &schema).is_err());
    assert!(parsed.bind(&definition, b"{}").is_err());
    let mut wrong_id = record;
    wrong_id["definition"]["id"] = json!("type/other");
    assert!(
        Instance::parse(&bytes(&wrong_id))
            .unwrap()
            .bind(&definition, &schema)
            .is_err()
    );
}

#[test]
fn declared_authoring_fields_are_checked_without_injecting_defaults() {
    let (record, definition, schema) = fixture();
    let mut missing = record.clone();
    missing["authored"] = json!({});
    assert!(
        Instance::parse(&bytes(&missing))
            .unwrap()
            .bind(&definition, &schema)
            .is_err()
    );
    let mut extra = record;
    extra["authored"]["unknown"] = json!(1);
    assert!(
        Instance::parse(&bytes(&extra))
            .unwrap()
            .bind(&definition, &schema)
            .is_err()
    );
    let mut def: Value = serde_json::from_slice(&definition).unwrap();
    def["authoring"]["required"] = json!([]);
    def["authoring"]["defaults"] = json!({"title":"default"});
    let raw_def = bytes(&def);
    missing["definition"]["sha256"] = json!(Object::from_bytes(&raw_def).sha256);
    missing["definition"]["bytes"] = json!(raw_def.len());
    let bound = Instance::parse(&bytes(&missing))
        .unwrap()
        .bind(&raw_def, &schema)
        .unwrap();
    assert!(bound.instance().value().authored.is_empty());
}

#[test]
fn mixed_ordered_streams_remain_an_explicit_new_selection() {
    let (base, definition, schema) = fixture();
    let mut mixed = base.clone();
    mixed["streams"][0]["object"] = pin(b"image v2 from branch B");
    mixed["streams"][1]["provenance"] = json!({"branch":"A"});
    let bound = Instance::parse(&bytes(&mixed))
        .unwrap()
        .bind(&definition, &schema)
        .unwrap();
    assert_eq!(bound.instance().value().streams[0].slot, "cel-b");
    assert_eq!(mixed["streams"][1]["object"], base["streams"][1]["object"]);
    assert_eq!(mixed["selected"], base["selected"]); // Retained result is not proved fresh.
    assert!(!bound.complete_semantic_coverage());
}

#[test]
fn duplicate_slots_and_conflicting_digest_sizes_refuse() {
    let (mut record, definition, schema) = fixture();
    record["streams"][1]["slot"] = json!("cel-b");
    assert!(Instance::parse(&bytes(&record)).is_err());
    record["streams"][1]["slot"] = json!("cue-1");
    record["streams"][1]["object"] = record["streams"][0]["object"].clone();
    record["streams"][1]["object"]["bytes"] = json!(999);
    assert!(Instance::parse(&bytes(&record)).is_err());
    record["streams"][1]["object"] = pin(&schema);
    record["streams"][1]["object"]["bytes"] = json!(999);
    assert!(
        Instance::parse(&bytes(&record))
            .unwrap()
            .bind(&definition, &schema)
            .is_err()
    );
}

#[test]
fn explicit_null_is_required_for_planned_instances() {
    let (mut record, _, _) = fixture();
    record["selected"] = Value::Null;
    record["streams"] = json!([]);
    assert!(Instance::parse(&bytes(&record)).is_ok());
    record.as_object_mut().unwrap().remove("selected");
    assert!(Instance::parse(&bytes(&record)).is_err());
}

#[test]
fn strict_json_limits_unknown_fields_and_malformed_shapes_refuse() {
    let (record, _, _) = fixture();
    let raw = bytes(&record);
    let mut duplicate = raw[..raw.len() - 1].to_vec();
    duplicate.extend_from_slice(b",\"id\":\"duplicate\"}");
    assert!(Instance::parse(&duplicate).is_err());
    assert!(Instance::parse(&vec![b' '; 65_537]).is_err());
    for (field, value) in [
        ("unknown", json!(true)),
        ("authored", json!([])),
        ("status", json!("golden")),
        ("metadata", json!(null)),
        ("schema", json!("future")),
        ("id", json!("\n")),
    ] {
        let mut bad = record.clone();
        bad[field] = value;
        assert!(Instance::parse(&bytes(&bad)).is_err(), "{field}");
    }
    let mut float = record;
    float["authored"]["duration"] = json!(1.5);
    assert!(Instance::parse(&bytes(&float)).is_err());
}

#[test]
fn pins_verify_exact_bytes_and_refuse_invalid_digests_or_unsafe_sizes() {
    let exact: Pin = serde_json::from_value(pin(b"payload")).unwrap();
    assert!(exact.verify(b"payload").is_ok());
    assert!(exact.verify(b"changed").is_err());
    let mut unsafe_pin = pin(b"payload");
    unsafe_pin["bytes"] = json!(9_007_199_254_740_992u64);
    assert!(
        serde_json::from_value::<Pin>(unsafe_pin)
            .unwrap()
            .verify(b"payload")
            .is_err()
    );
    let mut invalid = pin(b"payload");
    invalid["sha256"] = json!("A".repeat(64));
    assert!(serde_json::from_value::<Pin>(invalid).is_err());
}

#[test]
fn stream_budget_and_role_group_token_limits_refuse() {
    let (record, _, _) = fixture();
    let mut large = record.clone();
    large["streams"] = Value::Array(
        (0..257)
            .map(|i| {
                let mut s = record["streams"][0].clone();
                s["slot"] = json!(i.to_string());
                s
            })
            .collect(),
    );
    // The stricter whole-record node/byte budget may refuse first.
    assert!(Instance::parse(&bytes(&large)).is_err());
    for field in ["slot", "role", "group"] {
        let mut bad = record.clone();
        bad["streams"][0][field] = json!("x".repeat(129));
        assert!(Instance::parse(&bytes(&bad)).is_err());
    }
}

#[test]
fn definitions_reject_incoherent_authoring_declarations_and_extra_pin_fields() {
    let (_, definition, _) = fixture();
    let base: Value = serde_json::from_slice(&definition).unwrap();
    for (field, value) in [
        ("properties", json!(["title", "title"])),
        ("required", json!(["other"])),
        ("defaults", json!({"other":1})),
    ] {
        let mut bad = base.clone();
        bad["authoring"][field] = value;
        assert!(Definition::parse(&bytes(&bad)).is_err());
    }
    let mut bad = base;
    bad["authored_schema"]["id"] = json!("extra");
    assert!(Definition::parse(&bytes(&bad)).is_err());
}

#[test]
fn optional_fields_reject_null_instead_of_erasing_wire_differences() {
    let (record, _, _) = fixture();
    for field in ["id", "media_type"] {
        let mut bad = record.clone();
        bad["streams"][0]["object"][field] = Value::Null;
        assert!(Instance::parse(&bytes(&bad)).is_err());
    }
    let mut bad = record;
    bad["streams"][0]["provenance"] = Value::Null;
    assert!(Instance::parse(&bytes(&bad)).is_err());
}
