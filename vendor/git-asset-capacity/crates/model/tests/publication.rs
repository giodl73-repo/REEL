#![cfg(feature = "schema")]
use git_asset_core::Object;
use git_asset_model::{
    native::Instance,
    publication::{Checker, Error},
    schema::{Dialect, Schema},
};
use serde_json::{Value, json};
use std::{fs, process::Command};

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}
struct Fixture {
    record: Value,
    definition: Vec<u8>,
    schema: Vec<u8>,
    policy: Value,
    receipt: Vec<u8>,
}
impl Fixture {
    fn new() -> Self {
        let subject = Object::from_bytes(b"synthetic invoice");
        let receipt = bytes(
            &json!({"rows":[{"sha256":subject.sha256,"bytes":subject.bytes,
            "path":"Reports/private-invoice.pdf"}],"measured":0.125,"remote_sync_proven":true}),
        );
        let schema = bytes(
            &json!({"type":"object","properties":{"target":{"type":"object"}},"required":["target"]}),
        );
        let definition = bytes(
            &json!({"schema":"tig.definition.design-1","id":"example/publication",
            "authored_schema":Object::from_bytes(&schema),"authoring":{"properties":["target"],"required":["target"],"defaults":{}},
            "stream_rules":{},"semantic_rules":{}}),
        );
        let mut pin = serde_json::to_value(Object::from_bytes(&definition)).unwrap();
        pin["id"] = json!("example/publication");
        let record = json!({"schema":"tig.instance.design-1","id":"private-example-id","definition":pin,
            "authored":{"target":{"provider":"example-provider","scope":"private-scope","receipt_slot":"receipt","receipt_selector":"/rows/0"}},
            "selected":subject,"streams":[{"slot":"payload","role":"output/document","group":"retained","object":subject},
                {"slot":"receipt","role":"evidence/publication","group":"evidence","object":Object::from_bytes(&receipt)}],
            "status":{"review":"unclassified"},"metadata":{}});
        let policy = json!({"schema":"tig.publication-evidence-policy.experimental-1","target_pointer":"/target",
            "subject_slot":"payload","subject_role":"output/document","receipt_role":"evidence/publication",
            "limits":{"bytes":1048576,"nodes":8192,"depth":32},
            "rules":[{"kind":"array-rows","root":"/rows","fields":{"sha256":"/sha256","bytes":"/bytes","target":"/path"}}]});
        Self {
            record,
            definition,
            schema,
            policy,
            receipt,
        }
    }
    fn checker(&self) -> Result<Checker, Box<dyn std::error::Error>> {
        let record = Instance::parse(&bytes(&self.record))?;
        let bound = record.bind(&self.definition, &self.schema)?;
        let checked = Schema::compile(bound.definition(), &self.schema, Dialect::Draft202012)?
            .check(&bound)?;
        Ok(Checker::new(&bound, &checked, &bytes(&self.policy))?)
    }
    fn receipt(&mut self, value: Value) {
        self.raw_receipt(bytes(&value));
    }
    fn raw_receipt(&mut self, raw: Vec<u8>) {
        self.record["streams"][1]["object"] =
            serde_json::to_value(Object::from_bytes(&raw)).unwrap();
        self.receipt = raw;
    }
    fn accepts(&self) -> bool {
        self.checker().is_ok_and(|c| c.check(&self.receipt).is_ok())
    }
}

#[test]
fn array_rows_bind_exact_subject_without_promoting_receipt_claims() {
    let f = Fixture::new();
    let r = f.checker().unwrap().check(&f.receipt).unwrap();
    assert!(
        r.receipt_bytes_verified
            && r.retained_receipt_binding_validated
            && r.receipt_length_asserted
    );
    assert!(
        !r.payload_verified && !r.provider_identity_verified && !r.current_remote_content_verified
    );
    assert!(
        !r.independent_custody_verified && !r.authorizes_eviction && !r.complete_semantic_coverage
    );
    assert_eq!(r.receipt, Object::from_bytes(&f.receipt));
    let output = serde_json::to_string(&r).unwrap();
    for secret in [
        "private-invoice",
        "private-scope",
        "private-example-id",
        "measured",
    ] {
        assert!(!output.contains(secret));
    }
}

fn retained_subject_fixture() -> Fixture {
    let mut f = Fixture::new();
    f.record["selected"] =
        serde_json::to_value(Object::from_bytes(b"new source revision")).unwrap();
    f.policy["subject_selection"] = json!("declared-stream");
    f
}

