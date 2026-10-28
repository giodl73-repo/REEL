#![cfg(feature = "tig")]
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_assets::native::Instance;
use tig_core::Object;

fn put(root: &Path, bytes: &[u8]) -> Object {
    let pin = Object::from_bytes(bytes);
    fs::write(root.join("objects").join(pin.sha256.as_str()), bytes).unwrap();
    pin
}
fn descriptor(pin: &Object) -> Value {
    json!({"sha256":pin.sha256,"bytes":pin.bytes,"cache_uri":format!("cache://sha256/{}",pin.sha256.as_str())})
}
fn file(root: &Path, name: &str, bytes: &[u8]) -> Value {
    let path = root.join(name);
    fs::write(&path, bytes).unwrap();
    let pin = Object::from_bytes(bytes);
    json!({"path":path,"sha256":pin.sha256,"bytes":pin.bytes})
}
fn definition(root: &Path, name: &str, fields: &[&str], required: &[&str]) -> Value {
    let schema = b"true";
    let def=serde_json::to_vec(&json!({"schema":"tig.definition.design-1","id":name,
        "authored_schema":Object::from_bytes(schema),"authoring":{"properties":fields,"required":required,"defaults":{}},
        "stream_rules":{},"semantic_rules":{}})).unwrap();
    json!({"definition":file(root,&format!("{name}.json"),&def),"authored_schema":file(root,&format!("{name}.schema.json"),schema)})
}
struct Fixture {
    root: tempfile::TempDir,
    profile: Value,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("objects")).unwrap();
        let mut versions = Vec::new();
        for (version, lock, spanish, role, english_role) in [
            (
                "v1",
                "v1",
                b"Vieja palabra".as_slice(),
                "es-original",
                "en-old",
            ),
            (
                "v2",
                "v2-inputs-immutable",
                b"Nueva palabra".as_slice(),
                "es-owner",
                "en-current",
            ),
        ] {
            let text = Object::from_bytes(spanish);
            let english = Object::from_bytes(b"A word");
            let package=put(root.path(),&serde_json::to_vec(&json!({"schema":"caminos.artifact-package.v1",
                "artifact_id":"poems/sample","version_id":lock,"assets":{role:descriptor(&text),english_role:descriptor(&english)},
                "metadata":{"review":"private holds stay in exact package"}})).unwrap());
            versions.push(json!({"version_id":version,"manifest":descriptor(&package),
                "version_lock":{"logical_id":"poems/sample","version_id":lock,"manifest_sha256":package.sha256}}));
        }
        let source=put(root.path(),&serde_json::to_vec(&json!({"schema":"caminos.artifact-record.v1","artifact_id":"poems/sample",
            "kind":"poem","current_version":"v2","public_release":false,"versions":versions,"unknown_rights":"retain exact source"})).unwrap());
        let parent = definition(
            root.path(),
            "parent",
            &[
                "artifact_id",
                "kind",
                "current_version",
                "public_release",
                "asset_ids",
            ],
            &["artifact_id", "kind", "current_version", "asset_ids"],
        );
        let leaf = definition(
            root.path(),
            "text",
            &["language", "translated_from"],
            &["language"],
        );
        let profile = json!({"schema":reel::tig_package_registration::SCHEMA,"source_record":source,
            "byte_source":{"places":[{"root":root.path(),"layout":"flat"}],"required":if cfg!(windows){"prevent-mutation"}else{"cooperative-immutable"}},
            "parent_definition":parent,"parent_filename":"poem.tig","entries":{
            "es":{"id":"poems/sample/text-es","filename":"es.tig","definition":leaf,"authored":{"language":"es"},"assets":{"v1":"es-original","v2":"es-owner"}},
            "en":{"id":"poems/sample/text-en","filename":"en.tig","definition":leaf,"authored":{"language":"en","translated_from":{"id":"poems/sample/text-es"}},"assets":{"v1":"en-old","v2":"en-current"}}},
            "max_output_bytes":1048576});
        Self { root, profile }
    }
    fn run(&self) -> std::process::Output {
        let profile = self.root.path().join("profile.json");
        fs::write(&profile, serde_json::to_vec(&self.profile).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_reel-tig-register"))
            .arg("--package-assets")
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
            "poems/sample",
            "unknown_rights",
            "sha256",
            &self.root.path().display().to_string(),
        ] {
            assert!(!logs.contains(private));
        }
        output
    }
    fn load(&self, path: &str) -> Value {
        serde_json::from_slice(&fs::read(self.root.path().join(path)).unwrap()).unwrap()
    }
    fn repackage(&mut self, change: impl FnOnce(&mut Value)) {
        let source: Object = serde_json::from_value(self.profile["source_record"].clone()).unwrap();
        let mut record = self.load(&format!("objects/{}", source.sha256.as_str()));
        let old = record["versions"][0]["manifest"]["sha256"]
            .as_str()
            .unwrap();
        let mut package = self.load(&format!("objects/{old}"));
        change(&mut package);
        let new = put(self.root.path(), &serde_json::to_vec(&package).unwrap());
        record["versions"][0]["manifest"] = descriptor(&new);
        record["versions"][0]["version_lock"]["manifest_sha256"] = json!(new.sha256);
        self.profile["source_record"] =
            json!(put(self.root.path(), &serde_json::to_vec(&record).unwrap()));
    }
}

