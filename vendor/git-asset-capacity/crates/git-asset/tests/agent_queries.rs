#![cfg(feature = "schema")]
#[path = "agent_queries/admission_cases.rs"]
mod admission_cases;
#[path = "agent_queries/catalogue_compare_cases.rs"]
mod catalogue_compare_cases;
#[path = "agent_queries/catalogue_impact_cases.rs"]
mod catalogue_impact_cases;
mod common;
#[path = "agent_queries/manifest_cases.rs"]
mod manifest_cases;
#[path = "agent_queries/manifest_impact_cases.rs"]
mod manifest_impact_cases;
#[path = "agent_queries/preview_cases.rs"]
mod preview_cases;
use common::{git, git_executable};
use git_asset::catalogue::{
    Config,
    query::{self, Request},
};
use git_asset_core::Object;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};

fn write(root: &Path, path: &str, value: &Value) -> Object {
    let raw = serde_json::to_vec(value).unwrap();
    fs::write(root.join(path), &raw).unwrap();
    Object::from_bytes(&raw)
}
struct Fixture {
    temp: tempfile::TempDir,
    pins: BTreeMap<String, Value>,
    enrollment: std::path::PathBuf,
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
        let schema = write(&root, "definitions/authored.json", &json!(true));
        let mut pins = BTreeMap::new();
        let mut profiles = Vec::new();
        for (name, class, kind, payloads) in [
            ("source", "primary", "production", "SelectedObject"),
            ("scene", "container", "production", "None"),
            ("render", "output", "composition", "None"),
            ("delivery", "output", "publication", "None"),
        ] {
            let id = format!("type/{name}");
            let object = write(
                &root,
                &format!("definitions/{name}.json"),
                &json!({"schema":"tig.definition.design-1","id":id,"authored_schema":schema,"authoring":{"properties":["inputs","prompt","language"],"required":["inputs"],"defaults":{}},"stream_rules":{},"semantic_rules":{}}),
            );
            pins.insert(
                name.into(),
                json!({"id":id,"sha256":object.sha256,"bytes":object.bytes}),
            );
            profiles.push(json!({"definition_id":id,"definition":object,"class":class,"covered":["production","composition","publication"],"rules":[{"pointer":"/inputs","kind":kind,"collection":"array","optional":false,"binding":"Either"}],"payloads":payloads}));
        }
        let enrollment = temp.path().join("enrollment.json");
        let f = Self {
            temp,
            pins,
            enrollment,
        };
        for (id, ty, inputs) in [
            ("cel-a", "source", vec![]),
            ("cel-b", "source", vec![]),
            ("text-es", "source", vec![]),
            ("text-en", "source", vec!["text-es"]),
            ("cue", "source", vec!["text-en"]),
            ("scene", "scene", vec!["cel-a", "cel-b", "cue"]),
            ("render", "render", vec!["scene"]),
            ("delivery", "delivery", vec!["render"]),
        ] {
            f.record(
                id,
                ty,
                json!(
                    inputs
                        .into_iter()
                        .map(|id| json!({"id":id}))
                        .collect::<Vec<_>>()
                ),
            );
        }
        let profile = f.temp.path().join("profile.json");
        write(
            f.temp.path(),
            "profile.json",
            &json!({"schema":"tig.asset-resolver-profile.experimental-1","git_executable":git_executable(),"repository":root,"commit":f.commit(),"records":["records"],"definitions":["definitions"]}),
        );
        write(
            f.temp.path(),
            "enrollment.json",
            &json!({"schema":"tig.catalogue-enrollment.experimental-1","projects":[{"id":"project","label":"Fixture","profile":profile,"graph_profiles":profiles,"display":[{"category":"items","title_pointers":["/id"]}]}]}),
        );
        f
    }
    fn root(&self) -> std::path::PathBuf {
        self.temp.path().join("repo")
    }
    fn record(&self, id: &str, ty: &str, inputs: Value) -> Object {
        write(
            &self.root(),
            &format!("records/{id}.asset"),
            &json!({"schema":"tig.instance.design-1","id":id,"definition":self.pins[ty],"authored":{"inputs":inputs,"prompt":format!("DO_NOT_EXPORT {id}")},"streams":[],"selected":Object::from_bytes(b"payload deliberately not stored"),"status":{"lifecycle":"draft"},"metadata":{"private":"DO_NOT_EXPORT"}}),
        )
    }
    fn commit(&self) -> String {
        git(&self.root(), &["add", "."]);
        git(&self.root(), &["commit", "-m", "fixture"]);
        String::from_utf8(git(&self.root(), &["rev-parse", "HEAD"]).stdout)
            .unwrap()
            .trim()
            .into()
    }
    fn advance(&self) {
        let head = self.commit();
        let path = self.temp.path().join("profile.json");
        let mut profile: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        profile["commit"] = json!(head);
        fs::write(path, serde_json::to_vec(&profile).unwrap()).unwrap();
    }
    fn request(&self, query: Value) -> Value {
        json!({"schema":"tig.query.experimental-1","enrollment":self.enrollment,"query":query})
    }
    fn evaluate(&self, request: &Value) -> Value {
        let request = Request::parse(&serde_json::to_vec(request).unwrap()).unwrap();
        let result = query::execute(Config::load(&self.enrollment).unwrap(), &request).unwrap();
        git_asset_model::schema::check_document(
            query::RESULT_SCHEMA,
            git_asset_model::schema::Dialect::Draft202012,
            &result,
        )
        .unwrap();
        result
    }
    fn run(&self, request: &Value, extra: &[&str]) -> (Output, Value) {
        let path = self.temp.path().join("request.json");
        fs::write(&path, serde_json::to_vec(request).unwrap()).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_git-asset-query"))
            .arg("run")
            .arg(path)
            .args(extra)
            .env("PATH", "")
            .output()
            .unwrap();
        assert!(output.stderr.is_empty(), "{:?}", output);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        git_asset_model::schema::check_document(
            query::RESULT_SCHEMA,
            git_asset_model::schema::Dialect::Draft202012,
            &value,
        )
        .unwrap();
        assert!(!String::from_utf8_lossy(&output.stdout).contains("DO_NOT_EXPORT"));
        assert!(
            !String::from_utf8_lossy(&output.stdout)
                .contains(&self.temp.path().display().to_string())
        );
        (output, value)
    }
}
fn impact(asset: &str) -> Value {
    json!({"operation":"impact","project":"project","asset":asset,"kinds":["production","composition","publication"]})
}

