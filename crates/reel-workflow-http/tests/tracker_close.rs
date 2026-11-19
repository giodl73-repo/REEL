use reel_workflow::{
    ErrorKind, canonical_digest, digest,
    tracker::{
        Draft,
        close::{CloseDraft, Issue},
        operation,
    },
};
use reel_workflow_http::{
    Config as Http,
    tracker::{CloseConfig, CloseReceipt, Config},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
    thread,
};
fn original() -> Draft {
    Draft {
        schema: "reel.workflow-issue-draft.v1".into(),
        operation_id: operation("example/project", &"a".repeat(64)).unwrap(),
        finding_key: "a".repeat(64),
        report_sha256: "b".repeat(64),
        repository: "example/project".into(),
        title: "Synthetic issue".into(),
        body: "Explicit synthetic body".into(),
        route: reel_workflow::review::Route::Project,
        synthetic_reproduction: None,
    }
}
fn http(endpoint: String) -> Http {
    Http {
        schema: "reel.workflow-http.v1".into(),
        endpoint,
        timeout_seconds: 2,
        request_byte_limit: 1024 * 1024,
        response_byte_limit: 1024 * 1024,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: true,
    }
}
fn config(root: &Path, base: &str) -> CloseConfig {
    let issue = root.join("create");
    let close = root.join("close");
    fs::create_dir(&issue).unwrap();
    fs::create_dir(&close).unwrap();
    let make = |registry, endpoint, lookup| Config {
        schema: "reel.workflow-tracker.v1".into(),
        registry,
        create: http(endpoint),
        lookup: http(lookup),
        repositories: BTreeMap::from([(
            "example/project".into(),
            "https://tracker.invalid/example/project/issues/".into(),
        )]),
    };
    CloseConfig {
        schema: "reel.workflow-tracker-close.v1".into(),
        issue_delivery: make(
            issue,
            format!("{base}/create"),
            format!("{base}/create-lookup"),
        ),
        transport: make(
            close,
            format!("{base}/close"),
            format!("{base}/close-lookup"),
        ),
    }
}
fn draft(original: &Draft) -> CloseDraft {
    let mut d = CloseDraft {
        schema: "reel.workflow-issue-close-draft.v1".into(),
        operation_id: String::new(),
        repository: original.repository.clone(),
        finding_key: original.finding_key.clone(),
        baseline_report_sha256: original.report_sha256.clone(),
        issue: Issue {
            create_operation_id: original.operation_id.clone(),
            create_request_sha256: digest(&original.payload().unwrap()),
            issue_id: "7".into(),
            issue_url: "https://tracker.invalid/example/project/issues/7".into(),
        },
        resolution_sha256: "c".repeat(64),
        expected_revision: "rev-1".into(),
        note: "Corrección verificada en el contexto declarado.".into(),
    };
    d.operation_id = reel_workflow::tracker::close::operation(&d).unwrap();
    d
}
fn server(
    root: &Path,
    truncated: bool,
    wrong: &'static str,
) -> (String, TcpListener, thread::JoinHandle<Vec<Value>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let observer = listener.try_clone().unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let root = root.to_path_buf();
    let handle = thread::spawn(move || {
        let mut requests = vec![];
        let mut close_hash = String::new();
        for n in 0..if truncated { 3 } else { 2 } {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(3)))
                .unwrap();
            let mut head = vec![];
            let mut byte = [0];
            while !head.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                head.push(byte[0]);
                assert!(head.len() < 32768);
            }
            let head = String::from_utf8(head).unwrap();
            let length = head
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|s| s.parse::<usize>().unwrap())
                })
                .unwrap();
            assert!(length < 1024 * 1024);
            let mut raw = vec![0; length];
            stream.read_exact(&mut raw).unwrap();
            let body: Value = serde_json::from_slice(&raw).unwrap();
            let op = head
                .lines()
                .find_map(|l| {
                    l.to_lowercase()
                        .strip_prefix("x-reel-operation-id: ")
                        .map(str::to_owned)
                })
                .unwrap();
            let folder = if n == 0 { "create" } else { "close" };
            assert!(
                root.join(folder)
                    .join(format!("{op}.intent.json"))
                    .is_file()
            );
            let response = if n == 0 {
                assert!(head.starts_with("POST /create HTTP/1.1"));
                json!({"schema":"reel.workflow-issue-receipt.v1","operation_id":body["operation_id"],"repository":body["repository"],"request_sha256":digest(&raw),"issue_id":"7","issue_url":"https://tracker.invalid/example/project/issues/7"})
            } else {
                if n == 1 {
                    assert!(head.starts_with("POST /close HTTP/1.1"));
                    assert_eq!(body["schema"], "reel.workflow-issue-close.v1");
                    assert_eq!(body["expected_state"], "open");
                    assert_eq!(body["expected_revision"], "rev-1");
                    assert!(body.get("finding_key").is_none());
                    assert!(body.get("baseline_report_sha256").is_none());
                    assert!(fs::read_dir(root.join("close")).unwrap().any(|p| {
                        p.unwrap()
                            .file_name()
                            .to_string_lossy()
                            .ends_with(".close-owner.json")
                    }));
                    close_hash = digest(&raw);
                } else {
                    assert!(head.starts_with("POST /close-lookup HTTP/1.1"));
                    assert_eq!(body["schema"], "reel.workflow-issue-close-lookup.v1");
                    assert_eq!(body["request_sha256"], close_hash);
                }
                let mut r = json!({"schema":"reel.workflow-issue-close-receipt.v1","operation_id":body["operation_id"],"repository":"example/project","request_sha256":close_hash,"issue_id":"7","issue_url":"https://tracker.invalid/example/project/issues/7","state":"closed","previous_revision":"rev-1","revision":"rev-2","resolution_sha256":"c".repeat(64)});
                match wrong {
                    "url" => r["issue_url"] = json!("https://foreign.invalid/7"),
                    "revision" => r["previous_revision"] = json!("different"),
                    "same-revision" => r["revision"] = json!("rev-1"),
                    "resolution" => r["resolution_sha256"] = json!("d".repeat(64)),
                    "state" => r["state"] = json!("open"),
                    _ => (),
                };
                r
            };
            requests.push(body);
            if truncated && n == 1 {
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nx-request-id: close-fixture\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{").unwrap();
            } else {
                let raw = serde_json::to_vec(&response).unwrap();
                write!(stream,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",raw.len()).unwrap();
                stream.write_all(&raw).unwrap();
            }
        }
        requests
    });
    (base, observer, handle)
}
#[test]
fn close_cli_preserves_exact_target_revision_and_replays_without_another_effect() {
    let root = tempfile::tempdir().unwrap();
    let (base, observer, server) = server(root.path(), false, "");
    let config = config(root.path(), &base);
    let original = original();
    config.issue_delivery.submit(&original, |_| None).unwrap();
    let draft = draft(&original);
    for (n, v) in [
        ("config", serde_json::to_value(&config).unwrap()),
        ("original", serde_json::to_value(&original).unwrap()),
        ("draft", serde_json::to_value(&draft).unwrap()),
    ] {
        fs::write(root.path().join(n), serde_json::to_vec(&v).unwrap()).unwrap();
    }
    let call = || {
        Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
            .arg("close-issue")
            .arg(root.path().join("config"))
            .arg(root.path().join("original"))
            .arg(root.path().join("draft"))
            .arg("--apply")
            .output()
            .unwrap()
    };
    let first = call();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stdout)
    );
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(call().status.success());
    let result = config.close(&original, &draft, |_| None).unwrap();
    let _: CloseReceipt = result.receipt;
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}
#[test]
fn lost_close_reply_uses_lookup_only_and_missing_target_owner_never_resets_operation() {
    let root = tempfile::tempdir().unwrap();
    let (base, observer, server) = server(root.path(), true, "");
    let config = config(root.path(), &base);
    let original = original();
    config.issue_delivery.submit(&original, |_| None).unwrap();
    let draft = draft(&original);
    assert_eq!(
        config.close(&original, &draft, |_| None).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    assert!(config.close(&original, &draft, |_| None).is_err());
    let mut changed = draft.clone();
    changed.note = "Different close text".into();
    assert_eq!(
        config
            .close(&original, &changed, |_| None)
            .unwrap_err()
            .code,
        "tracker-close-original-target-owner"
    );
    let mut changed = draft.clone();
    changed.resolution_sha256 = "d".repeat(64);
    changed.operation_id = reel_workflow::tracker::close::operation(&changed).unwrap();
    assert_eq!(
        config
            .close(&original, &changed, |_| None)
            .unwrap_err()
            .code,
        "tracker-close-original-target-owner"
    );
    let owner = fs::read_dir(&config.transport.registry)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.to_string_lossy().ends_with(".close-owner.json"))
        .unwrap();
    let preserved = owner.with_extension("preserved");
    fs::rename(&owner, &preserved).unwrap();
    assert_eq!(
        config.close(&original, &draft, |_| None).unwrap_err().code,
        "tracker-close-target-owner-missing"
    );
    assert!(!owner.exists());
    fs::rename(preserved, owner).unwrap();
    let head: Value = serde_json::from_slice(
        &fs::read(
            config
                .transport
                .registry
                .join(format!("{}.head.json", draft.operation_id)),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(head["request_id"], "close-fixture");
    let mut lost = vec![];
    for suffix in ["request", "intent", "head", "response", "complete"] {
        let path = config
            .transport
            .registry
            .join(format!("{}.{suffix}.json", draft.operation_id));
        if path.exists() {
            let preserved = path.with_extension("preserved");
            fs::rename(&path, &preserved).unwrap();
            lost.push((path, preserved));
        }
    }
    assert_eq!(
        config.close(&original, &draft, |_| None).unwrap_err().code,
        "tracker-original-effect-uncertain"
    );
    for (path, preserved) in lost {
        fs::rename(preserved, path).unwrap();
    }
    let closed = config
        .reconcile(&original, &draft, "query-1", |_| None)
        .unwrap();
    assert_eq!(closed.receipt.revision, "rev-2");
    assert_eq!(server.join().unwrap().len(), 3);
    config.close(&original, &draft, |_| None).unwrap();
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}
#[test]
fn foreign_or_unbound_close_receipts_never_become_delivery_or_allow_reposts() {
    for wrong in ["url", "revision", "same-revision", "resolution", "state"] {
        let root = tempfile::tempdir().unwrap();
        let (base, observer, server) = server(root.path(), false, wrong);
        let config = config(root.path(), &base);
        let original = original();
        config.issue_delivery.submit(&original, |_| None).unwrap();
        let draft = draft(&original);
        assert_eq!(
            config.close(&original, &draft, |_| None).unwrap_err().code,
            "tracker-close-receipt-binding"
        );
        assert_eq!(server.join().unwrap().len(), 2);
        assert!(config.close(&original, &draft, |_| None).is_err());
        assert!(
            !config
                .transport
                .registry
                .join(format!("{}.receipt.json", draft.operation_id))
                .exists()
        );
        observer.set_nonblocking(true).unwrap();
        assert!(observer.accept().is_err());
    }
}
#[test]
fn close_requires_verified_original_delivery_before_network() {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let config = config(
        root.path(),
        &format!("http://{}", listener.local_addr().unwrap()),
    );
    let original = original();
    let draft = draft(&original);
    assert!(config.close(&original, &draft, |_| None).is_err());
    assert!(listener.accept().is_err());
    assert!(
        !config
            .transport
            .registry
            .join(format!("{}.intent.json", draft.operation_id))
            .exists()
    );
    assert_ne!(
        canonical_digest(&original).unwrap(),
        canonical_digest(&draft).unwrap()
    );
}
