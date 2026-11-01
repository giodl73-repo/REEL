#![cfg(feature = "tig")]
use reel::tig_versioned_registration::{prepare, prepare_update, version_slot};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_assets::native::Instance;
use tig_core::Object;

fn definitions() -> (Vec<u8>, Vec<u8>) {
    let schema = serde_json::to_vec(&json!({"type":"object"})).unwrap();
    let definition = serde_json::to_vec(
        &json!({"schema":"tig.definition.design-1","id":"types/source-package",
        "authored_schema":Object::from_bytes(&schema),
        "authoring":{"properties":["artifact_id","kind","current_version","public_release"],
        "required":["artifact_id","kind","current_version"],"defaults":{}},
        "stream_rules":{},"semantic_rules":{}}),
    )
    .unwrap();
    (definition, schema)
}
fn source(kind: &str) -> Value {
    let versions = ["v1", "v2"].map(|name| {
        let object = Object::from_bytes(name.as_bytes());
        json!({"version_id":name,"manifest":{"sha256":object.sha256,"bytes":object.bytes,
            "cache_uri":format!("cache://sha256/{}",object.sha256.as_str())},
            "version_lock":{"logical_id":"project/item","version_id":name,"manifest_sha256":object.sha256}})
    });
    json!({"schema":"caminos.artifact-record.v1","artifact_id":"project/item","kind":kind,
        "current_version":"v2","versions":versions,"public_release":false,
        "extra_legacy_metadata":{"rights":"preserve this exact source","approval":"original subject only"}})
}
fn candidate(value: &Value) -> anyhow::Result<Vec<u8>> {
    let (definition, schema) = definitions();
    let bytes = serde_json::to_vec(value).unwrap();
    prepare(&Object::from_bytes(&bytes), &bytes, &definition, &schema)
}

#[test]
fn source_updates_preserve_conflicting_history_exact_previous_bytes_and_do_not_transfer_status() {
    let (definition, schema) = definitions();
    let mut old_value: Value =
        serde_json::from_slice(&candidate(&source("episode")).unwrap()).unwrap();
    old_value["status"]["approval"] = json!("only the exact previous subject");
    let old = serde_json::to_vec_pretty(&old_value).unwrap();
    let previous = Object::from_bytes(&old);
    let mut changed = source("episode");
    changed["versions"].as_array_mut().unwrap().remove(0);
    let next_package = Object::from_bytes(b"replacement under the same display version");
    changed["versions"][0]["manifest"] = json!({"sha256":next_package.sha256,"bytes":next_package.bytes,
        "cache_uri":format!("cache://sha256/{}",next_package.sha256.as_str())});
    changed["versions"][0]["version_lock"]["manifest_sha256"] = json!(next_package.sha256);
    let raw = serde_json::to_vec(&changed).unwrap();
    let result = prepare_update(
        &Object::from_bytes(&raw),
        &raw,
        &previous,
        &old,
        &definition,
        &schema,
    )
    .unwrap();
    let record = Instance::parse(&result).unwrap();
    assert_eq!(
        record.value().selected.as_ref().unwrap().object(),
        next_package
    );
    assert_eq!(record.value().status["production_state"], "unclassified");
    assert!(record.value().status.get("approval").is_none());
    assert_eq!(
        record
            .value()
            .streams
            .iter()
            .find(|s| s.slot == "previous-record")
            .unwrap()
            .object
            .object(),
        previous
    );
    for (index, stream) in old_value["streams"].as_array().unwrap().iter().enumerate() {
        let retained_slot = if stream["slot"] == "version/v1" {
            "version/v1".into()
        } else {
            format!("retained/{}/{index}", previous.sha256.as_str())
        };
        let retained = record
            .value()
            .streams
            .iter()
            .find(|s| s.slot == retained_slot)
            .unwrap();
        assert_eq!(
            retained.object.object(),
            serde_json::from_value::<Object>(stream["object"].clone()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&retained.provenance).unwrap(),
            stream.get("provenance").cloned().unwrap_or(Value::Null)
        );
    }
    // A retry against the same source is byte-identical, without another history link.
    let again = prepare_update(
        &Object::from_bytes(&raw),
        &raw,
        &Object::from_bytes(&result),
        &result,
        &definition,
        &schema,
    )
    .unwrap();
    assert_eq!(again, result);
    changed["extra_legacy_metadata"]["next_observation"] = json!(true);
    let later_raw = serde_json::to_vec(&changed).unwrap();
    let later = prepare_update(
        &Object::from_bytes(&later_raw),
        &later_raw,
        &Object::from_bytes(&result),
        &result,
        &definition,
        &schema,
    )
    .unwrap();
    let later = Instance::parse(&later).unwrap();
    assert_eq!(
        later
            .value()
            .streams
            .iter()
            .find(|s| s.slot == "previous-record")
            .unwrap()
            .object
            .object(),
        Object::from_bytes(&result)
    );
    for stream in &record.value().streams {
        assert!(
            later
                .value()
                .streams
                .iter()
                .any(|retained| retained.object == stream.object
                    && retained.role == stream.role
                    && retained.group == stream.group
                    && retained.provenance == stream.provenance)
        );
    }
}

