#![cfg(windows)]
use reel_workflow::digest;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

fn server(reply: Vec<u8>) -> (String, thread::JoinHandle<Vec<u8>>, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/call", listener.local_addr().unwrap());
    let observer = listener.try_clone().unwrap();
    let worker = thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut stream = loop {
            match listener.accept() {
                Ok((s, _)) => break s,
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    thread::sleep(Duration::from_millis(5))
                }
                Err(e) => panic!("bounded synthetic accept: {e}"),
            }
        };
        // Windows accepted sockets may inherit the listener's nonblocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut raw = Vec::new();
        let mut byte = [0; 1];
        while !raw.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            raw.push(byte[0]);
            assert!(raw.len() < 32768);
        }
        let header = String::from_utf8(raw.clone()).unwrap();
        let count = header
            .lines()
            .find_map(|s| {
                s.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        assert!(count < 1024 * 1024);
        let mut body = vec![0; count];
        stream.read_exact(&mut body).unwrap();
        raw.extend(body);
        stream.write_all(&reply).unwrap();
        raw
    });
    (endpoint, worker, observer)
}
fn ok(body: &[u8]) -> Vec<u8> {
    let mut raw=format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nx-request-id: synthetic-17\r\nConnection: close\r\n\r\n",body.len()).into_bytes();
    raw.extend(body);
    raw
}
fn issue_spec() -> Value {
    let identity = "1".repeat(64);
    let key = "2".repeat(64);
    json!({"schema":"reel.workflow-github-issue.v1","repository":"private/film",
        "finding_identity":identity,"draft_key":key,
        "issue":{"title":"Review picture: C100","body":format!("Synthetic finding only\n\nDraft version: `{key}`\n\n<!-- reel-film-finding:{identity} -->\n")}})
}
#[test]
fn github_issue_uses_one_owned_operation_and_resumes_without_reposting_or_credentials() {
    let spec = issue_spec();
    let body = serde_json::to_vec(&json!({"number":17,"url":"https://api.github.com/repos/private/film/issues/17","html_url":"https://github.com/private/film/issues/17","title":spec["issue"]["title"],"body":spec["issue"]["body"]})).unwrap();
    let mut reply = format!(
        "HTTP/1.1 201 Created\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .into_bytes();
    reply.extend(body);
    let (endpoint, worker, _observer) = server(reply);
    let endpoint = endpoint.replace("/call", "/repos/private/film/issues");
    let mut f = Fixture::new(endpoint);
    f.request["payload"] = json!({"kind":"github-issue","spec":spec});
    f.save();
    let typed: reel_workflow_http::github_issue::Spec =
        serde_json::from_value(spec.clone()).unwrap();
    let operation = typed.operation_id().unwrap();
    let owned = f.root.path().join(format!("owned/http-{operation}"));
    let (success, result) = f.invoke("post");
    assert!(success, "{result}");
    assert_eq!(result["issue_submitted"], false);
    assert_eq!(result["simulated"], true);
    let sent = String::from_utf8(worker.join().unwrap()).unwrap();
    assert!(sent.starts_with("POST /repos/private/film/issues HTTP/1.1"));
    assert!(
        sent.to_ascii_lowercase()
            .contains("x-github-api-version: 2026-03-10")
    );
    assert!(
        sent.to_ascii_lowercase()
            .contains("user-agent: reel-workflow/0.1.0")
    );
    let receipt = fs::read(owned.join("issue.json")).unwrap();
    assert!(f.invoke("resume").0);
    assert_eq!(fs::read(owned.join("issue.json")).unwrap(), receipt);
    assert!(!f.invoke("post").0);
    // A refined note/version cannot produce a new operation in this registry.
    f.request["payload"]["spec"]["draft_key"] = json!("3".repeat(64));
    f.request["payload"]["spec"]["issue"]["body"] = json!(
        spec["issue"]["body"]
            .as_str()
            .unwrap()
            .replace(&"2".repeat(64), &"3".repeat(64))
    );
    f.save();
    assert!(!f.invoke("post").0);
    assert!(!f.invoke("resume").0);
    assert_eq!(fs::read(owned.join("issue.json")).unwrap(), receipt);
    f.request["payload"]["spec"] = spec;
    f.save();
    fs::write(owned.join("issue.json"), b"changed receipt").unwrap();
    assert!(!f.invoke("resume").0);
    assert_eq!(
        fs::read(owned.join("issue.json")).unwrap(),
        b"changed receipt"
    );
}
#[test]
fn github_interrupted_creation_retains_original_prefix_without_an_issue_receipt_or_repost() {
    let reply = b"HTTP/1.1 201 Created\r\nContent-Length: 1000\r\nx-github-request-id: issue-interrupted\r\nConnection: close\r\n\r\n{\"number\":17".to_vec();
    let (endpoint, worker, _observer) = server(reply);
    let mut f = Fixture::new(endpoint.replace("/call", "/repos/private/film/issues"));
    f.request["payload"] = json!({"kind":"github-issue","spec":issue_spec()});
    f.save();
    assert!(!f.invoke("post").0);
    worker.join().unwrap();
    let spec: reel_workflow_http::github_issue::Spec =
        serde_json::from_value(issue_spec()).unwrap();
    let root = f
        .root
        .path()
        .join(format!("owned/http-{}", spec.operation_id().unwrap()));
    assert!(root.join("response.bin").is_file());
    let head: Value =
        serde_json::from_slice(&fs::read(root.join("http-head.json")).unwrap()).unwrap();
    assert_eq!(head["request_id"], "issue-interrupted");
    assert!(!root.join("response-complete.json").exists());
    assert!(!root.join("issue.json").exists());
    let (success, value) = f.invoke("resume");
    assert!(!success);
    assert_eq!(value["error"]["kind"], "needs-recovery");
    assert!(!f.invoke("post").0);
}
#[test]
fn github_uncertain_response_preserves_original_evidence_and_never_claims_submission() {
    let (endpoint, worker, _observer) = server(ok(b"{}"));
    let mut f = Fixture::new(endpoint.replace("/call", "/repos/private/film/issues"));
    f.request["payload"] = json!({"kind":"github-issue","spec":issue_spec()});
    f.save();
    let (success, value) = f.invoke("post");
    assert!(!success);
    assert_eq!(value["error"]["kind"], "needs-recovery");
    worker.join().unwrap();
    let spec: reel_workflow_http::github_issue::Spec =
        serde_json::from_value(issue_spec()).unwrap();
    let root = f
        .root
        .path()
        .join(format!("owned/http-{}", spec.operation_id().unwrap()));
    assert!(root.join("response-complete.json").is_file());
    assert!(!root.join("issue.json").exists());
    assert!(!f.invoke("resume").0);
    assert!(!f.invoke("post").0);
}
#[test]
fn synthetic_listener_waits_for_headers_after_accept() {
    let (endpoint, worker, _observer) = server(ok(b"{}"));
    let authority = endpoint
        .strip_prefix("http://")
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let mut stream = std::net::TcpStream::connect(authority).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    thread::sleep(Duration::from_millis(100));
    let request =
        format!("POST /call HTTP/1.1\r\nHost: {authority}\r\nContent-Length: 2\r\n\r\n{{}}");
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert_eq!(worker.join().unwrap(), request.as_bytes());
}
struct Fixture {
    root: tempfile::TempDir,
    path: PathBuf,
    request: Value,
    pin: String,
}
impl Fixture {
    fn new(endpoint: String) -> Self {
        let root = tempfile::tempdir().unwrap();
        for name in ["registry", "owned"] {
            fs::create_dir(root.path().join(name)).unwrap();
        }
        let body = root.path().join("body.json");
        fs::write(&body, b"{}").unwrap();
        let request = json!({"schema":"reel.workflow-http-call.v1","http":{"schema":"reel.workflow-http.v1","endpoint":endpoint,"timeout_seconds":3,"request_byte_limit":65536,"response_byte_limit":65536,"secret_headers":{},"allow_loopback_http":true},"payload":{"kind":"json","operation_id":"original-operation","body":{"path":body,"sha256":digest(b"{}"),"bytes":2}},"registry":root.path().join("registry"),"workspace":{"parent":root.path().join("owned"),"maximum_bytes":16*1024*1024,"capacity_policy":{"minimum_free_bytes":1024*1024,"minimum_free_basis_points":0,"max_object_bytes":32*1024*1024,"tier_minimum_free_bytes":{}}}});
        let path = root.path().join("request.json");
        let mut f = Self {
            root,
            path,
            request,
            pin: String::new(),
        };
        f.save();
        f
    }
    fn save(&mut self) {
        let raw = serde_json::to_vec(&self.request).unwrap();
        self.pin = digest(&raw);
        fs::write(&self.path, raw).unwrap();
    }
    fn command(&self, action: &str, pin: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"));
        command.arg(action).arg(&self.path).arg(pin);
        if action == "post" {
            command.arg("--apply");
        }
        command
    }
    fn invoke(&self, action: &str) -> (bool, Value) {
        let output = self.command(action, &self.pin).output().unwrap();
        (
            output.status.success(),
            serde_json::from_slice(&output.stdout).unwrap(),
        )
    }
    fn owned(&self) -> PathBuf {
        self.root.path().join("owned/http-original-operation")
    }
    fn owner(&self) -> Value {
        serde_json::from_slice(
            &fs::read(
                self.root
                    .path()
                    .join("registry/calls/original-operation.json"),
            )
            .unwrap(),
        )
        .unwrap()
    }
    fn capacity(&self) -> PathBuf {
        self.root.path().join("registry/admissions").join(format!(
            "{}.json",
            self.owner()["reservation"].as_str().unwrap()
        ))
    }
}
#[test]
fn raw_call_records_evidence_and_resumes_without_credentials_inputs_or_a_second_request() {
    let (endpoint, worker, observer) = server(ok(b"{}"));
    let mut fixture = Fixture::new(endpoint);
    fixture.request["http"]["secret_headers"] =
        json!({"authorization":{"variable":"SYNTHETIC_HTTP_KEY","prefix":"Bearer "}});
    fixture.save();
    let output = fixture
        .command("post", &fixture.pin)
        .env("SYNTHETIC_HTTP_KEY", "synthetic-secret")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["task_completed"], false);
    assert_eq!(value["native_retained"], false);
    let sent = String::from_utf8(worker.join().unwrap()).unwrap();
    assert!(sent.contains("synthetic-secret"));
    let root = fixture.owned();
    assert_eq!(fs::read(root.join("response.bin")).unwrap(), b"{}");
    assert!(fixture.capacity().is_file());
    let head: Value =
        serde_json::from_slice(&fs::read(root.join("http-head.json")).unwrap()).unwrap();
    assert_eq!(head["request_id"], "synthetic-17");
    fs::rename(
        fixture.root.path().join("body.json"),
        fixture.root.path().join("original-body.json"),
    )
    .unwrap();
    let resumed = fixture.invoke("resume");
    assert!(resumed.0, "{:?}", resumed.1);
    assert_eq!(resumed.1["capacity_released"], false);
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
    fs::write(root.join("response.bin"), b"xx").unwrap();
    let changed = fixture.invoke("resume");
    assert!(!changed.0);
    assert_eq!(changed.1["error"]["code"], "http-original-response-binding");
    assert!(fixture.capacity().exists());
}
#[test]
fn partial_response_keeps_original_operation_prefix_and_capacity_without_reposting() {
    let (endpoint, worker, observer) =
        server(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort".to_vec());
    let fixture = Fixture::new(endpoint);
    let failed = fixture.invoke("post");
    assert!(!failed.0);
    assert_eq!(failed.1["error"]["kind"], "needs-recovery");
    worker.join().unwrap();
    assert_eq!(
        fs::read(fixture.owned().join("response.bin")).unwrap(),
        b"short"
    );
    assert!(fixture.capacity().exists());
    let resumed = fixture.invoke("resume");
    assert!(!resumed.0);
    assert_eq!(
        resumed.1["error"]["code"],
        "http-original-outcome-uncertain"
    );
    let retried = fixture.invoke("post");
    assert!(!retried.0);
    assert_eq!(retried.1["error"]["code"], "http-original-operation-exists");
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}
#[test]
fn request_pins_and_explicit_execution_refuse_before_any_owned_effect() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut fixture = Fixture::new(format!("http://{}/call", listener.local_addr().unwrap()));
    let output = fixture.command("post", &"0".repeat(64)).output().unwrap();
    assert!(!output.status.success());
    assert!(
        fs::read_dir(fixture.root.path().join("owned"))
            .unwrap()
            .next()
            .is_none()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
        .arg("post")
        .arg(&fixture.path)
        .arg(&fixture.pin)
        .output()
        .unwrap();
    assert!(!output.status.success());
    fixture.request["workspace"]["maximum_bytes"] = json!(8 * 1024 * 1024);
    fixture.save();
    let budget = fixture.invoke("post");
    assert!(!budget.0);
    assert_eq!(budget.1["error"]["kind"], "budget");
    assert!(listener.accept().is_err());
    assert!(
        fs::read_dir(fixture.root.path().join("owned"))
            .unwrap()
            .next()
            .is_none()
    );
}
#[test]
fn same_operation_cannot_move_to_a_new_workspace_or_replace_its_original_directory() {
    let (endpoint, worker, observer) = server(ok(b"{}"));
    let mut fixture = Fixture::new(endpoint);
    assert!(fixture.invoke("post").0);
    worker.join().unwrap();
    let duplicate = fixture.invoke("post");
    assert!(!duplicate.0);
    assert_eq!(
        duplicate.1["error"]["code"],
        "http-original-operation-exists"
    );
    let original = fixture.owned();
    let preserved = fixture.root.path().join("preserved");
    fs::rename(&original, &preserved).unwrap();
    fs::create_dir(&original).unwrap();
    let replaced = fixture.invoke("resume");
    assert!(!replaced.0);
    assert_eq!(replaced.1["error"]["code"], "http-original-owner-binding");
    let other = fixture.root.path().join("other-owned");
    fs::create_dir(&other).unwrap();
    fixture.request["workspace"]["parent"] = json!(other);
    fixture.save();
    assert!(!fixture.invoke("post").0);
    assert!(fs::read_dir(&other).unwrap().next().is_none());
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}
fn proxy_spec() -> Value {
    let plan:reel_workflow::Plan=serde_json::from_value(json!({"schema":reel_workflow::PLAN_SCHEMA,"project":"synthetic-film","source_commit":"a".repeat(40),"tasks":[{"id":"note","operation":"text-proposal","language":"es","prompt":"Return a synthetic note.","inputs":[],"depends_on":[],"outputs":{"note.txt":{"media_type":"text/plain","max_bytes":1024,"required":true}}}]})).unwrap();
    json!({"packet":reel_workflow::make_packet(&plan,"note",Default::default()).unwrap(),"attempt_id":"attempt-1","provider":"synthetic","model":"synthetic-v1"})
}
#[test]
fn proxy_call_produces_the_existing_result_and_native_return_contract_then_recovers_lost_metadata()
{
    let spec = proxy_spec();
    let typed: reel_workflow_http::proxy::Spec = serde_json::from_value(spec.clone()).unwrap();
    let operation = typed.operation_id().unwrap();
    let reply = json!({"schema":"reel.workflow-proxy-response.v1","operation_id":operation,"provider":"synthetic","model":"synthetic-v1","outputs":{"note.txt":{"media_type":"text/plain","base64":"aGVsbG8="}}});
    let (endpoint, worker, observer) = server(ok(&serde_json::to_vec(&reply).unwrap()));
    let mut fixture = Fixture::new(endpoint);
    fixture.request["payload"] = json!({"kind":"proxy","spec":spec,"inputs":{}});
    fixture.save();
    let ran = fixture.invoke("post");
    assert!(ran.0, "{:?}", ran.1);
    assert_eq!(ran.1["candidate_ready"], true);
    assert_eq!(ran.1["task_completed"], false);
    let sent = worker.join().unwrap();
    let offset = sent.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
    let request: Value = serde_json::from_slice(&sent[offset..]).unwrap();
    assert_eq!(request["packet"]["task_key"], typed.packet.task_key);
    let root = fixture
        .root
        .path()
        .join("owned")
        .join(format!("http-{operation}"));
    let result: reel_workflow::ReturnedResult =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    reel_workflow::check_output(
        &typed.packet,
        &result,
        "note.txt",
        &fs::read(root.join("outputs/note.txt")).unwrap(),
    )
    .unwrap();
    assert_eq!(result.provenance.mode, reel_workflow::Mode::Direct);
    let returns: Value =
        serde_json::from_slice(&fs::read(root.join("returns.json")).unwrap()).unwrap();
    assert_eq!(returns["schema"], "reel.workflow-returns.v1");
    assert_eq!(returns["files"]["note.txt"], "note.txt");
    fs::rename(root.join("result.json"), root.join("original-result.json")).unwrap();
    assert!(fixture.invoke("resume").0);
    assert_eq!(
        fs::read(root.join("result.json")).unwrap(),
        fs::read(root.join("original-result.json")).unwrap()
    );
    fs::write(root.join("outputs/note.txt"), b"changed").unwrap();
    assert!(!fixture.invoke("resume").0);
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}
#[test]
fn concurrent_posts_have_one_original_owner_and_only_one_network_effect() {
    let (endpoint, worker, observer) = server(ok(b"{}"));
    let fixture = Fixture::new(endpoint);
    let mut first = fixture
        .command("post", &fixture.pin)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let second = fixture.invoke("post");
    let first = first.wait().unwrap();
    assert_ne!(first.success(), second.0);
    worker.join().unwrap();
    observer.set_nonblocking(true).unwrap();
    assert!(observer.accept().is_err());
}

#[test]
fn failed_preparation_fences_the_operation_before_capacity_or_workspace_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut fixture = Fixture::new(format!("http://{}/call", listener.local_addr().unwrap()));
    fixture.request["workspace"]["capacity_policy"]["minimum_free_basis_points"] = json!(10000);
    fixture.save();
    let denied = fixture.invoke("post");
    assert!(!denied.0);
    assert_eq!(denied.1["error"]["kind"], "budget");
    let claim = fixture
        .root
        .path()
        .join("registry/calls/original-operation.prepare.json");
    assert!(claim.is_file());
    assert!(!fixture.owned().exists());
    let retry = fixture.invoke("post");
    assert!(!retry.0);
    assert_eq!(retry.1["error"]["code"], "http-original-operation-exists");
    assert!(listener.accept().is_err());
    assert!(!fixture.owned().exists());
}
