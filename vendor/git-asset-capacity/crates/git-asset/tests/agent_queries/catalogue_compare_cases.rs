use super::catalogue_impact_cases::Server;
use super::*;

fn snapshot(f: &Fixture) -> String {
    let profile: Value =
        serde_json::from_slice(&fs::read(f.temp.path().join("profile.json")).unwrap()).unwrap();
    profile["commit"].as_str().unwrap().into()
}
fn path(f: &Fixture, left: &str, right: &str) -> String {
    let commit = snapshot(f);
    format!(
        "/api/compare?left_project=project&left_id={left}&left_snapshot={commit}&right_project=project&right_id={right}&right_snapshot={commit}"
    )
}
fn edit(f: &Fixture, id: &str, change: impl FnOnce(&mut Value)) {
    let path = f.root().join(format!("records/{id}.asset"));
    let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    change(&mut record);
    fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
}

#[test]
fn catalogue_comparison_matches_agent_and_keeps_equal_bytes_distinct_from_records() {
    let f = Fixture::new();
    let server = Server::new(&f);
    let url = path(&f, "cel-a", "cel-b");
    let (code, report) = server.json(&url);
    assert_eq!(code, 200, "{report}");
    let agent=f.evaluate(&f.request(json!({"operation":"compare","left":{"project":"project","id":"cel-a"},"right":{"project":"project","id":"cel-b"}})));
    assert_eq!(report["data"], agent["data"]);
    assert_eq!(report["basis_sha256"], agent["basis_sha256"]);
    assert_eq!(report["data"]["selected_equal"], true);
    assert_eq!(report["data"]["record_equal"], false);
    assert_eq!(report["data"]["authored_changed_fields"], json!(["prompt"]));
    assert_eq!(report["coverage"]["payloads_opened"], 0);
    assert_eq!(report["authorizes_eviction"], false);
    let (code, _) = server.json(&format!(
        "{url}&basis={}",
        report["basis_sha256"].as_str().unwrap()
    ));
    assert_eq!(code, 200);
    assert_eq!(server.get(&url, false).0, 403);
    assert!(server.get("/", false).1.contains("/compare.js"));
    assert!(
        server
            .get("/compare.js", false)
            .1
            .contains("function drawComparison")
    );
}

#[test]
fn catalogue_comparison_exposes_slot_binding_changes_without_raw_authored_values() {
    let f = Fixture::new();
    let bytes = Object::from_bytes(b"not stored");
    edit(
        &f,
        "cel-a",
        |r| r["streams"] = json!([{"slot":"shared","role":"input","group":"old","object":bytes},{"slot":"removed","role":"input","group":"old","object":bytes}]),
    );
    edit(
        &f,
        "cel-b",
        |r| r["streams"] = json!([{"slot":"shared","role":"result","group":"new","object":bytes},{"slot":"added","role":"input","group":"new","object":bytes}]),
    );
    f.advance();
    let server = Server::new(&f);
    let (_, report) = server.json(&path(&f, "cel-a", "cel-b"));
    let streams = &report["data"]["streams"];
    assert_eq!(streams["added"][0]["slot"], "added");
    assert_eq!(streams["removed"][0]["slot"], "removed");
    assert_eq!(streams["changed"][0]["slot"], "shared");
    assert_eq!(
        streams["changed"][0]["before"]["object"],
        streams["changed"][0]["after"]["object"]
    );
    assert_ne!(
        streams["changed"][0]["before"]["role"],
        streams["changed"][0]["after"]["role"]
    );
    assert!(report["data"].get("authored").is_none());
}

#[test]
fn catalogue_comparison_reports_missing_selection_as_unknown_and_definition_difference_separately()
{
    let f = Fixture::new();
    edit(&f, "cel-a", |r| r["selected"] = Value::Null);
    f.advance();
    let server = Server::new(&f);
    let (_, report) = server.json(&path(&f, "cel-a", "scene"));
    assert!(report["data"]["selected_equal"].is_null());
    assert_eq!(report["data"]["definition_equal"], false);
    let (_, self_compare) = server.json(&path(&f, "cel-a", "cel-a"));
    assert_eq!(self_compare["data"]["record_equal"], true);
    assert!(self_compare["data"]["selected_equal"].is_null());
}

#[test]
fn catalogue_comparison_cross_project_snapshots_are_explicit_and_match_agent_basis() {
    let f = Fixture::new();
    let other = Fixture::new();
    edit(&other, "cel-a", |r| r["authored"]["language"] = json!("en"));
    other.advance();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    let mut enrollment: Value =
        serde_json::from_slice(&fs::read(&other.enrollment).unwrap()).unwrap();
    enrollment["projects"][0]["id"] = json!("other");
    config["projects"]
        .as_array_mut()
        .unwrap()
        .push(enrollment["projects"][0].clone());
    fs::write(&f.enrollment, serde_json::to_vec(&config).unwrap()).unwrap();
    let server = Server::new(&f);
    let url = format!(
        "/api/compare?left_project=project&left_id=cel-a&left_snapshot={}&right_project=other&right_id=cel-a&right_snapshot={}",
        snapshot(&f),
        snapshot(&other)
    );
    let (code, report) = server.json(&url);
    assert_eq!(code, 200, "{report}");
    let agent=f.evaluate(&f.request(json!({"operation":"compare","left":{"project":"project","id":"cel-a"},"right":{"project":"other","id":"cel-a"}})));
    assert_eq!(report["data"], agent["data"]);
    assert_eq!(report["basis_sha256"], agent["basis_sha256"]);
    assert_eq!(report["snapshots"]["project"], snapshot(&f));
    assert_eq!(report["snapshots"]["other"], snapshot(&other));
    assert_eq!(report["data"]["selected_equal"], true);
    assert_eq!(report["data"]["record_equal"], false);
    assert_eq!(
        server
            .json(&url.replace(
                &format!("right_snapshot={}", snapshot(&other)),
                &format!("right_snapshot={}", "0".repeat(40))
            ))
            .0,
        400
    );
}

#[test]
fn catalogue_comparison_refuses_unfenced_or_general_queries_and_conflicting_byte_lengths() {
    let f = Fixture::new();
    let server = Server::new(&f);
    let url = path(&f, "cel-a", "cel-b");
    for invalid in [
        url.replace("left_project=project", "left_project=all"),
        url.replace("right_project=project", "right_project=missing"),
        url.replace("left_id=cel-a", "left_id="),
        url.replace("right_id=cel-b", "right_id=unknown"),
        url.replace(&format!("left_snapshot={}", snapshot(&f)), "left_snapshot="),
        format!("{url}&operation=manifest_references"),
        format!("{url}&offset=1"),
        format!("{url}&basis={}", "0".repeat(64)),
        url.replace("left_id=cel-a", "left_id=%00"),
    ] {
        assert_eq!(server.json(&invalid).0, 400, "{invalid}");
    }
    drop(server);
    edit(&f, "cel-b", |r| r["selected"]["bytes"] = json!(1));
    f.advance();
    let server = Server::new(&f);
    let (code, error) = server.json(&path(&f, "cel-a", "cel-b"));
    assert_eq!(code, 400);
    assert_eq!(error["error"], "conflicting_identity");
}
