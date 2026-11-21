#![cfg(feature = "schema")]
use git_asset::catalogue::{Catalogue, Config, storage::objects::Query};
use git_asset_core::Object;
use serde_json::Value;
use std::{fs, process::Command};
#[path = "common/catalogue_server.rs"]
mod catalogue_server;

#[test]
fn joined_return_acquires_backup_then_restores_working_and_preserves_expired_retry() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("joined return restore");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-return-restore")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 11);
    assert_eq!(result["complete_course"], false);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-6");
    assert_eq!(
        book["lessons"][9]["restoration"]["phase"],
        "backup_acquired"
    );
    assert_eq!(
        book["lessons"][10]["restoration"]["phase"],
        "working_restored"
    );
    assert_eq!(book["lessons"][9]["restoration"]["working_stream_bytes"], 0);
    assert_eq!(
        book["lessons"][10]["restoration"]["working_stream_bytes"],
        29
    );
    assert_eq!(
        book["lessons"][10]["restoration"]["old_retry"]["new_removed_bytes"],
        0
    );
    assert_eq!(
        book["lessons"][10]["restoration"]["old_retry"]["historical_removed_bytes"],
        58
    );
    render_book(&book);
    drop(server);
    for (text, roles) in [
        (
            b"Measurement series: 2,4,8,16\n".as_slice(),
            vec!["working", "backup"],
        ),
        (
            b"Measurement series: 2,4,8,32\n".as_slice(),
            vec!["history", "backup"],
        ),
        (
            b"Shared reference grid v1\n".as_slice(),
            vec!["working", "backup"],
        ),
    ] {
        let object = Object::from_bytes(text);
        for role in ["working", "history", "backup"] {
            let path = git_asset_store::existing::Layout::Flat
                .object_path(&root.join(role), &object.sha256);
            if roles.contains(&role) {
                assert_eq!(fs::read(path).unwrap(), text);
            } else {
                assert!(!path.exists());
            }
        }
    }
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let acquired: Value = serde_json::from_slice(
        &fs::read(root.join("reports/returned-backup-acquired.json")).unwrap(),
    )
    .unwrap();
    let restored: Value = serde_json::from_slice(
        &fs::read(root.join("reports/returned-working-restored.json")).unwrap(),
    )
    .unwrap();
    for bad in [
        "consent",
        "clock",
        "seed",
        "seed_attempt",
        "guard",
        "retention_flag",
        "authority",
        "phase",
        "parent_ref",
        "missing_ref",
        "previous_ref",
        "source",
        "source_sequence",
        "source_hash",
        "seed_jump",
        "time_drift",
        "providers",
        "parent_action",
        "birth",
        "publication",
        "ack",
        "stream",
        "charge",
        "allocation",
        "grant_capacity",
        "enrollment",
        "before",
        "after",
        "sibling",
        "unknown_role",
        "status",
        "acq_apply",
        "acq_replay",
        "restore_apply",
        "restore_status",
        "effect",
        "retry_new",
        "retry_history",
        "retry_count",
        "retry_authority",
        "retry_state",
        "retry_floor",
        "retry_capacity",
        "retry_ledger_order",
        "retry_time",
    ] {
        let mut a = acquired.clone();
        let mut r = restored.clone();
        let foreign =
            serde_json::to_value(git_asset_core::Digest::blob(b"foreign joined proof")).unwrap();
        match bad {
            "consent" => a["synthetic_owner_consent"] = false.into(),
            "clock" => a["simulated_clock"] = false.into(),
            "seed" => a["seed_disabled"] = false.into(),
            "seed_attempt" => a["seed_attempts_after"] = 999.into(),
            "guard" => a["history_final_guard"] = false.into(),
            "retention_flag" => a["retention_verified_at_capture"] = false.into(),
            "authority" => a["authorizes_storage"] = true.into(),
            "phase" => a["phase"] = "working_restored".into(),
            "parent_ref" => a["final_parent_report"]["sha256"] = foreign,
            "missing_ref" => a["missing_report"]["sha256"] = foreign,
            "previous_ref" => a["previous_report"]["sha256"] = foreign,
            "source" => a["source_commit"] = serde_json::json!("0".repeat(40)),
            "source_sequence" => a["source_checkpoint"]["sequence"] = 999.into(),
            "source_hash" => a["source_checkpoint"]["hash"] = foreign,
            "seed_jump" => {
                r["seed_attempts_before"] = 999.into();
                r["seed_attempts_after"] = 999.into();
            }
            "time_drift" => r["acquisition_time"] = 99.into(),
            "providers" => a["providers"].as_array_mut().unwrap().reverse(),
            "parent_action" => {
                a["acquisition"]["plan"]["parent"]["request"]["publication"]["operation"] =
                    "foreign-parent".into()
            }
            "birth" => a["acquisition"]["plan"]["birth"]["index"] = 0.into(),
            "publication" => {
                a["acquisition"]["publication"]["request"]["operation"] =
                    "foreign-publication".into()
            }
            "ack" => a["acquisition"]["publication"]["ack"] = "provider_durable".into(),
            "stream" => a["acquisition"]["publication"]["streamed_bytes"] = 0.into(),
            "charge" => a["acquisition"]["publication"]["charged_footprint"] = 0.into(),
            "allocation" => a["acquisition"]["publication"]["allocation"]["genesis"] = foreign,
            "grant_capacity" => {
                a["acquisition"]["retention"]["record"]["allocation"]["genesis"] = foreign;
                let bytes = git_asset_core::identity::canonical_json(
                    &a["acquisition"]["retention"]["record"],
                )
                .unwrap();
                a["acquisition"]["retention"]["sha256"] =
                    serde_json::to_value(git_asset_core::Digest::blob(&bytes)).unwrap();
            }
            "enrollment" => {
                a["placement_plan"]["enrollment"] = foreign;
                let p: git_asset_store::placement::Plan =
                    serde_json::from_value(a["placement_plan"].clone()).unwrap();
                a["placement_status"]["plan"] = serde_json::to_value(p.id().unwrap()).unwrap();
            }
            "before" => {
                a["before"]["backup"]["original"] =
                    a["acquisition"]["publication"]["request"]["object"].clone()
            }
            "after" => {
                a["after"]["backup"]
                    .as_object_mut()
                    .unwrap()
                    .remove("original");
            }
            "sibling" => {
                a["after"]["backup"]
                    .as_object_mut()
                    .unwrap()
                    .remove("shared");
            }
            "unknown_role" => a["after"]["unknown"] = serde_json::json!({}),
            "status" => a["placement_status"]["page_converged"] = true.into(),
            "acq_apply" => a["placement_apply"] = r["placement_apply"].clone(),
            "acq_replay" => a["replay"] = r["replay"].clone(),
            "restore_apply" => r["placement_apply"] = Value::Null,
            "restore_status" => r["placement_status"]["storage_converged"] = false.into(),
            "effect" => r["placement_apply"]["effects"][0]["provider"] = r["providers"][1].clone(),
            "retry_new" => r["replay"]["new_removed_bytes"] = 1.into(),
            "retry_history" => r["replay"]["historical_removed_bytes"] = 0.into(),
            "retry_count" => r["replay"]["selected_roots"] = 0.into(),
            "retry_authority" => r["replay"]["removal_authority"] = true.into(),
            "retry_state" => r["after_replay"]["source"]["sequence"] = 999.into(),
            "retry_floor" => {
                r["before_replay"]["witness"]["sequence"] = 0.into();
                r["after_replay"]["witness"]["sequence"] = 0.into();
            }
            "retry_capacity" => {
                r["before_replay"]["witness"]["genesis"] = foreign.clone();
                r["after_replay"]["witness"]["genesis"] = foreign;
            }
            "retry_ledger_order" => {
                r["before_replay"]["ledgers"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
                r["after_replay"]["ledgers"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            }
            "retry_time" => r["retry_time"] = 100.into(),
            _ => unreachable!(),
        }
        r["acquisition"] = a["acquisition"].clone();
        let mut changed = trace.clone();
        let object = save_evidence(&root, &a);
        changed["completed"][9]["report"] = object.clone();
        r["previous_report"] = object;
        changed["completed"][10]["report"] = save_evidence(&root, &r);
        let mut config = config.clone();
        config["teaching"]["trace"] = save_evidence(&root, &changed);
        let path = root.join("negative-joined.json");
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    for bad in ["missing_final", "out_of_order"] {
        let mut changed = trace.clone();
        if bad == "missing_final" {
            changed["completed"].as_array_mut().unwrap().pop();
        } else {
            changed["completed"].as_array_mut().unwrap().swap(9, 10);
        }
        let mut config = config.clone();
        config["teaching"]["trace"] = save_evidence(&root, &changed);
        let path = root.join("negative-joined-order.json");
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    let raw = fs::read(root.join("returned-old-retry.json")).unwrap();
    let replay: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(replay["new_logical_copy_bytes_removed"], 0);
    assert_eq!(replay["historical_deleted_copy_bytes"], 58);
    assert_eq!(
        result["joined_restoration_evidence"]["old_retry"],
        serde_json::to_value(Object::from_bytes(&raw)).unwrap()
    );
}

#[test]
fn git_return_after_final_loss_selects_references_but_keeps_missing_bytes_held() {
    use git_asset::catalogue::teaching;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("Git return after final loss");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-git-return")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 9);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("git-return.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["git_return_evidence"]
    );
    let evidence: Value = serde_json::from_slice(&raw).unwrap();
    for field in ["first_commit", "returned_commit"] {
        let commit = evidence["git_return"][field].as_str().unwrap();
        let actual = Command::new("git")
            .arg("-C")
            .arg(root.join("project"))
            .arg("show")
            .arg(format!("{commit}:records/series.asset"))
            .output()
            .unwrap();
        assert!(actual.status.success());
        assert_eq!(
            serde_json::to_value(Object::from_bytes(&actual.stdout)).unwrap(),
            evidence["git_return"]["first_record"]
        );
    }
    assert_eq!(
        evidence["git_return"]["first_record"],
        evidence["git_return"]["returned_record"]
    );
    assert_ne!(
        evidence["git_return"]["first_commit"],
        evidence["source_commit"]
    );
    assert_eq!(evidence["git_return"]["seed_disabled"], true);
    assert_eq!(
        evidence["git_return"]["seed_attempts_after"]
            .as_u64()
            .unwrap()
            - evidence["git_return"]["seed_attempts_before"]
                .as_u64()
                .unwrap(),
        1
    );
    assert_eq!(evidence["receipt"]["stream_bytes"], 29);
    assert_eq!(evidence["receipt"]["storage_converged"], false);
    let original = Object::from_bytes(b"Measurement series: 2,4,8,16\n");
    let fixed = Object::from_bytes(b"Measurement series: 2,4,8,32\n");
    let shared = Object::from_bytes(b"Shared reference grid v1\n");
    for (name, object, bytes, roles) in [
        (
            "original",
            &original,
            b"Measurement series: 2,4,8,16\n".as_slice(),
            vec![],
        ),
        (
            "fixed",
            &fixed,
            b"Measurement series: 2,4,8,32\n".as_slice(),
            vec!["history", "backup"],
        ),
        (
            "shared",
            &shared,
            b"Shared reference grid v1\n".as_slice(),
            vec!["working", "backup"],
        ),
    ] {
        for role in ["working", "history", "backup"] {
            let path = git_asset_store::existing::Layout::Flat
                .object_path(&root.join(role), &object.sha256);
            if roles.contains(&role) {
                assert_eq!(fs::read(path).unwrap(), bytes, "{name}/{role}");
            } else {
                assert!(!path.exists(), "unexpected {name}/{role}");
            }
        }
    }
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let catalogue = Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-4");
    let chapter = &book["lessons"][8];
    assert_eq!(chapter["source_commit"], evidence["source_commit"]);
    assert!(
        chapter["holds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h == "final_retirement")
    );
    let row = chapter["objects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "original")
        .unwrap();
    assert_eq!(row["observed"], serde_json::json!([]));
    assert_eq!(row["unknown"], serde_json::json!([]));
    assert!(
        row["desired"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "working")
    );
    render_book(&book);
    drop(server);
    assert_eq!(
        teaching::execute(catalogue.teaching_request().unwrap()).unwrap(),
        book
    );
    for bad in [
        "commit",
        "prior",
        "record",
        "seed",
        "seed_count",
        "enrollment",
        "stream",
        "source",
        "source_floor",
        "snapshot",
        "source_pending",
        "lost",
        "before",
        "after",
        "copies",
        "unknown",
        "converged",
        "status",
        "holds",
        "state",
        "effects",
        "provider",
        "authority",
    ] {
        let mut changed = evidence.clone();
        let foreign =
            serde_json::to_value(git_asset_core::Digest::blob(b"foreign returned source")).unwrap();
        match bad {
            "commit" => changed["source_commit"] = changed["git_return"]["first_commit"].clone(),
            "prior" => {
                changed["git_return"]["prior_commit"] =
                    changed["git_return"]["first_commit"].clone()
            }
            "record" => {
                changed["git_return"]["returned_record"] =
                    serde_json::to_value(Object::from_bytes(b"wrong record")).unwrap()
            }
            "seed" => changed["git_return"]["seed_disabled"] = false.into(),
            "seed_count" => changed["git_return"]["seed_attempts_after"] = 0.into(),
            "source" => changed["source_checkpoint"]["genesis"] = foreign,
            "source_floor" => {
                changed["git_return"]["source_before"] =
                    changed["git_return"]["source_selected"].clone()
            }
            "snapshot" => {
                changed["git_return"]["selected_source"]["current"]["selection"]["context"] =
                    foreign
            }
            "source_pending" => {
                changed["git_return"]["selected_source"]["pending"] = serde_json::json!({})
            }
            "lost" => {
                changed["git_return"]["lost_object"] = serde_json::to_value(fixed.clone()).unwrap()
            }
            "before" => {
                changed["git_return"]["before"]["working"]["original"] =
                    serde_json::to_value(original.clone()).unwrap()
            }
            "after" => {
                changed["git_return"]["after"]["backup"]["original"] =
                    serde_json::to_value(original.clone()).unwrap()
            }
            "copies" => {
                changed["observations"]["original"]["verified_copies"] =
                    serde_json::json!(["backup"])
            }
            "unknown" => {
                changed["observations"]["original"]["unknown_copies"] =
                    serde_json::json!(["backup"])
            }
            "converged" => changed["receipt"]["page_converged"] = true.into(),
            "status" => changed["status"]["storage_converged"] = true.into(),
            "holds" => changed["receipt"]["holds"] = serde_json::json!([]),
            "state" => changed["status"]["observations"][0]["state"] = "invented".into(),
            "effects" => {
                changed["receipt"]["effects"][0]["object"] =
                    serde_json::to_value(original.clone()).unwrap()
            }
            "enrollment" => {
                changed["plan"]["enrollment"] = foreign;
                let plan: git_asset_store::placement::Plan =
                    serde_json::from_value(changed["plan"].clone()).unwrap();
                let id = serde_json::to_value(plan.id().unwrap()).unwrap();
                changed["receipt"]["plan"] = id.clone();
                changed["status"]["plan"] = id;
            }
            "stream" => changed["receipt"]["stream_bytes"] = 0.into(),
            "provider" => changed["plan"]["providers"][0] = foreign,
            "authority" => changed["git_return"]["authorizes_storage"] = true.into(),
            _ => unreachable!(),
        }
        let mut altered = trace.clone();
        altered["completed"][8]["report"] = save_evidence(&root, &changed);
        let mut config = config.clone();
        config["teaching"]["trace"] = save_evidence(&root, &altered);
        let path = root.join("negative-return.json");
        fs::write(&path, serde_json::to_vec(&config).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
}

#[test]
fn category_age_executes_real_paged_loss_and_revalidates_deadline() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("category aged execution");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-aged-category")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let raw = fs::read(root.join("aged-category-loss.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["aged_execution_evidence"]
    );
    let evidence: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(evidence["early_deadline_refused"], true);
    assert_eq!(evidence["simulated_clock"], true);
    assert_eq!(evidence["report"]["phase"], "completed");
    assert_eq!(
        evidence["report"]["completed_pages"],
        evidence["report"]["total_pages"]
    );
    assert_eq!(evidence["report"]["age_inheritance_unknown"], false);
    assert_eq!(evidence["report"]["authorizes_removal"], false);
    assert!(evidence["decision"]["age"].is_object());
    assert_eq!(evidence["occurrences"].as_array().unwrap().len(), 2);
    assert_eq!(result["completed_checkpoints"], 8);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-4");
    let lesson = &book["lessons"][7];
    assert_eq!(lesson["final_loss"]["new_removed_bytes"], 58);
    assert_eq!(lesson["final_loss"]["required_age_millis"], 500);
    let allocation = evidence["report"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["receipt"]["historical"] == false)
        .map(|e| e["receipt"]["file_allocation_before"].as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        lesson["final_loss"]["file_allocation_before_new_removals"],
        allocation
    );
    render_book(&book);
    drop(server);
    let recorded: Value = serde_json::from_slice(
        &fs::read(root.join("reports/aged-category-final-loss.json")).unwrap(),
    )
    .unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    for bad in [
        "claim_consent",
        "final_consent",
        "providers",
        "decision",
        "scope",
        "age",
        "time",
        "expired",
        "source",
        "before",
        "count",
        "effect",
        "index",
        "bytes",
        "protected",
        "unselected",
        "unknown",
    ] {
        let mut changed = recorded.clone();
        match bad {
            "claim_consent" => changed["synthetic_claim_consent"] = false.into(),
            "final_consent" => changed["synthetic_final_consent"] = false.into(),
            "providers" => changed["providers"].as_array_mut().unwrap().reverse(),
            "decision" => {
                changed["receipt"]["decision"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign decision")).unwrap()
            }
            "scope" => {
                changed["decision"]["prepared"] =
                    serde_json::to_value(Object::from_bytes(b"missing prepared scope")).unwrap()
            }
            "age" => {
                changed["decision"]["age"] =
                    serde_json::to_value(Object::from_bytes(b"missing age proof")).unwrap()
            }
            "time" => changed["execution_time"] = serde_json::json!({"earliest":500,"latest":500}),
            "expired" => {
                let expires = changed["decision"]["request"]["window"]["expires"].clone();
                changed["execution_time"] =
                    serde_json::json!({"earliest":expires,"latest":expires});
            }
            "source" => {
                changed["source_checkpoint"]["genesis"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign source")).unwrap()
            }
            "before" => {
                changed["observations"]["fixed"]["before_copies"] = serde_json::json!([]);
                changed["observations"]["fixed"]["verified_copies"] = serde_json::json!([]);
            }
            "count" => changed["receipt"]["completed_pages"] = 0.into(),
            "effect" => {
                changed["receipt"]["effects"][0]["receipt"]["plan"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign action")).unwrap()
            }
            "index" => changed["receipt"]["effects"][0]["action"] = 32.into(),
            "bytes" => {
                changed["receipt"]["effects"][0]["receipt"]["logical_bytes_removed"] = 0.into()
            }
            "protected" => {
                changed["observations"]["shared"]["verified_copies"] = serde_json::json!([])
            }
            "unselected" => {
                changed["observations"]["fixed"]["verified_copies"] = serde_json::json!([])
            }
            "unknown" => {
                changed["observations"]["original"]["unknown_copies"] =
                    serde_json::json!(["backup"])
            }
            _ => unreachable!(),
        }
        let mut edited_trace = trace.clone();
        edited_trace["completed"][7]["report"] = save_evidence(&root, &changed);
        let mut enrollment = config.clone();
        enrollment["teaching"]["trace"] = save_evidence(&root, &edited_trace);
        let path = root.join("negative-paged.json");
        fs::write(&path, serde_json::to_vec(&enrollment).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../examples/catalogue/lifecycle.design-1.json"
    ))
    .unwrap();
    let original =
        git_asset_core::Digest::parse(fixture["objects"]["original"]["sha256"].as_str().unwrap())
            .unwrap();
    for role in ["working", "history", "backup"] {
        assert!(
            !git_asset_store::existing::Layout::Flat
                .object_path(&root.join(role), &original)
                .exists()
        );
    }
    for role in ["working", "backup"] {
        for name in ["shared", "fixed"] {
            let digest =
                git_asset_core::Digest::parse(fixture["objects"][name]["sha256"].as_str().unwrap())
                    .unwrap();
            assert_eq!(
                fs::read(
                    git_asset_store::existing::Layout::Flat.object_path(&root.join(role), &digest)
                )
                .unwrap(),
                fixture["objects"][name]["text"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
        }
    }
}

#[test]
fn retained_age_reopens_real_facts_and_keeps_recent_and_unknown_occurrences_held() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("retained age lesson");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-retained-age")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["final_deletion_exercised"], false);
    let raw = fs::read(root.join("retained-age.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["retained_age_evidence"]
    );
    let facts: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(facts["simulated_clock"], true);
    assert_eq!(facts["registry_reopened"], true);
    assert_eq!(facts["observer_before"], facts["observer_after"]);
    assert_eq!(facts["assessments"][0]["old_enough"], true);
    assert_eq!(facts["assessments"][0]["age_lower_bound_millis"], 900);
    assert_eq!(facts["assessments"][1]["old_enough"], false);
    assert_eq!(facts["assessments"][1]["age_lower_bound_millis"], 50);
    assert_eq!(facts["assessments"][2]["old_enough"], false);
    assert!(facts["assessments"][2]["status"]["evidence"].is_null());
    assert_eq!(
        facts["occurrences"][0]["selection"]["root"],
        facts["occurrences"][2]["selection"]["root"]
    );
    assert_ne!(
        facts["occurrences"][0]["revision"],
        facts["occurrences"][2]["revision"]
    );
    let category_raw = fs::read(root.join("native-category.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&category_raw)).unwrap(),
        result["native_category_evidence"]
    );
    let category: Value = serde_json::from_slice(&category_raw).unwrap();
    assert_eq!(category["unknown_category_refused"], true);
    assert_eq!(category["candidate_adopted"], false);
    assert_eq!(category["preview"]["assets"].as_array().unwrap().len(), 1);
    assert_eq!(
        category["preview"]["source_checkpoint"],
        facts["assessments"][0]["status"]["target"]["selection"]["context"]
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../examples/catalogue/lifecycle.design-1.json"
    ))
    .unwrap();
    for name in ["original", "shared"] {
        assert!(
            category["preview"]["objects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|o| o["sha256"] == fixture["objects"][name]["sha256"])
        );
    }
    for a in facts["assessments"].as_array().unwrap() {
        assert_eq!(a["authorizes_removal"], false);
        assert_eq!(a["authorizes_claim_release"], false);
    }
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    render_book(&book);
}

#[test]
fn actual_final_child_exit_reopens_intent_and_resumes_exact_loss() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("actual final child exit");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-process")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let loss: Value =
        serde_json::from_slice(&fs::read(root.join("final-loss.json")).unwrap()).unwrap();
    assert_eq!(loss["actual_child_exit_code"], 94);
    assert_eq!(
        loss["interruption"],
        "actual child process exit at durable intent"
    );
    assert_eq!(loss["copies_before"], loss["copies_after_intent"]);
    assert_eq!(loss["report"]["phase"], "completed");
    assert_eq!(loss["report"]["new_logical_copy_bytes_removed"], 58);
    assert_eq!(loss["retry"]["new_logical_copy_bytes_removed"], 0);
    let frame: Value =
        serde_json::from_slice(&fs::read(root.join("final-child-frame.json")).unwrap()).unwrap();
    let plan = git_asset_store::final_retirement::bulk::Plan::from_json(
        &git_asset_core::identity::canonical_json(&frame["plan"]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(plan.id().unwrap()).unwrap(),
        loss["report"]["plan"]
    );
    // Malformed internal frames must refuse before opening the fixture's stores.
    let frame_path = root.join("final-child-frame.json");
    let original_frame = fs::read(&frame_path).unwrap();
    for bad in [
        "oversize",
        "digest",
        "canonical",
        "unknown",
        "ownership",
        "providers",
        "namespace",
        "enrollment",
    ] {
        let mut changed = frame.clone();
        match bad {
            "ownership" => changed["plan"]["request"]["operation"] = "foreign-frame".into(),
            "unknown" => changed["unexpected"] = true.into(),
            "providers" => changed["providers"] = serde_json::json!([]),
            "namespace" => {
                changed["namespace"]["genesis"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign namespace"))
                        .unwrap()
            }
            "enrollment" => {
                changed["enrollment"]["context"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign context")).unwrap()
            }
            _ => (),
        }
        let mut raw = git_asset_core::identity::canonical_json(&changed).unwrap();
        if bad == "oversize" {
            raw = vec![b' '; 65537];
        }
        if bad == "canonical" {
            raw.push(b'\n');
        }
        if bad != "ownership" {
            fs::write(&frame_path, &raw).unwrap();
        }
        let digest = if bad == "digest" {
            git_asset_core::Digest::blob(b"wrong frame")
        } else {
            git_asset_core::Digest::blob(&raw)
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
            .args(["__final-fixture-child", digest.as_str()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        use std::io::Write;
        child.stdin.take().unwrap().write_all(&raw).unwrap();
        let rejected = child.wait_with_output().unwrap();
        assert_eq!(
            rejected.status.code(),
            Some(1),
            "accepted {bad}: {rejected:?}"
        );
        let error: Value = serde_json::from_slice(&rejected.stdout).unwrap();
        assert_eq!(error["schema"], "tig.sample-error.design-1");
        let reason = match bad {
            "oversize" | "digest" => "invalid fixture child frame",
            "canonical" => "noncanonical fixture frame",
            "unknown" => "unknown field",
            "ownership" => "fixture frame ownership differs",
            "providers" | "namespace" => "fixture child scope differs",
            "enrollment" => "fixture child enrollment differs",
            _ => unreachable!(),
        };
        assert!(
            error["error"].as_str().unwrap().contains(reason),
            "wrong refusal for {bad}: {error}"
        );
        fs::write(&frame_path, &original_frame).unwrap();
    }
    assert_eq!(
        fs::read(root.join("final-loss.json")).unwrap(),
        git_asset_core::identity::canonical_json(&loss).unwrap()
    );
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    render_book(&book);
}

#[test]
fn old_final_retry_spares_explicit_same_hash_reacquisition_and_retention() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("fresh same hash birth");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-reacquire")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 9);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-5");
    let chapter = &book["lessons"][8];
    assert_eq!(chapter["stream_bytes"], 29);
    assert_eq!(chapter["reacquisition"]["added_logical_copy_bytes"], 29);
    assert_eq!(chapter["reacquisition"]["old_retry_new_removed_bytes"], 0);
    assert_eq!(
        chapter["reacquisition"]["old_retry_historical_removed_bytes"],
        58
    );
    render_book(&book);
    drop(server);
    let observation: Value =
        serde_json::from_slice(&fs::read(root.join("reports/same-hash-reacquired.json")).unwrap())
            .unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    assert_eq!(observation["before_replay"], observation["after_replay"]);
    for bad in [
        "consent",
        "clock",
        "verified",
        "authority",
        "providers",
        "role",
        "old_plan",
        "parent",
        "birth_object",
        "birth_index",
        "publication",
        "ack",
        "stream",
        "replayed",
        "charge",
        "grant",
        "grant_capacity",
        "allocation",
        "checkpoint",
        "source",
        "before",
        "after",
        "unknown",
        "replay_new",
        "replay_history",
        "replay_count",
        "replay_drift",
        "replay_floor",
        "acquisition_expiry",
        "replay_time",
    ] {
        let mut changed = observation.clone();
        let foreign = serde_json::to_value(git_asset_core::Digest::blob(
            b"foreign reacquisition identity",
        ))
        .unwrap();
        match bad {
            "consent" => changed["synthetic_owner_consent"] = false.into(),
            "clock" => changed["simulated_clock"] = false.into(),
            "verified" => changed["retention_verified_at_capture"] = false.into(),
            "authority" => changed["authorizes_storage"] = true.into(),
            "providers" => changed["providers"].as_array_mut().unwrap().reverse(),
            "role" => changed["target_role"] = "history".into(),
            "old_plan" => changed["old_retry"]["plan"] = foreign,
            "parent" => {
                changed["plan"]["parent"]["request"]["publication"]["operation"] =
                    "foreign-parent".into()
            }
            "birth_object" => {
                changed["plan"]["birth"]["object"] =
                    serde_json::to_value(Object::from_bytes(b"wrong new birth")).unwrap()
            }
            "birth_index" => {
                changed["plan"]["birth"]["index"] =
                    changed["plan"]["parent"]["birth"]["index"].clone()
            }
            "publication" => {
                changed["publication"]["request"]["operation"] = "foreign-publication".into()
            }
            "ack" => changed["publication"]["ack"] = "verified_local".into(),
            "stream" => changed["publication"]["streamed_bytes"] = 0.into(),
            "replayed" => changed["publication"]["was_replayed"] = true.into(),
            "charge" => changed["publication"]["charged_footprint"] = 0.into(),
            "grant" => {
                changed["retention"]["record"]["plan"]["request"]["publication"]["operation"] =
                    "foreign-grant".into();
                let bytes =
                    git_asset_core::identity::canonical_json(&changed["retention"]["record"])
                        .unwrap();
                changed["retention"]["sha256"] =
                    serde_json::to_value(git_asset_core::Digest::blob(&bytes)).unwrap();
            }
            "grant_capacity" => {
                changed["retention"]["record"]["allocation"]["genesis"] = foreign;
                let bytes =
                    git_asset_core::identity::canonical_json(&changed["retention"]["record"])
                        .unwrap();
                changed["retention"]["sha256"] =
                    serde_json::to_value(git_asset_core::Digest::blob(&bytes)).unwrap();
            }
            "allocation" => changed["publication"]["allocation"]["genesis"] = foreign,
            "checkpoint" => changed["publication"]["checkpoint"]["genesis"] = foreign,
            "source" => changed["source_checkpoint"]["genesis"] = foreign,
            "before" => {
                changed["observations"]["fixed"]["before_copies"] = serde_json::json!([]);
                changed["observations"]["fixed"]["verified_copies"] = serde_json::json!([]);
            }
            "after" => {
                changed["observations"]["original"]["verified_copies"] = serde_json::json!([])
            }
            "unknown" => {
                changed["observations"]["original"]["unknown_copies"] =
                    serde_json::json!(["backup"])
            }
            "replay_new" => changed["old_retry"]["new_removed_bytes"] = 1.into(),
            "replay_history" => changed["old_retry"]["historical_removed_bytes"] = 0.into(),
            "replay_count" => changed["old_retry"]["selected_bindings"] = 0.into(),
            "replay_drift" => changed["after_replay"]["witness"]["hash"] = foreign,
            "replay_floor" => {
                changed["before_replay"]["witness"]["sequence"] = 0.into();
                changed["after_replay"]["witness"]["sequence"] = 0.into();
            }
            "acquisition_expiry" => {
                changed["acquisition_time"] =
                    changed["plan"]["request"]["window"]["expires"].clone()
            }
            "replay_time" => changed["retry_time"] = 100.into(),
            _ => unreachable!(),
        }
        let mut altered_trace = trace.clone();
        altered_trace["completed"][8]["report"] = save_evidence(&root, &changed);
        let mut enrolled = config.clone();
        enrolled["teaching"]["trace"] = save_evidence(&root, &altered_trace);
        let path = root.join("negative-reacquisition.json");
        fs::write(&path, serde_json::to_vec(&enrolled).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    let loss: Value =
        serde_json::from_slice(&fs::read(root.join("final-loss.json")).unwrap()).unwrap();
    let raw = fs::read(root.join("final-reacquisition.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        loss["reacquisition"]
    );
    let evidence: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(evidence["old_plan_retry"]["phase"], "completed");
    assert_eq!(evidence["old_plan_retry"]["plan"], loss["report"]["plan"]);
    assert_eq!(
        evidence["old_plan_retry"]["new_logical_copy_bytes_removed"],
        0
    );
    assert_eq!(
        evidence["old_plan_retry"]["historical_deleted_copy_bytes"],
        58
    );
    assert_eq!(evidence["retry_clock_after_old_window"], 700);
    assert_eq!(
        evidence["plan"]["birth"]["index"].as_u64().unwrap(),
        evidence["plan"]["parent"]["birth"]["index"]
            .as_u64()
            .unwrap()
            + 1
    );
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../examples/catalogue/lifecycle.design-1.json"
    ))
    .unwrap();
    let original =
        git_asset_core::Digest::parse(fixture["objects"]["original"]["sha256"].as_str().unwrap())
            .unwrap();
    for role in ["working", "history", "backup"] {
        let path = git_asset_store::existing::Layout::Flat.object_path(&root.join(role), &original);
        if role == "backup" {
            assert_eq!(
                fs::read(path).unwrap(),
                fixture["objects"]["original"]["text"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
            assert_eq!(
                evidence["copies_after"][role]["original"]["sha256"],
                fixture["objects"]["original"]["sha256"]
            );
        } else {
            assert!(!path.exists());
        }
    }
    for role in ["working", "backup"] {
        for name in ["shared", "fixed"] {
            let digest =
                git_asset_core::Digest::parse(fixture["objects"][name]["sha256"].as_str().unwrap())
                    .unwrap();
            let bytes = fs::read(
                git_asset_store::existing::Layout::Flat.object_path(&root.join(role), &digest),
            )
            .unwrap();
            assert_eq!(
                bytes,
                fixture["objects"][name]["text"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
        }
    }
}

#[test]
fn explicit_final_cancellation_preserves_partial_loss_and_stops_remaining_removal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cancel final loss");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-cancel")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    let raw = fs::read(root.join("final-loss.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["final_loss_evidence"]
    );
    let loss: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(loss["cancelled_after_first_removal"], true);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    render_book(&book);
    drop(server);
    for report in ["report", "retry"] {
        assert_eq!(loss[report]["phase"], "cancelled");
        assert_eq!(loss[report]["new_logical_copy_bytes_removed"], 0);
        assert_eq!(loss[report]["historical_deleted_copy_bytes"], 29);
    }
    assert_eq!(loss["copies_after_completion"], loss["copies_after"]);
    assert_eq!(
        ["working", "history", "backup"]
            .iter()
            .filter(|role| loss["copies_after"][**role].get("original").is_some())
            .count(),
        1
    );
    for name in ["fixed", "shared"] {
        for role in ["working", "backup"] {
            assert_eq!(
                loss["copies_before"][role][name],
                loss["copies_after"][role][name]
            );
        }
    }
}

#[test]
fn explicit_manual_final_loss_removes_only_original_after_separate_claim_retirement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("manual final loss with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-manual")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["complete_course"], false);
    assert_eq!(result["final_deletion_exercised"], true);
    assert_eq!(result["completed_checkpoints"], 8);
    let raw = fs::read(root.join("final-loss.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["final_loss_evidence"]
    );
    let loss: Value = serde_json::from_slice(&raw).unwrap();
    let recorded_raw = fs::read(root.join("reports/final-loss-completed.json")).unwrap();
    let recorded: Value = serde_json::from_slice(&recorded_raw).unwrap();
    assert_eq!(
        recorded["schema"],
        "tig.teaching-final-observation.design-1"
    );
    assert_eq!(recorded["receipt"], loss["report"]);
    assert_eq!(recorded["plan"], loss["plan"]);
    assert_eq!(
        recorded["observations"]["original"]["before_copies"],
        serde_json::json!(["backup", "history"])
    );
    assert_eq!(
        recorded["observations"]["original"]["verified_copies"],
        serde_json::json!([])
    );
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    assert_eq!(
        trace["completed"][7]["report"],
        serde_json::to_value(Object::from_bytes(&recorded_raw)).unwrap()
    );
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-4");
    assert_eq!(
        book["lessons"][7]["final_loss"]["file_allocation_before_new_removals"],
        loss["report"]["file_allocation_before_new_removals"]
    );
    render_book(&book);
    drop(server);
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    for bad in [
        "consent",
        "source_namespace",
        "before",
        "providers",
        "plan",
        "bytes",
        "count",
        "source",
        "status",
        "copies",
        "protected",
        "unknown",
        "effects",
    ] {
        let mut changed = recorded.clone();
        match bad {
            "consent" => changed["synthetic_owner_consent"] = false.into(),
            "source_namespace" => {
                changed["source_checkpoint"]["genesis"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign manual namespace"))
                        .unwrap()
            }
            "before" => {
                changed["observations"]["fixed"]["before_copies"] = serde_json::json!([]);
                changed["observations"]["fixed"]["verified_copies"] = serde_json::json!([]);
            }
            "providers" => changed["providers"].as_array_mut().unwrap().reverse(),
            "plan" => {
                changed["receipt"]["plan"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign plan")).unwrap()
            }
            "bytes" => changed["receipt"]["new_logical_copy_bytes_removed"] = 0.into(),
            "count" => changed["receipt"]["selected_bindings"] = 0.into(),
            "source" => changed["receipt"]["source_status"][0]["settlement_current"] = false.into(),
            "status" => {
                changed["receipt"]["action_status"][0]["plan"] =
                    serde_json::to_value(git_asset_core::Digest::blob(b"foreign action")).unwrap()
            }
            "copies" => {
                changed["observations"]["original"]["verified_copies"] =
                    serde_json::json!(["backup"])
            }
            "protected" => {
                changed["observations"]["fixed"]["verified_copies"] = serde_json::json!([])
            }
            "unknown" => {
                changed["observations"]["original"]["unknown_copies"] =
                    serde_json::json!(["backup"])
            }
            "effects" => {
                changed["receipt"]["effects"][0]["receipt"]["logical_bytes_removed"] = 0.into()
            }
            _ => unreachable!(),
        }
        let mut edited_trace = trace.clone();
        edited_trace["completed"][7]["report"] = save_evidence(&root, &changed);
        let mut enrollment = config.clone();
        enrollment["teaching"]["trace"] = save_evidence(&root, &edited_trace);
        let path = root.join("negative-final.json");
        fs::write(&path, serde_json::to_vec(&enrollment).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    assert_eq!(loss["report"]["phase"], "completed");
    assert_eq!(loss["report"]["new_logical_copy_bytes_removed"], 58);
    assert_eq!(loss["copies_before"], loss["copies_after_intent"]);
    assert_eq!(loss["copies_after_completion"], loss["copies_after"]);
    assert_eq!(loss["retry"]["phase"], "completed");
    assert_eq!(loss["retry"]["new_logical_copy_bytes_removed"], 0);
    assert_eq!(loss["retry"]["historical_deleted_copy_bytes"], 58);
    assert_eq!(loss["retry"]["plan"], loss["report"]["plan"]);
    assert_eq!(loss["synthetic_owner_consent"], true);
    assert_eq!(loss["simulated_clock"], true);
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../examples/catalogue/lifecycle.design-1.json"
    ))
    .unwrap();
    let original =
        git_asset_core::Digest::parse(fixture["objects"]["original"]["sha256"].as_str().unwrap())
            .unwrap();
    for role in ["working", "history", "backup"] {
        assert!(
            !git_asset_store::existing::Layout::Flat
                .object_path(&root.join(role), &original)
                .exists()
        );
        assert!(loss["copies_after"][role].get("original").is_none());
    }
    for role in ["working", "backup"] {
        for name in ["fixed", "shared"] {
            let digest =
                git_asset_core::Digest::parse(fixture["objects"][name]["sha256"].as_str().unwrap())
                    .unwrap();
            let bytes = fs::read(
                git_asset_store::existing::Layout::Flat.object_path(&root.join(role), &digest),
            )
            .unwrap();
            assert_eq!(
                bytes,
                fixture["objects"][name]["text"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
            );
        }
    }
    let held: Value =
        serde_json::from_slice(&fs::read(root.join("final-preview.json")).unwrap()).unwrap();
    assert_eq!(held["copies_before"], held["copies_after"]);
    assert_ne!(
        held["source_checkpoint"],
        loss["source_after_job_history_retirement"]
    );
    assert_ne!(held["plan_id"], loss["report"]["plan"]);
}

#[test]
fn fresh_final_enrollment_runs_ordinary_lifecycle_without_claiming_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("final enrollment with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-enrollment")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 7);
    assert_eq!(result["complete_course"], false);
    assert_eq!(result["final_deletion_exercised"], false);
    let preview_raw = fs::read(root.join("final-preview.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&preview_raw)).unwrap(),
        result["final_preview_evidence"]
    );
    let preview: Value = serde_json::from_slice(&preview_raw).unwrap();
    assert_eq!(preview["copies_before"], preview["copies_after"]);
    assert_eq!(preview["preview_only"], true);
    assert_eq!(preview["removal_authority"], false);
    let plan = git_asset_store::final_retirement::bulk::Plan::from_json(
        &git_asset_core::identity::canonical_json(&preview["plan"]).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(plan.id().unwrap()).unwrap(),
        preview["plan_id"]
    );
    assert_eq!(plan.subjects().len(), 3);
    assert!(
        plan.subjects()
            .iter()
            .all(|s| s.hold == Some(git_asset_store::final_retirement::bulk::Hold::Protected))
    );
    assert_eq!(plan.actions().len(), 0);
    let raw = fs::read(root.join("final-enrollment.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["final_enrollment_evidence"]
    );
    let evidence: Value = serde_json::from_slice(&raw).unwrap();
    let roster: git_asset_store::final_retirement::claims::Roster =
        serde_json::from_value(evidence["roster"].clone()).unwrap();
    assert_eq!(
        serde_json::to_value(roster.scope().unwrap()).unwrap(),
        evidence["scope"]
    );
    assert_eq!(evidence["enrollment_only"], true);
    let copies: Vec<git_asset_store::final_retirement::bulk::Copy> =
        serde_json::from_value(evidence["copies"].clone()).unwrap();
    assert_eq!(copies.len(), 6);
    for role in ["working", "history", "backup"] {
        let store = git_asset_store::local::LocalStore::open(&root.join(role)).unwrap();
        let policy = store.final_policy().unwrap();
        assert_eq!(policy.scope, roster.scope().unwrap());
        assert_eq!(
            policy.owners,
            vec![git_asset_store::Key::parse("sample").unwrap()]
        );
        for copy in copies.iter().filter(|c| &c.store == store.identity()) {
            let plan = git_asset_store::final_retirement::provider::plan(
                &store,
                git_asset_store::final_retirement::provider::Request {
                    operation: git_asset_store::Key::parse("readonly-publication-validation")
                        .unwrap(),
                    owner: git_asset_store::Key::parse("sample").unwrap(),
                    publication: copy.publication.clone(),
                    capacity: copy.capacity.clone(),
                    context: git_asset_core::Digest::blob(b"fixture context"),
                    window: git_asset_store::final_retirement::provider::Window {
                        not_before: 50,
                        expires: 500,
                    },
                },
            )
            .unwrap();
            plan.id().unwrap(); // validates the actual current publication and grants, no authorization
        }
    }
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    render_book(&book);
    drop(server);
    let before = fs::read(root.join("backup/store.json")).unwrap();
    let retry = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-final-enrollment")
        .arg(&root)
        .output()
        .unwrap();
    assert!(!retry.status.success());
    assert_eq!(fs::read(root.join("backup/store.json")).unwrap(), before);
}

#[test]
fn public_rename_preserves_bytes_and_copies_and_refuses_conflicting_destination() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("rename sample with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-rename")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 8);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("rename.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["rename_evidence"]
    );
    let proof: Value = serde_json::from_slice(&raw).unwrap();
    assert_ne!(proof["before_commit"], proof["after_commit"]);
    assert_eq!(proof["copies_before"], proof["copies_after"]);
    assert_eq!(proof["conflicting_destination_refused"], true);
    assert_eq!(proof["stream_bytes"], 0);
    assert_eq!(proof["effects"], serde_json::json!([]));
    assert!(!root.join("project/records/series.asset").exists());
    let record = fs::read(root.join("project/records/renamed-series.asset")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&record)).unwrap(),
        proof["record_object"]
    );
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, book) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(book["lessons"][7]["id"], "same-byte-rename");
    assert_eq!(book["lessons"][7]["effects"], serde_json::json!([]));
    render_book(&book);
}

#[test]
fn public_course_runs_real_git_and_providers_then_loads_the_catalogue() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("public sample");
    let external = temp.path().join("external");
    fs::create_dir(&external).unwrap();
    fs::write(external.join("keep"), b"unchanged external fixture").unwrap();
    let template = temp.path().join("template");
    fs::create_dir(&template).unwrap();
    fs::write(template.join("must-not-copy"), b"unadmitted template").unwrap();
    let global = temp.path().join("global.config");
    fs::write(
        &global,
        format!(
            "[init]\n\ttemplateDir = {}\n",
            template.display().to_string().replace('\\', "/")
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-ordinary")
        .arg(&root)
        .env("GIT_DIR", external.join("trap.git"))
        .env("GIT_WORK_TREE", &external)
        .env("GIT_INDEX_FILE", external.join("trap-index"))
        .env("GIT_TEMPLATE_DIR", &template)
        .env("GIT_CONFIG_GLOBAL", &global)
        .env("GIT_CONFIG_COUNT", "1")
        .env("GIT_CONFIG_KEY_0", "core.bare")
        .env("GIT_CONFIG_VALUE_0", "true")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    assert_eq!(
        fs::read(external.join("keep")).unwrap(),
        b"unchanged external fixture"
    );
    assert!(!external.join("trap.git").exists());
    assert!(!external.join("trap-index").exists());
    assert!(!root.join("project/.git/must-not-copy").exists());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 7);
    assert_eq!(result["complete_course"], false);
    let resolver: Value =
        serde_json::from_slice(&fs::read(root.join("resolver.json")).unwrap()).unwrap();
    let repository = git_asset::source::Repository::new(
        resolver["git_executable"].as_str().unwrap().into(),
        root.join("project"),
        git_asset::source::Limits::default(),
    )
    .unwrap();
    let heads = repository
        .named_commits()
        .unwrap()
        .into_iter()
        .filter(|(name, _)| name.starts_with("refs/heads/"))
        .map(|(name, _)| name)
        .collect::<Vec<_>>();
    assert_eq!(heads, vec!["refs/heads/main".to_owned()]);
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    assert_eq!(trace["completed"].as_array().unwrap().len(), 7);
    let mut reports = std::collections::BTreeMap::new();
    for row in trace["completed"].as_array().unwrap() {
        let raw = fs::read(root.join(row["file"].as_str().unwrap())).unwrap();
        assert_eq!(
            serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
            row["report"]
        );
        let report: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(report["declared_fixture_domains"], 3);
        assert_eq!(report["independent_hardware_backup_proven"], false);
        reports.insert(row["lesson"].as_str().unwrap(), report);
    }
    assert_eq!(
        reports["merge-v2-job-active"]["observations"]["original"]["verified_copies"],
        serde_json::json!(["backup", "working"])
    );
    assert_eq!(
        reports["job-ended"]["observations"]["original"]["verified_copies"],
        serde_json::json!(["backup", "history"])
    );
    assert_eq!(
        reports["revert-v1"]["observations"]["original"]["verified_copies"],
        serde_json::json!(["backup", "working"])
    );
    assert_eq!(
        reports["return-v2"]["observations"]["fixed"]["verified_copies"],
        serde_json::json!(["backup", "working"])
    );
    let mut catalogue =
        Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let summaries = catalogue.summaries();
    assert_eq!(summaries["teaching_available"], true);
    check_lessons_and_refusals(&root, &catalogue);
    assert_eq!(summaries["projects"][0]["work_count"], 2);
    let commit = summaries["projects"][0]["commit"].as_str().unwrap();
    catalogue.observe_registered("sample", commit, "").unwrap();
    let inventory = catalogue
        .object_inventory(&Query {
            project: "sample",
            kind: "",
            usage: "",
            anomaly: "",
            primary: "",
            observed: "",
            q: "",
            sort: "bytes",
            offset: 0,
            sha: "",
        })
        .unwrap();
    assert_eq!(inventory["total"], 3);
    assert!(
        inventory["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["copies"].as_array().unwrap().len() == 2)
    );
}

fn check_lessons_and_refusals(root: &std::path::Path, catalogue: &Catalogue) {
    use git_asset::catalogue::teaching;
    let book = teaching::execute(catalogue.teaching_request().unwrap()).unwrap();
    assert_eq!(book["lessons"].as_array().unwrap().len(), 7);
    assert_eq!(book["authorizes_storage"], false);
    assert_eq!(book["independent_hardware_backup_proven"], false);
    let original = |index: usize| {
        book["lessons"][index]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "original")
            .unwrap()
    };
    assert_eq!(
        original(3)["observed"],
        serde_json::json!(["backup", "working"])
    );
    assert_eq!(
        original(4)["observed"],
        serde_json::json!(["backup", "history"])
    );
    assert_eq!(
        original(5)["observed"],
        serde_json::json!(["backup", "working"])
    );
    assert_eq!(original(3)["object"], original(5)["object"]);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    assert_ne!(server.get("/api/lessons", false).0, 200);
    let (code, received) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(received, book);
    render_book(&received);
    assert_ne!(server.json("/api/lessons?path=unadmitted").0, 200);
    let (code, script) = server.get("/lessons.js", false);
    assert_eq!(code, 200);
    assert!(script.contains("recorded-rehearsal"));
    drop(server);

    let original_config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let object: Object = serde_json::from_value(trace["completed"][0]["report"].clone()).unwrap();
    let path = git_asset_store::existing::Layout::Flat
        .object_path(&root.join("lesson-evidence"), &object.sha256);
    let raw = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(teaching::execute(catalogue.teaching_request().unwrap()).is_err());
    fs::write(&path, b"corrupt").unwrap();
    assert!(teaching::execute(catalogue.teaching_request().unwrap()).is_err());
    fs::write(&path, &raw).unwrap();

    let run = |config: &Value| {
        let path = root.join("negative-catalogue.json");
        fs::write(&path, serde_json::to_vec(config).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        teaching::execute(c.teaching_request().unwrap())
    };
    let mut swapped = original_config.clone();
    swapped["teaching"]["roles"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    assert!(run(&swapped).is_err());

    // Repin admitted synthetic negative metadata so semantic checks, rather
    // than the hash guard alone, must reject it. No payload store is modified.
    for mutation in ["schema", "plan", "role", "hardware", "size"] {
        let mut report: Value = serde_json::from_slice(&raw).unwrap();
        match mutation {
            "schema" => report["schema"] = "unsupported".into(),
            "plan" => {
                report["receipt"]["plan"] =
                    serde_json::to_value(Object::from_bytes(b"foreign").sha256).unwrap()
            }
            "role" => {
                report["observations"]["original"]["verified_copies"] =
                    serde_json::json!(["foreign"])
            }
            "hardware" => report["independent_hardware_backup_proven"] = true.into(),
            "size" => report["observations"]["original"]["object"]["bytes"] = 999.into(),
            _ => unreachable!(),
        }
        let mut edited = trace.clone();
        edited["completed"][0]["report"] = save_evidence(root, &report);
        let mut config = original_config.clone();
        config["teaching"]["trace"] = save_evidence(root, &edited);
        assert!(run(&config).is_err(), "accepted {mutation}");
    }
    let mut oversized = trace.clone();
    oversized["completed"] = Value::Array(vec![trace["completed"][0].clone(); 17]);
    let mut config = original_config;
    config["teaching"]["trace"] = save_evidence(root, &oversized);
    assert!(run(&config).is_err());
    assert_eq!(
        teaching::execute(catalogue.teaching_request().unwrap()).unwrap(),
        book
    );
}

fn save_evidence(root: &std::path::Path, value: &Value) -> Value {
    let raw = git_asset_core::identity::canonical_json(value).unwrap();
    let object = Object::from_bytes(&raw);
    let path = git_asset_store::existing::Layout::Flat
        .object_path(&root.join("lesson-evidence"), &object.sha256);
    fs::write(path, raw).unwrap();
    serde_json::to_value(object).unwrap()
}

#[test]
fn sample_discovery_and_existing_or_checkout_roots_cannot_mutate_data() {
    let cli = env!("CARGO_BIN_EXE_git-asset-sample");
    let described = Command::new(cli).arg("describe").output().unwrap();
    assert!(described.status.success());
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("keep"), b"keep").unwrap();
    let denied = Command::new(cli)
        .arg("rehearse-ordinary")
        .arg(temp.path())
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert_eq!(fs::read(temp.path().join("keep")).unwrap(), b"keep");
    fs::create_dir(temp.path().join(".git")).unwrap();
    let nested = temp.path().join("must-not-create");
    let denied = Command::new(cli)
        .arg("rehearse-ordinary")
        .arg(&nested)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(!nested.exists());
}

#[test]
fn public_exchange_preserves_dirty_work_and_reconciles_only_after_exact_pull() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("public exchange with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-exchange")
        .arg(&root)
        .env("GIT_ALLOW_PROTOCOL", "ssh")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 10);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("exchange.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["exchange_evidence"]
    );
    let proof: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(proof["published"], proof["after_fetch"]);
    assert_ne!(proof["published"], proof["pulled_v1"]);
    assert_ne!(proof["pulled_v1"], proof["returned_v2"]);
    assert_eq!(proof["dirty_pull_refused"], true);
    assert_eq!(proof["selected_payloads_in_cloned_tree"], false);
    assert_eq!(
        proof["dirty_pull_reason"],
        "local changes would be overwritten by merge"
    );
    assert_eq!(proof["observer_is_enrolled_writer"], false);
    let catalogue = Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let book =
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap();
    assert_eq!(book["lessons"].as_array().unwrap().len(), 10);
    for (index, id, expected) in [
        (7, "publish-v2", serde_json::json!(["backup", "history"])),
        (8, "pull-v1", serde_json::json!(["backup", "working"])),
        (
            9,
            "pull-return-v2",
            serde_json::json!(["backup", "history"]),
        ),
    ] {
        assert_eq!(book["lessons"][index]["id"], id);
        let rows = book["lessons"][index]["objects"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|row| row["name"] == "original").unwrap()["observed"],
            expected
        );
    }
    assert_eq!(book["lessons"][7]["stream_bytes"], 0);
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, received) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(received, book);
    render_book(&received);
    let refused = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-exchange")
        .arg(&root)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(root.join("exchange.json")).unwrap(), raw);
}

#[test]
fn public_worktree_index_is_exact_and_job_outlives_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("public worktree with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-worktree")
        .arg(&root)
        .env("GIT_INDEX_FILE", temp.path().join("must-not-create-index"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!temp.path().join("must-not-create-index").exists());
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 12);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("worktree.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["worktree_evidence"]
    );
    let proof: Value = serde_json::from_slice(&raw).unwrap();
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../examples/catalogue/lifecycle.design-1.json"
    ))
    .unwrap();
    assert_ne!(proof["main_commit"], proof["worktree_commit"]);
    assert_ne!(proof["staged_index"], proof["unstaged_index"]);
    assert_ne!(proof["staged_native"], proof["unstaged_native"]);
    assert_eq!(
        proof["staged_selected"]["sha256"],
        corpus["objects"]["fixed"]["sha256"]
    );
    assert_eq!(
        proof["unstaged_selected"]["sha256"],
        corpus["objects"]["original"]["sha256"]
    );
    assert_eq!(proof["dirty_status"], "MM records/series.asset");
    assert_eq!(proof["dirty_remove_refused"], true);
    assert_eq!(proof["worktree_removed"], true);
    assert_eq!(proof["automatic_writer_enrollment"], false);
    assert!(!root.join("study").exists());
    let catalogue = Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let book =
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap();
    assert_eq!(book["lessons"].as_array().unwrap().len(), 12);
    for (index, id) in [
        (7, "worktree-open-v1"),
        (8, "index-staged-v2"),
        (9, "index-unstaged-v1"),
        (10, "worktree-closed-job-active"),
        (11, "worktree-job-ended"),
    ] {
        assert_eq!(book["lessons"][index]["id"], id);
        let original = book["lessons"][index]["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "original")
            .unwrap();
        assert_eq!(
            original["observed"],
            if index == 11 {
                serde_json::json!(["backup", "history"])
            } else {
                serde_json::json!(["backup", "working"])
            }
        );
    }
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, received) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(received, book);
    render_book(&received);
    let refused = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-worktree")
        .arg(&root)
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert_eq!(fs::read(root.join("worktree.json")).unwrap(), raw);
}

fn render_book(received: &Value) {
    let mut renderer = Command::new("node")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/ux/recorded-lesson.cjs"
        ))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("teaching qualification requires Node.js");
    {
        use std::io::Write;
        renderer
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(received).unwrap())
            .unwrap();
    }
    let rendered = renderer.wait_with_output().unwrap();
    assert!(rendered.status.success(), "{rendered:?}");
}

#[test]
fn public_backup_outage_holds_unknown_custody_and_retries_without_seed_or_new_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("backup outage with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-backup-outage")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 9);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("backup.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["backup_evidence"]
    );
    let proof: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(proof["held_plan"], proof["retry_plan"]);
    assert_eq!(proof["seed_attempts_during_retry"], 0);
    assert_eq!(proof["physical_hardware_fault_qualified"], false);
    assert!(root.join("backup").is_dir());
    assert!(!root.join("offline-backup").exists());
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let catalogue = Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let book =
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap();
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-2");
    for id in ["merge-backup-offline", "job-ended-backup-offline"] {
        let lesson = book["lessons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap();
        assert_eq!(lesson["converged"], false);
        assert_eq!(lesson["storage_converged"], false);
        for object in lesson["objects"].as_array().unwrap() {
            assert_eq!(object["unknown"], serde_json::json!(["backup"]));
            assert!(
                !object["observed"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!("backup"))
            );
        }
        let original = lesson["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "original")
            .unwrap();
        assert!(
            original["observed"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("working"))
        );
    }
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, received) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(received, book);
    render_book(&received);
    drop(server);
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let index = trace["completed"]
        .as_array()
        .unwrap()
        .iter()
        .position(|l| l["lesson"] == "job-ended-backup-offline")
        .unwrap();
    let raw = fs::read(root.join("reports/job-ended-backup-offline.json")).unwrap();
    for bad in [
        "overlap",
        "foreign",
        "missing",
        "legacy",
        "legacy-null",
        "converged",
        "no-held-evidence",
        "hidden-unknown",
        "duplicate",
        "required",
    ] {
        let mut report: Value = serde_json::from_slice(&raw).unwrap();
        match bad {
            "overlap" => report["observations"]["original"]["verified_copies"]
                .as_array_mut()
                .unwrap()
                .push("backup".into()),
            "foreign" => {
                report["observations"]["original"]["unknown_copies"] =
                    serde_json::json!(["foreign"])
            }
            "missing" => {
                report["observations"]["original"]
                    .as_object_mut()
                    .unwrap()
                    .remove("unknown_copies");
            }
            "legacy" => report["schema"] = "tig.teaching-observation.design-1".into(),
            "legacy-null" => {
                report["schema"] = "tig.teaching-observation.design-1".into();
                for observation in report["observations"].as_object_mut().unwrap().values_mut() {
                    observation["unknown_copies"] = Value::Null;
                }
            }
            "converged" => report["receipt"]["page_converged"] = true.into(),
            "hidden-unknown" => {
                report["observations"]["original"]["unknown_copies"] = serde_json::json!([])
            }
            "no-held-evidence" => {
                for observation in report["receipt"]["observations"].as_array_mut().unwrap() {
                    observation["state"] = "retained".into();
                }
            }
            "duplicate" => {
                let first = report["receipt"]["observations"][0].clone();
                report["receipt"]["observations"]
                    .as_array_mut()
                    .unwrap()
                    .push(first);
            }
            "required" => {
                let old = report["receipt"]["observations"][0]["required"]
                    .as_bool()
                    .unwrap();
                report["receipt"]["observations"][0]["required"] = (!old).into();
            }
            _ => unreachable!(),
        }
        let mut changed = trace.clone();
        changed["completed"][index]["report"] = save_evidence(&root, &report);
        let mut enrollment = config.clone();
        enrollment["teaching"]["trace"] = save_evidence(&root, &changed);
        let path = root.join("negative-coverage.json");
        fs::write(&path, serde_json::to_vec(&enrollment).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    assert_eq!(
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap(),
        book
    );
}

#[test]
fn public_stale_plan_refuses_before_effects_then_fresh_plans_respect_new_job() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("stale plan with spaces");
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-stale-plan")
        .arg(&root)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["completed_checkpoints"], 10);
    assert_eq!(result["complete_course"], false);
    let raw = fs::read(root.join("stale.json")).unwrap();
    assert_eq!(
        serde_json::to_value(Object::from_bytes(&raw)).unwrap(),
        result["stale_evidence"]
    );
    let proof: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(proof["error"], "stale");
    assert_eq!(
        proof["copies_before_rejection"],
        proof["copies_after_rejection"]
    );
    assert_ne!(proof["planning_basis"], proof["observed_basis"]);
    assert_ne!(proof["planning_source"], proof["protected_source"]);
    assert_ne!(proof["rejected_plan"], proof["fresh_job_plan"]);
    assert_ne!(proof["rejected_plan"], proof["fresh_job_ended_plan"]);
    assert_eq!(proof["status_stream_bytes"], 0);
    assert_eq!(proof["status_effects"], serde_json::json!([]));
    let config: Value =
        serde_json::from_slice(&fs::read(root.join("catalogue.json")).unwrap()).unwrap();
    let catalogue = Catalogue::load(Config::load(&root.join("catalogue.json")).unwrap()).unwrap();
    let book =
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap();
    assert_eq!(book["schema"], "tig.catalogue-teaching.design-3");
    for (id, expected) in [
        (
            "stale-plan-rejected",
            serde_json::json!(["backup", "working"]),
        ),
        (
            "fresh-plan-job-active",
            serde_json::json!(["backup", "working"]),
        ),
        (
            "fresh-plan-job-ended",
            serde_json::json!(["backup", "history"]),
        ),
    ] {
        let lesson = book["lessons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == id)
            .unwrap();
        let original = lesson["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["name"] == "original")
            .unwrap();
        assert_eq!(original["observed"], expected);
        if id == "stale-plan-rejected" {
            assert_eq!(
                original["desired"],
                serde_json::json!(["history", "backup"])
            );
            assert_eq!(lesson["rejection"]["error"], "stale");
            assert_eq!(lesson["basis_current"], false);
        }
    }
    let server = catalogue_server::Server::new(&root.join("catalogue.json"));
    let (code, received) = server.json("/api/lessons");
    assert_eq!(code, 200);
    assert_eq!(received, book);
    render_book(&received);
    drop(server);
    let trace: Value = serde_json::from_slice(&fs::read(root.join("trace.json")).unwrap()).unwrap();
    let index = trace["completed"]
        .as_array()
        .unwrap()
        .iter()
        .position(|l| l["lesson"] == "stale-plan-rejected")
        .unwrap();
    let raw = fs::read(root.join("reports/stale-plan-rejected.json")).unwrap();
    let released: Value =
        serde_json::from_slice(&fs::read(root.join("reports/release-v1.json")).unwrap()).unwrap();
    let actual_effect = released["receipt"]["effects"][0].clone();
    assert!(actual_effect.is_object());
    for bad in [
        "missing",
        "null",
        "version",
        "error",
        "basis",
        "same",
        "current",
        "converged",
        "stream",
        "effects",
        "page",
    ] {
        let mut report: Value = serde_json::from_slice(&raw).unwrap();
        match bad {
            "missing" => {
                report.as_object_mut().unwrap().remove("rejection");
            }
            "null" => report["rejection"] = Value::Null,
            "version" => report["schema"] = "tig.teaching-observation.design-2".into(),
            "error" => report["rejection"]["error"] = "busy".into(),
            "basis" => {
                report["rejection"]["observed_basis"] =
                    serde_json::to_value(Object::from_bytes(b"foreign-basis").sha256).unwrap()
            }
            "same" => {
                let basis = report["plan"]["basis"].clone();
                report["rejection"]["observed_basis"] = basis.clone();
                report["receipt"]["observed_basis"] = basis;
            }
            "current" => report["receipt"]["basis_current"] = true.into(),
            "converged" => report["receipt"]["page_converged"] = true.into(),
            "stream" => report["receipt"]["stream_bytes"] = 1.into(),
            "effects" => report["receipt"]["effects"] = serde_json::json!([actual_effect]),
            "page" => {
                report["receipt"]["next"] = serde_json::json!({"basis":report["plan"]["basis"],"after":report["plan"]["basis"]});
            }
            _ => unreachable!(),
        }
        let mut changed = trace.clone();
        changed["completed"][index]["report"] = save_evidence(&root, &report);
        let mut enrollment = config.clone();
        enrollment["teaching"]["trace"] = save_evidence(&root, &changed);
        let path = root.join("negative-rejection.json");
        fs::write(&path, serde_json::to_vec(&enrollment).unwrap()).unwrap();
        let c = Catalogue::load(Config::load(&path).unwrap()).unwrap();
        assert!(
            git_asset::catalogue::teaching::execute(c.teaching_request().unwrap()).is_err(),
            "accepted {bad}"
        );
    }
    assert_eq!(
        git_asset::catalogue::teaching::execute(catalogue.teaching_request().unwrap()).unwrap(),
        book
    );
}

#[cfg(not(feature = "upload"))]
#[test]
fn remote_publication_without_upload_support_creates_no_root() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("unsupported remote sample");
    let out = Command::new(env!("CARGO_BIN_EXE_git-asset-sample"))
        .arg("rehearse-remote-publication")
        .arg(&root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(!root.exists());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(
        value["error"]
            .as_str()
            .unwrap()
            .contains("requires schema,upload")
    );
}