#[test]
fn source_update_admission_rejects_wrong_previous_identity_pin_definition_and_inconsistent_retry() {
    let (definition, schema) = definitions();
    let value = source("episode");
    let raw = serde_json::to_vec(&value).unwrap();
    let pin = Object::from_bytes(&raw);
    let good = candidate(&value).unwrap();
    for case in 0..5 {
        let mut previous: Value = serde_json::from_slice(&good).unwrap();
        match case {
            0 => previous["id"] = json!("other/item"),
            1 => previous["definition"]["id"] = json!("types/other"),
            2 => previous["selected"] = json!(Object::from_bytes(b"foreign selected bytes")),
            3 => previous["authored"]["kind"] = json!("changed without changing source evidence"),
            _ => {
                previous["metadata"]["versioned_registration"]["source_schema"] =
                    json!("other-format")
            }
        }
        let previous = serde_json::to_vec(&previous).unwrap();
        assert!(
            prepare_update(
                &pin,
                &raw,
                &Object::from_bytes(&previous),
                &previous,
                &definition,
                &schema
            )
            .is_err()
        );
    }
    assert!(
        prepare_update(
            &pin,
            &raw,
            &Object::from_bytes(b"foreign"),
            &good,
            &definition,
            &schema
        )
        .is_err()
    );
    assert!(
        prepare_update(
            &pin,
            &raw,
            &Object::from_bytes(&vec![b' '; 65537]),
            &vec![b' '; 65537],
            &definition,
            &schema
        )
        .is_err()
    );
}

#[test]
fn long_display_labels_keep_full_identity_without_exceeding_native_slot_limits() {
    let mut value = source("episode-production-evidence");
    let labels = [
        format!("{}a", "x".repeat(144)),
        format!("{}b", "x".repeat(144)),
    ];
    for (index, label) in labels.iter().enumerate() {
        value["versions"][index]["version_id"] = json!(label);
        value["versions"][index]["version_lock"]["version_id"] =
            json!(format!("immutable-lock-{index}"));
    }
    value["current_version"] = json!(labels[1]);
    let raw = serde_json::to_vec(&value).unwrap();
    let bytes = candidate(&value).unwrap();
    let parsed = Instance::parse(&bytes).unwrap();
    let record = parsed.value();
    assert_eq!(record.authored["current_version"], labels[1]);
    assert_eq!(record.streams[0].object.object(), Object::from_bytes(&raw));
    assert_eq!(
        record.selected.as_ref().unwrap().object(),
        Object::from_bytes(b"v2")
    );
    for (index, label) in labels.iter().enumerate() {
        let stream = &record.streams[index + 1];
        let expected = format!(
            "version-sha256/{}",
            Object::from_bytes(label.as_bytes()).sha256.as_str()
        );
        assert_eq!(stream.slot, expected);
        assert!(stream.slot.len() <= 128);
        assert_eq!(
            stream.provenance.as_ref().unwrap()["source_version_id"],
            *label
        );
        assert_eq!(
            stream.provenance.as_ref().unwrap()["version_lock_id"],
            format!("immutable-lock-{index}")
        );
    }
    assert_ne!(record.streams[1].slot, record.streams[2].slot);
    // Even a literal label resembling an encoded slot has a disjoint address.
    let literal = record.streams[1].slot.clone();
    assert_eq!(
        version_slot(&literal).unwrap(),
        format!("version/{literal}")
    );
    assert_ne!(version_slot(&literal).unwrap(), record.streams[1].slot);
}

