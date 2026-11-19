//! Exact existing native inputs -> one synthetic HTTP call -> retained evidence.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use reel_workflow_native::{
    NativeProfile, admission::Prepared, dispatch, graph, provider, staging,
};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
};

fn stage(parent: PathBuf) -> staging::Profile {
    fs::create_dir(&parent).unwrap();
    staging::Profile {
        schema: "reel.workflow-staging.v1".into(),
        parent,
        maximum_bytes: 32 * 1024 * 1024,
        capacity_policy: reel_assets::admission::Policy {
            minimum_free_bytes: 65536,
            minimum_free_basis_points: 0,
            max_object_bytes: 512 * 1024 * 1024,
            ..Default::default()
        },
    }
}

#[test]
fn proxy_dispatch_requires_declared_evidence_and_native_inputs_before_network_effects() {
    for evidence in [false, true] {
        let mut fixture = Fixture::new();
        if evidence {
            for (name, limit) in [
                ("provider-request.json", 4 * 1024 * 1024),
                ("provider-response.json", 4 * 1024 * 1024),
                ("provider-config.json", 1024 * 1024),
            ] {
                fixture.packet.task.outputs.insert(
                    name.into(),
                    OutputContract {
                        media_type: "application/json".into(),
                        max_bytes: limit,
                        required: true,
                    },
                );
            }
            fixture.repin();
        }
        let case = AdmissionCase::new(&fixture);
        let root = case.temp.path();
        let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .accept()
            .unwrap();
        let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
        let profile_path = root.join("profile.json");
        fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
        let journal = root.join("journal");
        reel_workflow_native::handoff::create(&journal, &fixture.packet, &profile).unwrap();
        let plan = Plan {
            schema: PLAN_SCHEMA.into(),
            project: fixture.packet.project.clone(),
            source_commit: fixture.packet.source_commit.clone(),
            tasks: vec![fixture.packet.task.clone()],
        };
        let bindings = graph::Bindings {
            schema: "reel.workflow-run-bindings.v1".into(),
            tasks: BTreeMap::from([(
                fixture.packet.task.id.clone(),
                graph::Binding {
                    journal,
                    profile: graph::PinnedFile {
                        path: profile_path.clone(),
                        sha256: digest(&fs::read(profile_path).unwrap()),
                    },
                    staging_ticket: None,
                },
            )]),
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let config = dispatch::Config {
            schema: "reel.workflow-dispatch.v1".into(),
            input_staging: stage(root.join("inputs")),
            worker_staging: None,
            provider: Some(provider::Config {
                service: None,
                schema: "reel.workflow-provider-proxy.v1".into(),
                provider: "synthetic".into(),
                model: "model".into(),
                send_inputs: vec!["input/source".into()],
                workspace: stage(root.join("outputs")),
                http: reel_workflow_http::Config {
                    schema: "reel.workflow-http.v1".into(),
                    endpoint: format!("http://{}/call", listener.local_addr().unwrap()),
                    timeout_seconds: 1,
                    request_byte_limit: 4 * 1024 * 1024,
                    response_byte_limit: 4 * 1024 * 1024,
                    secret_headers: BTreeMap::new(),
                    allow_loopback_http: true,
                },
            }),
        };
        let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
        let error = dispatch::run(
            &plan,
            &bindings,
            &fixture.packet.task.id,
            &config,
            &root.join("dispatch"),
        )
        .err()
        .unwrap();
        if !evidence {
            assert_eq!(error.code, "provider-declared-evidence-required");
        }
        assert!(
            listener.accept().is_err(),
            "provider called without declared/native inputs"
        );
        assert_eq!(
            before,
            fs::read(case.profile.index_root.join("claims.json")).unwrap()
        );
        assert!(!root.join("dispatch/provider-ticket.json").exists());
    }
}
#[test]
#[ignore = "Existing declared native pool and synthetic local HTTP only: protected call, retained evidence, original-call fencing and uncertainty preservation"]
fn protected_proxy_call_retains_request_response_and_config_and_never_reissues_uncertainty() {
    run_case(false);
}
#[test]
#[ignore = "Existing declared native pool and synthetic local HTTP only: uncertain response cannot become another call; preserves roots"]
fn protected_proxy_uncertain_response_preserves_original_call_and_claims() {
    run_case(true);
}
fn run_case(uncertain: bool) {
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("Set REEL_NATIVE_INPUT_FIXTURE");
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object.clone());
    fixture.temp.disable_cleanup(true);
    let bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&bytes),
        bytes: bytes.len() as u64,
    };
    for (name, limit) in [
        ("provider-request.json", 4 * 1024 * 1024),
        ("provider-response.json", 4 * 1024 * 1024),
        ("provider-config.json", 1024 * 1024),
    ] {
        fixture.packet.task.outputs.insert(
            name.into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: limit,
                required: true,
            },
        );
    }
    fixture.repin();
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    eprintln!(
        "Native provider root: {}; source: {}; uncertain: {}",
        root.display(),
        fixture.temp.path().display(),
        uncertain
    );
    let evidence = std::env::var_os("REEL_PROVIDER_EVIDENCE_DIRECTORY").map(PathBuf::from);
    if let Some(directory) = &evidence {
        fs::write(
            directory.join("original-operation-roots.json"),
            serde_json::to_vec(
                &json!({"root":root,"source":fixture.temp.path(),"uncertain":uncertain}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let executable = std::env::var_os("REEL_NATIVE_HANDOFF_TOOL")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_reel-workflow-native")));
    let profile = handoff_profile(&case, receipt.clone(), executable.clone());
    let profile_path = root.join("profile.json");
    fs::write(&profile_path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let packet_path = root.join("packet.json");
    fs::write(&packet_path, serde_json::to_vec(&fixture.packet).unwrap()).unwrap();
    let journal = root.join("journal");
    let created = Command::new(&executable)
        .arg("create-handoff")
        .arg(&packet_path)
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
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![fixture.packet.task.clone()],
    };
    let bindings = graph::Bindings {
        schema: "reel.workflow-run-bindings.v1".into(),
        tasks: BTreeMap::from([(
            fixture.packet.task.id.clone(),
            graph::Binding {
                journal: journal.clone(),
                profile: graph::PinnedFile {
                    path: profile_path.clone(),
                    sha256: digest(&fs::read(&profile_path).unwrap()),
                },
                staging_ticket: None,
            },
        )]),
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/generate", listener.local_addr().unwrap());
    let observer = listener.try_clone().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let expected = object.clone();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        count.fetch_add(1, Ordering::SeqCst);
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(30)))
            .unwrap();
        let mut header = Vec::new();
        let mut byte = [0u8; 1];
        while !header.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            header.push(byte[0]);
            assert!(header.len() < 16384);
        }
        let header = String::from_utf8(header).unwrap();
        let length = header
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(length < 4 * 1024 * 1024);
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["schema"], "reel.workflow-proxy-request.v1");
        let input = STANDARD
            .decode(request["inputs"][0]["base64"].as_str().unwrap())
            .unwrap();
        assert_eq!(Object::from_bytes(&input), expected);
        let raw=serde_json::to_vec(&json!({"schema":"reel.workflow-proxy-response.v1","operation_id":request["operation_id"],"provider":"synthetic-proxy","model":"fixture-model","outputs":{"note":{"media_type":"text/plain","base64":STANDARD.encode(b"synthetic provider result")}}})).unwrap();
        let length = raw.len() + if uncertain { 10 } else { 0 };
        write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").unwrap();
        stream.write_all(&raw).unwrap();
    });
    let config = dispatch::Config {
        schema: "reel.workflow-dispatch.v1".into(),
        input_staging: stage(root.join("inputs")),
        worker_staging: None,
        provider: Some(provider::Config {
            service: None,
            schema: "reel.workflow-provider-proxy.v1".into(),
            provider: "synthetic-proxy".into(),
            model: "fixture-model".into(),
            send_inputs: vec!["input/source".into()],
            http: reel_workflow_http::Config {
                schema: "reel.workflow-http.v1".into(),
                endpoint,
                timeout_seconds: 30,
                request_byte_limit: 4 * 1024 * 1024,
                response_byte_limit: 4 * 1024 * 1024,
                secret_headers: BTreeMap::new(),
                allow_loopback_http: true,
            },
            workspace: stage(root.join("provider-scratch")),
        }),
    };
    let plan_path = root.join("plan.json");
    let bindings_path = root.join("bindings.json");
    let config_path = root.join("config.json");
    for (path, bytes) in [
        (&plan_path, serde_json::to_vec(&plan).unwrap()),
        (&bindings_path, serde_json::to_vec(&bindings).unwrap()),
        (&config_path, serde_json::to_vec(&config).unwrap()),
    ] {
        fs::write(path, bytes).unwrap();
    }
    let directory = root.join("dispatch");
    let invoke = || {
        Command::new(&executable)
            .arg("dispatch-task")
            .arg(&plan_path)
            .arg(&bindings_path)
            .arg(&fixture.packet.task.id)
            .arg(&config_path)
            .arg(&directory)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let first = invoke();
    server.join().unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    if uncertain {
        assert!(
            !first.status.success(),
            "uncertain call unexpectedly completed"
        );
    } else {
        assert!(
            first.status.success(),
            "{}: {} {}",
            first.status,
            String::from_utf8_lossy(&first.stdout),
            String::from_utf8_lossy(&first.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&first.stdout).unwrap()["provider_invoked"],
            true
        );
    }
    let ticket_path = directory.join("provider-ticket.json");
    let ticket_bytes = fs::read(&ticket_path).unwrap();
    let ticket: provider::Ticket = serde_json::from_slice(&ticket_bytes).unwrap();
    let workspace = ticket.directory();
    let header = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let second = invoke();
    assert_eq!(first.status.success(), second.status.success());
    assert_eq!(
        header,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    assert_eq!(ticket_bytes, fs::read(&ticket_path).unwrap());
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err(), "provider call was reissued");
    if uncertain {
        assert!(workspace.join("call-intent.json").is_file());
        assert!(!workspace.join("response-complete.json").exists());
        assert!(fs::metadata(workspace.join("response.bin")).unwrap().len() > 0);
        assert_eq!(
            reel_workflow::handoff::Journal::open(&journal)
                .unwrap()
                .state(),
            reel_workflow::handoff::State::Waiting
        );
        drop(
            Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
                .unwrap()
                .reopen(&receipt)
                .unwrap(),
        );
        fs::rename(
            workspace.join("call-intent.json"),
            workspace.join("preserved-call-intent.json"),
        )
        .unwrap();
        fs::rename(
            workspace.join("response.bin"),
            workspace.join("preserved-response.bin"),
        )
        .unwrap();
        let lost_local = invoke();
        assert!(!lost_local.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&lost_local.stdout).unwrap()["error"]["code"],
            "provider-original-call-owner-exists"
        );
        assert!(observer.accept().is_err());
        assert!(!workspace.join("response.bin").exists());
        fs::rename(
            workspace.join("preserved-call-intent.json"),
            workspace.join("call-intent.json"),
        )
        .unwrap();
        fs::rename(
            workspace.join("preserved-response.bin"),
            workspace.join("response.bin"),
        )
        .unwrap();
    } else {
        let result: ReturnedResult =
            serde_json::from_slice(&fs::read(workspace.join("result.json")).unwrap()).unwrap();
        for name in [
            "provider-request.json",
            "provider-response.json",
            "provider-config.json",
        ] {
            assert!(result.artifacts.contains_key(name));
        }
        assert_eq!(result.provenance.mode, Mode::Direct);
        assert_eq!(
            reel_workflow::handoff::Journal::open(&journal)
                .unwrap()
                .state(),
            reel_workflow::handoff::State::Retained
        );
    }
    if let Some(evidence) = evidence {
        fs::write(evidence.join("first.json"), first.stdout).unwrap();
        fs::write(evidence.join("replay.json"), second.stdout).unwrap();
        fs::write(evidence.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"workspace":workspace,"uncertain":uncertain,"calls":1,"no_repeat_on_replay":true,"claims_preserved":true,"selected":false,"scratch_retired":false})).unwrap()).unwrap();
    }
}