#[test]
fn retained_subject_preserves_current_selection_and_reports_exact_difference() {
    let f = retained_subject_fixture();
    let original = bytes(&f.record);
    let report = f.checker().unwrap().check(&f.receipt).unwrap();
    assert_eq!(report.subject, Object::from_bytes(b"synthetic invoice"));
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["subject_selection"], "declared-stream");
    assert_eq!(value["subject_is_selected"], false);
    assert!(!report.payload_verified && !report.current_remote_content_verified);
    assert!(!report.independent_custody_verified && !report.authorizes_eviction);
    assert_eq!(bytes(&f.record), original);
    let default = Fixture::new();
    let old = default.checker().unwrap().check(&default.receipt).unwrap();
    assert_eq!(
        report.declared_target_identity,
        old.declared_target_identity
    );
}

#[test]
fn retained_subject_handles_unbuilt_selection_without_inventing_currentness() {
    let mut f = retained_subject_fixture();
    f.record["selected"] = Value::Null;
    let report = f.checker().unwrap().check(&f.receipt).unwrap();
    let value = serde_json::to_value(&report).unwrap();
    assert_eq!(value["subject_is_selected"], false);
    assert_eq!(report.subject, Object::from_bytes(b"synthetic invoice"));
    assert!(!report.complete_semantic_coverage && !report.authorizes_eviction);
}

#[test]
fn default_subject_policy_stays_strict_and_report_shape_is_compatible() {
    let mut f = retained_subject_fixture();
    f.policy
        .as_object_mut()
        .unwrap()
        .remove("subject_selection");
    assert!(matches!(
        f.checker().err().unwrap().downcast_ref::<Error>(),
        Some(Error::Subject)
    ));
    f.policy["subject_selection"] = json!("selected");
    assert!(matches!(
        f.checker().err().unwrap().downcast_ref::<Error>(),
        Some(Error::Subject)
    ));
    let f = Fixture::new();
    let implicit = serde_json::to_value(f.checker().unwrap().check(&f.receipt).unwrap()).unwrap();
    assert!(implicit.get("subject_selection").is_none());
    assert!(implicit.get("subject_is_selected").is_none());
    let mut explicit = Fixture::new();
    explicit.policy["subject_selection"] = json!("selected");
    let mut explicit = serde_json::to_value(
        explicit
            .checker()
            .unwrap()
            .check(&explicit.receipt)
            .unwrap(),
    )
    .unwrap();
    explicit["evidence_policy"] = implicit["evidence_policy"].clone();
    assert_eq!(implicit, explicit);
}

#[test]
fn retained_subject_exact_match_is_distinct_from_remote_or_build_currency() {
    let mut f = Fixture::new();
    f.policy["subject_selection"] = json!("declared-stream");
    let report = f.checker().unwrap().check(&f.receipt).unwrap();
    assert_eq!(
        serde_json::to_value(&report).unwrap()["subject_is_selected"],
        true
    );
    assert!(!report.current_remote_content_verified && !report.payload_verified);
    assert!(!report.provider_identity_verified && !report.complete_semantic_coverage);
}

#[test]
fn retained_subject_never_falls_back_from_wrong_slot_role_or_receipt() {
    for field in ["subject_slot", "subject_role"] {
        let mut f = retained_subject_fixture();
        f.policy[field] = json!("unknown");
        assert!(!f.accepts());
    }
    let mut f = retained_subject_fixture();
    f.record["streams"][0]["object"] = f.record["selected"].clone();
    assert!(!f.accepts());
    for mode in [Value::Null, json!("latest"), json!(true), json!({})] {
        let mut f = retained_subject_fixture();
        f.policy["subject_selection"] = mode;
        assert!(!f.accepts());
    }
}

#[test]
fn independent_targets_can_bind_different_retained_revisions_of_one_asset() {
    let mut f = retained_subject_fixture();
    let older = f.checker().unwrap().check(&f.receipt).unwrap();
    let newer = f.record["selected"].clone();
    f.record["streams"].as_array_mut().unwrap().push(json!({
        "slot":"newer-payload","role":"output/document","group":"retained","object":newer
    }));
    f.policy["subject_slot"] = json!("newer-payload");
    f.record["authored"]["target"]["provider"] = json!("second-provider");
    f.receipt(json!({"rows":[{"sha256":newer["sha256"],"bytes":newer["bytes"],"path":"Reports/private-invoice.pdf"}]}));
    let current = f.checker().unwrap().check(&f.receipt).unwrap();
    assert_eq!(
        serde_json::to_value(&current).unwrap()["subject_is_selected"],
        true
    );
    assert_ne!(current.subject, older.subject);
    assert_ne!(
        current.declared_target_identity,
        older.declared_target_identity
    );
    assert!(!current.current_remote_content_verified && !older.current_remote_content_verified);
}