#[test]
fn slot_encoding_uses_utf8_byte_boundaries_and_never_relaxes_source_label_limits() {
    for label in ["x".repeat(120), "é".repeat(60)] {
        assert_eq!(version_slot(&label).unwrap().len(), 128);
        assert_eq!(version_slot(&label).unwrap(), format!("version/{label}"));
    }
    for label in [
        "x".repeat(121),
        format!("{}x", "é".repeat(60)),
        "é".repeat(61),
        "x".repeat(256),
    ] {
        assert!(version_slot(&label).unwrap().starts_with("version-sha256/"));
        let mut value = source("episode");
        value["versions"][0]["version_id"] = json!(label);
        assert!(candidate(&value).is_ok());
    }
    for label in [
        String::new(),
        "x".repeat(257),
        "é".repeat(129),
        "bad\nlabel".into(),
    ] {
        assert!(version_slot(&label).is_err());
    }
    let ordinary = candidate(&source("episode")).unwrap();
    let record = Instance::parse(&ordinary).unwrap();
    assert_eq!(record.value().streams[1].slot, "version/v1");
    assert!(
        record.value().streams[1]
            .provenance
            .as_ref()
            .unwrap()
            .get("source_version_id")
            .is_none()
    );
}

#[test]
fn fractional_evidence_is_raw_pinned_without_entering_native_identity() {
    let (definition, schema) = definitions();
    let base = serde_json::to_string(&source("review")).unwrap();
    for metadata in [
        r#"{"score":91.5,"weight":1e-3,"zero":-0.0}"#,
        r#"{"score":0.1234567890123456789012345}"#,
    ] {
        let raw = format!("{{\"evidence\":{metadata},{}", &base[1..]);
        let pin = Object::from_bytes(raw.as_bytes());
        assert!(tig_core::identity::parse_json(raw.as_bytes()).is_err());
        let output = prepare(&pin, raw.as_bytes(), &definition, &schema).unwrap();
        let record = Instance::parse(&output).unwrap();
        assert_eq!(record.value().streams[0].object.object(), pin);
        assert!(record.value().metadata.get("evidence").is_none());
        tig_core::identity::parse_json(&output).unwrap();
    }
}