#[test]
fn agent_describes_request_schema_and_returns_json_errors_without_source_access() {
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-query"))
        .arg("describe")
        .output()
        .unwrap();
    assert!(out.status.success());
    let description: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(description["read_only"], true);
    assert_eq!(
        description["request_schema"]["properties"]["schema"]["const"],
        "tig.query.experimental-1"
    );
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-query"))
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["error"]["code"],
        "invalid_arguments"
    );
    for raw in [br#"{"schema":"a","schema":"b"}"#.as_slice(), b"{}", b"[]"] {
        assert!(Request::parse(raw).is_err());
    }
}

#[test]
fn agent_impact_explains_transitive_render_and_delivery_paths() {
    let f = Fixture::new();
    let request = f.request(impact("cel-a"));
    let (out, result) = f.run(&request, &[]);
    assert!(out.status.success(), "{result}");
    assert_eq!(result["status"], "complete");
    assert_eq!(result["data"]["total_known"], 3);
    assert_eq!(result["data"]["items"][0]["asset"]["id"], "scene");
    let delivery = &result["data"]["items"][2];
    assert_eq!(delivery["asset"]["id"], "delivery");
    assert_eq!(delivery["distance"], 3);
    assert_eq!(delivery["path"].as_array().unwrap().len(), 3);
    assert_eq!(delivery["path"][0]["kind"], "publication");
    assert_eq!(delivery["path"][2]["target"]["id"], "cel-a");
    assert_eq!(result["coverage"]["payloads_opened"], 0);
    assert_eq!(result["authorizes_eviction"], false);
    let text = f.evaluate(&f.request(impact("text-es")));
    assert_eq!(text["data"]["total_known"], 5);
    assert_eq!(text["data"]["items"][0]["asset"]["id"], "text-en");
}

