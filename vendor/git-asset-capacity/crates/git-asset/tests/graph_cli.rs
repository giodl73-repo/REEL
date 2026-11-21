#![cfg(feature = "assets")]
mod common;
#[path = "graph_cli/verification_cases.rs"]
mod verification_cases;
use common::{git, git_executable};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Fixture {
    temp: tempfile::TempDir,
    text: Value,
    output: Value,
    profiles: Value,
}
fn write(root: &Path, path: &str, value: &Value) -> Object {
    let bytes = serde_json::to_vec(value).unwrap();
    fs::write(root.join(path), &bytes).unwrap();
    Object::from_bytes(&bytes)
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        fs::create_dir_all(root.join("records")).unwrap();
        fs::create_dir(root.join("definitions")).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(&root, &["config", "user.name", "Fixture"]);
        git(&root, &["config", "user.email", "fixture@example.invalid"]);
        fs::write(root.join(".gitattributes"), "* text eol=lf\n").unwrap();
        let schema = write(&root, "definitions/authored.json", &json!(true));
        let mut pins = Vec::new();
        let mut profiles = Vec::new();
        for (name, class, payloads) in [
            ("text", "primary", json!("SelectedObject")),
            ("output", "output", json!("None")),
        ] {
            let id = format!("types/{name}");
            let pin = write(
                &root,
                &format!("definitions/{name}.json"),
                &json!({"schema":"tig.definition.design-1","id":id,
                "authored_schema":schema,"authoring":{"properties":["translation_sources","private_text"],"required":["translation_sources"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
            );
            pins.push(json!({"id":id,"sha256":pin.sha256,"bytes":pin.bytes}));
            profiles.push(json!({"definition_id":id,"definition":pin,"class":class,"covered":["production"],
                "rules":[{"pointer":"/translation_sources","kind":"production","collection":"array","optional":false,"binding":"SnapshotSelected"}],"payloads":payloads}));
        }
        let f = Self {
            temp,
            text: pins[0].clone(),
            output: pins[1].clone(),
            profiles: json!(profiles),
        };
        f.record("es", "private/spanish", &f.text, json!([]), b"source-v1");
        f.record(
            "en",
            "private/english",
            &f.text,
            json!([{"id":"private/spanish"}]),
            b"translation",
        );
        f.commit();
        f
    }
    fn root(&self) -> PathBuf {
        self.temp.path().join("repo")
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn record(&self, file: &str, id: &str, definition: &Value, deps: Value, payload: &[u8]) {
        write(
            &self.root(),
            &format!("records/{file}.asset"),
            &json!({"schema":"tig.instance.design-1","id":id,
            "definition":definition,"authored":{"translation_sources":deps,"private_text":"PRIVATE AUTHORING SENTENCE"},
            "selected":Object::from_bytes(payload),"streams":[{"slot":"version/older","role":"history","group":"history","object":Object::from_bytes(b"historical output must not enter current demand")}],
            "status":{"pretend_primary":true},"metadata":{"secret":"PRIVATE METADATA"}}),
        );
    }
    fn commit(&self) -> String {
        git(&self.root(), &["add", "."]);
        git(&self.root(), &["commit", "-m", "synthetic graph update"]);
        String::from_utf8(git(&self.root(), &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .into()
    }
    fn profile(&self) -> Value {
        json!({"schema":"tig.graph-inspection-profile.experimental-1","git_executable":git_executable(),"repository":self.root(),
            "commit":String::from_utf8(git(&self.root(),&["rev-parse","HEAD"]).stdout).unwrap().trim(),"records":["records"],"definitions":["definitions"],
            "roots":["private/english"],"kinds":["production"],"profiles":self.profiles,
            "query_limits":{"nodes":100,"edges":100,"depth":16,"bytes":1048576},"max_report_bytes":1048576})
    }
    fn save(&self, name: &str, value: &Value) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
    fn run(&self, mode: &str, paths: &[&Path]) -> Output {
        let result = Command::new(env!("CARGO_BIN_EXE_git-asset-graph"))
            .arg(mode)
            .args(paths)
            .env("PATH", "")
            .output()
            .unwrap();
        let logs = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        for secret in [
            "private/english",
            "private/spanish",
            "PRIVATE AUTHORING",
            "PRIVATE METADATA",
            "sha256",
            &self.temp.path().display().to_string(),
        ] {
            assert!(!logs.contains(secret), "private data in CLI diagnostics");
        }
        result
    }
    fn load(&self, name: &str) -> Value {
        serde_json::from_slice(&fs::read(self.path(name)).unwrap()).unwrap()
    }
}
#[test]
fn independent_leaf_edit_changes_translation_graph_without_parent_edits_or_history_demand() {
    let f = Fixture::new();
    let old = f.save("old.json", &f.profile());
    let before = f.path("before.json");
    assert!(f.run("inspect", &[&old, &before]).status.success());
    let report = f.load("before.json");
    assert_eq!(report["graph"]["nodes"].as_array().unwrap().len(), 2);
    assert_eq!(
        report["graph"]["production_requirements"]["primary_objects"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(report["payloads_verified"], false);
    assert_eq!(report["graph"]["authored_schemas_validated"], false);
    assert_eq!(report["authorizes_eviction"], false);
    let private = fs::read_to_string(before).unwrap();
    assert!(!private.contains("PRIVATE AUTHORING"));
    assert!(!private.contains("PRIVATE METADATA"));
    let english = fs::read(f.root().join("records/en.asset")).unwrap();
    f.record("es", "private/spanish", &f.text, json!([]), b"source-v2");
    f.commit();
    assert_eq!(
        fs::read(f.root().join("records/en.asset")).unwrap(),
        english
    );
    let new = f.save("new.json", &f.profile());
    let compared = f.path("change.json");
    assert!(f.run("compare", &[&old, &new, &compared]).status.success());
    let comparison = f.load("change.json");
    assert_eq!(
        comparison["change"]["added"],
        json!([Object::from_bytes(b"source-v2")])
    );
    assert_eq!(
        comparison["change"]["unreferenced_by_new_query"],
        json!([Object::from_bytes(b"source-v1")])
    );
    assert_eq!(
        comparison["change"]["shared"],
        json!([Object::from_bytes(b"translation")])
    );
    assert_eq!(comparison["complete_claim_coverage"], false);
    assert_eq!(comparison["authorizes_eviction"], false);
    assert!(
        f.run("inspect", &[&old, &f.path("historical.json")])
            .status
            .success()
    );
    assert_eq!(
        f.load("historical.json")["graph"]["production_requirements"],
        report["graph"]["production_requirements"]
    );
    assert!(!f.root().join("cache").exists());
}
#[test]
fn incomplete_or_incompatible_graphs_never_emit_empty_successful_candidate_sets() {
    let f = Fixture::new();
    let old = f.save("old.json", &f.profile());
    for (name, deps, definition) in [
        ("missing", json!([{"id":"private/missing"}]), &f.text),
        ("null", Value::Null, &f.text),
        ("output", json!([]), &f.output),
    ] {
        if name == "output" {
            f.record(
                "es",
                "private/spanish",
                definition,
                deps,
                b"compiled-output",
            );
        } else {
            f.record("en", "private/english", definition, deps, b"translation");
        }
        f.commit();
        let new = f.save(&format!("{name}.json"), &f.profile());
        let report = f.path(&format!("{name}-report.json"));
        assert!(!f.run("inspect", &[&new, &report]).status.success());
        assert!(
            !f.load(&format!("{name}-report.json"))["graph"]["gaps"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let compare = f.path(&format!("{name}-compare.json"));
        assert!(!f.run("compare", &[&old, &new, &compare]).status.success());
        assert!(f.load(&format!("{name}-compare.json"))["change"].is_null());
        f.record(
            "en",
            "private/english",
            &f.text,
            json!([{"id":"private/spanish"}]),
            b"translation",
        );
        f.record("es", "private/spanish", &f.text, json!([]), b"source-v1");
    }
    f.commit();
    let mut incompatible = f.profile();
    incompatible["profiles"][0]["rules"][0]["binding"] = json!("Either");
    let incompatible = f.save("incompatible.json", &incompatible);
    assert!(
        !f.run(
            "compare",
            &[&old, &incompatible, &f.path("incompatible-report.json")]
        )
        .status
        .success()
    );
    assert!(f.load("incompatible-report.json")["change"].is_null());
    let report = f.load("incompatible-report.json");
    assert!(report["before"]["gaps"].as_array().unwrap().is_empty());
    assert!(report["after"]["gaps"].as_array().unwrap().is_empty());
    assert_ne!(
        report["before"]["admission_sha256"],
        report["after"]["admission_sha256"]
    );
}
#[test]
fn strict_admission_and_report_limits_refuse_without_overwrite_or_temporary_leaks() {
    let f = Fixture::new();
    for mode in [
        "top",
        "profile",
        "payload",
        "rule",
        "roots",
        "budget",
        "missing-root",
        "limits",
        "zero-limit",
        "kinds",
    ] {
        let mut value = f.profile();
        match mode {
            "top" => value["assume_safe"] = json!(true),
            "profile" => value["profiles"][0]["assume_safe"] = json!(true),
            "payload" => {
                value["profiles"][0]["payloads"] =
                    json!({"Named":{"selected":true,"slots":[],"assume_safe":true}})
            }
            "rule" => value["profiles"][0]["rules"][0]["assume_safe"] = json!(true),
            "roots" => value["roots"] = json!(["private/english", "private/english"]),
            "missing-root" => value["roots"] = json!(["private/missing"]),
            "limits" => value["query_limits"]["unknown"] = json!(true),
            "zero-limit" => value["query_limits"]["nodes"] = json!(0),
            "kinds" => value["kinds"] = json!(["unknown"]),
            _ => value["max_report_bytes"] = json!(1),
        }
        let input = f.save(&format!("{mode}.json"), &value);
        let output = f.path(&format!("{mode}-report.json"));
        assert!(
            !f.run("inspect", &[&input, &output]).status.success(),
            "{mode}"
        );
        assert!(!output.exists(), "{mode}");
    }
    let profile = f.save("good.json", &f.profile());
    let existing = f.path("keep.json");
    fs::write(&existing, b"keep").unwrap();
    assert!(!f.run("inspect", &[&profile, &existing]).status.success());
    assert_eq!(fs::read(existing).unwrap(), b"keep");
    assert!(!fs::read_dir(f.temp.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".git-asset-graph-")
    }));
}
#[test]
fn serialized_admission_roundtrips_without_changing_existing_digest_or_wire_spelling() {
    let f = Fixture::new();
    let value = f.profiles.clone();
    let profiles: Vec<git_asset_model::graph::Profile> =
        serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(profiles).unwrap(), value);
    let before = f.save("before.json", &f.profile());
    fs::write(f.root().join("notes.txt"), "unrelated edit").unwrap();
    f.commit();
    let after = f.save("after.json", &f.profile());
    assert!(
        f.run("compare", &[&before, &after, &f.path("unrelated.json")])
            .status
            .success()
    );
    let report = f.load("unrelated.json");
    assert!(report["change"]["added"].as_array().unwrap().is_empty());
    assert!(
        report["change"]["unreferenced_by_new_query"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        report["before"]["binding_sha256"],
        report["after"]["binding_sha256"]
    );
}
