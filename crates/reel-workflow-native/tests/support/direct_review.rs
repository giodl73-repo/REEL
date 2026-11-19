//! Exact retained comic inputs -> direct structured provider -> native report.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow_native::{
    NativeProfile, admission::Prepared, dispatch, graph, provider, retention, staging,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
fn load<T: serde::de::DeserializeOwned>(p: impl AsRef<Path>) -> T {
    decode(&fs::read(p).unwrap()).unwrap()
}
fn save(p: impl AsRef<Path>, v: &impl serde::Serialize) {
    fs::write(p, serde_json::to_vec(v).unwrap()).unwrap();
}
fn stage(p: PathBuf) -> staging::Profile {
    fs::create_dir(&p).unwrap();
    staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent: p,
        maximum_bytes: 32 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    }
}
fn success(o: &std::process::Output) {
    assert!(
        o.status.success(),
        "{} {}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
#[test]
#[ignore = "Existing retained synthetic comic inputs only; direct structured loopback review, native retention/history/replay; preserves roots"]
fn direct_comic_review_retains_valid_findings_and_reopens_without_local_returns() {
    run(false);
}
#[test]
#[ignore = "Existing retained synthetic comic inputs only; structurally valid wrong-unit model response must refuse before import intent without another call"]
fn direct_comic_review_refuses_wrong_unit_before_registration_without_reissue() {
    run(true);
}
fn run(invalid: bool) {
    let prior = PathBuf::from(
        std::env::var_os("REEL_RETAINED_REVIEW_FIXTURE").expect("Set retained review fixture"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_DIRECT_REVIEW_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let exe = PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set frozen CLI"));
    let original_source: NativeProfile = load(prior.join("source.json"));
    let original_packet: Packet = load(prior.join("packet.json"));
    let old_admission: reel_workflow_native::admission::Profile =
        load(prior.join("admission.json"));
    let old_claims = fs::read(old_admission.index_root.join("claims.json")).unwrap();
    let copy: review::Copy = load(prior.join("input-returns/copy.json"));
    let manifest: serde_json::Value = load(prior.join("input-returns/manifest.json"));
    let reading: BTreeMap<String, Artifact> =
        serde_json::from_value(manifest["reading"].clone()).unwrap();
    assert_eq!(reading.len(), 1);
    let png = reading.values().next().unwrap().clone();
    let mut fixture = Fixture::new();
    fixture.temp.disable_cleanup(true);
    fixture.profile = original_source.snapshot.clone();
    fixture.packet = original_packet;
    fixture.packet.task.id = if invalid {
        "direct-comic-review-invalid"
    } else {
        "direct-comic-review"
    }
    .into();
    fixture.packet.task.prompt="Review only supplied synthetic comic material. Preserve Spanish wording. This is a scripted technical fixture, not creative approval.".into();
    for n in [
        "text.json",
        "http.json",
        "provider-request.json",
        "provider-response.json",
        "provider-config.json",
    ] {
        fixture.packet.task.outputs.insert(
            n.into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: if ["provider-request.json", "provider-response.json"].contains(&n) {
                    4 * 1024 * 1024
                } else {
                    1024 * 1024
                },
                required: true,
            },
        );
    }
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, original_source.source.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = original_source;
    case.source.source.pool.control = control;
    let root = case.temp.path();
    let bare = root.join("history.git");
    let cloned = Command::new(&case.source.snapshot.git.path)
        .args(["clone", "--bare", "--no-hardlinks"])
        .arg(&case.source.snapshot.repository)
        .arg(&bare)
        .output()
        .unwrap();
    success(&cloned);
    case.source.snapshot.repository = bare;
    let marker = json!({"root":root,"prior":prior,"harness_source":fixture.temp.path(),"task_key":fixture.packet.task_key,"invalid_unit":invalid,"runtime":exe});
    save(root.join("original-operation.json"), &marker);
    fs::write(root.join("prior-claims-before.bin"), &old_claims).unwrap();
    save(evidence.join("original-operation-roots.json"), &marker);
    save(root.join("packet.json"), &fixture.packet);
    save(root.join("source.json"), &case.source);
    save(root.join("admission.json"), &case.profile);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    save(root.join("receipt.json"), &receipt);
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
    let waiting = fs::read(journal.join("00000.json")).unwrap();
    let claims = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![fixture.packet.task.clone()],
    };
    save(root.join("plan.json"), &plan);
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            fixture.packet.task.id.clone(),
            graph::Binding {
                journal: journal.clone(),
                profile: graph::PinnedFile {
                    path: root.join("profile.json"),
                    sha256: digest(&fs::read(root.join("profile.json")).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    save(root.join("bindings.json"), &bindings);
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let observer = listener.try_clone().unwrap();
    let endpoint = format!("http://{}/v1/responses", listener.local_addr().unwrap());
    let unit = copy
        .units
        .iter()
        .find(|(_, u)| {
            matches!(
                u,
                review::Unit::Comic {
                    balloon_id: Some(_),
                    ..
                }
            )
        })
        .unwrap()
        .0
        .clone();
    let report = review::Report {
        schema: "reel.workflow-review-report.v1".into(),
        copy_sha256: canonical_digest(&copy).unwrap(),
        findings: vec![review::Finding {
            code: "synthetic-direct-review".into(),
            unit: if invalid {
                "missing-unit".into()
            } else {
                unit.clone()
            },
            frames: None,
            modality: review::Modality::Text,
            route: review::Route::Project,
            requested_change: "Synthetic test observation; preserve approved Spanish text.".into(),
            evidence: vec![png.clone()],
        }],
    };
    let report_raw = serde_json::to_vec(&report).unwrap();
    save(root.join("expected-report.json"), &report);
    let expected_report = report_raw.clone();
    let expected_copy = canonical_digest(&copy).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let server = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(180);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(10))
                }
                Err(e) => panic!("synthetic provider accept: {e}"),
            }
        };
        count.fetch_add(1, Ordering::SeqCst);
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let mut head = vec![];
        let mut byte = [0];
        while !head.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            head.push(byte[0]);
            assert!(head.len() < 16384);
        }
        let head = String::from_utf8(head).unwrap();
        assert!(head.starts_with("POST /v1/responses HTTP/1.1"));
        let len = head
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|v| v.parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(len < 4 * 1024 * 1024);
        let mut raw = vec![0; len];
        stream.read_exact(&mut raw).unwrap();
        let body: serde_json::Value = decode(&raw).unwrap();
        assert_eq!(body["store"], false);
        assert_eq!(body["tools"], json!([]));
        assert_eq!(body["text"]["format"]["strict"], true);
        assert_eq!(
            body["text"]["format"]["schema"]["properties"]["copy_sha256"]["enum"][0],
            expected_copy
        );
        let content = body["input"][0]["content"].as_array().unwrap();
        let brief: review::Brief = decode(content[0]["text"].as_str().unwrap().as_bytes()).unwrap();
        assert_eq!(brief.language, "es");
        assert!(brief.units.contains_key(&unit));
        assert_eq!(brief.production_commit, copy.source_commit);
        assert_eq!(brief.report_output, "report");
        let images: Vec<_> = content
            .iter()
            .filter(|p| p["type"] == "input_image")
            .collect();
        assert_eq!(images.len(), 1);
        let raw = STANDARD
            .decode(
                images[0]["image_url"]
                    .as_str()
                    .unwrap()
                    .strip_prefix("data:image/png;base64,")
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(digest(&raw), png.sha256);
        assert_eq!(raw.len() as u64, png.bytes);
        let reply=serde_json::to_vec(&json!({"object":"response","id":"resp_comic_fixture","model":"reported-model","status":"completed","error":null,"incomplete_details":null,"output":[{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":String::from_utf8(expected_report).unwrap(),"annotations":[]}]}]})).unwrap();
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nx-request-id: fixture-direct-review\r\nConnection: close\r\n\r\n",reply.len()).unwrap();
        stream.write_all(&reply).unwrap();
    });
    let config=dispatch::Config{schema:"reel.workflow-dispatch.v1".into(),auxiliary_programs:BTreeMap::new(),input_staging:stage(root.join("inputs")),worker_staging:None,provider:Some(provider::Config{schema:"reel.workflow-provider-proxy.v1".into(),provider:"openai".into(),model:"declared-review-model".into(),send_inputs:fixture.packet.task.inputs.iter().map(|a|a.logical_id.clone()).collect(),http:reel_workflow_http::Config{schema:"reel.workflow-http.v1".into(),endpoint,timeout_seconds:30,request_byte_limit:4*1024*1024,response_byte_limit:4*1024*1024,secret_headers:BTreeMap::new(),allow_loopback_http:true},workspace:stage(root.join("provider-scratch")),service:Some(provider::Service::OpenaiText{spec:serde_json::from_value(json!({"schema":"reel.openai-text.v1","format":"review-report","output":"report","max_output_tokens":2048})).unwrap()})})};
    save(root.join("config.json"), &config);
    let invoke = || {
        Command::new(&exe)
            .arg("dispatch-task")
            .arg(root.join("plan.json"))
            .arg(root.join("bindings.json"))
            .arg(&fixture.packet.task.id)
            .arg(root.join("config.json"))
            .arg(root.join("dispatch"))
            .arg("--apply")
            .output()
            .unwrap()
    };
    let first = invoke();
    save(
        root.join("first.json"),
        &decode::<serde_json::Value>(&first.stdout).unwrap(),
    );
    server.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let ticket: provider::Ticket = load(root.join("dispatch/provider-ticket.json"));
    let workspace = ticket.directory();
    assert_eq!(
        fs::read(workspace.join("packet/report")).unwrap(),
        report_raw
    );
    if invalid {
        assert!(!first.status.success());
        let out: serde_json::Value = decode(&first.stdout).unwrap();
        assert_eq!(out["error"]["code"], "review-finding-unit");
        assert_eq!(
            handoff::Journal::open(&journal).unwrap().state(),
            handoff::State::Waiting
        );
        assert_eq!(fs::read_dir(&journal).unwrap().count(), 1);
        assert_eq!(fs::read(journal.join("00000.json")).unwrap(), waiting);
        assert_eq!(
            fs::read(case.profile.index_root.join("claims.json")).unwrap(),
            claims
        );
        assert_eq!(fs::read_dir(&profile.acknowledgements).unwrap().count(), 0);
    } else {
        success(&first);
        assert_eq!(
            handoff::Journal::open(&journal).unwrap().state(),
            handoff::State::Retained
        );
        let result: ReturnedResult = load(workspace.join("result.json"));
        let ack: retention::Acknowledgement = load(profile.acknowledgements.join(format!(
            "retained-{}-attempt-1.json",
            fixture.packet.task_key
        )));
        save(root.join("result.json"), &result);
        save(root.join("ack.json"), &ack);
        let owned = root.canonicalize().unwrap();
        let source = workspace.join("packet").canonicalize().unwrap();
        let destination = owned.join("preserved-local-return-packet");
        assert!(source.starts_with(&owned));
        assert!(destination.starts_with(&owned));
        assert!(!destination.exists());
        fs::rename(source, destination).unwrap();
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
    }
    let replay_claims = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let replay = invoke();
    save(
        root.join("replay.json"),
        &decode::<serde_json::Value>(&replay.stdout).unwrap(),
    );
    assert_eq!(first.status.success(), replay.status.success());
    assert_eq!(
        fs::read(case.profile.index_root.join("claims.json")).unwrap(),
        replay_claims
    );
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
    assert_eq!(
        fs::read(old_admission.index_root.join("claims.json")).unwrap(),
        old_claims
    );
    let outcome = json!({"root":root,"prior":prior,"workspace":workspace,"invalid_unit":invalid,"calls":1,"native_retained":!invalid,"refused_before_import_intent":invalid,"native_history_without_local_returns":!invalid,"bare_git":true,"replay_without_post_or_claim_change":true,"prior_claims_unchanged":true,"selected":false,"scratch_retired":false});
    save(root.join("qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}

#[test]
#[ignore = "Original retained direct-review operation only; resumes qualification after harness listener wait, no readmission or provider reissue"]
fn retained_direct_comic_review_resumes_original_history_and_completion_checks() {
    let root = PathBuf::from(
        std::env::var_os("REEL_DIRECT_REVIEW_RESUME_ROOT")
            .expect("Set original retained operation root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_DIRECT_REVIEW_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let packet: Packet = load(root.join("packet.json"));
    let source: NativeProfile = load(root.join("source.json"));
    let admission: reel_workflow_native::admission::Profile = load(root.join("admission.json"));
    let receipt: reel_workflow_native::admission::Receipt = load(root.join("receipt.json"));
    let result: ReturnedResult = load(root.join("result.json"));
    let ack: retention::Acknowledgement = load(root.join("ack.json"));
    let marker: serde_json::Value = load(root.join("original-operation.json"));
    let exe = PathBuf::from(marker["runtime"].as_str().unwrap());
    let prior = PathBuf::from(marker["prior"].as_str().unwrap());
    let old: reel_workflow_native::admission::Profile = load(prior.join("admission.json"));
    let before_old = fs::read(old.index_root.join("claims.json")).unwrap();
    let before = fs::read(admission.index_root.join("claims.json")).unwrap();
    let ticket: provider::Ticket = load(root.join("dispatch/provider-ticket.json"));
    let workspace = ticket.directory();
    assert!(!workspace.join("packet").exists());
    assert!(root.join("preserved-local-return-packet/report").is_file());
    assert!(source.snapshot.repository.join("HEAD").is_file());
    assert!(!source.snapshot.repository.join(".git").exists());
    assert_eq!(
        handoff::Journal::open(&root.join("journal"))
            .unwrap()
            .state(),
        handoff::State::Retained
    );
    let proof = retention::verify(&packet, &source, &admission, &receipt, &result, &ack).unwrap();
    assert_eq!(proof.review_report().unwrap().findings().len(), 1);
    drop(proof);
    let replay = Command::new(exe)
        .arg("dispatch-task")
        .arg(root.join("plan.json"))
        .arg(root.join("bindings.json"))
        .arg(&packet.task.id)
        .arg(root.join("config.json"))
        .arg(root.join("dispatch"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&replay);
    let replay_value: serde_json::Value = decode(&replay.stdout).unwrap();
    assert_eq!(replay_value["provider_invoked"], false);
    assert_eq!(replay_value["summary"]["provider_called"], false);
    assert_eq!(replay_value["summary"]["already_ready"], true);
    assert_eq!(
        before,
        fs::read(admission.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(
        before_old,
        fs::read(old.index_root.join("claims.json")).unwrap()
    );
    save(root.join("resumed-replay.json"), &replay_value);
    let outcome = json!({"root":root,"prior":prior,"workspace":workspace,"original_identity_resumed":true,"no_readmission_or_input_publication":true,"native_retained":true,"native_history_without_local_returns":true,"bare_git":true,"checked_findings":1,"replay_without_post_or_claim_change":true,"prior_claims_unchanged_during_resume":true,"initial_prior_claim_comparison_not_reached":true,"initial_harness_listener_failure_preserved":true,"selected":false,"scratch_retired":false});
    save(root.join("resume-qualification-result.json"), &outcome);
    save(evidence.join("qualification-result.json"), &outcome);
}
