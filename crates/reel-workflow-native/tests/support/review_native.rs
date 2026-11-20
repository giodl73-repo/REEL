//! Producer-derived comic review -> explicit native structured report lifecycle.
use super::*;
use reel_workflow::publication::InputReader;
use reel_workflow_native::{NativeInputs, NativeProfile, admission::Prepared, response, retention};
#[test]
#[ignore = "Existing synthetic native pool and prior comic review export only; admits/retains structured report, preserves roots"]
fn structured_report_refuses_bad_findings_before_registration_and_reopens_native_history() {
    let descriptor: serde_json::Value = serde_json::from_slice(
        &fs::read(
            std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set existing pool descriptor"),
        )
        .unwrap(),
    )
    .unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(descriptor["source"].clone()).unwrap();
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REVIEW_NATIVE_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exported = PathBuf::from(
        std::env::var_os("REEL_COMIC_REVIEW_EXPORT")
            .expect("Set qualified prior comic review export directory"),
    );
    let copy_raw = fs::read(exported.join("comic-review-copy.json")).unwrap();
    let copy: review::Copy = decode(&copy_raw).unwrap();
    let manifest_raw = fs::read(exported.join("comic-review-manifest.json")).unwrap();
    assert_eq!(copy.review.sha256, digest(&manifest_raw));
    let manifest: serde_json::Value = decode(&manifest_raw).unwrap();
    let request_raw = serde_json::to_vec(&review::Request {
        production: None,
        schema: "reel.workflow-review-request.v1".into(),
        production_commit: copy.source_commit.clone(),
        copy_input: "control/review-copy".into(),
        report_output: "report".into(),
    })
    .unwrap();
    let pin = |id: &str, raw: &[u8]| Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    };
    let mut pins = vec![
        pin(review::REQUEST_ID, &request_raw),
        pin("control/review-copy", &copy_raw),
        copy.publication.clone(),
        copy.review.clone(),
    ];
    let reading: BTreeMap<String, Artifact> =
        serde_json::from_value(manifest["reading"].clone()).unwrap();
    pins.extend(reading.into_values());
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let template: serde_json::Value =
        decode(&fs::read(fixture.temp.path().join("records/input.tig")).unwrap()).unwrap();
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, pin) in pins.iter().enumerate() {
        let record = format!("records/review-{i}.tig");
        let payload = format!("payloads/review-{i}.bin");
        let object = Object {
            sha256: tig_core::Digest::parse(&pin.sha256).unwrap(),
            bytes: pin.bytes,
        };
        let mut value = template.clone();
        value["id"] = json!(pin.logical_id);
        value["selected"] = serde_json::to_value(&object).unwrap();
        value["streams"] = json!([]);
        write(fixture.temp.path(), &record, value);
        fs::write(
            fixture.temp.path().join(&payload),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.bindings.insert(
            pin.logical_id.clone(),
            Binding {
                selected: LogicalPath::parse(&payload).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        fixture
            .profile
            .input_streams
            .insert(pin.logical_id.clone(), None);
    }
    git(fixture.temp.path(), &["add", "."]);
    git(
        fixture.temp.path(),
        &[
            "commit",
            "-m",
            "synthetic structured review consumer inputs",
        ],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    let git_raw = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_raw),
        bytes: git_raw.len() as u64,
    };
    fixture.packet.project = copy.project.clone();
    fixture.packet.task.operation = Operation::ReviewReport;
    fixture.packet.task.language = Some(copy.language.clone());
    fixture.packet.task.inputs = pins;
    fixture.packet.task.outputs = BTreeMap::from([(
        "report".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: MAX_CONTROL_BYTES as u64,
            required: true,
        },
    )]);
    fixture.repin();
    assert_ne!(fixture.packet.source_commit, copy.source_commit);
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        movie_verifier: None,
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(descriptor["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let save = |name: &str, value: &serde_json::Value| {
        fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap()
    };
    let marker = json!({"root":root,"source":fixture.temp.path(),"task_key":fixture.packet.task_key,"production_commit":copy.source_commit,"consumer_commit":fixture.packet.source_commit});
    save("original-operation.json", &marker);
    fs::write(
        evidence.join("original-operation-roots.json"),
        serde_json::to_vec(&marker).unwrap(),
    )
    .unwrap();
    save(
        "packet.json",
        &serde_json::to_value(&fixture.packet).unwrap(),
    );
    save("source.json", &serde_json::to_value(&case.source).unwrap());
    save(
        "admission.json",
        &serde_json::to_value(&case.profile).unwrap(),
    );
    save("phase.json", &json!({"phase":"accept-original-job"}));
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    save("receipt.json", &serde_json::to_value(&receipt).unwrap());
    let ingress = root.join("input-returns");
    fs::create_dir(&ingress).unwrap();
    save(
        "phase.json",
        &json!({"phase":"publish-declared-new-controls"}),
    );
    for (name, raw) in [
        ("request.json", request_raw),
        ("copy.json", copy_raw),
        ("manifest.json", manifest_raw),
    ] {
        fs::write(ingress.join(name), &raw).unwrap();
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: ingress.clone(),
                source: LogicalPath::parse(name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(&raw)),
                include_private_location: false,
                maximum_bytes: MAX_CONTROL_BYTES as u64,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let mut inputs =
        NativeInputs::open_admitted(&fixture.packet, &case.source, &case.profile, &receipt)
            .unwrap();
    for pin in &fixture.packet.task.inputs {
        inputs.read(pin, pin.bytes).unwrap();
    }
    drop(inputs);
    let handoff_profile = handoff_profile(&case, receipt.clone(), std::env::current_exe().unwrap());
    save(
        "handoff-profile.json",
        &serde_json::to_value(&handoff_profile).unwrap(),
    );
    let journal = root.join("journal");
    assert_eq!(
        reel_workflow_native::handoff::create(&journal, &fixture.packet, &handoff_profile).unwrap(),
        handoff::State::Waiting
    );
    let waiting_record = fs::read(journal.join("00000.json")).unwrap();
    let returns = root.join("returns");
    fs::create_dir(&returns).unwrap();
    let mut report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: "b".repeat(64),
        findings: vec![review::Finding {
            code: "synthetic-line-review".into(),
            unit: copy.units.keys().next().unwrap().clone(),
            frames: None,
            modality: review::Modality::Text,
            route: review::Route::Project,
            requested_change: "Synthetic candidate observation; preserve approved source wording."
                .into(),
            evidence: vec![copy.review.clone()],
        }],
    };
    let make_result = |raw: &[u8]| ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: fixture.packet.task.id.clone(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([("report".into(), pin("candidate/review-report", raw))]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    let returned = response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: returns.clone(),
        files: BTreeMap::from([("report".into(), "report.json".into())]),
    };
    let raw = serde_json::to_vec(&report).unwrap();
    fs::write(returns.join("report.json"), &raw).unwrap();
    let bad = make_result(&raw);
    save("refused-result.json", &serde_json::to_value(&bad).unwrap());
    save(
        "phase.json",
        &json!({"phase":"refuse-stale-findings-before-registration"}),
    );
    let error = reel_workflow_native::handoff::import(&journal, &handoff_profile, bad, &returned)
        .unwrap_err();
    assert_eq!(error.code, "review-copy-pin");
    assert_eq!(
        handoff::Journal::open(&journal).unwrap().state(),
        handoff::State::Waiting
    );
    assert_eq!(
        waiting_record,
        fs::read(journal.join("00000.json")).unwrap()
    );
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 1);
    save("refused.json", &serde_json::to_value(error).unwrap());
    assert_eq!(
        before,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    // Preserve the refused caller-owned bytes, then supply a valid response
    // under the original attempt; no candidate effect occurred for the refusal.
    let owned = returns.canonicalize().unwrap();
    let refused_source = owned.join("report.json").canonicalize().unwrap();
    let refused_destination = owned.join("refused-report.json");
    assert_eq!(refused_source.parent(), Some(owned.as_path()));
    assert_eq!(refused_destination.parent(), Some(owned.as_path()));
    fs::rename(refused_source, refused_destination).unwrap();
    report.copy_sha256 = canonical_digest(&copy).unwrap();
    let raw = serde_json::to_vec(&report).unwrap();
    fs::write(returns.join("report.json"), &raw).unwrap();
    let result = make_result(&raw);
    save("result.json", &serde_json::to_value(&result).unwrap());
    save("returns.json", &serde_json::to_value(&returned).unwrap());
    save(
        "phase.json",
        &json!({"phase":"ingest-valid-original-attempt"}),
    );
    assert_eq!(
        reel_workflow_native::handoff::import(
            &journal,
            &handoff_profile,
            result.clone(),
            &returned
        )
        .unwrap(),
        handoff::State::Retained
    );
    let ack: retention::Acknowledgement = serde_json::from_slice(
        &fs::read(handoff_profile.acknowledgements.join(format!(
            "retained-{}-attempt-1.json",
            fixture.packet.task_key
        )))
        .unwrap(),
    )
    .unwrap();
    save(
        "candidate.json",
        &serde_json::to_value(&ack.candidate).unwrap(),
    );
    save("ack.json", &serde_json::to_value(&ack).unwrap());
    let claims = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    assert_eq!(
        reel_workflow_native::handoff::resume(&journal, &handoff_profile, None).unwrap(),
        handoff::State::Retained
    );
    assert_eq!(
        claims,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    // Historical read uses bare Git metadata and native bytes, not local returns.
    let history = root.join("history.git");
    git(
        root,
        &[
            "clone",
            "--bare",
            "--no-hardlinks",
            fixture.temp.path().to_str().unwrap(),
            history.to_str().unwrap(),
        ],
    );
    case.source.snapshot.repository = history;
    save(
        "historical-source.json",
        &serde_json::to_value(&case.source).unwrap(),
    );
    save(
        "phase.json",
        &json!({"phase":"verify-retained-report-from-bare-history"}),
    );
    let guard = retention::verify(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    assert_eq!(guard.review_report().unwrap().findings().len(), 1);
    drop(guard);
    assert_eq!(
        claims,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    save("phase.json", &json!({"phase":"verified"}));
    let outcome = json!({"root":root,"source":fixture.temp.path(),"stale_report_refused_before_registration":true,"refused_journal_still_waiting":true,"refused_claims_unchanged":true,"original_attempt_used":true,"structured_report_native_retained":true,"retained_journal_replay_without_returns":true,"bare_history_semantics_verified":true,"history_claims_unchanged":true,"source_job_active":true,"task_completed":true,"selected":false});
    save("qualification-result.json", &outcome);
    fs::write(
        evidence.join("qualification-result.json"),
        serde_json::to_vec(&outcome).unwrap(),
    )
    .unwrap();
}
