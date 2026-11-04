#![cfg(feature = "tig-history")]
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::Path};
use tig_core::Object;

fn local(root: &Path, name: &str, value: &Value) -> Value {
    let raw = serde_json::to_vec(value).unwrap();
    fs::write(root.join(name), &raw).unwrap();
    let pin = Object::from_bytes(&raw);
    json!({"path":root.join(name),"sha256":pin.sha256,"bytes":pin.bytes})
}
fn table(root: &Path, rows: &Value) -> Object {
    let raw = serde_json::to_vec(
        &json!({"copies":rows,"uninterpreted_source_metadata":"x".repeat(70_000)}),
    )
    .unwrap();
    let pin = Object::from_bytes(&raw);
    fs::write(root.join("cache/objects").join(pin.sha256.as_str()), raw).unwrap();
    pin
}
fn run(root: &Path, profile: &Value, name: &str) -> anyhow::Result<Value> {
    fs::write(
        root.join("profile.json"),
        serde_json::to_vec(profile).unwrap(),
    )
    .unwrap();
    reel::tig_delivery_table_registration::run(&root.join("profile.json"), &root.join(name))
}
fn records(root: &Path, name: &str) -> BTreeMap<String, Value> {
    fs::read_dir(root.join(name))
        .unwrap()
        .filter_map(|entry| {
            let path = entry.unwrap().path();
            (path.extension().is_some_and(|s| s == "tig")).then(|| {
                let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                (value["id"].as_str().unwrap().to_owned(), value)
            })
        })
        .collect()
}
#[test]
fn targets_stay_independent_and_stable_across_row_order_and_byte_revisions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::create_dir_all(root.join("cache/objects")).unwrap();
    let authored = local(
        root,
        "authored.json",
        &json!({"$schema":"https://json-schema.org/draft/2020-12/schema","type":"object","required":["target"],"properties":{"target":{"type":"object"}},"additionalProperties":false}),
    );
    let definition = local(
        root,
        "definition.json",
        &json!({"schema":"tig.definition.design-1","id":"types/file","authored_schema":{"sha256":authored["sha256"],"bytes":authored["bytes"]},"authoring":{"properties":["target"],"required":["target"],"defaults":{}},"stream_rules":{"profile":"test.file.v1"},"semantic_rules":{"profile":"test.file.v1"}}),
    );
    let policy = local(
        root,
        "policy.json",
        &json!({"schema":"tig.publication-evidence-policy.experimental-1","target_pointer":"/target","subject_selection":"declared-stream","subject_slot":"retained-readback","subject_role":"delivery/file","receipt_role":"evidence/delivery-receipt","limits":{"bytes":1048576,"nodes":8192,"depth":32},"rules":[{"kind":"array-rows","root":"/copies","fields":{"sha256":"/asset/sha256","bytes":"/asset/bytes","target":"/relative_path"}}]}),
    );
    // Payloads are deliberately absent: registration may read only the table.
    let a = Object::from_bytes(b"primary recording A");
    let b = Object::from_bytes(b"primary recording B");
    let row_a = json!({"relative_path":"Spanish/a.wav","asset":a});
    let row_b = json!({"relative_path":"English/b.wav","asset":a});
    let receipt = table(root, &json!([row_a, row_b]));
    let mut profile = json!({"schema":"reel.tig-delivery-table-registration.experimental-1","byte_source":{"places":[{"root":root.join("cache"),"layout":"flat"}],"required":"cooperative-immutable"},"receipt":receipt,"rows_pointer":"/copies","asset_pointer":"/asset","path_pointer":"/relative_path","provider":"files","scope":"retained-edition","id_prefix":"delivery/files/edition","definition":{"definition":definition,"authored_schema":authored},"receipt_policy":policy,"authored":{},"provenance":{"source":Object::from_bytes(b"retained original source")},"max_rows":10,"max_output_bytes":65536});
    let result = run(root, &profile, "first").unwrap();
    assert_eq!(result["records"], 2);
    assert_eq!(result["primary_media_opened"], 0);
    assert_eq!(result["adopted"], false);
    let original = records(root, "first");
    assert_eq!(original.len(), 2); // Equal bytes do not collapse two targets.
    assert!(run(root, &profile, "first").is_err());
    profile["receipt"] = serde_json::to_value(table(root, &json!([row_b, row_a]))).unwrap();
    run(root, &profile, "reordered").unwrap();
    let reordered = records(root, "reordered");
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        reordered.keys().collect::<Vec<_>>()
    );
    for (id, record) in &original {
        assert_eq!(record["selected"], reordered[id]["selected"]);
    }
    let row_a_v2 = json!({"relative_path":"Spanish/a.wav","asset":b});
    profile["receipt"] = serde_json::to_value(table(root, &json!([row_a_v2, row_b]))).unwrap();
    run(root, &profile, "revised").unwrap();
    let revised = records(root, "revised");
    assert_eq!(
        original.keys().collect::<Vec<_>>(),
        revised.keys().collect::<Vec<_>>()
    );
    assert_eq!(
        original
            .iter()
            .filter(|(id, r)| r["selected"] != revised[*id]["selected"])
            .count(),
        1
    );
    assert_eq!(records(root, "first"), original); // Earlier candidates untouched.
    profile["receipt"] = serde_json::to_value(table(root, &json!([row_a, row_a]))).unwrap();
    assert!(run(root, &profile, "duplicates").is_err());
    assert!(!root.join("duplicates").exists());
    profile["receipt"] = serde_json::to_value(table(
        root,
        &json!([{"relative_path":"../escape.wav","asset":a}]),
    ))
    .unwrap();
    assert!(run(root, &profile, "escape").is_err());
    assert!(!root.join("escape").exists());
    let wrong_uri = json!({"relative_path":"safe.wav","asset":{"sha256":a.sha256,"bytes":a.bytes,"cache_uri":"cache://sha256/wrong"}});
    profile["receipt"] = serde_json::to_value(table(root, &json!([wrong_uri]))).unwrap();
    assert!(run(root, &profile, "wrong-uri").is_err());
    assert!(!root.join("wrong-uri").exists());
    let wrong_mapping = json!({"relative_path":"a.wav","other_path":"b.wav","asset":a});
    profile["receipt"] = serde_json::to_value(table(root, &json!([wrong_mapping]))).unwrap();
    profile["path_pointer"] = json!("/other_path");
    assert!(run(root, &profile, "wrong-target").is_err());
    assert!(!root.join("wrong-target").exists());
    profile["path_pointer"] = json!("/relative_path");
    let conflicting =
        json!({"relative_path":"different.wav","asset":{"sha256":a.sha256,"bytes":a.bytes+1}});
    profile["receipt"] = serde_json::to_value(table(root, &json!([row_a, conflicting]))).unwrap();
    assert!(run(root, &profile, "conflicting-lengths").is_err());
    assert!(!root.join("conflicting-lengths").exists());

    // Historical receipts use named variant rows and literal Windows locators.
    profile["row_layout"] = json!("object");
    profile["target_kind"] = json!("opaque-locator");
    let opaque_rows = json!({"with/voice~1":{"relative_path":"G:\\Shared\\with.mp3","asset":a},"without":{"relative_path":"G:\\Shared\\without.mp3","asset":a}});
    profile["receipt"] = serde_json::to_value(table(root, &opaque_rows)).unwrap();
    profile["receipt_policy"] = local(
        root,
        "opaque-policy.json",
        &json!({"schema":"tig.publication-evidence-policy.experimental-1","target_pointer":"/target","subject_selection":"declared-stream","subject_slot":"retained-readback","subject_role":"delivery/file","receipt_role":"evidence/delivery-receipt","limits":{"bytes":1048576,"nodes":8192,"depth":32},"rules":[{"kind":"object-rows","root":"/copies","fields":{"sha256":"/asset/sha256","bytes":"/asset/bytes","target":{"kind":"opaque-locator","pointer":"/relative_path"}}}]}),
    );
    assert_eq!(run(root, &profile, "opaque-objects").unwrap()["records"], 2);
    let opaque = records(root, "opaque-objects");
    assert!(
        opaque
            .values()
            .any(|r| r["authored"]["target"]["receipt_selector"] == "/copies/with~1voice~01")
    );
    profile["target_kind"] = json!("logical-path");
    assert!(run(root, &profile, "opaque-as-path").is_err());
    assert!(!root.join("opaque-as-path").exists());

    // Keyed tables preserve paths containing pointer metacharacters verbatim.
    profile["target_from_member_key"] = json!(true);
    profile["asset_pointer"] = json!("");
    profile["path_pointer"] = json!("");
    profile["receipt"] =
        serde_json::to_value(table(root, &json!({"Spanish/a~b.wav":a,"English/b.wav":a}))).unwrap();
    profile["receipt_policy"] = local(
        root,
        "keyed-policy.json",
        &json!({"schema":"tig.publication-evidence-policy.experimental-1","target_pointer":"/target","subject_selection":"declared-stream","subject_slot":"retained-readback","subject_role":"delivery/file","receipt_role":"evidence/delivery-receipt","limits":{"bytes":1048576,"nodes":8192,"depth":32},"rules":[{"kind":"object-rows","root":"/copies","fields":{"sha256":"/sha256","bytes":"/bytes","target":{"kind":"member-key"}}}]}),
    );
    assert_eq!(run(root, &profile, "keyed-objects").unwrap()["records"], 2);
    assert!(
        records(root, "keyed-objects")
            .values()
            .any(|r| r["authored"]["target"]["receipt_selector"] == "/copies/Spanish~1a~0b.wav")
    );
    profile["row_layout"] = json!("array");
    assert!(run(root, &profile, "member-key-on-array").is_err());
    assert!(!root.join("member-key-on-array").exists());
}
