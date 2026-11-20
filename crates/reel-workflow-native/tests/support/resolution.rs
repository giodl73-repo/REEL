//! Original retained repair + contextual report -> checked resolution proposal.
use super::*;
use reel_workflow_native::{
    NativeInputs, NativeProfile, admission::Prepared, graph, resolution::RetainedReview, retention,
};
fn load<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> T {
    decode(&fs::read(p).unwrap()).unwrap()
}
fn save(p: impl AsRef<Path>, v: &impl serde::Serialize) {
    fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
fn success(o: &std::process::Output) {
    assert!(
        o.status.success(),
        "{} {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
fn bundle(root: &Path) -> RetainedReview {
    RetainedReview {
        packet: load(root.join("packet.json")),
        source: load(root.join("source.json")),
        admission: load(root.join("admission.json")),
        receipt: load(root.join("receipt.json")),
        result: load(root.join("result.json")),
        acknowledgement: load(root.join("ack.json")),
    }
}
#[test]
#[ignore = "Existing retained corrected comic and original finding only; new protected follow-up report and private native resolution export; preserves roots"]
fn corrected_comic_follow_up_binds_retained_production_and_resolution() {
    let original = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_BASELINE_REVIEW")
            .expect("Set original retained review root"),
    );
    let corrected = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_CORRECTED_COMIC")
            .expect("Set retained corrected comic root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exe = PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set frozen CLI"));
    let before = bundle(&original);
    let old_claims = fs::read(before.admission.index_root.join("claims.json")).unwrap();
    let producer: NativeProfile = load(corrected.join("source.json"));
    let producer_profile: reel_workflow_native::handoff::Profile =
        load(corrected.join("handoff-profile.json"));
    let producer_claims =
        fs::read(producer_profile.admission.index_root.join("claims.json")).unwrap();
    let baseline: Plan = load(corrected.join("baseline.json"));
    let repair: review::Repair = load(corrected.join("repair.json"));
    let revision: reel_workflow::repair::Revision = load(corrected.join("revision.json"));
    let proof = retention::verify(
        &before.packet,
        &before.source,
        &before.admission,
        &before.receipt,
        &before.result,
        &before.acknowledgement,
    )
    .unwrap();
    let checked = proof.review_report().unwrap().clone();
    drop(proof);
    let candidate =
        reel_workflow::repair::materialize(&baseline, &checked, &repair, &revision).unwrap();
    let actual_plan: Plan = load(corrected.join("automatic-plan.json"));
    assert_eq!(
        canonical_digest(&candidate.plan).unwrap(),
        canonical_digest(&actual_plan).unwrap()
    );
    let rebuilds: graph::Bindings = load(corrected.join("automatic-bindings.json"));
    let copy_raw = fs::read(corrected.join("corrected-review-copy.json")).unwrap();
    let copy: review::Copy = decode(&copy_raw).unwrap();
    assert_eq!(copy.source_commit, candidate.plan.source_commit);
    let manifest_raw = fs::read(corrected.join("corrected-review-manifest.json")).unwrap();
    assert_eq!(digest(&manifest_raw), copy.review.sha256);
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
    let template: serde_json::Value = load(fixture.temp.path().join("records/input.tig"));
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, pin) in pins.iter().enumerate() {
        let record = format!("records/follow-up-{i}.tig");
        let payload = format!("payloads/follow-up-{i}.bin");
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
            "synthetic corrected contextual review inputs",
        ],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    fixture.profile.git = producer.snapshot.git.clone();
    fixture.packet.project = copy.project.clone();
    fixture.packet.task.id = "corrected-context-review".into();
    fixture.packet.task.operation = Operation::ReviewReport;
    fixture.packet.task.prompt =
        "Synthetic technical follow-up for the full corrected comic context; no creative approval."
            .into();
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
    let mut case = AdmissionCase::with_store(&fixture, producer.source.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = producer;
    case.source.snapshot = fixture.profile.clone();
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let marker = json!({"root":root,"source":fixture.temp.path(),"baseline_review":original,"corrected_comic":corrected,"task_key":fixture.packet.task_key,"runtime":exe});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    fs::write(root.join("baseline-claims-before.bin"), &old_claims).unwrap();
    fs::write(root.join("producer-claims-before.bin"), &producer_claims).unwrap();
    save(root.join("packet.json"), &fixture.packet);
    save(root.join("source.json"), &case.source);
    save(root.join("admission.json"), &case.profile);
    save(root.join("baseline-bundle.json"), &before);
    save(root.join("baseline-plan.json"), &baseline);
    save(root.join("repair.json"), &repair);
    save(root.join("revision.json"), &revision);
    save(root.join("rebuild-bindings.json"), &rebuilds);
    save(root.join("candidate-plan.json"), &candidate.plan);
    save(
        root.join("phase.json"),
        &json!({"phase":"accept-follow-up"}),
    );
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    save(root.join("receipt.json"), &receipt);
    let ingress = root.join("input-returns");
    fs::create_dir(&ingress).unwrap();
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
    let profile = handoff_profile(&case, receipt.clone(), exe.clone());
    save(root.join("profile.json"), &profile);
    let journal = root.join("journal");
    let created = Command::new(&exe)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(root.join("profile.json"))
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    success(&created);
    let report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(&copy).unwrap(),
        findings: vec![],
    };
    let report_raw = serde_json::to_vec(&report).unwrap();
    let mut native =
        NativeInputs::open_admitted(&fixture.packet, &case.source, &case.profile, &receipt)
            .unwrap();
    let bound = review::bind(&fixture.packet, &mut native).unwrap();
    let result = bound.pack_handoff("attempt-1", &report_raw).unwrap();
    drop(native);
    drop(bound);
    let returned = root.join("returned");
    fs::create_dir(&returned).unwrap();
    fs::write(returned.join("report.json"), &report_raw).unwrap();
    save(root.join("result.json"), &result);
    let returns = reel_workflow_native::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: returned,
        files: BTreeMap::from([("report".into(), "report.json".into())]),
    };
    save(root.join("returns.json"), &returns);
    save(
        root.join("phase.json"),
        &json!({"phase":"retain-follow-up"}),
    );
    let imported = Command::new(&exe)
        .arg("import-handoff")
        .arg(&journal)
        .arg(root.join("profile.json"))
        .arg(root.join("result.json"))
        .arg(root.join("returns.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&imported);
    fs::write(root.join("import-stdout.json"), imported.stdout).unwrap();
    let ack: retention::Acknowledgement = load(profile.acknowledgements.join(format!(
        "retained-{}-attempt-1.json",
        fixture.packet.task_key
    )));
    save(root.join("ack.json"), &ack);
    let after = bundle(root);
    save(
        root.join("corrected-bundles.json"),
        &BTreeMap::from([("es", &after)]),
    );
    let decision=reel_workflow::resolution::Decision{schema:"reel.workflow-resolution-decision.v1".into(),baseline_report_sha256:checked.report_sha256().into(),candidate_plan_sha256:candidate.plan_sha256,contexts:BTreeMap::from([("es".into(),reel_workflow::resolution::Context{report_sha256:canonical_digest(&report).unwrap(),production_task:"comic-render".into(),unit_modalities:copy.units.keys().map(|k|(k.clone(),vec![review::Modality::Text])).collect()})]),unit_map:checked.copy().units.keys().map(|k|(k.clone(),k.clone())).collect(),evaluations:checked.findings().iter().map(|(k,f)|reel_workflow::resolution::Evaluation{finding_key:k.clone(),corrected_unit:f.finding().unit.clone(),rationale:"Scripted technical follow-up declaration for retained corrected wording; not actual viewing or approval.".into(),evidence:vec![copy.review.clone()]}).collect()};
    save(root.join("decision.json"), &decision);
    save(
        root.join("phase.json"),
        &json!({"phase":"export-resolution"}),
    );
    let exported = Command::new(&exe)
        .arg("export-resolution")
        .arg(root.join("baseline-bundle.json"))
        .arg(root.join("baseline-plan.json"))
        .arg(root.join("repair.json"))
        .arg(root.join("revision.json"))
        .arg(root.join("rebuild-bindings.json"))
        .arg(root.join("corrected-bundles.json"))
        .arg(root.join("decision.json"))
        .arg(root.join("proposal.json"))
        .arg("--apply")
        .output()
        .unwrap();
    fs::write(root.join("export-stdout.json"), &exported.stdout).unwrap();
    success(&exported);
    let proposal: serde_json::Value = load(root.join("proposal.json"));
    assert_eq!(
        proposal["proposal"]["finding_keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(proposal["proposal"]["remote_issues_closed"], false);
    assert_eq!(
        proposal["productions"]["es"]["task_key"],
        load::<Packet>(corrected.join("packet.json")).task_key
    );
    assert_eq!(
        proposal["productions"]["es"]["copy_sha256"],
        canonical_digest(&copy).unwrap()
    );
    assert_eq!(
        old_claims,
        fs::read(before.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        producer_claims,
        fs::read(producer_profile.admission.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"source":fixture.temp.path(),"original_baseline_review":original,"retained_corrected_comic":corrected,"follow_up_retained":true,"producer_copy_exact":true,"resolution_proposed":true,"baseline_and_producer_claims_unchanged":true,"provider_called":false,"rendered":false,"issue_closed":false,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