#[test]
fn keyed_hash_rows_decode_escaped_paths_and_do_not_assert_length() {
    let mut f = Fixture::new();
    let hash = f.record["selected"]["sha256"].clone();
    f.receipt(json!({"hashes":{"Reports/a~b.pdf":hash}}));
    f.record["authored"]["target"]["receipt_selector"] = json!("/hashes/Reports~1a~0b.pdf");
    f.policy["rules"] = json!([{"kind":"keyed-hashes","root":"/hashes"}]);
    let r = f.checker().unwrap().check(&f.receipt).unwrap();
    assert!(!r.receipt_length_asserted);
    assert!(r.retained_receipt_binding_validated);
}

fn singleton_fixture() -> Fixture {
    let mut f = Fixture::new();
    let subject = f.record["selected"].clone();
    f.receipt(json!({"asset":subject,"path":"Reports/private-invoice.pdf","measured":0.125}));
    f.record["authored"]["target"]["receipt_selector"] = json!("");
    f.policy["rules"] = json!([{"kind":"single-row","root":"","fields":{
        "sha256":"/asset/sha256","bytes":"/asset/bytes","target":"/path"}}]);
    f
}

#[test]
fn singleton_root_receipt_binds_original_bytes_without_a_wrapper() {
    let f = singleton_fixture();
    let report = f.checker().unwrap().check(&f.receipt).unwrap();
    assert!(report.receipt_bytes_verified && report.receipt_length_asserted);
    assert_eq!(report.subject, Object::from_bytes(b"synthetic invoice"));
    assert_eq!(report.receipt, Object::from_bytes(&f.receipt));
    assert!(!report.payload_verified && !report.authorizes_eviction);
    let original = Fixture::new();
    let original = original
        .checker()
        .unwrap()
        .check(&original.receipt)
        .unwrap();
    assert_eq!(
        report.declared_target_identity,
        original.declared_target_identity
    );
    assert_ne!(report.evidence_policy, original.evidence_policy);
}

#[test]
fn singleton_requires_exact_object_selector_and_distinguishes_empty_member() {
    let mut f = singleton_fixture();
    let row: Value = serde_json::from_slice(&f.receipt).unwrap();
    f.receipt(json!({"receipt":row}));
    f.policy["rules"][0]["root"] = json!("/receipt");
    for selector in ["", "/receipt/asset", "/other"] {
        f.record["authored"]["target"]["receipt_selector"] = json!(selector);
        assert!(matches!(
            f.checker().unwrap().check(&f.receipt),
            Err(Error::Selector)
        ));
    }
    f.record["authored"]["target"]["receipt_selector"] = json!("/receipt");
    assert!(f.accepts());
    f.receipt(json!({"":row}));
    f.policy["rules"][0]["root"] = json!("/");
    f.record["authored"]["target"]["receipt_selector"] = json!("/");
    assert!(f.accepts());
    f.policy["rules"][0]["root"] = json!("");
    assert!(matches!(
        f.checker().unwrap().check(&f.receipt),
        Err(Error::Selector)
    ));
}

#[test]
fn singleton_never_wraps_nonobjects_or_falls_back_from_bad_fields() {
    for case in [
        "array", "null", "string", "hash", "length", "target", "escape", "missing",
    ] {
        let mut f = singleton_fixture();
        let mut row: Value = serde_json::from_slice(&f.receipt).unwrap();
        match case {
            "array" => row = json!([row]),
            "null" => row = Value::Null,
            "string" => row = json!("not a row"),
            "hash" => row["asset"]["sha256"] = json!("0".repeat(64)),
            "length" => row["asset"]["bytes"] = json!(1.5),
            "target" => row["path"] = json!(false),
            "escape" => row["path"] = json!("../private-escape.pdf"),
            _ => {
                row.as_object_mut().unwrap().remove("asset");
            }
        }
        f.receipt(row);
        assert!(!f.accepts(), "accepted {case}");
    }
}

