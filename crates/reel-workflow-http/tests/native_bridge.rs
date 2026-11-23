//! Explicit existing-store qualification through already landed CLI protocols.
//! No SDK dependency, native store initialization, project assets or cleanup.
#![cfg(windows)]
use reel_workflow::{Artifact, Mode, Operation, Plan, Provenance, digest, make_packet};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Command,
    thread,
};
fn read(path: &Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    let raw = serde_json::to_vec(value).unwrap();
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    file.write_all(&raw).unwrap();
    file.sync_all().unwrap();
}
#[test]
#[ignore = "Explicit completed synthetic native pool/index and qualified lifecycle runtime only; preserves the new attempt, evidence and capacity; no initialization or project work"]
fn proxy_candidate_enters_original_native_handoff_and_retained_close() {
    let source = PathBuf::from(
        std::env::var_os("REEL_HTTP_NATIVE_SOURCE")
            .expect("Set original synthetic source evidence"),
    );
    let lifecycle = PathBuf::from(
        std::env::var_os("REEL_HTTP_NATIVE_LIFECYCLE")
            .expect("Set completed owned film CLI evidence"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_HTTP_NATIVE_EVIDENCE")
            .expect("Set NEW preserved qualification directory"),
    );
    let predecessor = std::env::var_os("REEL_HTTP_NATIVE_PREDECESSOR")
        .map(PathBuf::from)
        .unwrap_or_else(|| lifecycle.clone());
    let prior = read(&predecessor.join("complete.json"));
    assert_eq!(prior["closed"]["slot_idle"], true);
    assert_eq!(prior["closed"]["original_clear_complete"], true);
    let locations = read(&source.join("locations.json"));
    let qualified = read(&lifecycle.join("qualified-source.json"));
    let native_bytes = fs::read(lifecycle.join("reel-workflow-native.exe")).unwrap();
    assert_eq!(
        digest(&native_bytes),
        qualified["driver_sha256"].as_str().unwrap()
    );
    assert_eq!(
        native_bytes.len() as u64,
        qualified["driver_bytes"].as_u64().unwrap()
    );
    fs::create_dir(&evidence).unwrap();
    let native = evidence.join("reel-workflow-native.exe");
    fs::write(&native, &native_bytes).unwrap();
    let http = evidence.join("reel-workflow-http.exe");
    fs::copy(env!("CARGO_BIN_EXE_reel-workflow-http"), &http).unwrap();
    let http_bytes = fs::read(&http).unwrap();
    save(
        &evidence,
        "runtime-pins.json",
        &json!({"native_source":qualified,"native_sha256":digest(&native_bytes),"http_sha256":digest(&http_bytes),"http_bytes":http_bytes.len()}),
    );
    let original: reel_workflow::Packet =
        serde_json::from_value(locations["packet"].clone()).unwrap();
    let mut task = original.task.clone();
    task.id = "proxy-note".into();
    task.operation = Operation::TextProposal;
    task.tool = None;
    task.prompt = "Return one synthetic editorial note.".into();
    task.depends_on.clear();
    task.outputs = serde_json::from_value(
        json!({"note.txt":{"media_type":"text/plain","max_bytes":1024,"required":true}}),
    )
    .unwrap();
    let packet = make_packet(
        &Plan {
            schema: reel_workflow::PLAN_SCHEMA.into(),
            project: original.project,
            source_commit: original.source_commit,
            task_snapshots: BTreeMap::new(),
            tasks: vec![task],
        },
        "proxy-note",
        BTreeMap::new(),
    )
    .unwrap();
    let sequence = std::env::var("REEL_HTTP_NATIVE_SEQUENCE")
        .unwrap_or_else(|_| "1".into())
        .parse::<u8>()
        .unwrap();
    assert!((1..=16).contains(&sequence));
    let attempt = format!("http-proxy-attempt-{sequence}");
    let admission_operation = format!("http-proxy-job-{sequence}");
    let mut profile = locations["admission_profile"].clone();
    profile["scope"]["operation"] = json!(admission_operation);
    save(&evidence, "packet.json", &packet);
    save(&evidence, "source.json", &locations["native"]);
    save(&evidence, "admission-profile.json", &profile);
    save(
        &evidence,
        "attempt.json",
        &json!({"attempt_id":attempt,"operation":admission_operation,"predecessor_closed":true,"predecessor":predecessor,"predecessor_complete_sha256":digest(&fs::read(predecessor.join("complete.json")).unwrap())}),
    );
    let path = |name: &str| evidence.join(name).to_str().unwrap().to_owned();
    let run = |label: &str, args: Vec<String>| {
        eprintln!("native bridge phase: {label}");
        let output = Command::new(&native).args(args).output().unwrap();
        fs::write(evidence.join(format!("{label}.stdout")), &output.stdout).unwrap();
        fs::write(evidence.join(format!("{label}.stderr")), &output.stderr).unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{label}: {value}");
        value
    };
    run(
        "accept",
        vec![
            "accept-admission".into(),
            path("packet.json"),
            path("source.json"),
            path("admission-profile.json"),
            attempt.clone(),
            path("admission.json"),
            "--apply".into(),
        ],
    );
    run(
        "inputs",
        vec![
            "check-native-inputs".into(),
            path("packet.json"),
            path("source.json"),
        ],
    );
    let receipt = read(&evidence.join("admission.json"));
    let target = json!({"schema":"reel.workflow-retained-target.v1","actor":"retained","owner":profile["scope"]["owner"]});
    let runtime = json!({"path":native,"sha256":digest(&native_bytes),"bytes":native_bytes.len()});
    let lifecycle_tool = Artifact {
        logical_id: "workflow/tool/native-lifecycle".into(),
        sha256: digest(&native_bytes),
        bytes: native_bytes.len() as u64,
        media_type: "application/octet-stream".into(),
    };
    fs::create_dir(evidence.join("acks")).unwrap();
    save(
        &evidence,
        "handoff-profile.json",
        &json!({"schema":"reel.workflow-native-handoff.v1","source":locations["native"],"admission":profile,"binding":{"schema":"reel.workflow-native-handoff-binding.v1","admission":receipt,"target":target},"runtime":runtime,"acknowledgements":evidence.join("acks")}),
    );
    run(
        "handoff",
        vec![
            "create-handoff".into(),
            path("packet.json"),
            path("handoff-profile.json"),
            path("journal"),
            "--apply".into(),
        ],
    );
    let spec = reel_workflow_http::proxy::Spec {
        packet: packet.clone(),
        attempt_id: attempt.clone(),
        provider: "synthetic".into(),
        model: "synthetic-v1".into(),
    };
    let operation = spec.operation_id().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/proxy", listener.local_addr().unwrap());
    let expected = operation.clone();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let mut raw = Vec::new();
        let mut byte = [0; 1];
        while !raw.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            raw.push(byte[0]);
            assert!(raw.len() < 32768);
        }
        let head = String::from_utf8(raw).unwrap();
        let length = head
            .lines()
            .find_map(|line| {
                line.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(length < 1024 * 1024);
        let mut body = vec![0; length];
        stream.read_exact(&mut body).unwrap();
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["operation_id"], expected);
        let response=serde_json::to_vec(&json!({"schema":"reel.workflow-proxy-response.v1","operation_id":expected,"provider":"synthetic","model":"synthetic-v1","outputs":{"note.txt":{"media_type":"text/plain","base64":"aGVsbG8="}}})).unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        )
        .unwrap();
        stream.write_all(&response).unwrap();
        body
    });
    for name in ["http-registry", "http-owned"] {
        fs::create_dir(evidence.join(name)).unwrap();
    }
    let request = json!({"schema":"reel.workflow-http-call.v1","http":{"schema":"reel.workflow-http.v1","endpoint":endpoint,"timeout_seconds":10,"request_byte_limit":65536,"response_byte_limit":65536,"secret_headers":{},"allow_loopback_http":true},"payload":{"kind":"proxy","spec":spec,"inputs":{}},"registry":evidence.join("http-registry"),"workspace":{"parent":evidence.join("http-owned"),"maximum_bytes":16*1024*1024,"capacity_policy":{"minimum_free_bytes":1024*1024,"minimum_free_basis_points":0,"max_object_bytes":32*1024*1024,"tier_minimum_free_bytes":{}}}});
    save(&evidence, "http-request.json", &request);
    let request_pin = digest(&fs::read(evidence.join("http-request.json")).unwrap());
    save(
        &evidence,
        "http-request-pin.json",
        &json!({"sha256":request_pin}),
    );
    let output = Command::new(&http)
        .args(["post", &path("http-request.json"), &request_pin, "--apply"])
        .output()
        .unwrap();
    save(
        &evidence,
        "http-outcome.json",
        &serde_json::from_slice::<Value>(&output.stdout).unwrap(),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    fs::write(evidence.join("proxy-received.json"), worker.join().unwrap()).unwrap();
    let root = evidence
        .join("http-owned")
        .join(format!("http-{operation}"));
    let result = read(&root.join("result.json"));
    let typed: reel_workflow::ReturnedResult = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(
        typed.provenance,
        Provenance {
            mode: Mode::Direct,
            provider: Some("synthetic".into()),
            model: Some("synthetic-v1".into()),
            executable: None
        }
    );
    let imported = run(
        "import",
        vec![
            "import-handoff".into(),
            path("journal"),
            path("handoff-profile.json"),
            root.join("result.json").to_str().unwrap().into(),
            root.join("returns.json").to_str().unwrap().into(),
            "--apply".into(),
        ],
    );
    assert_eq!(imported["task_completed"], true);
    let ack = evidence
        .join("acks")
        .join(format!("retained-{}-{attempt}.json", packet.task_key));
    run(
        "verify",
        vec![
            "verify-retained".into(),
            path("packet.json"),
            path("source.json"),
            path("admission-profile.json"),
            path("admission.json"),
            root.join("result.json").to_str().unwrap().into(),
            ack.to_str().unwrap().into(),
        ],
    );
    save(
        &evidence,
        "close.json",
        &json!({"schema":"reel.workflow-native-close.v1","journal":evidence.join("journal"),"packet":packet,"source":locations["native"],"admission":profile,"receipt":receipt,"result":result,"acknowledgement":read(&ack),"operation":format!("http-proxy-close-{sequence}"),"original_lifecycle_tool":lifecycle_tool,"runtime":runtime}),
    );
    let close_pin = digest(&fs::read(evidence.join("close.json")).unwrap());
    save(&evidence, "close-pin.json", &json!({"sha256":close_pin}));
    run(
        "stage-close",
        vec![
            "stage-close".into(),
            path("close.json"),
            close_pin.clone(),
            path("staged.json"),
            "--apply".into(),
        ],
    );
    let closed = run(
        "recover-close",
        vec![
            "recover-close".into(),
            path("close.json"),
            close_pin,
            path("closed.json"),
            "--apply".into(),
        ],
    );
    assert_eq!(closed["slot_idle"], true);
    assert_eq!(closed["original_clear_complete"], true);
    let owner = read(
        &evidence
            .join("http-registry/calls")
            .join(format!("{operation}.json")),
    );
    assert!(
        evidence
            .join("http-registry/admissions")
            .join(format!("{}.json", owner["reservation"].as_str().unwrap()))
            .is_file()
    );
    save(
        &evidence,
        "complete.json",
        &json!({"closed":closed,"http_candidate_retained":true,"capacity_preserved":true,"selected":false,"project_adopted":false}),
    );
}
