use super::*;

fn profiles(f: &Fixture) -> Value {
    serde_json::from_slice::<Value>(&fs::read(&f.enrollment).unwrap()).unwrap()["projects"][0]["graph_profiles"].clone()
}
fn replace_profiles(f: &Fixture, profiles: Value) {
    let mut c: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    c["projects"][0]["graph_profiles"] = profiles;
    fs::write(&f.enrollment, serde_json::to_vec(&c).unwrap()).unwrap();
}
fn preview(f: &Fixture, profiles: Value) -> Value {
    let basis=f.evaluate(&f.request(json!({"operation":"find","project":"project","text":""})))["basis_sha256"].clone();
    let mut request=f.request(json!({"operation":"preview_admission","project":"project","asset":"cel-a","kinds":["production","composition","publication"],"profiles":profiles}));
    request["expected_basis"] = basis;
    request
}

#[test]
fn preview_identical_profiles_has_no_changes_and_never_mutates_the_catalogue() {
    let f = Fixture::new();
    let proposal = profiles(&f);
    let bytes = fs::read(&f.enrollment).unwrap();
    let request = preview(&f, proposal);
    let mut catalogue =
        git_asset::catalogue::Catalogue::load(Config::load(&f.enrollment).unwrap()).unwrap();
    let before = catalogue
        .agent_query(
            &Request::parse(&serde_json::to_vec(&f.request(impact("cel-a"))).unwrap()).unwrap(),
        )
        .unwrap();
    let result = catalogue
        .agent_query(&Request::parse(&serde_json::to_vec(&request).unwrap()).unwrap())
        .unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["data"]["total"], 0);
    assert_eq!(result["data"]["still_known"], 3);
    assert_eq!(result["data"]["enrollment_changed"], false);
    assert_eq!(result["data"]["admission_authorized"], false);
    let after = catalogue
        .agent_query(
            &Request::parse(&serde_json::to_vec(&f.request(impact("cel-a"))).unwrap()).unwrap(),
        )
        .unwrap();
    assert_eq!(before, after);
    assert_eq!(fs::read(&f.enrollment).unwrap(), bytes);
}