#[test]
fn agent_compare_distinguishes_equal_bytes_metadata_and_slot_changes() {
    let f = Fixture::new();
    let query = json!({"operation":"compare","left":{"project":"project","id":"cel-a"},"right":{"project":"project","id":"cel-b"}});
    let first = f.evaluate(&f.request(query.clone()));
    assert_eq!(first["data"]["selected_equal"], true);
    assert_eq!(first["data"]["record_equal"], false);
    assert_eq!(first["data"]["definition_equal"], true);
    assert_eq!(first["data"]["authored_changed_fields"], json!(["prompt"]));
    let path = f.root().join("records/cel-b.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["selected"] = Value::Null;
    record["streams"] = json!([{"slot":"history","role":"result","group":"retained","object":Object::from_bytes(b"prior")}]);
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    f.advance();
    let result = f.evaluate(&f.request(query));
    assert!(result["data"]["selected_equal"].is_null());
    assert_eq!(
        result["data"]["streams"]["added"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn agent_coverage_gaps_are_partial_not_zero_consumers_and_pin_conflicts_refuse() {
    let f = Fixture::new();
    let path = &f.enrollment;
    let mut config: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    config["projects"][0]["graph_profiles"] = json!([]);
    fs::write(path, serde_json::to_vec(&config).unwrap()).unwrap();
    let (out, result) = f.run(&f.request(impact("cel-a")), &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["status"], "partial");
    assert_eq!(result["coverage"]["complete_declared_scope"], false);
    assert_eq!(result["data"]["unadmitted_definition_count"], 4);
    assert!(result["data"]["gap_count"].as_u64().unwrap() > 0);
    let mut request = f.request(json!({"operation":"find","project":"project","text":"cel"}));
    request["expected_snapshots"] = json!({"project":"0".repeat(40)});
    let (out, result) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(result["error"]["code"], "stale_snapshot");
}

#[test]
fn agent_frozen_old_revision_is_not_a_consumer_of_new_selected_record() {
    let f = Fixture::new();
    let old = Object::from_bytes(&fs::read(f.root().join("records/cel-a.asset")).unwrap());
    f.record(
        "frozen-scene",
        "scene",
        json!([{"id":"cel-a","sha256":old.sha256,"bytes":old.bytes}]),
    );
    let path = f.root().join("records/cel-a.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["authored"]["prompt"] = json!("DO_NOT_EXPORT changed");
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    f.advance();
    let result = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(result["status"], "partial");
    assert_eq!(result["data"]["total_known"], 3);
    assert!(
        !result["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["asset"]["id"] == "frozen-scene")
    );
}

#[test]
fn agent_paging_fences_schema_limits_and_atomic_output_are_explicit() {
    let f = Fixture::new();
    let mut request = f.request(json!({"operation":"find","project":"project","text":"cel"}));
    let result = f.evaluate(&request);
    assert_eq!(result["paging"]["total_known"], 2);
    let commit = result["basis"]["project"]["commit"].clone();
    request["expected_snapshots"] = json!({"project":commit});
    request["offset"] = json!(1);
    let page = f.evaluate(&request);
    assert_eq!(page["paging"]["returned"], 1);
    assert!(page["paging"]["next_offset"].is_null());
    let output = f.temp.path().join("report.json");
    let extra = ["--out", output.to_str().unwrap()];
    let (out, summary) = f.run(&request, &extra);
    assert!(out.status.success());
    assert_eq!(summary["report_written"], true);
    let saved = fs::read(&output).unwrap();
    let (out, error) = f.run(&request, &extra);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "output_refused");
    assert_eq!(fs::read(output).unwrap(), saved);
    for (key, value) in [
        ("offset", json!(1000001)),
        ("extra", json!(true)),
        ("schema", json!("future")),
    ] {
        let mut bad = request.clone();
        bad[key] = value;
        assert!(Request::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    let mut duplicate = f.request(impact("cel-a"));
    duplicate["query"]["kinds"] = json!(["production", "production"]);
    assert!(Request::parse(&serde_json::to_vec(&duplicate).unwrap()).is_err());
}

#[test]
fn agent_diamond_consumers_are_unique_and_requested_edge_kinds_are_explicit() {
    let f = Fixture::new();
    f.record("scene-two", "scene", json!([{"id":"cel-a"}]));
    f.record(
        "render",
        "render",
        json!([{"id":"scene"},{"id":"scene-two"}]),
    );
    f.advance();
    let result = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(result["status"], "complete");
    assert_eq!(result["data"]["total_known"], 4);
    let render = result["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["asset"]["id"] == "render")
        .unwrap();
    assert_eq!(render["distance"], 2);
    let mut production = impact("cel-a");
    production["kinds"] = json!(["production"]);
    let result = f.evaluate(&f.request(production));
    assert_eq!(result["data"]["total_known"], 2);
    let mut publication = impact("cel-a");
    publication["kinds"] = json!(["publication"]);
    let result = f.evaluate(&f.request(publication));
    assert_eq!(result["data"]["total_known"], 0);
    assert_eq!(result["coverage"]["complete_declared_scope"], true);
}

#[test]
fn agent_cycles_and_reverse_depth_limits_return_partial_without_looping() {
    let f = Fixture::new();
    f.record("cel-a", "source", json!([{"id":"cel-b"}]));
    f.record("cel-b", "source", json!([{"id":"cel-a"}]));
    f.advance();
    let result = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(result["status"], "partial");
    assert!(
        result["data"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["reason"] == "Cycle")
    );
    let f = Fixture::new();
    for n in 0..70 {
        let dep = if n == 0 {
            "cel-a".to_string()
        } else {
            format!("chain-{}", n - 1)
        };
        f.record(&format!("chain-{n}"), "scene", json!([{"id":dep}]));
    }
    f.advance();
    let result = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(result["status"], "partial");
    assert_eq!(result["data"]["impact_depth_limited"], true);
    assert!(
        result["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["path"].as_array().unwrap().len() <= 64)
    );
}

#[test]
fn agent_impact_pages_many_consumers_and_rejects_mixed_source_pages() {
    let f = Fixture::new();
    for n in 0..110 {
        f.record(
            &format!("consumer-{n:03}"),
            "scene",
            json!([{"id":"cel-a"}]),
        );
    }
    f.advance();
    let mut request = f.request(impact("cel-a"));
    let first = f.evaluate(&request);
    assert_eq!(first["data"]["total_known"], 113);
    assert_eq!(first["paging"]["returned"], 100);
    assert_eq!(first["paging"]["next_offset"], 100);
    request["expected_snapshots"] = json!({"project":first["basis"]["project"]["commit"]});
    request["offset"] = json!(100);
    let second = f.evaluate(&request);
    assert_eq!(second["paging"]["returned"], 13);
    let first_ids = first["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["asset"]["id"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        second["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| !first_ids.contains(r["asset"]["id"].as_str().unwrap()))
    );
    f.record("new-consumer", "scene", json!([{"id":"cel-a"}]));
    f.advance();
    let (out, error) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_snapshot");
}

#[test]
fn agent_compare_refuses_conflicting_lengths_for_the_same_object_hash() {
    let f = Fixture::new();
    let path = f.root().join("records/cel-b.asset");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let size = record["selected"]["bytes"].as_u64().unwrap();
    record["selected"]["bytes"] = json!(size + 1);
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    f.advance();
    let request=f.request(json!({"operation":"compare","left":{"project":"project","id":"cel-a"},"right":{"project":"project","id":"cel-b"}}));
    let (out, error) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "conflicting_identity");
}

#[test]
fn agent_basis_fences_changed_interpretation_even_when_git_commit_is_unchanged() {
    let f = Fixture::new();
    let mut request = f.request(json!({"operation":"find","project":"project","text":"cel"}));
    let before = f.evaluate(&request);
    request["expected_basis"] = before["basis_sha256"].clone();
    assert_eq!(f.evaluate(&request)["status"], "complete");
    let mut config: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    config["projects"][0]["display"][0]["category"] = json!("changed-category");
    fs::write(&f.enrollment, serde_json::to_vec(&config).unwrap()).unwrap();
    let (out, result) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(result["error"]["code"], "stale_basis");
    request.as_object_mut().unwrap().remove("expected_basis");
    let after = f.evaluate(&request);
    assert_eq!(
        before["basis"]["project"]["commit"],
        after["basis"]["project"]["commit"]
    );
    assert_ne!(before["basis_sha256"], after["basis_sha256"]);
}
