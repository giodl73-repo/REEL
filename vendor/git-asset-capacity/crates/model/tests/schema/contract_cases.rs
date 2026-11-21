use super::*;
use git_asset_model::contracts::{Contract, Error as ContractError};
fn bind_doc(docs: &(Vec<u8>, Vec<u8>, Vec<u8>)) -> BoundInstance {
    super::bound(docs)
}

fn rule(assertion: Value) -> Value {
    json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":assertion})
}
fn make(
    streams_rule: Value,
    record_rule: Value,
    streams: Value,
    authored: Value,
    selected: Value,
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (instance, definition, schema) = documents(&json!({"type":"object"}), authored);
    let mut definition: Value = serde_json::from_slice(&definition).unwrap();
    definition["stream_rules"] = rule(streams_rule);
    definition["semantic_rules"] = rule(record_rule);
    let definition = serde_json::to_vec(&definition).unwrap();
    let pin = Object::from_bytes(&definition);
    let mut instance: Value = serde_json::from_slice(&instance).unwrap();
    instance["definition"]["sha256"] = json!(pin.sha256);
    instance["definition"]["bytes"] = json!(pin.bytes);
    instance["streams"] = streams;
    instance["selected"] = selected;
    (serde_json::to_vec(&instance).unwrap(), definition, schema)
}
fn stream(slot: &str, role: &str, group: &str, attempt: &str) -> Value {
    json!({"slot":slot,"role":role,"group":group,"object":Object::from_bytes(slot.as_bytes()),"provenance":{"attempt":attempt}})
}
fn check(docs: &(Vec<u8>, Vec<u8>, Vec<u8>)) -> Result<(), ContractError> {
    let bound = bind_doc(docs);
    Contract::compile(bound.definition(), &docs.2, Dialect::Draft202012)?
        .check(&bound)
        .map(|_| ())
}

#[test]
fn local_contract_enforces_stream_roles_groups_cardinality_and_order_on_mixed_revisions() {
    let rules = json!({"type":"array","minItems":3,"maxItems":3,"allOf":[{"items":{"properties":{"group":{"const":"input"},"role":{"enum":["image","audio"]}}}}],
        "prefixItems":[{"properties":{"role":{"const":"image"}}},{"properties":{"role":{"const":"image"}}},{"properties":{"role":{"const":"audio"}}}]});
    let a = stream("cel-a", "image", "input", "branch-one");
    let b = stream("cel-b", "image", "input", "branch-two");
    let cue = stream("shared-cue", "audio", "input", "original");
    let good = make(
        rules.clone(),
        json!(true),
        json!([a, b, cue]),
        json!({}),
        Value::Null,
    );
    assert!(check(&good).is_ok()); // Unusual mixing is valid when the contract permits it.
    for streams in [
        json!([cue, b, a]),
        json!([a, b]),
        json!([a, b, cue, stream("other", "image", "input", "x")]),
        json!([a, stream("cel-b", "image", "history", "x"), cue]),
    ] {
        assert!(
            check(&make(
                rules.clone(),
                json!(true),
                streams,
                json!({}),
                Value::Null
            ))
            .is_err()
        );
    }
}

#[test]
fn local_contract_executes_cross_field_ready_and_planned_conditions_without_setting_status() {
    let rules = json!({"if":{"properties":{"authored":{"properties":{"kind":{"const":"ready"}}}}},
        "then":{"properties":{"selected":{"type":"object"},"streams":{"minItems":1}}},"else":{"properties":{"selected":{"type":"null"}}}});
    let image = stream("cel", "image", "input", "one");
    for (kind, streams, selected, valid) in [
        ("draft", json!([]), Value::Null, true),
        (
            "ready",
            json!([image]),
            json!(Object::from_bytes(b"master")),
            true,
        ),
        ("ready", json!([image]), Value::Null, false),
        (
            "ready",
            json!([]),
            json!(Object::from_bytes(b"master")),
            false,
        ),
        (
            "draft",
            json!([image]),
            json!(Object::from_bytes(b"master")),
            false,
        ),
    ] {
        let docs = make(
            json!(true),
            rules.clone(),
            streams,
            json!({"kind":kind}),
            selected,
        );
        let original = docs.0.clone();
        assert_eq!(check(&docs).is_ok(), valid);
        assert_eq!(docs.0, original);
        assert!(bind_doc(&docs).instance().value().status.is_empty());
    }
}