#[test]
fn singleton_policy_refuses_member_keys_overlaps_and_unknown_shapes() {
    for case in ["member", "overlap", "duplicate", "unknown", "extra"] {
        let mut f = singleton_fixture();
        match case {
            "member" => f.policy["rules"][0]["fields"]["target"] = json!({"kind":"member-key"}),
            "overlap" => {
                let mut other = f.policy["rules"][0].clone();
                other["root"] = json!("/nested");
                f.policy["rules"].as_array_mut().unwrap().push(other);
            }
            "duplicate" => {
                let other = f.policy["rules"][0].clone();
                f.policy["rules"].as_array_mut().unwrap().push(other);
            }
            "unknown" => f.policy["rules"][0]["kind"] = json!("automatic-row"),
            _ => f.policy["rules"][0]["automatic"] = json!(true),
        }
        let error = f.checker().err().expect("invalid policy accepted");
        assert!(
            matches!(error.downcast_ref::<Error>(), Some(Error::Policy)),
            "accepted {case}"
        );
    }
}

#[test]
fn singleton_opaque_target_preserves_domain_pins_and_strict_reuse_budgets() {
    let mut f = singleton_fixture();
    let mut row: Value = serde_json::from_slice(&f.receipt).unwrap();
    row["path"] = json!(r"G:\private-invoice.pdf");
    f.receipt(row);
    assert!(!f.accepts());
    f.policy["rules"][0]["fields"]["target"] = json!({"kind":"opaque-locator","pointer":"/path"});
    let checker = f.checker().unwrap();
    let parsed = checker.parse_receipt(&f.receipt).unwrap();
    let report = checker.check_parsed(&parsed).unwrap();
    assert!(report.receipt_length_asserted && !report.provider_identity_verified);
    let mut wrong = f.receipt.clone();
    wrong.push(b' ');
    assert!(matches!(checker.check(&wrong), Err(Error::Pin)));
    f.policy["limits"]["nodes"] = json!(1);
    assert!(matches!(
        f.checker().unwrap().check_parsed(&parsed),
        Err(Error::Budget)
    ));
}

fn member_key_fixture(path: &str) -> Fixture {
    let mut f = Fixture::new();
    let object = f.record["selected"].clone();
    let mut files = serde_json::Map::new();
    files.insert(path.into(), object);
    f.receipt(json!({"files":files}));
    f.record["authored"]["target"]["receipt_selector"] = json!(format!(
        "/files/{}",
        path.replace('~', "~0").replace('/', "~1")
    ));
    f.policy["rules"] = json!([{"kind":"object-rows","root":"/files","fields":{
        "sha256":"/sha256","bytes":"/bytes","target":{"kind":"member-key"}}}]);
    f
}

fn opaque_fixture(locator: Value) -> Fixture {
    let mut f = Fixture::new();
    let subject = f.record["selected"].clone();
    f.receipt(
        json!({"rows":[{"sha256":subject["sha256"],"bytes":subject["bytes"],"path":locator}]}),
    );
    f.policy["rules"][0]["fields"]["target"] = json!({"kind":"opaque-locator","pointer":"/path"});
    f
}

#[test]
fn opaque_provider_locators_require_explicit_mode_and_never_relax_logical_paths() {
    for address in [
        r"G:\My Drive\private\episode.mp4",
        "https://example.invalid/private-file",
        "provider-item:123",
    ] {
        let f = opaque_fixture(json!(address));
        let report = f.checker().unwrap().check(&f.receipt).unwrap();
        assert!(report.retained_receipt_binding_validated && report.receipt_length_asserted);
        assert!(
            !report.provider_identity_verified
                && !report.current_remote_content_verified
                && !report.authorizes_eviction
        );
        assert!(!serde_json::to_string(&report).unwrap().contains(address));
        let mut ordinary = f;
        ordinary.policy["rules"][0]["fields"]["target"] = json!("/path");
        assert!(!ordinary.accepts());
    }
}

#[test]
fn opaque_locator_identity_has_a_separate_domain_and_preserves_literal_spelling() {
    let ordinary = Fixture::new();
    let opaque = opaque_fixture(json!("Reports/private-invoice.pdf"));
    assert_ne!(
        ordinary
            .checker()
            .unwrap()
            .check(&ordinary.receipt)
            .unwrap()
            .declared_target_identity,
        opaque
            .checker()
            .unwrap()
            .check(&opaque.receipt)
            .unwrap()
            .declared_target_identity
    );
    let addresses = [
        r"G:\Folder\item",
        "G:/Folder/item",
        "g:/Folder/item",
        "G:/Folder/%69tem",
    ];
    let mut identities = std::collections::BTreeSet::new();
    for address in addresses {
        let f = opaque_fixture(json!(address));
        identities.insert(
            f.checker()
                .unwrap()
                .check(&f.receipt)
                .unwrap()
                .declared_target_identity,
        );
    }
    assert_eq!(identities.len(), addresses.len());
}