#[test]
fn native_text_assets_preserve_versions_references_and_unknown_evidence_without_opening_payloads() {
    let f = Fixture::new();
    let result = f.run();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["records"], 3);
    assert_eq!(summary["primary_media_opened"], 0);
    let parent = f.load("candidates/poem.tig");
    assert_eq!(parent["id"], "poems/sample");
    assert_eq!(
        parent["authored"]["asset_ids"]["es"]["id"],
        "poems/sample/text-es"
    );
    assert_eq!(parent["streams"].as_array().unwrap().len(), 3);
    assert_eq!(parent["streams"][0]["object"], f.profile["source_record"]);
    let es = f.load("candidates/inputs/es.tig");
    let en = f.load("candidates/inputs/en.tig");
    assert_eq!(es["selected"], json!(Object::from_bytes(b"Nueva palabra")));
    assert_eq!(
        es["streams"][0]["object"],
        json!(Object::from_bytes(b"Vieja palabra"))
    );
    assert_eq!(
        es["streams"][1]["provenance"]["version_lock_id"],
        "v2-inputs-immutable"
    );
    assert_eq!(en["authored"]["translated_from"]["id"], es["id"]);
    for record in [es, en] {
        Instance::parse(&serde_json::to_vec(&record).unwrap()).unwrap();
        assert_eq!(record["status"]["production_state"], "unclassified");
        for pin in record["streams"]
            .as_array()
            .unwrap()
            .iter()
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
    assert!(!f.run().status.success());
}
#[test]
fn explicit_prior_absence_is_distinct_from_a_missing_or_guessed_mapping() {
    let mut f = Fixture::new();
    f.profile["entries"]["en"]["assets"]["v1"] = Value::Null;
    assert!(f.run().status.success());
    assert_eq!(
        f.load("candidates/inputs/en.tig")["streams"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.load("candidates/poem.tig")["streams"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}
#[test]
fn invalid_mapping_identity_definition_and_budget_refuse_before_output_creation() {
    for case in [
        "missing-version",
        "extra-version",
        "current-absent",
        "missing-role",
        "duplicate-id",
        "duplicate-file",
        "escape",
        "reserved",
        "definition",
        "override",
        "budget",
        "source-budget",
    ] {
        let mut f = Fixture::new();
        match case {
            "missing-version" => {
                f.profile["entries"]["es"]["assets"]
                    .as_object_mut()
                    .unwrap()
                    .remove("v1");
            }
            "extra-version" => f.profile["entries"]["es"]["assets"]["v0"] = Value::Null,
            "current-absent" => f.profile["entries"]["es"]["assets"]["v2"] = Value::Null,
            "missing-role" => f.profile["entries"]["es"]["assets"]["v1"] = json!("missing"),
            "duplicate-id" => {
                f.profile["entries"]["en"]["id"] = f.profile["entries"]["es"]["id"].clone()
            }
            "duplicate-file" => f.profile["entries"]["en"]["filename"] = json!("ES.tig"),
            "escape" => f.profile["entries"]["es"]["filename"] = json!("../outside.tig"),
            "reserved" => f.profile["entries"]["es"]["filename"] = json!("CON.tig"),
            "definition" => f.profile["entries"]["es"]["authored"]["unadmitted"] = json!(true),
            "override" => f.profile["parent_authored"] = json!({"artifact_id":"foreign"}),
            "source-budget" => f.profile["source_record"]["bytes"] = json!(65537),
            _ => f.profile["max_output_bytes"] = json!(1),
        }
        assert!(!f.run().status.success(), "{case}");
        assert!(!f.root.path().join("candidates").exists(), "{case}");
    }
}
#[test]
fn checked_package_bytes_cannot_claim_another_scope_lock_schema_or_asset_uri() {
    for case in ["scope", "lock", "schema", "uri"] {
        let mut f = Fixture::new();
        f.repackage(|package| match case {
            "scope" => package["artifact_id"] = json!("foreign"),
            "lock" => package["version_id"] = json!("foreign"),
            "schema" => package["schema"] = json!("foreign"),
            _ => package["assets"]["es-original"]["cache_uri"] = json!("cache://sha256/wrong"),
        });
        assert!(!f.run().status.success(), "{case}");
        assert!(!f.root.path().join("candidates").exists());
    }
}
