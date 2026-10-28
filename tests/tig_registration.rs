#![cfg(feature = "tig")]
use reel::tig_native_registration::{self, Profile};
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_assets::native::Instance;
use tig_core::Object;
use tig_store::existing::ReadCache;
#[path = "support/tig_registration_variants.rs"]
mod variants;

struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
}
fn file(path: &Path, bytes: &[u8]) -> Value {
    fs::write(path, bytes).unwrap();
    let pin = Object::from_bytes(bytes);
    json!({"path":path,"sha256":pin.sha256,"bytes":pin.bytes})
}
fn put(root: &Path, bytes: &[u8]) -> Object {
    let object = Object::from_bytes(bytes);
    fs::write(root.join("objects").join(object.sha256.as_str()), bytes).unwrap();
    object
}
fn definition(root: &Path, name: &str, fields: &[&str]) -> Value {
    let schema = b"true";
    let schema_file = file(&root.join(format!("{name}.schema.json")), schema);
    let bytes = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":name,
        "authored_schema":Object::from_bytes(schema),"authoring":{"properties":fields,"required":fields,"defaults":{}},
        "stream_rules":{},"semantic_rules":{}})).unwrap();
    json!({"definition":file(&root.join(format!("{name}.json")), &bytes),"authored_schema":schema_file})
}
impl Fixture {
    fn new(bindings: Value) -> Self {
        Self::raw(&serde_json::to_vec(&bindings).unwrap())
    }
    fn raw(bindings: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("objects")).unwrap();
        let scene = serde_json::to_vec(&json!({"schema":"reel.scene-authoring.v2","scene_id":"scene:01","episode_id":"episode:01",
            "authoring_state":"draft","source_authority_id":"original-spanish-text","languages":{},"holds":["pictures-unapproved"]})).unwrap();
        let scene = put(root.path(), &scene);
        let bindings = put(root.path(), bindings);
        let descriptor = |o: &Object| json!({"sha256":o.sha256,"bytes":o.bytes,"cache_uri":format!("cache://sha256/{}",o.sha256.as_str())});
        let package = put(root.path(), &serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1","artifact_id":"source/episode",
            "version_id":"draft-v2","assets":{"source/scene":descriptor(&scene),"source/bindings":descriptor(&bindings)}})).unwrap());
        let scene_def = definition(
            root.path(),
            "scene-type",
            &[
                "scene_id",
                "episode_id",
                "source_package",
                "binding_ids",
                "public_release",
            ],
        );
        let primary_def = definition(
            root.path(),
            "primary-type",
            &[
                "scene_id",
                "binding_key",
                "source_package",
                "public_release",
            ],
        );
        let profile = json!({"schema":tig_native_registration::SCHEMA,"package":package,"scene_asset":"source/scene","bindings_asset":"source/bindings",
            "byte_source":{"places":[{"root":root.path(),"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}},
            "scene_definition":scene_def,"primary_definition":primary_def,"controls":[],"metadata":{"private_origin":{"commit":"fixture-source"}},"max_output_bytes":1048576});
        Self { root, profile }
    }
    fn standard() -> Self {
        Self::new(
            json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene:01","assets":{
            "cel-a":{"logical_id":"cel:A","sha256":"a".repeat(64),"bytes":999999999,"cache_uri":format!("cache://sha256/{}","a".repeat(64)),"selection_state":"private-unselected-candidate"},
            "shared-cue":{"logical_id":"cue:shared","sha256":"b".repeat(64),"bytes":12345,"cache_uri":format!("cache://sha256/{}","b".repeat(64)),"selection_state":"selected-private-production"}}}),
        )
    }
    fn parsed(&self) -> Profile {
        serde_json::from_value(self.profile.clone()).unwrap()
    }
    fn run(&self, output: &Path) -> std::process::Output {
        let path = self.root.path().join("profile.json");
        fs::write(&path, serde_json::to_vec(&self.profile).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .arg(path)
            .arg(output)
            .output()
            .unwrap()
    }
}
#[test]
fn registers_one_file_per_input_preserving_drafts_and_pins_without_opening_media() {
    let f = Fixture::standard();
    let profile = f.parsed();
    let mut cache = ReadCache::new(profile.byte_source.clone()).unwrap();
    let prepared = tig_native_registration::prepare(&profile, &mut cache).unwrap();
    assert_eq!(cache.requests(), 3);
    assert_eq!(cache.active_readers(), 0);
    let output = f.root.path().join("candidate");
    let report = prepared.write(&output).unwrap();
    assert_eq!(report["records"], 3);
    assert_eq!(report["primary_media_opened"], 0);
    assert_eq!(report["adopted"], false);
    let parent = Instance::parse(&fs::read(output.join("scene.tig")).unwrap()).unwrap();
    assert_eq!(parent.value().id, "scene:01");
    assert_eq!(
        parent.value().authored["binding_ids"]["shared-cue"]["id"],
        "cue:shared"
    );
    let input = Instance::parse(&fs::read(output.join("inputs/cel-a.tig")).unwrap()).unwrap();
    assert_eq!(input.value().id, "cel:A");
    assert_eq!(
        input.value().status["selection_state"],
        "private-unselected-candidate"
    );
    assert_eq!(input.value().selected.as_ref().unwrap().bytes, 999999999);
    assert_eq!(input.value().authored["public_release"], false);
    assert_eq!(
        input.value().metadata["native_registration"]["version_id"],
        "draft-v2"
    );
    assert!(prepared.write(&output).is_err());
    assert_eq!(fs::read_dir(output.join("inputs")).unwrap().count(), 2);
}
#[test]
fn cli_has_bounded_count_diagnostics_and_never_overwrites_an_existing_destination() {
    let f = Fixture::standard();
    let output = f.root.path().join("cli-candidate");
    let result = f.run(&output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["records"], 3);
    assert!(!String::from_utf8_lossy(&result.stdout).contains("fixture-source"));
    fs::write(output.join("sentinel"), b"preserve").unwrap();
    assert!(!f.run(&output).status.success());
    assert_eq!(fs::read(output.join("sentinel")).unwrap(), b"preserve");
}
#[test]
fn invalid_scope_identity_annotations_budget_and_definition_refuse_before_output() {
    for fault in [
        "package",
        "definition",
        "asset",
        "annotation",
        "unknown",
        "budget",
        "filename",
        "collision",
        "provenance",
    ] {
        let mut f = Fixture::standard();
        match fault {
            "package" => f.profile["package"]["sha256"] = json!("c".repeat(64)),
            "definition" => {
                f.profile["primary_definition"]["authored_schema"]["sha256"] = json!("d".repeat(64))
            }
            "asset" => f.profile["scene_asset"] = json!("not-in-package"),
            "annotation" => {
                f.profile["primary_annotations"]["cel-a"] = json!({"public_release":true})
            }
            "unknown" => f.profile["primary_annotations"]["unknown"] = json!({"label":"x"}),
            "budget" => f.profile["max_output_bytes"] = json!(1),
            "filename" => f.profile["filenames"]["cel-a"] = json!("../escape.tig"),
            "collision" => {
                f.profile["filenames"] = json!({"cel-a":"Same.tig","shared-cue":"same.tig"})
            }
            _ => f.profile["metadata"]["native_registration"] = json!({}),
        }
        let output = f.root.path().join("refused");
        let result = f.run(&output);
        assert!(!result.status.success(), "{fault}");
        assert!(!output.exists(), "{fault}");
    }
}
#[test]
fn duplicate_ids_and_wrong_binding_scope_do_not_silently_rename_source_assets() {
    let asset = json!({"logical_id":"same-id","sha256":"a".repeat(64),"bytes":1,
        "cache_uri":format!("cache://sha256/{}","a".repeat(64)),"selection_state":"draft"});
    for bindings in [
        json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene:01","assets":{"one":asset,"two":asset}}),
        json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"wrong-scene","assets":{"one":asset}}),
    ] {
        let f = Fixture::new(bindings);
        let output = f.root.path().join("refused");
        assert!(!f.run(&output).status.success());
        assert!(!output.exists());
    }
}

#[test]
fn duplicate_binding_keys_and_conflicting_digest_sizes_cannot_collapse_during_intake() {
    let asset = json!({"logical_id":"one","sha256":"a".repeat(64),"bytes":1,
        "cache_uri":format!("cache://sha256/{}","a".repeat(64)),"selection_state":"draft"});
    let raw = format!(
        r#"{{"schema":"reel.scene-asset-bindings.v1","scope_id":"scene:01","assets":{{"one":{asset},"one":{asset}}}}}"#
    );
    let duplicate = Fixture::raw(raw.as_bytes());
    let mut different_size = asset.clone();
    different_size["logical_id"] = json!("two");
    different_size["bytes"] = json!(2);
    let conflicting = Fixture::new(
        json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"scene:01",
        "assets":{"one":asset,"two":different_size}}),
    );
    for f in [duplicate, conflicting] {
        let output = f.root.path().join("refused");
        assert!(!f.run(&output).status.success());
        assert!(!output.exists());
    }
}