#[test]
fn local_contract_legacy_unknown_incomplete_and_wrong_dialect_rules_are_not_accepted() {
    let docs = documents(&json!(true), json!({}));
    assert!(matches!(
        Contract::compile(bind_doc(&docs).definition(), &docs.2, Dialect::Draft202012),
        Err(ContractError::Unspecified)
    ));
    let base = make(json!(true), json!(true), json!([]), json!({}), Value::Null);
    for invalid in [
        json!({"groups":[]}),
        json!({"schema":"unknown","dialect":"2020-12","assertion":true}),
        json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12"}),
        json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"2020-12","assertion":true,"extra":true}),
        json!({"schema":"tig.local-json-schema-rules.experimental-1","dialect":"7","assertion":true}),
    ] {
        let mut definition: Value = serde_json::from_slice(&base.1).unwrap();
        definition["stream_rules"] = invalid;
        let definition = Definition::parse(&serde_json::to_vec(&definition).unwrap()).unwrap();
        assert!(Contract::compile(&definition, &base.2, Dialect::Draft202012).is_err());
    }
}

#[test]
fn local_contract_pin_failures_and_diagnostics_do_not_reveal_private_values_or_invent_coverage() {
    let docs = make(
        json!(true),
        json!({"properties":{"authored":{"properties":{"text":{"const":"required-private-text"}}}}}),
        json!([]),
        json!({"text":"PRIVATE ACTUAL TEXT"}),
        Value::Null,
    );
    let bound = bind_doc(&docs);
    let contract = Contract::compile(bound.definition(), &docs.2, Dialect::Draft202012).unwrap();
    let error = contract.check(&bound).unwrap_err();
    assert!(!format!("{error:?} {error}").contains("private"));
    let other = make(json!(true), json!(true), json!([]), json!({}), Value::Null);
    let good = Contract::compile(
        bind_doc(&other).definition(),
        &other.2,
        Dialect::Draft202012,
    )
    .unwrap();
    assert!(good.check(&bound).is_err());
    let checked = good.check(&bind_doc(&other)).unwrap();
    assert!(!checked.complete_semantic_coverage());
    assert_eq!(checked.policy(), &Object::from_bytes(good.policy_bytes()));
    assert_eq!(checked.instance(), &Object::from_bytes(&other.0));
    assert_ne!(checked.authored_policy(), checked.policy());
    assert!(!format!("{checked:?}").contains("DO NOT PRINT"));
}

#[test]
fn local_contract_rules_reuse_offline_interpreter_and_authored_validation() {
    let external = make(
        json!({"$ref":"https://127.0.0.1:9/not-allowed"}),
        json!(true),
        json!([]),
        json!({}),
        Value::Null,
    );
    assert!(check(&external).is_err());
    let false_rule = make(json!(false), json!(true), json!([]), json!({}), Value::Null);
    assert!(check(&false_rule).is_err());
    let mut docs = make(
        json!(true),
        json!(true),
        json!([]),
        json!({"text":"private"}),
        Value::Null,
    );
    let authored = serde_json::to_vec(&json!({"properties":{"text":{"type":"integer"}}})).unwrap();
    let mut definition: Value = serde_json::from_slice(&docs.1).unwrap();
    definition["authored_schema"] = json!(Object::from_bytes(&authored));
    docs.1 = serde_json::to_vec(&definition).unwrap();
    docs.2 = authored;
    let pin = Object::from_bytes(&docs.1);
    let mut instance: Value = serde_json::from_slice(&docs.0).unwrap();
    instance["definition"]["sha256"] = json!(pin.sha256);
    instance["definition"]["bytes"] = json!(pin.bytes);
    docs.0 = serde_json::to_vec(&instance).unwrap();
    assert!(check(&docs).is_err());
}