#[test]
fn opaque_locator_markers_values_and_utf8_byte_bounds_are_strict() {
    for marker in [
        json!({"kind":"opaque-locator"}),
        json!({"kind":"opaque-locator","pointer":null}),
        json!({"kind":"opaque-locator","pointer":"/path","normalize":true}),
        json!({"kind":"opaque-locator","pointer":"/bad~2escape"}),
    ] {
        let mut f = opaque_fixture(json!("item"));
        f.policy["rules"][0]["fields"]["target"] = marker;
        assert!(f.checker().is_err());
    }
    for value in [
        json!(null),
        json!(42),
        json!({"id":"item"}),
        json!(""),
        json!("  "),
        json!("item\nsecret"),
        json!("item\u{0000}"),
        json!("é".repeat(2049)),
    ] {
        assert!(!opaque_fixture(value).accepts());
    }
    assert!(opaque_fixture(json!("é".repeat(2048))).accepts());
}

#[test]
fn opaque_locator_rows_still_require_exact_subject_length_and_pointer() {
    let mut f = opaque_fixture(json!(r"G:\Folder\item"));
    let mut receipt: Value = serde_json::from_slice(&f.receipt).unwrap();
    receipt["rows"][0]["bytes"] = json!(0);
    f.receipt(receipt);
    assert!(!f.accepts());
    let mut f = opaque_fixture(json!("item"));
    f.policy["rules"][0]["fields"]["target"]["pointer"] = json!("/missing");
    assert!(!f.accepts());
    let mut f = opaque_fixture(json!("item"));
    f.policy["rules"][0]["kind"] = json!("object-rows");
    f.record["authored"]["target"]["receipt_selector"] = json!("/rows/entry");
    let row: Value = serde_json::from_slice(&f.receipt).unwrap();
    f.receipt(json!({"rows":{"entry":row["rows"][0]}}));
    assert!(f.accepts());
}

#[test]
fn member_key_rows_bind_length_and_keep_target_identity_across_representations() {
    let original = Fixture::new();
    let old = original
        .checker()
        .unwrap()
        .check(&original.receipt)
        .unwrap();
    let f = member_key_fixture("Reports/private-invoice.pdf");
    let report = f.checker().unwrap().check(&f.receipt).unwrap();
    assert_eq!(
        report.declared_target_identity,
        old.declared_target_identity
    );
    assert_ne!(report.evidence_policy, old.evidence_policy);
    assert!(report.receipt_length_asserted && report.retained_receipt_binding_validated);
    assert!(
        !report.payload_verified
            && !report.authorizes_eviction
            && !report.current_remote_content_verified
    );
    let f = member_key_fixture("Reports/a~b.pdf");
    assert!(f.accepts());
    let percent = member_key_fixture("Reports/a%2Fb.pdf");
    let slash = member_key_fixture("Reports/a/b.pdf");
    assert_ne!(
        percent
            .checker()
            .unwrap()
            .check(&percent.receipt)
            .unwrap()
            .declared_target_identity,
        slash
            .checker()
            .unwrap()
            .check(&slash.receipt)
            .unwrap()
            .declared_target_identity
    );
}

#[test]
fn member_key_marker_is_explicit_strict_and_object_only() {
    for marker in [
        json!(null),
        json!({}),
        json!({"kind":"unknown"}),
        json!({"kind":"member-key","unexpected":true}),
        json!({"kind":"member-key","pointer":"/path"}),
    ] {
        let mut f = member_key_fixture("Reports/a.pdf");
        f.policy["rules"][0]["fields"]["target"] = marker;
        assert!(f.checker().is_err());
    }
    let mut f = member_key_fixture("Reports/a.pdf");
    f.policy["rules"][0]["kind"] = json!("array-rows");
    let error = match f.checker() {
        Ok(_) => panic!("array admitted member key"),
        Err(error) => error,
    };
    assert!(matches!(error.downcast_ref::<Error>(), Some(Error::Policy)));
    let mut f = member_key_fixture("Reports/a.pdf");
    f.policy["rules"][0]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("target");
    assert!(f.checker().is_err());
}

