use reel_workflow::{
    ErrorKind, digest,
    review::Route,
    tracker::{Draft, operation},
};
use reel_workflow_http::{
    Config as Http,
    tracker::{Config, Receipt},
};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    thread,
};
fn draft() -> Draft {
    let finding = "a".repeat(64);
    Draft {
        schema: "reel.workflow-issue-draft.v1".into(),
        operation_id: operation("example/project", &finding).unwrap(),
        finding_key: finding,
        report_sha256: "b".repeat(64),
        repository: "example/project".into(),
        title: "Synthetic issue".into(),
        body: "Explicit synthetic issue body.".into(),
        route: Route::Project,
        synthetic_reproduction: None,
    }
}
fn http(url: String) -> Http {
    Http {
        schema: "reel.workflow-http.v1".into(),
        endpoint: url,
        timeout_seconds: 2,
        request_byte_limit: 1024 * 1024,
        response_byte_limit: 1024 * 1024,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: true,
    }
}
fn config(root: &Path, base: &str) -> Config {
    Config {
        schema: "reel.workflow-tracker.v1".into(),
        registry: root.into(),
        create: http(format!("{base}/create")),
        lookup: http(format!("{base}/lookup")),
        repositories: BTreeMap::from([(
            "example/project".into(),
            "https://example.invalid/example/project/issues/".into(),
        )]),
    }
}
fn server(
    root: &Path,
    truncated: bool,
    wrong_url: bool,
) -> (String, thread::JoinHandle<Vec<serde_json::Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let root = root.to_path_buf();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        let mut original_hash = None;
        for n in 0..if truncated { 2 } else { 1 } {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut headers = Vec::new();
            let mut byte = [0u8; 1];
            while !headers.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                headers.push(byte[0]);
                assert!(headers.len() < 32768);
            }
            let header = String::from_utf8(headers).unwrap();
            let count = header
                .lines()
                .find_map(|s| {
                    s.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|n| n.parse::<usize>().unwrap())
                })
                .unwrap();
            let mut body = vec![0; count];
            stream.read_exact(&mut body).unwrap();
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
            let op = request["operation_id"].as_str().unwrap();
            assert!(root.join(format!("{op}.intent.json")).exists());
            let hash = if n == 0 {
                let hash = digest(&body);
                original_hash = Some(hash.clone());
                hash
            } else {
                assert_eq!(request["schema"], "reel.workflow-issue-lookup.v1");
                assert_eq!(
                    request["request_sha256"],
                    original_hash.as_ref().unwrap().as_str()
                );
                original_hash.clone().unwrap()
            };
            let receipt = Receipt {
                schema: "reel.workflow-issue-receipt.v1".into(),
                operation_id: op.into(),
                repository: "example/project".into(),
                request_sha256: hash,
                issue_id: "7".into(),
                issue_url: if wrong_url {
                    "https://other.invalid/7".into()
                } else {
                    "https://example.invalid/example/project/issues/7".into()
                },
            };
            requests.push(request);
            if truncated && n == 0 {
                stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nRequest-Id: synthetic-create\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{").unwrap();
            } else {
                let raw = serde_json::to_vec(&receipt).unwrap();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",raw.len()).unwrap();
                stream.write_all(&raw).unwrap();
            }
        }
        requests
    });
    (base, handle)
}
#[test]
fn create_persists_intent_before_post_and_replays_without_another_call() {
    let root = tempfile::tempdir().unwrap();
    let (base, server) = server(root.path(), false, false);
    let config = config(root.path(), &base);
    let draft = draft();
    let receipt = config.submit(&draft, |_| None).unwrap();
    assert_eq!(receipt.receipt.issue_id, "7");
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 1);
    let intent = fs::read(
        root.path()
            .join(format!("{}.intent.json", draft.operation_id)),
    )
    .unwrap();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap().receipt,
        receipt.receipt
    );
    let mut changed = draft.clone();
    changed.body.push_str(" Changed payload.");
    assert_eq!(
        config.submit(&changed, |_| None).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(
        intent,
        fs::read(
            root.path()
                .join(format!("{}.intent.json", draft.operation_id))
        )
        .unwrap()
    );
    assert!(
        !String::from_utf8(draft.payload().unwrap())
            .unwrap()
            .contains(&draft.report_sha256)
    );
}
#[test]
fn lost_create_reply_never_reposts_and_lookup_preserves_remote_url() {
    let root = tempfile::tempdir().unwrap();
    let (base, server) = server(root.path(), true, false);
    let config = config(root.path(), &base);
    let draft = draft();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    let intent = fs::read(
        root.path()
            .join(format!("{}.intent.json", draft.operation_id)),
    )
    .unwrap();
    let head: serde_json::Value = serde_json::from_slice(
        &fs::read(
            root.path()
                .join(format!("{}.head.json", draft.operation_id)),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(head["request_id"], "synthetic-create");
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    let config_path = root.path().join("config.json");
    let draft_path = root.path().join("draft.json");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::write(&draft_path, serde_json::to_vec(&draft).unwrap()).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
        .arg("reconcile-issue")
        .arg(&config_path)
        .arg(&draft_path)
        .args(["lookup-1", "--apply"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let delivered = config.reconcile(&draft, "lookup-1", |_| None).unwrap();
    assert_eq!(
        delivered.receipt.issue_url,
        "https://example.invalid/example/project/issues/7"
    );
    assert_ne!(delivered.evidence_key, draft.operation_id);
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["schema"], "reel.workflow-issue-create.v1");
    assert_eq!(
        config.submit(&draft, |_| None).unwrap().receipt,
        delivered.receipt
    );
    assert_eq!(
        config
            .reconcile(&draft, "lookup-2", |_| None)
            .unwrap()
            .receipt,
        delivered.receipt
    );
    assert_eq!(
        intent,
        fs::read(
            root.path()
                .join(format!("{}.intent.json", draft.operation_id))
        )
        .unwrap()
    );
    assert!(
        root.path()
            .join(format!("{}.response.json", draft.operation_id))
            .exists()
    );
}
#[test]
fn foreign_remote_url_refuses_and_complete_response_can_recover_local_receipt_loss() {
    let root = tempfile::tempdir().unwrap();
    let (base, server) = server(root.path(), false, true);
    let config = config(root.path(), &base);
    let draft = draft();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::Integrity
    );
    server.join().unwrap();
    assert!(
        !root
            .path()
            .join(format!("{}.receipt.json", draft.operation_id))
            .exists()
    );
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::Integrity
    );
    // A complete accepted response is sufficient for recovery after a local
    // receipt-write failure; no second POST or invented operation is required.
    let second = tempfile::tempdir().unwrap();
    let (base, server) = self::server(second.path(), false, false);
    let config = self::config(second.path(), &base);
    let expected = config.submit(&draft, |_| None).unwrap();
    server.join().unwrap();
    let owned = second.path().canonicalize().unwrap();
    let path = owned.join(format!("{}.receipt.json", draft.operation_id));
    let preserved = owned.join("receipt-preserved.json");
    assert_eq!(path.parent(), Some(owned.as_path()));
    assert_eq!(preserved.parent(), Some(owned.as_path()));
    fs::rename(path, preserved).unwrap();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap().receipt,
        expected.receipt
    );
}
#[test]
fn concurrent_owner_corrupt_intent_and_unallowed_repository_never_send() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), "http://127.0.0.1:1");
    let draft = draft();
    let path = root.path().join(format!("{}.lock", draft.operation_id));
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3);
    }
    let lock = options.open(path).unwrap();
    fs2::FileExt::lock_exclusive(&lock).unwrap();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::Busy
    );
    drop(lock);
    fs::write(
        root.path()
            .join(format!("{}.intent.json", draft.operation_id)),
        b"{",
    )
    .unwrap();
    assert_eq!(
        config.submit(&draft, |_| None).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    let mut other = draft;
    other.repository = "example/other".into();
    other.operation_id = operation(&other.repository, &other.finding_key).unwrap();
    assert_eq!(
        config.submit(&other, |_| None).unwrap_err().kind,
        ErrorKind::Denied
    );
    assert!(
        !root
            .path()
            .join(format!("{}.response.json", other.operation_id))
            .exists()
    );
}
#[test]
fn cli_submission_and_replay_leave_only_private_registry_evidence() {
    let root = tempfile::tempdir().unwrap();
    let registry = root.path().join("registry");
    fs::create_dir(&registry).unwrap();
    let (base, server) = server(&registry, false, false);
    let config = config(&registry, &base);
    let draft = draft();
    let config_path = root.path().join("config.json");
    let draft_path = root.path().join("draft.json");
    fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();
    fs::write(&draft_path, serde_json::to_vec(&draft).unwrap()).unwrap();
    let call = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
            .args(["submit-issue"])
            .arg(&config_path)
            .arg(&draft_path)
            .arg("--apply")
            .output()
            .unwrap()
    };
    let response = call();
    assert!(
        response.status.success(),
        "{}",
        String::from_utf8_lossy(&response.stdout)
    );
    server.join().unwrap();
    assert!(call().status.success());
    let result: serde_json::Value = serde_json::from_slice(&response.stdout).unwrap();
    assert_eq!(result["task_completed"], false);
    assert_eq!(result["native_retained"], false);
    assert!(!String::from_utf8_lossy(&response.stdout).contains("Explicit synthetic issue body"));
}