#[test]
fn external_metadata_cannot_hide_ambiguous_keys_invalid_pins_or_excess_structure() {
    let (definition, schema) = definitions();
    let base = serde_json::to_string(&source("review")).unwrap();
    for metadata in [
        r#"{"score":1.5,"score":2.5}"#.to_string(),
        r#"{"nested":[{"score":1,"score":2}]}"#.to_string(),
        r#"{"score":1e400}"#.to_string(),
        format!("{}0{}", "[".repeat(34), "]".repeat(34)),
        format!("[{}]", vec!["0"; 4100].join(",")),
    ] {
        let raw = format!("{{\"evidence\":{metadata},{}", &base[1..]);
        assert!(
            prepare(
                &Object::from_bytes(raw.as_bytes()),
                raw.as_bytes(),
                &definition,
                &schema
            )
            .is_err()
        );
    }
    for pin in [json!(2.0), json!(-1), json!(9_007_199_254_740_992_u64)] {
        let mut value = source("review");
        value["versions"][0]["manifest"]["bytes"] = pin;
        assert!(candidate(&value).is_err());
    }
}
#[test]
fn multiple_families_preserve_identity_current_prior_and_raw_source_without_approval() {
    for kind in [
        "episode",
        "song",
        "songbook",
        "arrangement",
        "poem",
        "manuscript",
        "music-video",
        "plan",
    ] {
        let value = source(kind);
        let original = serde_json::to_vec(&value).unwrap();
        let record = Instance::parse(&candidate(&value).unwrap()).unwrap();
        let record = record.value();
        assert_eq!(record.id, "project/item");
        assert_eq!(record.authored["kind"], kind);
        assert_eq!(record.authored["current_version"], "v2");
        assert_eq!(
            record.selected.as_ref().unwrap().object(),
            Object::from_bytes(b"v2")
        );
        assert_eq!(record.streams[0].slot, "source-record");
        assert_eq!(
            record.streams[0].object.object(),
            Object::from_bytes(&original)
        );
        assert_eq!(record.streams[1].slot, "version/v1");
        assert_eq!(record.streams[1].object.object(), Object::from_bytes(b"v1"));
        assert_eq!(record.streams[2].slot, "version/v2");
        assert_eq!(record.status["production_state"], "unclassified");
        assert!(record.status.get("approval").is_none());
        assert!(record.metadata.get("extra_legacy_metadata").is_none());
    }
}
#[test]
fn malformed_versions_cannot_silently_drop_history_or_select_a_foreign_package() {
    for case in [
        "duplicate",
        "current",
        "foreign",
        "lock-version",
        "lock-hash",
        "uri",
        "size",
        "empty",
        "limit",
        "release",
        "slot",
    ] {
        let mut value = source("episode");
        match case {
            "duplicate" => {
                let first = value["versions"][0].clone();
                value["versions"].as_array_mut().unwrap().push(first);
            }
            "current" => value["current_version"] = json!("absent"),
            "foreign" => value["versions"][0]["version_lock"]["logical_id"] = json!("other"),
            "lock-version" => value["versions"][0]["version_lock"]["version_id"] = json!(""),
            "lock-hash" => {
                value["versions"][0]["version_lock"]["manifest_sha256"] =
                    value["versions"][1]["manifest"]["sha256"].clone()
            }
            "uri" => value["versions"][0]["manifest"]["cache_uri"] = json!("elsewhere"),
            "size" => {
                let mut next = value["versions"][0].clone();
                next["version_id"] = json!("v3");
                next["version_lock"]["version_id"] = json!("v3");
                next["manifest"]["bytes"] = json!(99);
                value["versions"].as_array_mut().unwrap().push(next);
            }
            "empty" => value["versions"] = json!([]),
            "limit" => value["versions"] = json!(vec![value["versions"][0].clone(); 256]),
            "release" => {
                value["public_release"] = Value::Null;
            }
            _ => {
                let long = "x".repeat(257);
                value["versions"][0]["version_id"] = json!(long);
                value["versions"][0]["version_lock"]["version_id"] = json!(long);
            }
        }
        assert!(candidate(&value).is_err(), "accepted {case}");
    }
    let (definition, schema) = definitions();
    let bytes = serde_json::to_vec(&source("episode")).unwrap();
    assert!(prepare(&Object::from_bytes(b"other"), &bytes, &definition, &schema).is_err());
    assert!(prepare(&Object::from_bytes(&bytes), &bytes, &definition, b"{}").is_err());
    let duplicate = br#"{"schema":"a","schema":"b"}"#;
    assert!(
        prepare(
            &Object::from_bytes(duplicate),
            duplicate,
            &definition,
            &schema
        )
        .is_err()
    );
    let oversized = vec![b' '; 65537];
    assert!(
        prepare(
            &Object::from_bytes(&oversized),
            &oversized,
            &definition,
            &schema
        )
        .is_err()
    );
}

#[test]
fn display_versions_and_absent_release_declarations_remain_distinct_from_lock_identity() {
    let mut value = source("arrangement");
    value.as_object_mut().unwrap().remove("public_release");
    value["versions"][1]["version_lock"]["version_id"] = json!("v2-inputs-exact");
    let bytes = candidate(&value).unwrap();
    let record = Instance::parse(&bytes).unwrap();
    let instance = record.value();
    assert_eq!(instance.authored["current_version"], "v2");
    assert!(!instance.authored.contains_key("public_release"));
    assert_eq!(instance.streams[2].slot, "version/v2");
    assert_eq!(
        instance.streams[2].provenance.as_ref().unwrap()["version_lock_id"],
        value["versions"][1]["version_lock"]["version_id"]
    );
    assert_eq!(
        instance.selected.as_ref().unwrap().object(),
        Object::from_bytes(b"v2")
    );
    assert_eq!(instance.status["production_state"], "unclassified");
    assert_eq!(
        instance.streams[0].object.object(),
        Object::from_bytes(&serde_json::to_vec(&value).unwrap())
    );
}