#[test]
fn preview_missing_to_complete_exposes_new_knowledge_without_hiding_baseline_gaps() {
    let f = Fixture::new();
    let proposal = profiles(&f);
    replace_profiles(&f, json!([]));
    let bytes = fs::read(&f.enrollment).unwrap();
    let request = preview(&f, proposal);
    let (out, result) = f.run(&request, &[]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(result["status"], "partial");
    assert_eq!(
        result["data"]["baseline"]["complete_declared_coverage"],
        false
    );
    assert_eq!(
        result["data"]["candidate"]["complete_declared_coverage"],
        true
    );
    assert_eq!(result["data"]["newly_known"], 3);
    assert_eq!(
        result["data"]["candidate"]["items"][2]["path"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(fs::read(&f.enrollment).unwrap(), bytes);
    let regular = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(regular["data"]["total_known"], 0);
    assert_eq!(regular["status"], "partial");
}

#[test]
fn preview_reachability_disappearance_never_authorizes_retirement() {
    let f = Fixture::new();
    let mut proposal = profiles(&f);
    proposal[1]["rules"][0]["pointer"] = json!("/unused");
    proposal[1]["rules"][0]["optional"] = json!(true);
    let mut catalogue =
        git_asset::catalogue::Catalogue::load(Config::load(&f.enrollment).unwrap()).unwrap();
    let original =
        Request::parse(&serde_json::to_vec(&f.request(impact("cel-a"))).unwrap()).unwrap();
    let before = catalogue.agent_query(&original).unwrap();
    let request = Request::parse(&serde_json::to_vec(&preview(&f, proposal)).unwrap()).unwrap();
    let result = catalogue.agent_query(&request).unwrap();
    assert_eq!(result["status"], "complete");
    assert_eq!(result["data"]["no_longer_known"], 3);
    assert_eq!(result["data"]["candidate"]["total_known"], 0);
    assert_eq!(result["authorizes_eviction"], false);
    assert_eq!(before, catalogue.agent_query(&original).unwrap());
    let partial = f.evaluate(&preview(&f, json!([])));
    assert_eq!(partial["status"], "partial");
    assert_eq!(partial["data"]["no_longer_known"], 3);
    assert_eq!(
        partial["data"]["candidate"]["complete_declared_coverage"],
        false
    );
}

#[test]
fn preview_changed_edge_kinds_change_witnesses_even_with_the_same_consumers() {
    let f = Fixture::new();
    let mut proposal = profiles(&f);
    proposal[1]["rules"][0]["kind"] = json!("composition");
    let result = f.evaluate(&preview(&f, proposal));
    assert_eq!(result["status"], "complete");
    assert_eq!(result["data"]["newly_known"], 0);
    assert_eq!(result["data"]["no_longer_known"], 0);
    assert_eq!(result["data"]["witness_changed"], 3);
    assert!(
        result["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["change"] == "witness_changed"
                && r["before_witness_sha256"] != r["after_witness_sha256"])
    );
}

#[test]
fn preview_repairs_invalid_baseline_and_refuses_invalid_candidates_without_mutation() {
    let f = Fixture::new();
    let proposal = profiles(&f);
    let mut invalid = proposal.clone();
    invalid.as_array_mut().unwrap().push(proposal[0].clone());
    replace_profiles(&f, invalid);
    let bytes = fs::read(&f.enrollment).unwrap();
    let request = preview(&f, proposal.clone());
    let result = f.evaluate(&request);
    assert_eq!(result["status"], "partial");
    assert_eq!(
        result["data"]["baseline"]["unavailable"],
        "invalid_profile_set"
    );
    assert_eq!(
        result["data"]["candidate"]["complete_declared_coverage"],
        true
    );
    let mut bad = proposal;
    bad[0]["rules"][0]["pointer"] = json!("invalid");
    let (out, error) = f.run(&preview(&f, bad), &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "invalid_proposal");
    assert_eq!(fs::read(&f.enrollment).unwrap(), bytes);
}

#[test]
fn preview_requires_current_basis_and_a_typed_profile_contract() {
    let f = Fixture::new();
    let request = preview(&f, profiles(&f));
    let mut missing = request.clone();
    missing.as_object_mut().unwrap().remove("expected_basis");
    assert!(Request::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    let mut wrong = request.clone();
    wrong["expected_basis"] = json!("0".repeat(64));
    let (out, error) = f.run(&wrong, &[]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(error["error"]["code"], "stale_basis");
    let mut wrong = request;
    wrong["query"]["profiles"][0]["extra"] = json!(true);
    assert!(Request::parse(&serde_json::to_vec(&wrong).unwrap()).is_err());
}

#[test]
fn preview_delta_paging_counts_all_consumers_beyond_the_witness_sample() {
    let f = Fixture::new();
    for n in 0..110 {
        f.record(
            &format!("consumer-{n:03}"),
            "scene",
            json!([{"id":"cel-a"}]),
        );
    }
    f.advance();
    let proposal = profiles(&f);
    replace_profiles(&f, json!([]));
    let mut request = preview(&f, proposal);
    let first = f.evaluate(&request);
    assert_eq!(first["data"]["newly_known"], 113);
    assert_eq!(first["paging"]["returned"], 100);
    assert_eq!(first["paging"]["next_offset"], 100);
    assert_eq!(
        first["data"]["candidate"]["items"]
            .as_array()
            .unwrap()
            .len(),
        100
    );
    request["offset"] = json!(100);
    let second = f.evaluate(&request);
    assert_eq!(second["paging"]["returned"], 13);
    assert!(second["paging"]["next_offset"].is_null());
    assert_eq!(second["data"]["impact_details_first_page_only"], true);
}