#[test]
fn member_key_paths_and_rows_never_infer_or_coerce_missing_values() {
    for path in [
        "../secret",
        "/absolute",
        "Reports/../secret",
        "Reports\\a.pdf",
        "C:/file",
    ] {
        assert!(!member_key_fixture(path).accepts());
    }
    for value in [
        json!(null),
        json!({}),
        json!({"sha256":null,"bytes":17}),
        json!({"sha256":"bad","bytes":17}),
    ] {
        let mut f = member_key_fixture("Reports/a.pdf");
        f.receipt(json!({"files":{"Reports/a.pdf":value}}));
        assert!(!f.accepts());
    }
    for length in [json!(null), json!("17"), json!(17.5), json!(0)] {
        let mut f = member_key_fixture("Reports/a.pdf");
        let mut object = f.record["selected"].clone();
        object["bytes"] = length;
        f.receipt(json!({"files":{"Reports/a.pdf":object}}));
        assert!(!f.accepts());
    }
    let mut f = member_key_fixture("Reports/a.pdf");
    f.record["authored"]["target"]["receipt_selector"] = json!("/files/Reports~2a.pdf");
    assert!(f.checker().is_err());
}

#[test]
fn member_key_target_does_not_fall_back_to_a_row_path_or_claim_optional_length() {
    let mut f = member_key_fixture("Reports/a.pdf");
    let mut object = f.record["selected"].clone();
    object["path"] = json!("../not-the-admitted-target");
    f.receipt(json!({"files":{"Reports/a.pdf":object}}));
    assert!(f.accepts());
    f.policy["rules"][0]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("bytes");
    assert!(
        !f.checker()
            .unwrap()
            .check(&f.receipt)
            .unwrap()
            .receipt_length_asserted
    );
    f.policy["rules"][0]["fields"]["target"] = json!("/path");
    assert!(!f.accepts());
}

#[test]
fn keyed_object_rows_support_an_independent_font_consumer_and_field_map() {
    let mut f = Fixture::new();
    let object = Object::from_bytes(b"synthetic font");
    f.record["selected"] = serde_json::to_value(&object).unwrap();
    f.record["streams"][0]["object"] = serde_json::to_value(&object).unwrap();
    f.record["streams"][0]["role"] = json!("output/font");
    f.policy["subject_role"] = json!("output/font");
    f.receipt(json!({"items":{"font-a":{"object":{"oid":object.sha256,"length":object.bytes},"target":"Fonts/font-a.woff"}}}));
    f.record["authored"]["target"]["provider"] = json!("font-registry");
    f.record["authored"]["target"]["receipt_selector"] = json!("/items/font-a");
    f.policy["rules"] = json!([{"kind":"object-rows","root":"/items","fields":{"sha256":"/object/oid","bytes":"/object/length","target":"/target"}}]);
    assert!(f.accepts());
}

#[test]
fn exact_schema_checked_instance_is_required() {
    let f = Fixture::new();
    let first = Instance::parse(&bytes(&f.record)).unwrap();
    let bound = first.bind(&f.definition, &f.schema).unwrap();
    let checked = Schema::compile(bound.definition(), &f.schema, Dialect::Draft202012)
        .unwrap()
        .check(&bound)
        .unwrap();
    let mut second = f.record.clone();
    second["status"]["changed"] = json!(true);
    let second = Instance::parse(&bytes(&second)).unwrap();
    let other = second.bind(&f.definition, &f.schema).unwrap();
    assert!(matches!(
        Checker::new(&other, &checked, &bytes(&f.policy)),
        Err(Error::Schema)
    ));
    let mut f = Fixture::new();
    f.record["authored"]["target"] = json!(false);
    assert!(f.checker().is_err());
}

#[test]
fn selection_and_roles_cannot_substitute_other_streams() {
    for case in 0..6 {
        let mut f = Fixture::new();
        match case {
            0 => f.record["selected"] = Value::Null,
            1 => {
                f.record["selected"] = serde_json::to_value(Object::from_bytes(b"changed")).unwrap()
            }
            2 => f.record["streams"][0]["role"] = json!("evidence/not-an-output"),
            3 => f.record["streams"][1]["role"] = json!("output/not-evidence"),
            4 => f.record["authored"]["target"]["receipt_slot"] = json!("absent"),
            _ => f.record["authored"]["target"]["receipt_slot"] = json!("payload"),
        }
        assert!(!f.accepts(), "accepted case {case}");
    }
}

#[test]
fn receipt_pin_rejects_changed_bytes_or_lengths() {
    let f = Fixture::new();
    let c = f.checker().unwrap();
    assert!(matches!(c.check(b"{}"), Err(Error::Pin)));
    let mut changed = f.receipt.clone();
    changed.push(b' ');
    assert!(matches!(c.check(&changed), Err(Error::Pin)));
    let mut f = Fixture::new();
    f.record["streams"][1]["object"]["bytes"] = json!(1);
    assert!(matches!(
        f.checker().unwrap().check(&f.receipt),
        Err(Error::Pin)
    ));
}

