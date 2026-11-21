#![cfg(feature = "s3")]
use base64::{Engine as _, engine::general_purpose::STANDARD};
use git_asset_core::{Digest, Object, identity};
use git_asset_store::{
    AckLevel, Key, Request,
    ledger::{CapacityLedger, Checkpoint, JournalRef},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    orchestration::Place,
    publish,
    s3::{self, Budget, Client, Plan, PlanRequest},
};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    fs,
    io::{Cursor, Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    LostReply,
    CorruptReadback,
    Denied,
    Redirect,
    Race,
    RaceWrong,
    Conflict,
    Slow,
    MissingReadback,
}
struct State {
    mode: Mode,
    object: Option<Vec<u8>>,
    calls: Vec<String>,
    puts: usize,
}
struct Server {
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
    origin: String,
}
impl Server {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State {
            mode: Mode::Normal,
            object: None,
            calls: Vec::new(),
            puts: 0,
        }));
        let copy = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = thread::spawn(move || {
            while !stopped.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => serve(stream, &copy),
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept {e}"),
                }
            }
        });
        Self {
            state,
            stop,
            thread: Some(thread),
            origin,
        }
    }
    fn mode(&self, mode: Mode) {
        self.state.lock().unwrap().mode = mode;
    }
    fn counts(&self) -> (usize, usize) {
        let s = self.state.lock().unwrap();
        (s.calls.len(), s.puts)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn response(stream: &mut TcpStream, status: u16, body: &[u8], extra: &str) {
    let head = format!(
        "HTTP/1.1 {status} fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    );
    if stream.write_all(head.as_bytes()).is_ok() {
        let _ = stream.write_all(body);
    }
}
fn serve(mut stream: TcpStream, state: &Mutex<State>) {
    // Windows accept can inherit the nonblocking listener mode.
    stream.set_nonblocking(false).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let mut header = Vec::new();
    let mut one = [0];
    while !header.ends_with(b"\r\n\r\n") {
        if stream.read(&mut one).unwrap_or(0) == 0 {
            return;
        }
        header.push(one[0]);
        assert!(header.len() < 16384);
    }
    let head = String::from_utf8(header).unwrap();
    let line = head.lines().next().unwrap();
    let method = line.split_whitespace().next().unwrap().to_string();
    assert!(line.contains("?fixture-access="));
    let fields = head
        .lines()
        .skip(1)
        .filter_map(|s| s.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mode = {
        let mut s = state.lock().unwrap();
        s.calls.push(method.clone());
        s.mode
    };
    if mode == Mode::Slow {
        thread::sleep(Duration::from_millis(180));
    }
    if mode == Mode::Denied {
        response(&mut stream, 403, b"expired synthetic access", "");
        return;
    }
    if mode == Mode::Redirect {
        response(
            &mut stream,
            307,
            b"",
            "Location: http://127.0.0.1:1/forbidden\r\n",
        );
        return;
    }
    match method.as_str() {
        "GET" => {
            let body = state.lock().unwrap().object.clone();
            match (mode, body) {
                (Mode::MissingReadback, _) | (_, None) => {
                    response(&mut stream, 404, b"missing", "")
                }
                (Mode::CorruptReadback, Some(body)) => {
                    let wrong = vec![b'x'; body.len()];
                    response(&mut stream, 200, &wrong, "");
                }
                (_, Some(body)) => response(&mut stream, 200, &body, ""),
            }
        }
        "PUT" => {
            assert_eq!(fields.get("if-none-match").unwrap(), "*");
            assert_eq!(
                fields.get("content-type").unwrap(),
                "application/octet-stream"
            );
            let size: usize = fields.get("content-length").unwrap().parse().unwrap();
            assert!(size <= 8 * 1024 * 1024);
            let mut body = vec![0; size];
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            assert_eq!(
                fields.get("x-amz-checksum-sha256").unwrap(),
                &STANDARD.encode(Sha256::digest(&body))
            );
            let mut s = state.lock().unwrap();
            s.puts += 1;
            if mode == Mode::Conflict {
                response(&mut stream, 409, b"conditional conflict", "");
                return;
            }
            if mode == Mode::Race || mode == Mode::RaceWrong {
                s.object = Some(if mode == Mode::Race {
                    body.clone()
                } else {
                    vec![b'z'; size]
                });
            }
            if s.object.is_some() {
                response(&mut stream, 412, b"exists", "");
                return;
            }
            s.object = Some(body);
            if mode != Mode::LostReply {
                response(&mut stream, 200, b"", "");
            }
        }
        _ => panic!("unexpected remote mutation"),
    }
}
struct Fixture {
    root: tempfile::TempDir,
    server: Server,
    source: LocalStore,
    witness: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    minimum: Checkpoint,
    client: Client,
    plan: Plan,
    profile: Value,
}
impl Fixture {
    fn new() -> Self {
        Self::bytes(b"synthetic S3 fixture bytes", None)
    }
    fn bytes(bytes: &[u8], job_peak: Option<u64>) -> Self {
        let root = tempfile::tempdir().unwrap();
        let server = Server::new();
        let mut source = LocalStore::create(&root.path().join("source"), key("source")).unwrap();
        let witness = LocalStore::create(&root.path().join("witness"), key("witness")).unwrap();
        let publication = Request {
            operation: key("original"),
            owner: key("producer"),
            object: Object::from_bytes(bytes),
            required: AckLevel::ProcessRestart,
        };
        publish(&mut source, &publication, &mut Cursor::new(bytes)).unwrap();
        let url = format!(
            "{}/fixture/{}?fixture-access=private-test-only",
            server.origin,
            publication.object.sha256.as_str()
        );
        let client = Client::presigned(
            digest("remote-instance"),
            1,
            publication.object.clone(),
            &url,
            &url,
            true,
        )
        .unwrap();
        let mut ledger = CapacityLedger::create(
            &root.path().join("ledger"),
            key("capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        LocalCapacityProbe::bind(&witness, &mut ledger)
            .unwrap()
            .observe(&witness, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let plan = Plan::new(PlanRequest {
            operation: key("upload"),
            owner: key("operator"),
            source: Place::local(&source, 1).unwrap(),
            publication,
            witness: Place::local(&witness, 1).unwrap(),
            capacity: ledger.identity().clone(),
            target: client.target().clone(),
            context: digest("trusted-context"),
            budget: Budget {
                max_object_bytes: 8 * 1024 * 1024,
                transfer_bytes: 32 * 1024 * 1024,
                requests: 3,
                millis: 5000,
            },
        })
        .unwrap();
        let job = ledger
            .reserve(
                key("job"),
                key("operator"),
                job_peak.unwrap_or(plan.required_peak(&witness).unwrap()),
            )
            .unwrap()
            .reference()
            .clone();
        let minimum = ledger.checkpoint();
        let profile = json!({"schema":"tig.s3-profile.design-1","source_root":root.path().join("source"),"source":plan.source,
            "witness_root":root.path().join("witness"),"witness":plan.witness,"ledger_root":root.path().join("ledger"),"minimum":minimum,"job":job,
            "target":plan.target,"owner":plan.owner,"context":plan.context,"budget":plan.budget,"put_url":url,"get_url":url,"allow_upload":true,"loopback_fixture":true});
        write(root.path(), "profile.json", &profile);
        write(
            root.path(),
            "request.json",
            &json!({"schema":"tig.s3-upload-request.design-1","operation":plan.operation,"publication":plan.publication}),
        );
        fs::write(root.path().join("plan.json"), plan.to_json().unwrap()).unwrap();
        Self {
            root,
            server,
            source,
            witness,
            ledger,
            job,
            minimum,
            client,
            plan,
            profile,
        }
    }
    fn apply(&mut self) -> s3::Result<s3::Receipt> {
        s3::Execution {
            source: &self.source,
            witness: &self.witness,
            ledger: &mut self.ledger,
            job: &self.job,
            client: &self.client,
            context: &self.plan.context,
        }
        .apply(&self.plan)
    }
    fn status(&mut self) -> s3::Result<s3::Status> {
        s3::status(
            &self.witness,
            &mut self.ledger,
            &self.job,
            &self.client,
            &self.plan,
        )
    }
    fn cli(&self, action: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_git-asset-storage"))
            .args(["s3", action])
            .arg(self.root.path().join("profile.json"))
            .arg(self.root.path().join(if action == "plan" {
                "request.json"
            } else {
                "plan.json"
            }))
            .output()
            .unwrap()
    }
    fn sequence(&self) -> u64 {
        CapacityLedger::open(&self.root.path().join("ledger"), &self.minimum)
            .unwrap()
            .checkpoint()
            .sequence()
    }
    fn preserved(&self) {
        assert!(
            self.source
                .read_object(&self.plan.publication.object)
                .is_ok()
        );
    }
}
fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn success(out: Output) -> Value {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(out.stderr.is_empty());
    serde_json::from_slice(&out.stdout).unwrap()
}
fn failure(out: Output, code: &str) {
    assert!(!out.status.success());
    assert!(out.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&out.stdout).unwrap()["code"],
        code
    );
}
#[test]
fn actual_conditional_put_get_readback_replay_and_no_command_status() {
    let mut f = Fixture::new();
    let before = f.sequence();
    let s = f.status().unwrap();
    assert!(!s.historical_completion);
    assert_eq!(s.observed, s3::Observation::Absent);
    assert_eq!(f.sequence(), before);
    let result = f.apply().unwrap();
    assert!(!result.retained_custody && !result.removal_authority);
    assert_eq!(f.server.counts(), (4, 1));
    let seq = f.sequence();
    let s = f.status().unwrap();
    assert!(s.historical_completion && s.accounting_settled);
    assert!(!s.requires_recovery);
    assert_eq!(f.sequence(), seq);
    f.apply().unwrap();
    assert_eq!(f.server.counts().1, 1);
    assert_eq!(f.sequence(), seq);
    f.preserved();
    let text = String::from_utf8(f.plan.to_json().unwrap()).unwrap();
    assert!(!text.contains("fixture-access") && !text.contains(&f.server.origin));
    assert!(!format!("{:?}", f.client).contains("private-test-only"));
}
#[test]
fn lost_put_reply_recovers_in_another_cli_process_without_reupload() {
    let f = Fixture::new();
    f.server.mode(Mode::LostReply);
    failure(f.cli("apply"), "remote_unavailable");
    assert_eq!(f.server.counts(), (2, 1));
    f.preserved();
    f.server.mode(Mode::Normal);
    let s = success(f.cli("status"));
    assert_eq!(s["historical_completion"], false);
    assert_eq!(s["intent_recorded"], true);
    assert_eq!(s["observed"], "verified");
    assert_eq!(success(f.cli("resume"))["removal_authority"], false);
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
}
#[test]
fn conditional_winner_is_read_back_and_foreign_content_never_overwritten() {
    for mode in [Mode::Race, Mode::RaceWrong] {
        let mut f = Fixture::new();
        f.server.mode(mode);
        let result = f.apply();
        if mode == Mode::Race {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err(), s3::Error::Integrity);
        }
        assert_eq!(f.server.counts(), (3, 1));
        f.preserved();
    }
}
#[test]
fn missing_or_corrupt_readback_never_gets_completion_and_source_survives() {
    for mode in [Mode::CorruptReadback, Mode::MissingReadback] {
        let mut f = Fixture::new();
        f.server.mode(mode);
        assert_eq!(f.apply().unwrap_err(), s3::Error::Integrity);
        f.preserved();
        f.server.mode(Mode::Normal);
        let s = f.status().unwrap();
        assert!(!s.historical_completion && s.intent_recorded);
        f.apply().unwrap();
        assert_eq!(f.server.counts().1, 1);
    }
}
#[test]
fn acknowledged_remote_loss_is_an_incident_and_never_silently_reuploaded() {
    let mut f = Fixture::new();
    f.apply().unwrap();
    f.server.state.lock().unwrap().object = None;
    let s = f.status().unwrap();
    assert!(s.historical_completion && s.requires_recovery);
    assert_eq!(s.observed, s3::Observation::Absent);
    assert_eq!(f.apply().unwrap_err(), s3::Error::MissingAcknowledgedRemote);
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
}
#[test]
fn denied_redirect_conflict_timeout_and_request_budget_are_bounded() {
    for (mode, error) in [
        (Mode::Denied, s3::Error::Denied),
        (Mode::Redirect, s3::Error::Unavailable),
        (Mode::Conflict, s3::Error::Conflict),
        (Mode::Slow, s3::Error::Budget),
    ] {
        let mut f = Fixture::new();
        f.server.mode(mode);
        if mode == Mode::Slow {
            f.plan.budget.millis = 40;
        }
        assert_eq!(f.apply().unwrap_err(), error);
        assert!(f.server.counts().0 <= 2);
        f.preserved();
    }
    let mut f = Fixture::new();
    f.plan.budget.requests = 1;
    assert_eq!(f.apply().unwrap_err(), s3::Error::Budget);
    assert_eq!(f.server.counts(), (1, 0));
    f.preserved();
}
#[test]
fn cancellation_after_local_intent_performs_no_http_and_exact_resume_can_finish() {
    let mut f = Fixture::new();
    let mut calls = 0;
    let result = s3::Execution {
        source: &f.source,
        witness: &f.witness,
        ledger: &mut f.ledger,
        job: &f.job,
        client: &f.client,
        context: &f.plan.context,
    }
    .apply_cancellable(&f.plan, &mut || {
        calls += 1;
        if calls >= 2 {
            assert!(matches!(
                f.source.try_exclusive_object(&f.plan.publication.object),
                Err(git_asset_store::Error::Busy)
            ));
        }
        calls >= 2
    });
    assert_eq!(result.unwrap_err(), s3::Error::Cancelled);
    assert_eq!(f.server.counts(), (0, 0));
    assert!(f.status().unwrap().intent_recorded);
    f.apply().unwrap();
    f.preserved();
}
#[test]
fn capacity_admission_and_conflicting_reuse_refuse_before_http() {
    let mut f = Fixture::bytes(b"capacity", Some(1));
    assert!(f.apply().is_err());
    assert_eq!(f.server.counts(), (0, 0));
    f.preserved();
    let mut f = Fixture::new();
    f.apply().unwrap();
    let before = f.server.counts();
    f.plan.context = digest("new-context");
    assert!(matches!(
        f.apply(),
        Err(s3::Error::Conflict) | Err(s3::Error::NeedsRecovery)
    ));
    assert_eq!(f.server.counts(), before);
    f.preserved();
}
#[test]
fn empty_and_multibuffer_sources_keep_exact_bytes() {
    for bytes in [Vec::new(), vec![19; 192 * 1024 + 17]] {
        let mut f = Fixture::bytes(&bytes, None);
        f.apply().unwrap();
        assert_eq!(
            f.server.state.lock().unwrap().object.as_ref().unwrap(),
            &bytes
        );
        f.preserved();
    }
}
#[test]
fn cli_plan_status_disabled_policy_stale_context_and_offline_source() {
    let f = Fixture::new();
    let before = f.sequence();
    let p = success(f.cli("plan"));
    assert_eq!(p, serde_json::to_value(&f.plan).unwrap());
    assert_eq!(f.server.counts(), (0, 0));
    assert_eq!(f.sequence(), before);
    let peak = success(f.cli("peak"));
    assert!(peak["local_witness_peak_bytes"].as_u64().unwrap() > 0);
    assert_eq!(f.server.counts(), (0, 0));
    let mut profile = f.profile.clone();
    profile.as_object_mut().unwrap().remove("allow_upload");
    write(f.root.path(), "profile.json", &profile);
    failure(f.cli("apply"), "policy_denied");
    assert_eq!(f.server.counts(), (0, 0));
    profile = f.profile.clone();
    profile["context"] = json!(digest("different"));
    write(f.root.path(), "profile.json", &profile);
    failure(f.cli("apply"), "stale_admission");
    assert_eq!(f.server.counts(), (0, 0));
    write(f.root.path(), "profile.json", &f.profile);
    success(f.cli("apply"));
    let seq = f.sequence();
    fs::rename(
        f.root.path().join("source"),
        f.root.path().join("offline-source"),
    )
    .unwrap();
    let s = success(f.cli("status"));
    assert_eq!(s["historical_completion"], true);
    assert_eq!(s["accounting_settled"], true);
    assert_eq!(f.sequence(), seq);
}
#[test]
fn private_access_validation_credentials_refresh_and_portable_shape_refusals() {
    let mut f = Fixture::new();
    let obj = f.plan.publication.object.clone();
    for origin in [
        "http://example.test",
        "http://localhost:80",
        "https://user:pass@example.test",
    ] {
        let u = format!("{origin}/{}", obj.sha256.as_str());
        assert!(Client::presigned(digest("id"), 1, obj.clone(), &u, &u, true).is_err());
    }
    let changed = format!(
        "{}/fixture/{}?fixture-access=renewed-private-test",
        f.server.origin,
        obj.sha256.as_str()
    );
    let renewed = Client::presigned(
        f.plan.target.instance.clone(),
        1,
        obj,
        &changed,
        &changed,
        true,
    )
    .unwrap();
    assert_eq!(renewed.target(), f.client.target());
    let versioned = format!("{changed}&versionId=other-representation");
    assert!(
        Client::presigned(
            f.plan.target.instance.clone(),
            1,
            f.plan.publication.object.clone(),
            &versioned,
            &versioned,
            true
        )
        .is_err()
    );
    f.client = renewed;
    f.apply().unwrap();
    let mut value = serde_json::to_value(&f.plan).unwrap();
    value["unknown"] = json!(true);
    assert!(Plan::from_json(&identity::canonical_json(&value).unwrap()).is_err());
    let original = f.plan.to_json().unwrap();
    assert!(Plan::from_json(&[original.as_slice(), b"\n"].concat()).is_err());
}
#[test]
fn settled_missing_witness_bytes_refuse_without_remote_io_or_repair() {
    let mut f = Fixture::new();
    f.apply().unwrap();
    let mut removed = None;
    for path in fs::read_dir(f.root.path().join("witness/objects")).unwrap() {
        let path = path.unwrap().path();
        let bytes = fs::read(&path).unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        if value["schema"] == "tig.s3-upload-completion.design-1" {
            fs::remove_file(&path).unwrap();
            removed = Some(path);
        }
    }
    let removed = removed.unwrap();
    let before = f.server.counts();
    assert_eq!(f.status().unwrap_err(), s3::Error::NeedsRecovery);
    assert!(f.apply().is_err());
    assert_eq!(f.server.counts(), before);
    assert!(!removed.exists());
    f.preserved();
}

#[test]
fn payload_budget_unsupported_guarantees_and_unknown_roots_cannot_enable_upload() {
    let mut f = Fixture::new();
    f.plan.budget.transfer_bytes = 1;
    assert_eq!(f.apply().unwrap_err(), s3::Error::Budget);
    assert_eq!(f.server.counts(), (1, 0));
    f.preserved();
    let f = Fixture::new();
    let mut request = json!({"schema":"tig.s3-upload-request.design-1","operation":"upload","publication":f.plan.publication});
    request["require_retained_custody"] = json!(true);
    write(f.root.path(), "request.json", &request);
    failure(f.cli("plan"), "invalid_document");
    assert_eq!(f.server.counts(), (0, 0));
    for field in ["source_root", "witness_root"] {
        let mut p = f.profile.clone();
        let absent = f.root.path().join(format!("absent-{field}"));
        p[field] = json!(absent);
        write(f.root.path(), "profile.json", &p);
        failure(
            f.cli("apply"),
            if field == "source_root" {
                "source_unavailable"
            } else {
                "witness_unavailable"
            },
        );
        assert!(!absent.exists());
        assert_eq!(f.server.counts(), (0, 0));
    }
}

#[test]
fn ledger_open_contention_is_busy_before_http_and_missing_root_stays_unavailable() {
    let f = Fixture::new();
    let before = f.sequence();
    let blocker = rusqlite::Connection::open(f.root.path().join("ledger/ledger.sqlite")).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE;").unwrap();
    for action in ["plan", "peak", "status", "apply", "resume"] {
        failure(f.cli(action), "busy");
    }
    assert_eq!(f.server.counts(), (0, 0));
    f.preserved();
    blocker.execute_batch("ROLLBACK;").unwrap();
    assert_eq!(f.sequence(), before);
    let absent = f.root.path().join("absent-ledger");
    let mut profile = f.profile.clone();
    profile["ledger_root"] = json!(absent);
    write(f.root.path(), "profile.json", &profile);
    failure(f.cli("status"), "ledger_unavailable");
    assert!(!absent.exists());
    assert_eq!(f.server.counts(), (0, 0));
    f.preserved();
}

#[test]
fn concurrent_cli_publishers_preserve_source_and_retry_exact_shared_witnesses() {
    for hold_ledger_reader in [false, true] {
        let f = Fixture::new();
        let reader =
            rusqlite::Connection::open(f.root.path().join("ledger/ledger.sqlite")).unwrap();
        if hold_ledger_reader {
            reader
                .execute_batch("BEGIN; SELECT COUNT(*) FROM events;")
                .unwrap();
        }
        let spawn = || {
            Command::new(env!("CARGO_BIN_EXE_git-asset-storage"))
                .args(["s3", "apply"])
                .arg(f.root.path().join("profile.json"))
                .arg(f.root.path().join("plan.json"))
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap()
        };
        let first = spawn();
        let second = spawn();
        let a = first.wait_with_output().unwrap();
        let b = second.wait_with_output().unwrap();
        // Nonblocking ledger admission does not promise a winner per racing round.
        // A held reader deterministically exercises both writers returning Busy.
        if hold_ledger_reader {
            assert!(!a.status.success() && !b.status.success());
        }
        for result in [a, b] {
            if result.status.success() {
                success(result);
            } else {
                failure(result, "busy");
            }
        }
        if hold_ledger_reader {
            reader.execute_batch("ROLLBACK;").unwrap();
        }
        success(f.cli("resume"));
        let after = f.server.counts().1;
        success(f.cli("resume"));
        assert_eq!(f.server.counts().1, after);
        assert_eq!(
            f.server.state.lock().unwrap().object.as_ref().unwrap(),
            b"synthetic S3 fixture bytes"
        );
        f.preserved();
    }
}

#[test]
fn held_operation_scope_blocks_its_peer_but_allows_another_job_to_upload() {
    let mut f = Fixture::new();
    LocalCapacityProbe::bind(&f.witness, &mut f.ledger)
        .unwrap()
        .observe(
            &f.witness,
            &mut f.ledger,
            key("peer-probe"),
            key("peer-sample"),
        )
        .unwrap();
    let other_job = f
        .ledger
        .reserve(key("other-job"), key("operator"), 64 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let mut other_plan = f.plan.clone();
    other_plan.operation = key("other-upload");
    let mut profile = f.profile.clone();
    profile["job"] = json!(other_job);
    profile["minimum"] = json!(f.ledger.checkpoint());
    write(f.root.path(), "other-profile.json", &profile);
    fs::write(
        f.root.path().join("other-plan.json"),
        other_plan.to_json().unwrap(),
    )
    .unwrap();
    let root = f.root.path().to_path_buf();
    let run = |other: bool| {
        Command::new(env!("CARGO_BIN_EXE_git-asset-storage"))
            .args(["s3", "apply"])
            .arg(root.join(if other {
                "other-profile.json"
            } else {
                "profile.json"
            }))
            .arg(root.join(if other {
                "other-plan.json"
            } else {
                "plan.json"
            }))
            .output()
            .unwrap()
    };
    let mut checkpoint = 0;
    let result = s3::Execution {
        source: &f.source,
        witness: &f.witness,
        ledger: &mut f.ledger,
        job: &f.job,
        client: &f.client,
        context: &f.plan.context,
    }
    .apply_cancellable(&f.plan, &mut || {
        checkpoint += 1;
        if checkpoint == 2 {
            failure(run(false), "busy");
            success(run(true));
        }
        false
    })
    .unwrap();
    assert!(!result.retained_custody);
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
}
