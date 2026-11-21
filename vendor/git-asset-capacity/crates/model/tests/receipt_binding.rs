#![cfg(feature = "schema")]
use git_asset_core::Object;
use git_asset_model::publication::{Error, ParsedBinding, ReceiptChecker};
use serde_json::{Value, json};

fn raw(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}
fn fixture() -> (Value, Value, Vec<u8>) {
    let subject = Object::from_bytes(b"synthetic retained artifact");
    let receipt = raw(
        &json!({"rows":[{"sha256":subject.sha256,"bytes":subject.bytes,
        "path":"private-synthetic/item.bin"}],"duration":0.125}),
    );
    let mapping = json!({"schema":"tig.receipt-mapping.experimental-1",
        "limits":{"bytes":1048576,"nodes":8192,"depth":32},
        "rules":[{"kind":"array-rows","root":"/rows","fields":{"sha256":"/sha256","bytes":"/bytes","target":"/path"}}]});
    let binding = json!({"schema":"tig.receipt-binding.experimental-1","subject":subject,"receipt":Object::from_bytes(&receipt),
        "provider":"synthetic-provider","scope":"private-synthetic-scope","selector":"/rows/0","mapping":Object::from_bytes(&raw(&mapping))});
    (binding, mapping, receipt)
}

#[test]
fn explicit_receipt_binding_needs_no_native_asset_definition_or_authored_target_fields() {
    let (binding, mapping, receipt) = fixture();
    let checker = ReceiptChecker::new(&raw(&binding), &raw(&mapping)).unwrap();
    let result = checker.check(&receipt).unwrap();
    assert_eq!(result.binding, Some(Object::from_bytes(&raw(&binding))));
    assert!(
        result.receipt_bytes_verified
            && result.retained_receipt_binding_validated
            && result.receipt_length_asserted
    );
    assert!(
        !result.payload_verified
            && !result.provider_identity_verified
            && !result.current_remote_content_verified
    );
    assert!(
        !result.complete_semantic_coverage
            && !result.independent_custody_verified
            && !result.authorizes_eviction
    );
    let output = serde_json::to_string(&result).unwrap();
    for private in [
        "synthetic-provider",
        "private-synthetic",
        "duration",
        "selector",
    ] {
        assert!(!output.contains(private));
    }
}

#[test]
fn binding_mapping_and_receipt_pins_are_exact_and_wrong_subject_refuses() {
    let (binding, mapping, receipt) = fixture();
    let mut whitespace_mapping = raw(&mapping);
    whitespace_mapping.push(b' ');
    assert!(matches!(
        ReceiptChecker::new(&raw(&binding), &whitespace_mapping),
        Err(Error::Pin)
    ));
    assert_eq!(
        ReceiptChecker::new(&raw(&binding), &raw(&mapping))
            .unwrap()
            .check(b"wrong receipt")
            .err(),
        Some(Error::Pin)
    );
    let mut wrong = binding.clone();
    wrong["subject"] = serde_json::to_value(Object::from_bytes(b"other artifact")).unwrap();
    assert_eq!(
        ReceiptChecker::new(&raw(&wrong), &raw(&mapping))
            .unwrap()
            .check(&receipt)
            .err(),
        Some(Error::Subject)
    );
    let mut conflict = binding;
    conflict["subject"] = conflict["receipt"].clone();
    conflict["subject"]["bytes"] = json!(1);
    assert!(matches!(
        ParsedBinding::parse(&raw(&conflict)),
        Err(Error::Pin)
    ));
}

#[test]
fn strict_binding_and_mapping_schemas_do_not_accept_legacy_role_fields_or_duplicates() {
    let (binding, mapping, _) = fixture();
    for mode in [
        "binding-extra",
        "mapping-extra",
        "version",
        "selector",
        "duplicate",
    ] {
        let mut b = binding.clone();
        let mut m = mapping.clone();
        match mode {
            "binding-extra" => b["native_instance_required"] = json!(false),
            "mapping-extra" => m["subject_slot"] = json!("silently-ignored"),
            "version" => b["schema"] = json!("unsupported"),
            "selector" => b["selector"] = json!("/rows/~bad"),
            _ => (),
        }
        b["mapping"] = serde_json::to_value(Object::from_bytes(&raw(&m))).unwrap();
        let mut bytes = raw(&b);
        if mode == "duplicate" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replacen(
                    "\"provider\":",
                    "\"provider\":\"duplicate\",\"provider\":",
                    1,
                )
                .into_bytes();
        }
        assert!(ReceiptChecker::new(&bytes, &raw(&m)).is_err());
    }
}

#[test]
fn cached_receipt_still_enforces_each_binding_selector_and_stricter_limits() {
    let (binding, mapping, receipt) = fixture();
    let first = ReceiptChecker::new(&raw(&binding), &raw(&mapping)).unwrap();
    let parsed = first.parse_receipt(&receipt).unwrap();
    let mut stricter_mapping = mapping.clone();
    stricter_mapping["limits"]["nodes"] = json!(1);
    let mut stricter_binding = binding.clone();
    stricter_binding["mapping"] =
        serde_json::to_value(Object::from_bytes(&raw(&stricter_mapping))).unwrap();
    let stricter = ReceiptChecker::new(&raw(&stricter_binding), &raw(&stricter_mapping)).unwrap();
    assert_eq!(stricter.check_parsed(&parsed).err(), Some(Error::Budget));
    let mut absent_row = binding;
    absent_row["selector"] = json!("/rows/1");
    assert!(
        ReceiptChecker::new(&raw(&absent_row), &raw(&mapping))
            .unwrap()
            .check_parsed(&parsed)
            .is_err()
    );
    assert!(first.check_parsed(&parsed).is_ok());
}

#[test]
fn impossible_receipt_and_mapping_sizes_refuse_before_evidence_access() {
    let (binding, _, _) = fixture();
    for (field, bytes) in [("receipt", 16 * 1024 * 1024 + 1), ("mapping", 65_537)] {
        let mut oversized = binding.clone();
        oversized[field]["bytes"] = json!(bytes);
        assert!(matches!(
            ParsedBinding::parse(&raw(&oversized)),
            Err(Error::Budget)
        ));
    }
}
