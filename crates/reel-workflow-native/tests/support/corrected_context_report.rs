//! Exact retained controls and original image producers -> admitted v2 report.
//! Scripted transport checks do not assert viewing, creative review or closure.
use super::repair_control_native::check_waiting;
use super::repair_part_native::{load, phase, raw_save, save, success, verify_parent};
use super::*;
use reel_workflow::handoff::{Journal, State};
use reel_workflow::publication::InputReader;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission, closing, handoff, response, retention, staging,
    upstream,
};
use tig_store::source_claims::{Checkpoint, protection::Index};

macro_rules! invoke {
    ($tool:expr, $($arg:expr),+ $(,)?) => { Command::new($tool)$(.arg($arg))+.output().unwrap() };
}
fn observe(profile: &admission::Profile) -> Checkpoint {
    Index::open(&profile.index_root, &profile.index_minimum)
        .unwrap()
        .checkpoint()
}
fn certificate(root: &Path) {
    let raw = fs::read(root.join("qualified-report.json")).unwrap();
    assert_eq!(
        digest(&raw),
        "8e564493498681a8cacca304a4259c89252920199b212ed71a1b9ec77bcf47ce"
    );
    let certificate: serde_json::Value = decode(&raw).unwrap();
    for pin in certificate["files"].as_array().unwrap() {
        let raw = fs::read(pin["path"].as_str().unwrap()).unwrap();
        assert_eq!(raw.len() as u64, pin["bytes"].as_u64().unwrap());
        assert_eq!(digest(&raw), pin["sha256"].as_str().unwrap());
    }
}

