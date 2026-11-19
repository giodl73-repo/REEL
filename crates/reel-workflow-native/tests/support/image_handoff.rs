//! External PNG bytes -> guarded pack/import/retention, with no provider calls.
use super::*;
use reel_workflow_native::{NativeProfile, admission::Prepared, response::Returns, retention};
fn load<T: serde::de::DeserializeOwned>(path: impl AsRef<Path>) -> T {
    decode(&fs::read(path).unwrap()).unwrap()
}
fn save(path: impl AsRef<Path>, value: &impl serde::Serialize) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn run(exe: &Path, args: &[&std::ffi::OsStr]) -> std::process::Output {
    Command::new(exe).args(args).output().unwrap()
}
fn success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
#[ignore = "Existing synthetic pool only; invalid PNG refusal before import intent, caller PNG packing, retained completion/history and replay. Preserves roots; no API calls"]
fn png_handoff_refuses_bad_bytes_before_intent_and_retains_exact_manual_results() {
    let descriptor: serde_json::Value =
        load(std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set existing pool descriptor"));
    let reader: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(descriptor["source"].clone()).unwrap();
    let evidence = PathBuf::from(
        std::env::var_os("REEL_IMAGE_HANDOFF_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exe = PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set pinned CLI"));
    // Read-only compatibility check on a prior retained direct-API candidate.
    let prior = PathBuf::from(
        std::env::var_os("REEL_RETAINED_API_IMAGE_FIXTURE").expect("Set retained API fixture"),
    );
    let prior_packet: Packet = load(prior.join("packet.json"));
    let prior_profile: reel_workflow_native::handoff::Profile = load(prior.join("profile.json"));
    let provider_ticket: reel_workflow_native::provider::Ticket =
        load(prior.join("dispatch/provider-ticket.json"));
    let prior_result = load(provider_ticket.directory().join("result.json"));
    let prior_ack = load(prior_profile.acknowledgements.join(format!(
        "retained-{}-{}.json",
        prior_packet.task_key, prior_profile.binding.admission.attempt_id
    )));
    let prior_claims = fs::read(prior_profile.admission.index_root.join("claims.json")).unwrap();
    drop(
        retention::verify(
            &prior_packet,
            &prior_profile.source,
            &prior_profile.admission,
            &prior_profile.binding.admission,
            &prior_result,
            &prior_ack,
        )
        .unwrap(),
    );
    assert_eq!(
        prior_claims,
        fs::read(prior_profile.admission.index_root.join("claims.json")).unwrap()
    );
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    let inputs = provider_native::image_inputs(&mut fixture);
    fixture.profile.git = Tool {
        path: reader.pool.git.clone(),
        sha256: digest(&fs::read(&reader.pool.git).unwrap()),
        bytes: fs::metadata(&reader.pool.git).unwrap().len(),
    };
    fixture.packet.task.outputs = ["one.png", "two.png"]
        .into_iter()
        .map(|n| {
            (
                n.into(),
                OutputContract {
                    media_type: "image/png".into(),
                    max_bytes: 1024 * 1024,
                    required: true,
                },
            )
        })
        .collect();
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, reader.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        annex: serde_json::from_value(descriptor["annex"].clone()).unwrap(),
        source: reader,
        movie_verifier: None,
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let marker = json!({"root":root,"source":fixture.temp.path(),"prior_api":prior,"task_key":fixture.packet.task_key,"attempt":"attempt-1","runtime":exe});
    save(root.join("original-operation.json"), &marker);
    save(evidence.join("original-operation-roots.json"), &marker);
    save(root.join("packet.json"), &fixture.packet);
    save(root.join("source.json"), &case.source);
    save(root.join("admission.json"), &case.profile);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    save(root.join("receipt.json"), &receipt);
    let ingress = root.join("input-bytes");
    fs::create_dir(&ingress).unwrap();
    for (name, raw) in inputs {
        fs::write(ingress.join(&name), &raw).unwrap();
        tig_git::annex::workflow::publication::publish(
            &tig_git::annex::workflow::publication::Profile {
                schema: "tig.annex-file-publication.experimental-1".into(),
                pool: case.source.source.pool.clone(),
                store: case.profile.scope.store.clone(),
                source_root: ingress.clone(),
                source: LogicalPath::parse(&name).unwrap(),
                source_owned: true,
                expected: Some(Object::from_bytes(&raw)),
                include_private_location: false,
                maximum_bytes: 1024 * 1024,
                minimum_free_bytes: 100 * 1024 * 1024,
            },
        )
        .unwrap();
    }
    let profile = handoff_profile(&case, receipt, exe.clone());
    let profile_path = root.join("profile.json");
    save(&profile_path, &profile);
    let packet_path = root.join("packet.json");
    let source_path = root.join("source.json");
    let admission_path = root.join("admission.json");
    let receipt_path = root.join("receipt.json");
    let journal = root.join("journal");
    success(&run(
        &exe,
        &[
            "create-handoff".as_ref(),
            packet_path.as_os_str(),
            profile_path.as_os_str(),
            journal.as_os_str(),
            "--apply".as_ref(),
        ],
    ));
    let waiting = fs::read(journal.join("00000.json")).unwrap();
    let claims = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let returned = root.join("returned");
    fs::create_dir(&returned).unwrap();
    let first = provider_native::image_png(1024, true);
    let bad = b"not a PNG";
    fs::write(returned.join("one.png"), &first).unwrap();
    fs::write(returned.join("bad-two.png"), bad).unwrap();
    let mut returns = Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: returned.clone(),
        files: BTreeMap::from([
            ("one.png".into(), "one.png".into()),
            ("two.png".into(), "bad-two.png".into()),
        ]),
    };
    let bad_returns = root.join("bad-returns.json");
    save(&bad_returns, &returns);
    let refused = root.join("refused-result.json");
    let pack = |bindings: &Path, target: &Path| {
        run(
            &exe,
            &[
                "pack-image-result".as_ref(),
                packet_path.as_os_str(),
                source_path.as_os_str(),
                admission_path.as_os_str(),
                receipt_path.as_os_str(),
                bindings.as_os_str(),
                target.as_os_str(),
                "--apply".as_ref(),
            ],
        )
    };
    let output = pack(&bad_returns, &refused);
    assert!(!output.status.success());
    assert!(!refused.exists());
    fs::write(root.join("bad-pack-stdout.json"), output.stdout).unwrap();
    let artifact = |name: &str, raw: &[u8]| Artifact {
        logical_id: format!("candidate/{name}"),
        sha256: digest(raw),
        bytes: raw.len() as u64,
        media_type: "image/png".into(),
    };
    let forged = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: fixture.packet.task.id.clone(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([
            ("one.png".into(), artifact("one.png", &first)),
            ("two.png".into(), artifact("two.png", bad)),
        ]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    let forged_path = root.join("forged-result.json");
    save(&forged_path, &forged);
    let output = run(
        &exe,
        &[
            "import-handoff".as_ref(),
            journal.as_os_str(),
            profile_path.as_os_str(),
            forged_path.as_os_str(),
            bad_returns.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert!(!output.status.success());
    fs::write(root.join("bad-import-stdout.json"), output.stdout).unwrap();
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .state(),
        reel_workflow::handoff::State::Waiting
    );
    assert_eq!(waiting, fs::read(journal.join("00000.json")).unwrap());
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 1);
    assert_eq!(
        claims,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let second = provider_native::image_png(16, false);
    fs::write(returned.join("valid-two.png"), &second).unwrap();
    returns
        .files
        .insert("two.png".into(), "valid-two.png".into());
    let returns_path = root.join("returns.json");
    save(&returns_path, &returns);
    let result_path = root.join("result.json");
    let output = pack(&returns_path, &result_path);
    success(&output);
    fs::write(root.join("pack-stdout.json"), output.stdout).unwrap();
    let result: ReturnedResult = load(&result_path);
    assert_eq!(result.provenance.mode, Mode::Handoff);
    assert!(result.provenance.provider.is_none());
    assert!(result.provenance.model.is_none());
    assert_eq!(
        claims,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let output = run(
        &exe,
        &[
            "import-handoff".as_ref(),
            journal.as_os_str(),
            profile_path.as_os_str(),
            result_path.as_os_str(),
            returns_path.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    success(&output);
    fs::write(root.join("import-stdout.json"), output.stdout).unwrap();
    let ack = load(profile.acknowledgements.join(format!(
        "retained-{}-attempt-1.json",
        fixture.packet.task_key
    )));
    save(root.join("ack.json"), &ack);
    drop(
        retention::verify(
            &fixture.packet,
            &case.source,
            &case.profile,
            &profile.binding.admission,
            &result,
            &ack,
        )
        .unwrap(),
    );
    let retained_claims = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let replay = run(
        &exe,
        &[
            "resume-handoff".as_ref(),
            journal.as_os_str(),
            profile_path.as_os_str(),
            "-".as_ref(),
            "--apply".as_ref(),
        ],
    );
    success(&replay);
    fs::write(root.join("replay-stdout.json"), replay.stdout).unwrap();
    assert_eq!(
        retained_claims,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        prior_claims,
        fs::read(prior_profile.admission.index_root.join("claims.json")).unwrap()
    );
    let outcome = json!({"root":root,"source":fixture.temp.path(),"prior_api_candidate_reopened":true,"bad_pack_refused_without_result":true,"bad_import_refused_before_intent":true,"waiting_claims_unchanged":true,"png_outputs":2,"handoff_provenance_unknown":true,"retained":true,"native_png_validation":true,"replay_without_claim_changes":true,"provider_called":false,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
