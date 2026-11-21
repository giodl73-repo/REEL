use super::*;

fn admission() -> Value {
    json!({"operation":"admission","project":"project","kinds":["production","composition","publication"]})
}
fn edit_enrollment(f: &Fixture, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    edit(&mut value);
    fs::write(&f.enrollment, serde_json::to_vec(&value).unwrap()).unwrap();
}

#[test]
fn inspector_reuses_exact_pins_and_admitted_projection_without_raw_authored_maps() {
    let f = Fixture::new();
    let request = f.request(json!({"operation":"inspect","project":"project","asset":"cel-a"}));
    let (out, result) = f.run(&request, &[]);
    assert!(out.status.success());
    assert_eq!(result["operation"], "inspect");
    assert_eq!(result["data"]["record"]["id"], "cel-a");
    assert_eq!(result["data"]["definition"]["id"], "type/source");
    assert!(result["data"].get("authored").is_none());
    assert_eq!(result["data"]["payload_verified"], false);
    let mut request = request;
    request["offset"] = json!(1);
    assert!(Request::parse(&serde_json::to_vec(&request).unwrap()).is_err());
}

#[test]
fn owner_inspector_fields_are_explicit_and_changing_them_invalidates_basis() {
    let f = Fixture::new();
    let mut request = f.request(json!({"operation":"inspect","project":"project","asset":"cel-a"}));
    let before = f.evaluate(&request);
    assert_eq!(before["data"]["fields"], json!([]));
    edit_enrollment(
        &f,
        |c| {
            c["projects"][0]["fields"] =
                json!([{"label":"Lifecycle","section":"source","pointer":"/status/lifecycle"}])
        },
    );
    request["expected_basis"] = before["basis_sha256"].clone();
    let (out, error) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_basis");
    request.as_object_mut().unwrap().remove("expected_basis");
    let (out, after) = f.run(&request, &[]);
    assert!(out.status.success());
    assert_eq!(after["data"]["fields"][0]["value"], "draft");
    assert_eq!(
        before["basis"]["project"]["commit"],
        after["basis"]["project"]["commit"]
    );
}

#[test]
fn admission_reports_exact_contracts_and_reuses_graph_admission_fingerprint() {
    let f = Fixture::new();
    let request = f.request(admission());
    let (out, result) = f.run(&request, &[]);
    assert!(out.status.success(), "{result}");
    assert_eq!(result["data"]["total"], 4);
    assert_eq!(result["data"]["definitions_with_requested_coverage"], 4);
    assert_eq!(result["data"]["instance_edges_evaluated"], false);
    let source = result["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["definition"]["id"] == "type/source")
        .unwrap();
    assert_eq!(source["registered_records"], 5);
    assert_eq!(source["admitted"], true);
    assert_eq!(source["class"], "primary");
    assert_eq!(source["rules"][0]["pointer"], "/inputs");
    assert_eq!(source["contract"]["required"], json!(["inputs"]));
    assert_eq!(source["examples"].as_array().unwrap().len(), 3);
    let graph = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(
        result["data"]["profile_sets"]["project"]["sha256"],
        graph["data"]["admission_sha256"]
    );
}

#[test]
fn missing_and_uncovered_profiles_remain_partial_with_explicit_next_steps() {
    let f = Fixture::new();
    edit_enrollment(&f, |c| c["projects"][0]["graph_profiles"] = json!([]));
    let (out, result) = f.run(&f.request(admission()), &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["data"]["total"], 4);
    assert!(result["data"]["items"].as_array().unwrap().iter().all(|i|i["admitted"]==false&&i["next_action"]=="admit_exact_definition_profile"));
    let f = Fixture::new();
    edit_enrollment(&f, |c| {
        c["projects"][0]["graph_profiles"][0]["covered"] = json!(["production"])
    });
    let result = f.evaluate(&f.request(admission()));
    assert_eq!(result["status"], "partial");
    let source = result["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["definition"]["id"] == "type/source")
        .unwrap();
    assert_eq!(source["admitted"], true);
    assert_eq!(source["uncovered_kinds"].as_array().unwrap().len(), 2);
    assert_eq!(source["next_action"], "review_requested_kind_coverage");
}

#[test]
fn invalid_profile_sets_are_diagnosed_without_accepting_matching_names() {
    let f = Fixture::new();
    edit_enrollment(&f, |c| {
        let duplicate = c["projects"][0]["graph_profiles"][0].clone();
        c["projects"][0]["graph_profiles"]
            .as_array_mut()
            .unwrap()
            .push(duplicate);
    });
    let result = f.evaluate(&f.request(admission()));
    assert_eq!(result["status"], "partial");
    assert_eq!(result["data"]["invalid_profile_sets"], 1);
    assert_eq!(result["data"]["profile_sets"]["project"]["valid"], false);
    assert!(
        result["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["next_action"] == "fix_invalid_profile_set")
    );
}

fn add_revision(f: &Fixture, n: usize) {
    let mut definition: Value =
        serde_json::from_slice(&fs::read(f.root().join("definitions/source.json")).unwrap())
            .unwrap();
    definition["authoring"]["defaults"]["prompt"] = json!(format!("DO_NOT_EXPORT default {n}"));
    let pin = write(
        &f.root(),
        &format!("definitions/source-{n}.json"),
        &definition,
    );
    let mut record: Value =
        serde_json::from_slice(&fs::read(f.root().join("records/cel-a.asset")).unwrap()).unwrap();
    record["id"] = json!(format!("revision-{n}"));
    record["definition"] = json!({"id":"type/source","sha256":pin.sha256,"bytes":pin.bytes});
    write(&f.root(), &format!("records/revision-{n}.asset"), &record);
}

#[test]
fn same_definition_id_keeps_revisions_distinct_and_never_exports_defaults() {
    let f = Fixture::new();
    add_revision(&f, 0);
    f.advance();
    let mut query = admission();
    query["definition"] = json!("type/source");
    let (out, result) = f.run(&f.request(query), &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["data"]["total"], 2);
    let rows = result["data"]["items"].as_array().unwrap();
    assert_ne!(
        rows[0]["definition"]["object"],
        rows[1]["definition"]["object"]
    );
    assert_eq!(rows.iter().filter(|r| r["admitted"] == true).count(), 1);
    assert!(rows.iter().all(|r| r["contract"].get("defaults").is_none()));
}

#[test]
fn admission_pages_distinct_revisions_and_refuses_changed_rules() {
    let f = Fixture::new();
    for n in 0..101 {
        add_revision(&f, n);
    }
    f.advance();
    let mut query = admission();
    query["definition"] = json!("type/source");
    let mut request = f.request(query);
    let first = f.evaluate(&request);
    assert_eq!(first["data"]["total"], 102);
    assert_eq!(first["paging"]["next_offset"], 100);
    request["offset"] = json!(100);
    request["expected_basis"] = first["basis_sha256"].clone();
    let second = f.evaluate(&request);
    assert_eq!(second["paging"]["returned"], 2);
    assert!(second["paging"]["next_offset"].is_null());
    edit_enrollment(&f, |c| c["projects"][0]["graph_profiles"] = json!([]));
    let (out, error) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_basis");
}