#[test]
#[ignore = "Exact certified corrected-context report only; checked bound-policy control close, one original report admission with finite owner policy, actual v2 origin guard/PNGs/staging, invalid-return refusal before intent, scripted empty report retention and replay; no provider/viewing/finding closure or project adoption"]
fn pinned_v2_context_report_reopens_exact_origins_and_retains_without_inventing_review() {
    let env = |name| PathBuf::from(std::env::var_os(name).expect(name));
    let certified = env("REEL_CORRECTED_REPORT_CERTIFICATE");
    let control = env("REEL_CORRECTED_REPORT_CONTROL");
    let evidence = env("REEL_CORRECTED_REPORT_EVIDENCE");
    let waiting = env("REEL_CORRECTED_REPORT_WAITING");
    let executable = env("REEL_CORRECTED_REPORT_RUNTIME");
    certificate(&certified);
    let ready = control.join("report-preflight");
    let packet: Packet = load(&ready, "packet.json");
    let source: NativeProfile = load(&ready, "source.json");
    let admission: admission::Profile = load(&ready, "admission.json");
    let parents: upstream::Parents = load(&ready, "parents.json");
    let plan: Plan = load(&ready, "plan.json");
    assert_eq!(
        packet.task_key,
        "230314b16bc5b4fdbce6df59a133112226d7416266e40412ae9252dfa3c207a5"
    );
    assert_eq!(
        admission.scope.operation.as_str(),
        "corrected-issue-context-report-original"
    );
    let raw = fs::read(&executable).unwrap();
    let runtime = Tool {
        path: executable,
        sha256: digest(&raw),
        bytes: raw.len() as u64,
    };
    assert_eq!(
        runtime.sha256,
        "5c237208ccf8bc9fe13e2cc2ccdc154430c4d4f8ef07c51a613f5ab41a40abe1"
    );
    let before = observe(&admission);
    assert_eq!(
        canonical_digest(&before).unwrap(),
        "cd85666d41c325f334f763463b2f0f2b97e7d10088bb22c8f382a40f07435f87"
    );
    check_waiting(&waiting);
    let old_profile: handoff::Profile = load(&control, "profile.json");
    assert_eq!(
        old_profile.binding.schema,
        "reel.workflow-native-handoff-binding.v2"
    );
    assert!(old_profile.binding.retained_owner_policy.is_some());
    assert_eq!(
        Journal::open(&control.join("journal")).unwrap().state(),
        State::Retained
    );
    assert_eq!(admission.scope.actor, old_profile.admission.scope.actor);
    let root = evidence.join("job");
    fs::create_dir(&root).unwrap();
    let policy = retention::capacity::Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects: 1024,
        logical_bytes: 1024 * 1024 * 1024,
    };
    save(&root, "owner-policy.json", &policy);
    save(
        &root,
        "original-operation.json",
        &json!({"schema":"reel.corrected-context-report-original.v1","task_key":packet.task_key,"attempt_id":"corrected-context-attempt-1","scope":admission.scope,"runtime":runtime,"source_commit":packet.source_commit,"certificate":certified,"control":control,"old_control_runtime":old_profile.runtime,"waiting":waiting,"owner_policy":policy,"no_fresh_identity_on_uncertainty":true,"scripted_transport_only":true,"viewing_verified":false,"finding_closed":false}),
    );
    for name in [
        "packet.json",
        "plan.json",
        "source.json",
        "admission.json",
        "parents.json",
        "upstream.json",
    ] {
        raw_save(&root, name, &fs::read(ready.join(name)).unwrap());
    }
    phase(
        &root,
        "checked-bound-policy-control-close-before-report-admission",
    );
    let close = root.join("control-close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &old_profile.binding.target);
    let out = invoke!(
        &runtime.path,
        "prepare-close",
        control.join("journal"),
        control.join("profile.json"),
        close.join("target.json"),
        close.join("ticket.json"),
        "--apply"
    );
    success(&close, "prepare-result.json", &out);
    let ticket_pin = digest(&fs::read(close.join("ticket.json")).unwrap());
    let out = invoke!(
        &runtime.path,
        "stage-close",
        close.join("ticket.json"),
        &ticket_pin,
        close.join("staged.json"),
        "--apply"
    );
    success(&close, "stage-result.json", &out);
    let staged: closing::Outcome = load(&close, "staged.json");
    assert!(staged.handoff.is_some() && !staged.slot_idle);
    let out = invoke!(
        &runtime.path,
        "recover-close",
        close.join("ticket.json"),
        &ticket_pin,
        close.join("closed.json"),
        "--apply"
    );
    success(&close, "recover-result.json", &out);
    let closed: closing::Outcome = load(&close, "closed.json");
    assert!(closed.slot_idle && closed.original_clear_complete);
    assert_eq!(staged.handoff, closed.handoff);
    let after_close = observe(&admission);
    let out = invoke!(
        &old_profile.runtime.path,
        "resume-handoff",
        control.join("journal"),
        control.join("profile.json"),
        "-",
        "--apply"
    );
    success(&close, "original-runtime-historical-replay.json", &out);
    assert_eq!(after_close, observe(&admission));
    phase(
        &root,
        "accept-one-pinned-report-with-finite-retained-owner-policy",
    );
    let prepared = upstream::Admission::prepare(
        &packet,
        &source,
        &admission,
        "corrected-context-attempt-1",
        parents.borrowed().unwrap(),
    )
    .unwrap();
    assert_eq!(
        prepared.preview().unwrap(),
        load::<admission::Preview>(&ready, "declared-preview.json")
    );
    let receipt = prepared.accept().unwrap();
    save(&root, "receipt.json", &receipt);
    let acknowledgements = root.join("acknowledgements");
    fs::create_dir(&acknowledgements).unwrap();
    let profile = handoff::Profile {
        schema: "reel.workflow-native-handoff.v1".into(),
        source,
        admission,
        binding: handoff::Binding {
            schema: "reel.workflow-native-handoff-binding.v2".into(),
            retained_owner_policy: Some(policy),
            admission: receipt.clone(),
            target: old_profile.binding.target.clone(),
        },
        runtime: runtime.clone(),
        acknowledgements,
    };
    save(&root, "profile.json", &profile);
    let out = invoke!(
        &runtime.path,
        "create-handoff",
        root.join("packet.json"),
        root.join("profile.json"),
        root.join("journal"),
        "--apply"
    );
    success(&root, "create-result.json", &out);
    phase(
        &root,
        "open-admitted-v2-origins-and-stage-both-reading-images",
    );
    let stage_parent = root.join("input-workspaces");
    fs::create_dir(&stage_parent).unwrap();
    let staging_profile = staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: stage_parent,
        maximum_bytes: 64 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 64 * 1024 * 1024,
            ..Default::default()
        },
    };
    let ticket = staging::prepare(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &staging_profile,
    )
    .unwrap();
    save(&root, "input-ticket.json", &ticket);
    let ticket_pin = digest(&fs::read(root.join("input-ticket.json")).unwrap());
    let out = invoke!(
        &runtime.path,
        "export-stage",
        root.join("packet.json"),
        root.join("source.json"),
        root.join("admission.json"),
        root.join("input-ticket.json"),
        &ticket_pin
    );
    success(&root, "export-stage-result.json", &out);
    let out = invoke!(
        &runtime.path,
        "export-review-brief",
        root.join("packet.json"),
        root.join("source.json"),
        root.join("admission.json"),
        root.join("receipt.json"),
        root.join("review-brief.json"),
        "--apply"
    );
    success(&root, "export-brief-result.json", &out);
    let brief: review::Brief = load(&root, "review-brief.json");
    assert_eq!(brief.units.len(), 6);
    assert_eq!(brief.evidence.len(), 6);
    let mut native =
        NativeInputs::open_admitted(&packet, &profile.source, &profile.admission, &receipt)
            .unwrap();
    let bound = review::bind(&packet, &mut native).unwrap();
    let production = bound.production().unwrap();
    assert_eq!(
        production.packet.task_key,
        "69d0d1a011e1a088c283dc64d3e29146a4be3c1a2b5fd931d8e334fb9c536ac9"
    );
    let witness_key = production.packet.task_key.clone();
    let semantic = parents
        .parents
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                reel_workflow::upstream::Parent {
                    packet: &p.packet,
                    result: &p.result,
                },
            )
        })
        .collect();
    let frozen = profile.source.snapshot.upstream.as_ref().unwrap();
    review_control::check_request_origin(&packet, production, &frozen.bindings, &semantic).unwrap();
    let mut foreign = production.clone();
    foreign.result.attempt_id = "foreign-producer-attempt".into();
    assert_eq!(
        review_control::check_request_origin(&packet, &foreign, &frozen.bindings, &semantic)
            .unwrap_err()
            .code,
        "review-production-control-producer"
    );
    let request_pin = packet
        .task
        .inputs
        .iter()
        .find(|p| p.logical_id == review::REQUEST_ID)
        .unwrap();
    let request: review::Request =
        decode(&native.read(request_pin, MAX_CONTROL_BYTES as u64).unwrap()).unwrap();
    let copy_pin = packet
        .task
        .inputs
        .iter()
        .find(|p| p.logical_id == request.copy_input)
        .unwrap();
    let copy: review::Copy =
        decode(&native.read(copy_pin, MAX_CONTROL_BYTES as u64).unwrap()).unwrap();
    let manifest = native.read(&copy.review, MAX_CONTROL_BYTES as u64).unwrap();
    let images = review::comic_reading_inputs(&copy, &manifest).unwrap();
    assert_eq!(images.len(), 2);
    let input_paths: BTreeMap<String, String> = load(&ticket.directory(), "input-bindings.json");
    for image in &images {
        assert!(brief.evidence.contains(image));
        let raw = native.read(image, image.bytes).unwrap();
        assert!(raw.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            raw,
            fs::read(ticket.directory().join(&input_paths[&image.logical_id])).unwrap()
        );
    }
    let report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(&copy).unwrap(),
        findings: vec![],
    };
    drop(native);
    phase(
        &root,
        "refuse-stale-or-unbound-returned-reports-before-import-intent",
    );
    let waiting_record = fs::read(root.join("journal/00000.json")).unwrap();
    let before_returns = observe(&profile.admission);
    for mode in ["copy", "unit", "evidence"] {
        let refused = root.join(format!("refused-{mode}"));
        fs::create_dir(&refused).unwrap();
        let mut bad = report.clone();
        let mut finding = review::Finding {
            code: "synthetic-return-check".into(),
            unit: copy.units.keys().next().unwrap().clone(),
            frames: None,
            modality: review::Modality::Text,
            route: review::Route::Project,
            requested_change: "Negative transport check only; no creative defect asserted.".into(),
            evidence: vec![copy.review.clone()],
        };
        match mode {
            "copy" => bad.copy_sha256 = "0".repeat(64),
            "unit" => finding.unit = "unknown-unit".into(),
            "evidence" => finding.evidence[0].sha256 = "0".repeat(64),
            _ => unreachable!(),
        }
        bad.findings.push(finding);
        let raw = serde_json::to_vec(&bad).unwrap();
        raw_save(&refused, "report.json", &raw);
        let result = ReturnedResult {
            schema: RESULT_SCHEMA.into(),
            task_id: packet.task.id.clone(),
            task_key: packet.task_key.clone(),
            attempt_id: receipt.attempt_id.clone(),
            artifacts: BTreeMap::from([(
                "report".into(),
                Artifact {
                    logical_id: format!(
                        "workflow/review/{}/{}/report",
                        packet.task_key, receipt.attempt_id
                    ),
                    sha256: digest(&raw),
                    bytes: raw.len() as u64,
                    media_type: "application/json".into(),
                },
            )]),
            provenance: Provenance {
                mode: Mode::Handoff,
                provider: None,
                model: None,
                executable: None,
            },
            worker_receipt: None,
        };
        save(&refused, "result.json", &result);
        save(
            &refused,
            "returns.json",
            &response::Returns {
                schema: "reel.workflow-returns.v1".into(),
                root: refused.clone(),
                files: BTreeMap::from([("report".into(), "report.json".into())]),
            },
        );
        let out = invoke!(
            &runtime.path,
            "import-handoff",
            root.join("journal"),
            root.join("profile.json"),
            refused.join("result.json"),
            refused.join("returns.json"),
            "--apply"
        );
        save(
            &refused,
            "import-result.json",
            &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)}),
        );
        assert!(!out.status.success());
        let failure: serde_json::Value = decode(&out.stdout).unwrap();
        assert_eq!(
            failure["error"]["code"],
            match mode {
                "copy" => "review-copy-pin",
                "unit" => "review-finding-unit",
                "evidence" => "review-task-evidence-pin",
                _ => unreachable!(),
            }
        );
        assert_eq!(
            Journal::open(&root.join("journal")).unwrap().state(),
            State::Waiting
        );
        assert_eq!(fs::read_dir(root.join("journal")).unwrap().count(), 1);
        assert_eq!(
            waiting_record,
            fs::read(root.join("journal/00000.json")).unwrap()
        );
        assert_eq!(before_returns, observe(&profile.admission));
    }
    phase(
        &root,
        "retain-scripted-empty-context-report-without-viewing-or-closure-claims",
    );
    let returns = root.join("returns");
    fs::create_dir(&returns).unwrap();
    let raw = serde_json::to_vec_pretty(&report).unwrap();
    raw_save(&returns, "report.json", &raw);
    let packed = bound.pack_handoff(&receipt.attempt_id, &raw).unwrap();
    let out = invoke!(
        &runtime.path,
        "pack-review-result",
        root.join("packet.json"),
        root.join("source.json"),
        root.join("admission.json"),
        root.join("receipt.json"),
        returns.join("report.json"),
        root.join("result.json"),
        "--apply"
    );
    success(&root, "pack-result.json", &out);
    let result: ReturnedResult = load(&root, "result.json");
    assert_eq!(result, packed);
    save(
        &root,
        "returns.json",
        &response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: returns,
            files: BTreeMap::from([("report".into(), "report.json".into())]),
        },
    );
    let out = invoke!(
        &runtime.path,
        "import-handoff",
        root.join("journal"),
        root.join("profile.json"),
        root.join("result.json"),
        root.join("returns.json"),
        "--apply"
    );
    success(&root, "import-result.json", &out);
    assert_eq!(
        Journal::open(&root.join("journal")).unwrap().state(),
        State::Retained
    );
    let ack: retention::Acknowledgement = load(
        &profile.acknowledgements,
        &format!("retained-{}-{}.json", packet.task_key, receipt.attempt_id),
    );
    save(&root, "retained.json", &ack);
    let complete = observe(&profile.admission);
    let out = invoke!(
        &runtime.path,
        "resume-handoff",
        root.join("journal"),
        root.join("profile.json"),
        "-",
        "--apply"
    );
    success(&root, "completed-replay.json", &out);
    assert_eq!(complete, observe(&profile.admission));
    let guard = retention::verify(
        &packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    let checked = guard.review_report().unwrap();
    assert!(checked.findings().is_empty());
    assert_eq!(checked.production().unwrap().packet.task_key, witness_key);
    checked
        .check_production_task(&plan, "issue-assemble")
        .unwrap();
    save(&root, "checked-report.json", checked);
    drop(guard);
    for p in parents.parents.values() {
        verify_parent(p);
    }
    check_waiting(&waiting);
    certificate(&certified);
    assert_eq!(complete, observe(&profile.admission));
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.corrected-v2-context-report-qualification.v1","job":root,"task_key":packet.task_key,"attempt_id":receipt.attempt_id,"operation":profile.admission.scope.operation,"activation":receipt.activation,"work":receipt.work,"runtime":runtime,"retention_work":ack.work,"old_control_close":closed,"after_checkpoint_sha256":canonical_digest(&complete).unwrap(),"native_v2_origins_checked_under_own_claims":true,"foreign_producer_attempt_refused":true,"pages":2,"units":6,"reading_images_declared_and_staged":2,"stale_copy_unknown_unit_wrong_evidence_refused_before_intent":true,"refusals_preserved_index_and_waiting_record":true,"report_retained":true,"findings":0,"production_task_key":witness_key,"native_history_rechecks_production_witness":true,"completed_replay_index_unchanged":true,"parent_history_preserved":true,"waiting_claims_reopened":true,"provider_called":false,"viewing_verified":false,"finding_closed":false,"selected":false,"approved":false,"scratch_retired":false}),
    );
    phase(
        &root,
        "retained-v2-report-awaiting-explicit-contextual-evaluation-and-resolution",
    );
}