#[test]
fn row_subject_and_length_are_exact_not_coerced() {
    for value in [Value::Null, json!(-1), json!(17.0), json!("17"), json!(999)] {
        let mut f = Fixture::new();
        let mut receipt: Value = serde_json::from_slice(&f.receipt).unwrap();
        receipt["rows"][0]["bytes"] = value;
        f.receipt(receipt);
        assert!(!f.accepts());
    }
    for value in [Value::Null, json!(""), json!("A".repeat(64)), json!(false)] {
        let mut f = Fixture::new();
        let mut receipt: Value = serde_json::from_slice(&f.receipt).unwrap();
        receipt["rows"][0]["sha256"] = value;
        f.receipt(receipt);
        assert!(!f.accepts());
    }
}

#[test]
fn unsafe_or_missing_relative_targets_refuse() {
    for path in [
        "../escape.pdf",
        "/absolute.pdf",
        "C:/absolute.pdf",
        "a\\b.pdf",
        "a/../b.pdf",
        "",
        "a\nsecret.pdf",
    ] {
        let mut f = Fixture::new();
        let mut receipt: Value = serde_json::from_slice(&f.receipt).unwrap();
        receipt["rows"][0]["path"] = json!(path);
        f.receipt(receipt);
        assert!(!f.accepts(), "accepted unsafe path");
    }
    let mut f = Fixture::new();
    let mut receipt: Value = serde_json::from_slice(&f.receipt).unwrap();
    receipt["rows"][0].as_object_mut().unwrap().remove("path");
    f.receipt(receipt);
    assert!(!f.accepts());
}

#[test]
fn selectors_are_exact_immediate_members_with_strict_indices_and_escapes() {
    for selector in [
        "rows/0",
        "/rows",
        "/rows/00",
        "/rows/+0",
        "/rows/-",
        "/rows/0/path",
        "/rows/~2",
        "/absent/0",
        "",
    ] {
        let mut f = Fixture::new();
        f.record["authored"]["target"]["receipt_selector"] = json!(selector);
        assert!(!f.accepts(), "accepted {selector}");
    }
}

#[test]
fn rules_cannot_overlap_or_infer_undeclared_rows() {
    for root in ["/rows", "/rows/inner", ""] {
        let mut f = Fixture::new();
        f.policy["rules"]
            .as_array_mut()
            .unwrap()
            .push(json!({"kind":"keyed-hashes","root":root}));
        assert!(f.checker().is_err());
    }
    for rules in [
        json!([]),
        json!([{"kind":"unknown","root":"/rows"}]),
        json!([{"kind":"array-rows","root":"/rows"}]),
    ] {
        let mut f = Fixture::new();
        f.policy["rules"] = rules;
        assert!(f.checker().is_err());
    }
    let mut f = Fixture::new();
    f.policy["unexpected"] = json!(true);
    assert!(f.checker().is_err());
}

#[test]
fn missing_and_null_hash_members_never_become_matches() {
    for row in [
        json!({}),
        json!({"missing":null}),
        json!({"missing":""}),
        Value::Null,
    ] {
        let mut f = Fixture::new();
        f.receipt(json!({"hashes":row}));
        f.record["authored"]["target"]["receipt_selector"] = json!("/hashes/missing");
        f.policy["rules"] = json!([{"kind":"keyed-hashes","root":"/hashes"}]);
        assert!(!f.accepts());
    }
}

#[test]
fn duplicate_and_non_finite_metadata_refuse_even_with_matching_raw_pin() {
    for extra in [
        r#""a":1,"a":2"#,
        r#""nested":{"score":1,"\u0073core":2}"#,
        r#""score":1e400"#,
    ] {
        let mut f = Fixture::new();
        let base = String::from_utf8(f.receipt.clone()).unwrap();
        f.raw_receipt(format!("{{{extra},{}", &base[1..]).into_bytes());
        assert!(!f.accepts());
    }
    let mut f = Fixture::new();
    let base = String::from_utf8(f.receipt.clone()).unwrap();
    f.raw_receipt(
        format!(
            "{{\"precision\":0.1234567890123456789012345,\"exponent\":1e-3,\"zero\":-0.0,{}",
            &base[1..]
        )
        .into_bytes(),
    );
    assert!(f.accepts());
    assert!(git_asset_core::identity::parse_json(&f.receipt).is_err());
}