fn pinned_file(root: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    let object = Object::from_bytes(bytes);
    json!({"path":path,"bytes":object.bytes,"sha256":object.sha256})
}
#[test]
fn explicit_cli_uses_only_named_source_metadata_and_never_overwrites_candidates() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (definition, schema) = definitions();
    let source = serde_json::to_vec(&source("song")).unwrap();
    let object = Object::from_bytes(&source);
    let cache = root.join("cache");
    fs::create_dir_all(cache.join("objects")).unwrap();
    let source_path = cache.join("objects").join(object.sha256.as_str());
    fs::write(&source_path, &source).unwrap();
    let profile = json!({"schema":"reel.tig-versioned-registration.experimental-1","source_record":object,
        "byte_source":{"places":[{"root":cache,"layout":"flat"}],"required":"cooperative-immutable"},
        "definition":{"definition":pinned_file(root,"definition.json",&definition),"authored_schema":pinned_file(root,"schema.json",&schema)}});
    let input = root.join("profile.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let target = root.join("candidate.tig");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .arg("--versioned")
            .arg(&input)
            .arg(&target)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["versions"], 2);
    assert_eq!(summary["package_media_opened"], 0);
    assert_eq!(summary["adopted"], false);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("sha256"));
    let first = fs::read(&target).unwrap();
    assert!(!invoke().status.success());
    assert_eq!(fs::read(&target).unwrap(), first);
    fs::remove_file(&target).unwrap();
    fs::write(&source_path, b"corrupt").unwrap();
    let refused = invoke();
    assert!(!refused.status.success());
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(&root.display().to_string()));
    assert_eq!(
        fs::read(&input).unwrap(),
        serde_json::to_vec(&profile).unwrap()
    );
    assert_eq!(fs::read_dir(root).unwrap().count(), 4);
}

#[test]
fn update_cli_verifies_cached_previous_native_bytes_without_reading_packages() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let (definition, schema) = definitions();
    let original = source("episode");
    let old = candidate(&original).unwrap();
    let previous = Object::from_bytes(&old);
    let mut changed = original;
    changed["extra_legacy_metadata"]["changed"] = json!(true);
    let raw = serde_json::to_vec(&changed).unwrap();
    let source_pin = Object::from_bytes(&raw);
    let cache = root.join("cache");
    fs::create_dir_all(cache.join("objects")).unwrap();
    fs::write(cache.join("objects").join(source_pin.sha256.as_str()), &raw).unwrap();
    let previous_path = cache.join("objects").join(previous.sha256.as_str());
    fs::write(&previous_path, &old).unwrap();
    let profile = json!({"schema":"reel.tig-versioned-registration.experimental-1",
        "source_record":source_pin,"previous_instance":previous,
        "byte_source":{"places":[{"root":cache,"layout":"flat"}],"required":"cooperative-immutable"},
        "definition":{"definition":pinned_file(root,"definition.json",&definition),"authored_schema":pinned_file(root,"schema.json",&schema)}});
    let input = root.join("profile.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let target = root.join("update.tig");
    let invoke = || {
        Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .args(["--versioned"])
            .arg(&input)
            .arg(&target)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let result = invoke();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["previous_instances_verified"], 1);
    assert_eq!(report["package_media_opened"], 0);
    assert_eq!(report["versions"], 2);
    let produced = fs::read(&target).unwrap();
    assert_eq!(
        produced,
        prepare_update(&source_pin, &raw, &previous, &old, &definition, &schema).unwrap()
    );
    assert!(!invoke().status.success());
    assert_eq!(fs::read(&target).unwrap(), produced);
    fs::remove_file(&target).unwrap();
    fs::write(&previous_path, b"corrupt previous native record").unwrap();
    let refused = invoke();
    assert!(!refused.status.success());
    assert!(!target.exists());
    assert!(!String::from_utf8_lossy(&refused.stderr).contains(&root.display().to_string()));
}
