//! Existing corrected-comic evidence -> owned typed resolution retention.
use super::*;
use reel_workflow_native::{
    NativeProfile,
    admission::Prepared,
    resolution_job::{REQUEST_ID, Request},
    retention,
};
fn load<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> T {
    decode(&fs::read(p).unwrap()).unwrap()
}
fn save(p: impl AsRef<Path>, v: &impl serde::Serialize) {
    fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
#[test]
#[ignore = "Existing qualified resolution controls and native pool only; typed owned resolution production, complete input protection, retention and replay; preserves roots"]
fn owned_resolution_protects_parent_payloads_and_reopens_typed_native_proof() {
    let existing = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_LINEAGE_ROOT")
            .expect("Set qualified resolution lineage root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_RESOLUTION_OWNED_EVIDENCE_DIRECTORY")
            .expect("Set evidence directory"),
    );
    let exe = PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set frozen CLI"));
    let request = Request::capture(
        load(existing.join("baseline-bundle.json")),
        load(existing.join("baseline-plan.json")),
        load(existing.join("repair.json")),
        load(existing.join("revision.json")),
        load(existing.join("rebuild-bindings.json")),
        load(existing.join("corrected-bundles.json")),
        load(existing.join("decision.json")),
    )
    .unwrap();
    let resources = request.resources().unwrap();
    let request_raw = serde_json::to_vec(&request).unwrap();
    let plan_raw = serde_json::to_vec(&request.plan).unwrap();
    let pin = |id: &str, raw: &[u8]| Artifact {
        logical_id: id.into(),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    };
    let request_pin = pin(REQUEST_ID, &request_raw);
    let plan_pin = pin("control/resolution-plan", &plan_raw);
    let mut inputs = resources.clone();
    inputs.extend([request_pin.clone(), plan_pin.clone()]);
    let original_source: NativeProfile = load(existing.join("source.json"));
    let before_claims =
        fs::read(request.baseline.admission.index_root.join("claims.json")).unwrap();
    let after_claims = fs::read(
        request.corrected["es"]
            .admission
            .index_root
            .join("claims.json"),
    )
    .unwrap();
    let producer_claims = fs::read(
        request.productions["comic-render"]
            .profile
            .admission
            .index_root
            .join("claims.json"),
    )
    .unwrap();
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let template: serde_json::Value = load(fixture.temp.path().join("records/input.tig"));
    fixture.profile.records.clear();
    fixture.profile.bindings.clear();
    fixture.profile.input_streams.clear();
    for (i, input) in inputs.iter().enumerate() {
        let record = format!("records/resolution-{i}.tig");
        let leaf = format!("payloads/resolution-{i}.bin");
        let object = Object {
            sha256: tig_core::Digest::parse(&input.sha256).unwrap(),
            bytes: input.bytes,
        };
        let mut value = template.clone();
        value["id"] = json!(input.logical_id);
        value["selected"] = serde_json::to_value(&object).unwrap();
        value["streams"] = json!([]);
        write(fixture.temp.path(), &record, value);
        fs::write(
            fixture.temp.path().join(&leaf),
            Key::from_object(&object).unlocked_pointer(),
        )
        .unwrap();
        fixture.profile.records.push(record);
        fixture.profile.bindings.insert(
            input.logical_id.clone(),
            Binding {
                selected: LogicalPath::parse(&leaf).unwrap(),
                streams: BTreeMap::new(),
            },
        );
        fixture
            .profile
            .input_streams
            .insert(input.logical_id.clone(), None);
    }
    git(fixture.temp.path(), &["add", "."]);
    git(
        fixture.temp.path(),
        &["commit", "-m", "synthetic owned resolution closure"],
    );
    fixture.profile.commit = git(fixture.temp.path(), &["rev-parse", "HEAD"]);
    fixture.profile.git = original_source.snapshot.git.clone();
    fixture.packet.project = request.plan.project.clone();
    fixture.packet.task.id = "retained-resolution".into();
    fixture.packet.task.operation = Operation::DeterministicTool;
    fixture.packet.task.prompt = String::new();
    fixture.packet.task.language = Some("es".into());
    fixture.packet.task.inputs = inputs;
    fixture.packet.task.outputs = BTreeMap::from([(
        "resolution.json".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: MAX_CONTROL_BYTES as u64,
            required: true,
        },
    )]);
    let exe_raw = fs::read(&exe).unwrap();
    fixture.packet.task.tool = Some(reel_workflow::ToolContract {
        schema: "reel.workflow-tool.v1".into(),
        capability: ToolCapability::RepairResolution,
        publication: plan_pin,
        request: request_pin,
        executable: Artifact {
            logical_id: "tool/native-resolution".into(),
            sha256: digest(&exe_raw),
            bytes: exe_raw.len() as u64,
            media_type: "application/octet-stream".into(),
        },
        edition_id: "resolution".into(),
        source_bindings: resources
            .iter()
            .map(|a| {
                (
                    format!("resources/{}-{}", a.sha256, a.bytes),
                    a.logical_id.clone(),
                )
            })
            .collect(),
        auxiliary_executables: BTreeMap::new(),
        part: None,
    });
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, original_source.source.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = original_source;
    case.source.snapshot = fixture.profile.clone();
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let marker = json!({"root":root,"source":fixture.temp.path(),"lineage":existing,"task_key":fixture.packet.task_key,"resource_objects":resources.len(),"request_bytes":request_raw.len(),"runtime":exe});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    save(root.join("packet.json"), &fixture.packet);
    save(root.join("source.json"), &case.source);
    save(root.join("admission.json"), &case.profile);
    save(root.join("request.json"), &request);
    fs::write(root.join("baseline-claims-before.bin"), &before_claims).unwrap();
    fs::write(root.join("follow-up-claims-before.bin"), &after_claims).unwrap();
    fs::write(root.join("producer-claims-before.bin"), &producer_claims).unwrap();
    save(
        root.join("phase.json"),
        &json!({"phase":"preview-typed-resolution"}),
    );
    let prepared =
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1").unwrap();
    let preview = prepared.preview().unwrap();
    save(root.join("preview.json"), &preview);
    save(
        root.join("phase.json"),
        &json!({"phase":"accept-typed-resolution"}),
    );
    let receipt = prepared.accept().unwrap();
    save(root.join("receipt.json"), &receipt);
    for asset in &resources {
        assert!(
            case.protected(&Object {
                sha256: tig_core::Digest::parse(&asset.sha256).unwrap(),
                bytes: asset.bytes
            })
            .contains(&case.profile.scope.actor)
        );
    }
    let ingress = root.join("controls");
    fs::create_dir(&ingress).unwrap();
    for (name, raw) in [("request.json", request_raw), ("plan.json", plan_raw)] {
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
    let profile_path = root.join("profile.json");
    save(&profile_path, &profile);
    let journal = root.join("journal");
    let created = Command::new(&exe)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(&profile_path)
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    assert!(
        created.status.success(),
        "{}",
        String::from_utf8_lossy(&created.stdout)
    );
    save(
        root.join("phase.json"),
        &json!({"phase":"dispatch-typed-resolution"}),
    );
    let mut controls = Vec::new();
    automatic_dispatch::run_comic(
        &case,
        &fixture.packet,
        &profile,
        &journal,
        &profile_path,
        &exe,
        &root.join("result.json"),
        &mut controls,
        &BTreeMap::new(),
    );
    for (name, raw) in controls {
        fs::write(root.join(name), raw).unwrap();
    }
    let result: ReturnedResult = load(root.join("result.json"));
    let ack: retention::Acknowledgement = load(profile.acknowledgements.join(format!(
        "retained-{}-attempt-1.json",
        fixture.packet.task_key
    )));
    save(root.join("ack.json"), &ack);
    save(
        root.join("phase.json"),
        &json!({"phase":"reopen-native-resolution"}),
    );
    let proof = retention::verify(
        &fixture.packet,
        &case.source,
        &case.profile,
        &receipt,
        &result,
        &ack,
    )
    .unwrap();
    let typed = proof.resolution_proposal().unwrap();
    assert_eq!(typed.proposal.finding_keys.len(), 1);
    assert!(!typed.proposal.remote_issues_closed);
    assert_eq!(
        typed.productions["es"].task_key,
        request.productions["comic-render"].packet.task_key
    );
    let expected: serde_json::Value = load(existing.join("proposal.json"));
    assert_eq!(serde_json::to_value(typed).unwrap(), expected);
    drop(proof);
    assert_eq!(
        before_claims,
        fs::read(request.baseline.admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        after_claims,
        fs::read(
            request.corrected["es"]
                .admission
                .index_root
                .join("claims.json")
        )
        .unwrap()
    );
    assert_eq!(
        producer_claims,
        fs::read(
            request.productions["comic-render"]
                .profile
                .admission
                .index_root
                .join("claims.json")
        )
        .unwrap()
    );
    let outcome = json!({"root":root,"source":fixture.temp.path(),"lineage":existing,"resource_objects":resources.len(),"complete_payload_closure_protected":true,"owned_dispatch_retained":true,"replay_without_claim_changes":true,"typed_native_proof_reopened":true,"proposal_matches_qualified_lineage":true,"parent_claims_unchanged":true,"provider_called":false,"new_media_produced":false,"issue_closed":false,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
