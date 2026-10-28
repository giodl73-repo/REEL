#![cfg(feature = "tig")]
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_assets::native::Instance;
use tig_core::Object;

fn put(root: &Path, bytes: &[u8]) -> Object {
    let object = Object::from_bytes(bytes);
    fs::write(root.join("objects").join(object.sha256.as_str()), bytes).unwrap();
    object
}
fn local(root: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    let object = Object::from_bytes(bytes);
    json!({"path":path,"sha256":object.sha256,"bytes":object.bytes})
}
fn reference(bytes: &[u8]) -> Value {
    let object = Object::from_bytes(bytes);
    json!({"path":"private/original/source","sha256":object.sha256,"bytes":object.bytes})
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("objects")).unwrap();
        let schema = br#"{"type":"object","properties":{"language":{"type":"string"},"source_segment_id":{"type":"string"},"component_kind":{"enum":["scene","episode-presentation"]},"scene":{"type":"object"}},"required":["language","source_segment_id","component_kind","scene"],"additionalProperties":false}"#;
        let definition = serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":"test/component-output",
            "authored_schema":Object::from_bytes(schema),"authoring":{"properties":["language","source_segment_id","component_kind","scene"],
            "required":["language","source_segment_id","component_kind","scene"],"defaults":{}},"stream_rules":{},"semantic_rules":{}})).unwrap();
        let files = json!({"definition":local(root.path(),"definition.json",&definition),"authored_schema":local(root.path(),"schema.json",schema)});
        let mut conforms = serde_json::Map::new();
        let mut entries = serde_json::Map::new();
        for language in ["es", "en"] {
            let control = reference(b"original control");
            let master = reference(format!("{language} master bytes NOT PRESENT").as_bytes());
            let conform = json!({"schema":"reel.episode-conform.v1","episode_id":"EP01","language":language,
                "output_sample_rate":48000,"catalog":control,"master_template_definition":control,
                "episode":control,"season_bindings":control,"episode_bindings":control,"segments":[
                {"id":"scene-001","kind":"scene","master":master,"source_receipt":control,
                "delivery_job":reference(b"job"),"delivery_receipt":reference(b"receipt"),
                "semantic_delivery":reference(b"semantic"),"adoption_manifest":reference(b"adoption"),
                "source_text_evidence":reference(b"text"),"imported_source_manifest":reference(b"imported")},
                {"id":"closing","kind":"episode-presentation","master":reference(b"closing"),"source_receipt":control}]});
            conforms.insert(
                language.into(),
                json!(put(root.path(), &serde_json::to_vec(&conform).unwrap())),
            );
            entries.insert(language.into(),json!({"id":format!("episode/ep01/scene-001/output/{language}"),"filename":format!("{language}.tig"),
                "language":language,"segment_id":"scene-001","kind":"scene","definition":files,"authored":{"scene":{"id":"episode/ep01/scene-001"}}}));
        }
        let profile = json!({"schema":reel::tig_conform_registration::SCHEMA,"episode_id":"EP01",
            "byte_source":{"places":[{"root":root.path(),"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}},
            "conforms":conforms,"entries":entries,"metadata":{"private-origin":"preserved"},"max_output_bytes":1048576});
        Self { root, profile }
    }
    fn run(&self) -> std::process::Output {
        let profile = self.root.path().join("profile.json");
        fs::write(&profile, serde_json::to_vec(&self.profile).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .args(["--conform-components"])
            .arg(profile)
            .arg(self.root.path().join("candidates"))
            .env("PATH", "")
            .output()
            .unwrap();
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for private in [
            "EP01",
            "private-origin",
            "private/original",
            &self.root.path().display().to_string(),
        ] {
            assert!(!logs.contains(private), "private diagnostic escaped");
        }
        output
    }
    fn conform(&self, language: &str) -> Value {
        serde_json::from_slice(
            &fs::read(
                self.root.path().join("objects").join(
                    self.profile["conforms"][language]["sha256"]
                        .as_str()
                        .unwrap(),
                ),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn edit(&mut self, language: &str, change: impl FnOnce(&mut Value)) {
        let mut conform = self.conform(language);
        change(&mut conform);
        self.profile["conforms"][language] = json!(put(
            self.root.path(),
            &serde_json::to_vec(&conform).unwrap()
        ));
    }
    fn refused(&self) {
        assert!(!self.run().status.success());
        assert!(!self.root.path().join("candidates").exists());
    }
}

#[test]
fn independent_outputs_preserve_exact_provenance_without_reading_any_media_or_claiming_currency() {
    let f = Fixture::new();
    let result = f.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["records"], 2);
    assert_eq!(summary["primary_media_opened"], 0);
    assert_eq!(summary["payloads_verified"], false);
    assert_eq!(summary["authorizes_eviction"], false);
    for language in ["es", "en"] {
        let raw = fs::read(
            f.root
                .path()
                .join(format!("candidates/inputs/{language}.tig")),
        )
        .unwrap();
        Instance::parse(&raw).unwrap();
        let record: Value = serde_json::from_slice(&raw).unwrap();
        let original = f.conform(language);
        assert_eq!(
            record["selected"]["sha256"],
            original["segments"][0]["master"]["sha256"]
        );
        assert_eq!(record["authored"]["language"], language);
        assert_eq!(record["authored"]["scene"]["id"], "episode/ep01/scene-001");
        assert_eq!(record["status"]["production_state"], "unclassified");
        assert_eq!(record["status"]["new_approval"], false);
        assert_eq!(
            record["streams"][0]["object"],
            f.profile["conforms"][language]
        );
        assert_eq!(record["metadata"]["private-origin"], "preserved");
        assert_eq!(record["streams"].as_array().unwrap().len(), 8);
        for pin in record["streams"]
            .as_array()
            .unwrap()
            .iter()
            .skip(1)
            .map(|s| &s["object"])
            .chain([&record["selected"]])
        {
            assert!(
                !f.root
                    .path()
                    .join("objects")
                    .join(pin["sha256"].as_str().unwrap())
                    .exists()
            );
        }
    }
    // No clobber even when registration is exactly retried.
    assert!(!f.run().status.success());
}

#[test]
fn explicit_presentation_mapping_keeps_its_kind_and_position() {
    let mut f = Fixture::new();
    f.profile["entries"]["es"]["segment_id"] = json!("closing");
    f.profile["entries"]["es"]["kind"] = json!("episode-presentation");
    assert!(f.run().status.success());
    let record: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("candidates/inputs/es.tig")).unwrap())
            .unwrap();
    assert_eq!(record["authored"]["component_kind"], "episode-presentation");
    assert_eq!(
        record["metadata"]["conform_components_registration"]["segment_position"],
        1
    );
}

#[test]
fn scope_kind_identity_and_destination_conflicts_refuse_before_creating_candidates() {
    for case in 0..14 {
        let mut f = Fixture::new();
        match case {
            0 => f.profile["episode_id"] = json!("different"),
            1 => f.edit("en", |v| v["language"] = json!("es")),
            2 => f.profile["entries"]["es"]["kind"] = json!("episode-presentation"),
            3 => f.profile["entries"]["es"]["segment_id"] = json!("absent"),
            4 => f.profile["entries"]["en"]["id"] = f.profile["entries"]["es"]["id"].clone(),
            5 => f.profile["entries"]["en"]["filename"] = json!("ES.tig"),
            6 => f.profile["entries"]["es"]["filename"] = json!("../outside.tig"),
            7 => f.profile["entries"]["es"]["authored"]["language"] = json!("en"),
            8 => f.profile["metadata"]["conform_components_registration"] = json!({}),
            9 => f.profile["entries"]["en"]["language"] = json!("es"),
            10 => {
                f.profile["entries"]["es"]["authored"]
                    .as_object_mut()
                    .unwrap()
                    .remove("scene");
            }
            11 => f.edit("es", |v| v["segments"][1]["id"] = json!("scene-001")),
            12 => f.profile["max_output_bytes"] = json!(1),
            _ => f.profile["entries"]["es"]["unexpected"] = json!(true),
        }
        f.refused();
    }
}

#[test]
fn every_reference_is_validated_even_in_unmapped_segments_and_conform_controls() {
    for case in 0..4 {
        let mut f = Fixture::new();
        match case {
            0 => f.edit("es", |v| {
                v["segments"][1]["master"]["sha256"] = json!("invalid")
            }),
            1 => f.edit("es", |v| v["catalog"]["sha256"] = json!("invalid")),
            2 => f.edit("es", |v| {
                v["segments"][1]["source_receipt"]["bytes"] = json!(99)
            }),
            _ => f.edit("en", |v| v["episode"]["bytes"] = json!(99)),
        }
        f.refused();
    }
}

#[test]
fn corrupt_missing_oversized_metadata_and_stale_definitions_do_not_publish_candidates() {
    for case in 0..4 {
        let mut f = Fixture::new();
        let path = f
            .root
            .path()
            .join("objects")
            .join(f.profile["conforms"]["es"]["sha256"].as_str().unwrap());
        match case {
            0 => fs::write(path, b"corrupted").unwrap(),
            1 => fs::remove_file(path).unwrap(),
            2 => f.profile["conforms"]["es"]["bytes"] = json!(4 * 1024 * 1024 + 1),
            _ => fs::write(f.root.path().join("schema.json"), b"false").unwrap(),
        }
        f.refused();
    }
}
