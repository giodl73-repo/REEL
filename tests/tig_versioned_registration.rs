#![cfg(feature = "tig")]
use reel::tig_versioned_registration::prepare;
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
        "required":["artifact_id","kind","current_version","public_release"],"defaults":{}},
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
            "lock-version" => value["versions"][0]["version_lock"]["version_id"] = json!("other"),
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
                value.as_object_mut().unwrap().remove("public_release");
            }
            _ => {
                let long = "x".repeat(129);
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