#[test]
fn declared_and_hard_budgets_refuse_before_evaluation() {
    for (field, value) in [
        ("bytes", 0),
        ("bytes", 16 * 1024 * 1024 + 1),
        ("nodes", 0),
        ("nodes", 262145),
        ("depth", 0),
        ("depth", 33),
    ] {
        let mut f = Fixture::new();
        f.policy["limits"][field] = json!(value);
        assert!(f.checker().is_err());
    }
    let mut f = Fixture::new();
    f.policy["limits"]["bytes"] = json!(1);
    assert!(f.checker().is_err());
    for field in ["nodes", "depth"] {
        let mut f = Fixture::new();
        f.policy["limits"][field] = json!(1);
        assert!(!f.accepts());
    }
}

#[test]
fn parsed_evidence_is_reused_but_each_check_enforces_its_own_limits_and_pin() {
    let f = Fixture::new();
    let checker = f.checker().unwrap();
    let parsed = checker.parse_receipt(&f.receipt).unwrap();
    let first = checker.check_parsed(&parsed).unwrap();
    let mut strict = Fixture::new();
    strict.policy["limits"]["nodes"] = json!(1);
    assert!(matches!(
        strict.checker().unwrap().check_parsed(&parsed),
        Err(Error::Budget)
    ));
    let mut other = Fixture::new();
    let mut raw = other.receipt.clone();
    raw.push(b' ');
    other.raw_receipt(raw);
    assert!(matches!(
        other.checker().unwrap().check_parsed(&parsed),
        Err(Error::Pin)
    ));
    let mut same_target = Fixture::new();
    let mut raw = same_target.receipt.clone();
    raw.push(b' ');
    same_target.raw_receipt(raw);
    let next = same_target
        .checker()
        .unwrap()
        .check(&same_target.receipt)
        .unwrap();
    assert_eq!(
        first.declared_target_identity,
        next.declared_target_identity
    );
    assert_ne!(first.receipt, next.receipt);
}

#[test]
fn thousand_row_external_receipt_exceeds_native_json_budget_without_expanding_git_record() {
    let mut f = Fixture::new();
    let object = f.record["selected"].clone();
    let rows: Vec<_> = (0..1000).map(|i| json!({"sha256":object["sha256"],"bytes":object["bytes"],"path":format!("Reports/{i}.pdf")})).collect();
    f.receipt(json!({"rows":rows}));
    f.record["authored"]["target"]["receipt_selector"] = json!("/rows/999");
    assert!(f.receipt.len() > 65536 && bytes(&f.record).len() < 2048);
    assert!(f.accepts());
}

#[test]
fn explicit_cli_reads_only_metadata_and_reports_no_raw_private_values() {
    for f in [
        Fixture::new(),
        member_key_fixture("Reports/private-invoice.pdf"),
        opaque_fixture(json!(r"G:\private-invoice.pdf")),
        singleton_fixture(),
        retained_subject_fixture(),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let values = [
            bytes(&f.record),
            f.definition.clone(),
            f.schema.clone(),
            bytes(&f.policy),
            f.receipt.clone(),
        ];
        for (i, raw) in values.iter().enumerate() {
            fs::write(dir.path().join(format!("{i}.json")), raw).unwrap();
        }
        let paths: Vec<_> = (0..5)
            .map(|i| dir.path().join(format!("{i}.json")))
            .collect();
        let run = |extra: Option<&str>| {
            let mut command = Command::new(env!("CARGO_BIN_EXE_git-asset-publication"));
            command
                .arg(&paths[0])
                .arg(&paths[1])
                .arg(&paths[2])
                .arg("2020-12")
                .arg(&paths[3])
                .arg(&paths[4]);
            if let Some(extra) = extra {
                command.arg(extra);
            }
            command.output().unwrap()
        };
        let output = run(None);
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let report: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["retained_receipt_binding_validated"], true);
        assert_eq!(report["payload_verified"], false);
        let raw = String::from_utf8(output.stdout).unwrap();
        for marker in ["private-invoice", "private-scope", "private-example-id"] {
            assert!(!raw.contains(marker));
        }
        for extra in [None, Some("private-secret-unused-argument")] {
            if extra.is_none() {
                fs::write(&paths[4], b"private-secret-corrupt-evidence").unwrap();
            }
            let output = run(extra);
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8(output.stderr).unwrap().trim(),
                "publication evidence check refused; verify explicit inputs and policy"
            );
        }
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 5);
    }
}
