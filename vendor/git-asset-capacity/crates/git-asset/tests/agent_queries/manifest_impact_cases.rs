use super::manifest_cases::{Manifest, edit, leaf};
use super::*;
use git_asset_store::existing::Layout;

fn fixture() -> Manifest {
    Manifest::new(
        Layout::Flat,
        json!({"schema":"fixture.bundle.v1","assets":{"leaf":leaf()}}),
    )
}
fn request(m: &Manifest) -> Value {
    let mut q = m.request();
    q["query"]["operation"] = json!("manifest_impact");
    q["query"]["kinds"] = json!(["production", "composition", "publication"]);
    q
}
fn ids(report: &Value) -> Vec<&str> {
    report["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["asset"]["id"].as_str().unwrap())
        .collect()
}

#[test]
fn associations_join_native_consumers_with_exact_roots_and_reusable_content_fences() {
    let m = fixture();
    let q = request(&m);
    let (out, result) = m.f.run(&q, &[]);
    assert!(out.status.success(), "{result}");
    assert_eq!(ids(&result), ["render", "delivery"]);
    assert_eq!(result["data"]["targets"][0]["id"], "scene");
    assert_eq!(result["data"]["metadata_read_attempts"], 1);
    assert_eq!(result["data"]["items"][1]["distance"], 2);
    assert_eq!(
        result["data"]["items"][1]["target"],
        result["data"]["targets"][0]
    );
    assert_eq!(result["data"]["complete_native_coverage"], true);
    assert_eq!(result["data"]["complete_manifest_extraction"], true);
    assert_eq!(result["coverage"]["media_leaves_opened"], 0);
    assert!(result["coverage"]["payloads_opened"].is_null());
    assert_eq!(result["authorizes_eviction"], false);
    let mut provenance = m.request();
    provenance["query"]["expected_content"] = result["data"]["content_sha256"].clone();
    let refs = m.f.evaluate(&provenance);
    assert_eq!(
        refs["data"]["items"][0]["reference"]["pointer"],
        "/assets/leaf"
    );
    assert_eq!(
        refs["data"]["content_sha256"],
        result["data"]["content_sha256"]
    );
    assert!(!m.layout.object_path(&m.cache, &leaf().sha256).exists());
    let mut production = q;
    production["query"]["kinds"] = json!(["production"]);
    let limited = m.f.evaluate(&production);
    assert_eq!(limited["data"]["total_known"], 0);
    assert_eq!(limited["data"]["targets"].as_array().unwrap().len(), 1);
}

#[test]
fn missing_manifest_or_graph_coverage_stays_partial_instead_of_proving_no_use() {
    for missing_manifest in [true, false] {
        let m = fixture();
        if missing_manifest {
            fs::remove_file(m.layout.object_path(&m.cache, &m.root.sha256)).unwrap();
        } else {
            edit(&m.f, |c| c["projects"][0]["graph_profiles"] = json!([]));
        }
        let (out, result) = m.f.run(&request(&m), &[]);
        assert_eq!(out.status.code(), Some(3), "{result}");
        assert_eq!(result["data"]["total_known"], 0);
        assert_eq!(
            result["data"]["complete_manifest_extraction"],
            !missing_manifest
        );
        assert_eq!(result["data"]["complete_native_coverage"], missing_manifest);
        assert_eq!(result["coverage"]["complete_declared_scope"], false);
        assert_eq!(result["coverage"]["complete_project_scope"], false);
    }
}

#[test]
fn frozen_old_holder_pins_are_distinct_from_selected_manifest_owners() {
    let m = fixture();
    let path = m.f.root().join("records/scene.asset");
    let old = Object::from_bytes(&fs::read(&path).unwrap());
    m.f.record(
        "render-frozen",
        "render",
        json!([{"id":"scene","sha256":old.sha256,"bytes":old.bytes}]),
    );
    let mut scene: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    scene["authored"]["language"] = json!("revised");
    fs::write(&path, serde_json::to_vec(&scene).unwrap()).unwrap();
    m.f.advance();
    let result = m.f.evaluate(&request(&m));
    assert_eq!(result["status"], "partial");
    assert_eq!(result["data"]["complete_native_coverage"], false);
    assert!(
        result["data"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["at"]["object"] == json!(old) && g["reason"] == "Missing")
    );
    assert_eq!(ids(&result), ["render", "delivery"]);
    assert_ne!(
        result["data"]["targets"][0]["object"]["sha256"],
        json!(old.sha256)
    );
}

#[test]
fn all_associations_seed_one_graph_before_paging_and_shared_consumers_are_deduplicated() {
    let assets: serde_json::Map<_, _> = (0..110)
        .map(|n| (format!("a{n:03}"), json!(leaf())))
        .collect();
    let m = Manifest::new(
        Layout::GitLfs,
        json!({"schema":"fixture.bundle.v1","assets":assets}),
    );
    m.f.record("scene-z", "scene", json!([]));
    let pin = m.put(&json!({"schema":"fixture.bundle.v1","assets":{"other":leaf()}}));
    let path = m.f.root().join("records/scene-z.asset");
    let mut other: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    other["selected"] = json!(pin);
    fs::write(path, serde_json::to_vec(&other).unwrap()).unwrap();
    m.f.record("render-z", "render", json!([{"id":"scene-z"}]));
    m.f.record(
        "shared-delivery",
        "delivery",
        json!([{"id":"render"},{"id":"render-z"}]),
    );
    m.f.advance();
    edit(&m.f, |c| {
        c["projects"][0]["collections"][0]["roots"][0]["pattern"] = json!("^scene.*$");
        c["projects"][0]["storage"]["nested_content"]["adapters"][0]["roots"][0]["asset_pattern"] =
            json!("^scene.*$");
    });
    let q = request(&m);
    let result = m.f.evaluate(&q);
    assert_eq!(result["data"]["manifest"]["matching_associations"], 111);
    assert_eq!(result["data"]["targets"].as_array().unwrap().len(), 2);
    assert_eq!(result["data"]["total_known"], 4);
    assert_eq!(
        ids(&result)
            .iter()
            .filter(|s| **s == "shared-delivery")
            .count(),
        1
    );
    assert_eq!(result, m.f.evaluate(&q));
}

#[test]
fn consumer_pages_require_both_fences_and_reject_source_or_availability_drift() {
    let m = fixture();
    for n in 0..120 {
        m.f.record(&format!("render-{n:03}"), "render", json!([{"id":"scene"}]));
    }
    m.f.advance();
    let mut q = request(&m);
    let first = m.f.evaluate(&q);
    assert_eq!(first["data"]["total_known"], 122);
    assert_eq!(first["data"]["items"].as_array().unwrap().len(), 100);
    q["offset"] = json!(100);
    assert!(Request::parse(&serde_json::to_vec(&q).unwrap()).is_err());
    q["query"]["expected_content"] = first["data"]["content_sha256"].clone();
    let second = m.f.evaluate(&q);
    assert_eq!(second["data"]["items"].as_array().unwrap().len(), 22);
    assert!(ids(&first).iter().all(|id| !ids(&second).contains(id)));
    fs::remove_file(m.layout.object_path(&m.cache, &m.root.sha256)).unwrap();
    let (out, error) = m.f.run(&q, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_content");
    edit(&m.f, |c| c["projects"][0]["graph_profiles"] = json!([]));
    let (_, error) = m.f.run(&q, &[]);
    assert_eq!(error["error"]["code"], "stale_basis");
}

#[test]
fn retained_manifest_bindings_are_evidence_and_never_implicit_production_edges() {
    let m = fixture();
    let path = m.f.root().join("records/scene.asset");
    let mut scene: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    scene["streams"] =
        json!([{"slot":"previous","role":"inventory","group":"history","object":m.root}]);
    scene["selected"] = json!(Object::from_bytes(b"unrelated selection not read"));
    fs::write(path, serde_json::to_vec(&scene).unwrap()).unwrap();
    m.f.advance();
    edit(&m.f, |c| {
        c["projects"][0]["storage"]["nested_content"]["adapters"][0]["roots"][0]["slot_pattern"] =
            json!("^previous$")
    });
    let result = m.f.evaluate(&request(&m));
    assert_eq!(ids(&result), ["render", "delivery"]);
    assert_eq!(
        result["data"]["conditional_on"],
        "A manifest-bearing selected record changes"
    );
    for row in result["data"]["items"].as_array().unwrap() {
        assert!(
            row["path"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["kind"] != "production")
        );
    }
    let refs = m.f.evaluate(&m.request());
    assert_eq!(
        refs["data"]["items"][0]["reference"]["root_slot"],
        "previous"
    );
    assert_eq!(result["authorizes_eviction"], false);
}

#[test]
fn manifest_impact_requires_explicit_kinds_scope_and_fresh_basis() {
    let m = fixture();
    for invalid in [
        "basis",
        "missing_kinds",
        "empty_kinds",
        "unsupported_kind",
        "all_projects",
    ] {
        let mut q = request(&m);
        match invalid {
            "basis" => {
                q.as_object_mut().unwrap().remove("expected_basis");
            }
            "missing_kinds" => {
                q["query"].as_object_mut().unwrap().remove("kinds");
            }
            "empty_kinds" => q["query"]["kinds"] = json!([]),
            "unsupported_kind" => q["query"]["kinds"] = json!(["inferred"]),
            _ => q["query"]["project"] = json!("all"),
        }
        assert!(
            Request::parse(&serde_json::to_vec(&q).unwrap()).is_err(),
            "{invalid}"
        );
    }
}
