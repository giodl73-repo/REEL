#![cfg(feature = "upload")]
mod common;
use common::{git, key};
use git_asset::{
    pointer::Pointer,
    upload::{Backend, Plan, Profile},
};
use git_asset_core::{
    Digest, LogicalPath, Object,
    tree::{MemoryNodes, Tree, TreeLimits},
};
use git_asset_store::{
    existing::{Layout, RequiredProtection},
    ledger::{CapacityLedger, Checkpoint},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    orchestration::Place,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    process::{Command, Output, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    LostPut,
    Denied,
    VerifyDenied,
    LostVerify,
    BadScope,
    BadHeader,
    WrongOid,
    CorruptGet,
    MissingGet,
    Redirect,
    Unsupported,
    Expired,
    Slow,
    ScopedCredentials,
    DuplicateHeader,
    BadContentType,
    PastExpiry,
    InvalidExpiry,
    FutureExpiry,
    HoldPut,
}
struct State {
    mode: Mode,
    objects: BTreeMap<String, Vec<u8>>,
    verified: BTreeSet<String>,
    calls: Vec<String>,
    puts: usize,
    verifies: usize,
}
struct Server {
    origin: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Server {
    fn new() -> Self {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", l.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State {
            mode: Mode::Normal,
            objects: BTreeMap::new(),
            verified: BTreeSet::new(),
            calls: Vec::new(),
            puts: 0,
            verifies: 0,
        }));
        let copy = state.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let base = origin.clone();
        let thread = thread::spawn(move || {
            let mut workers = Vec::new();
            while !stopped.load(Ordering::SeqCst) {
                match l.accept() {
                    Ok((s, _)) => {
                        let base = base.clone();
                        let copy = copy.clone();
                        workers.push(thread::spawn(move || serve(s, &base, &copy)));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => panic!("fixture accept {e}"),
                }
            }
            for worker in workers {
                worker.join().unwrap();
            }
        });
        Self {
            origin,
            state,
            stop,
            thread: Some(thread),
        }
    }
    fn mode(&self, m: Mode) {
        self.state.lock().unwrap().mode = m;
    }
    fn counts(&self) -> (usize, usize, usize) {
        let s = self.state.lock().unwrap();
        (s.calls.len(), s.puts, s.verifies)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.thread.take().unwrap().join().unwrap();
    }
}
fn respond(s: &mut TcpStream, code: u16, body: &[u8], extra: &str) {
    let h = format!(
        "HTTP/1.1 {code} fixture\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n",
        body.len()
    );
    if s.write_all(h.as_bytes()).is_ok() {
        let _ = s.write_all(body);
    }
}
fn serve(mut s: TcpStream, base: &str, state: &Mutex<State>) {
    s.set_nonblocking(false).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    s.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut h = Vec::new();
    let mut one = [0];
    while !h.ends_with(b"\r\n\r\n") {
        if s.read(&mut one).unwrap_or(0) == 0 {
            return;
        }
        h.push(one[0]);
        assert!(h.len() <= 16384);
    }
    let h = String::from_utf8(h).unwrap();
    let mut first = h.lines().next().unwrap().split_whitespace();
    let method = first.next().unwrap();
    let path = first.next().unwrap();
    let headers = h
        .lines()
        .skip(1)
        .filter_map(|s| s.split_once(':'))
        .map(|(k, v)| (k.to_ascii_lowercase(), v.trim().to_owned()))
        .collect::<BTreeMap<_, _>>();
    let size = headers
        .get("content-length")
        .map(|n| n.parse::<usize>().unwrap())
        .unwrap_or(0);
    assert!(size <= 8 * 1024 * 1024);
    let mut body = vec![0; size];
    if s.read_exact(&mut body).is_err() {
        return;
    }
    let mode = {
        let mut st = state.lock().unwrap();
        st.calls.push(format!("{method} {path}"));
        st.mode
    };
    if mode == Mode::Slow {
        thread::sleep(Duration::from_millis(180));
    }
    if mode == Mode::Denied {
        respond(&mut s, 403, b"denied", "");
        return;
    }
    if mode == Mode::Redirect {
        respond(
            &mut s,
            307,
            b"",
            "Location: http://127.0.0.1:1/forbidden\r\n",
        );
        return;
    }
    if path == "/repo/objects/batch" {
        assert_eq!(method, "POST");
        assert_eq!(
            headers.get("authorization").map(String::as_str),
            Some("Bearer fixture-batch")
        );
        assert_eq!(
            headers.get("content-type").map(String::as_str),
            Some("application/vnd.git-lfs+json")
        );
        let request: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["ref"]["name"], "refs/heads/main");
        assert_eq!(request["transfers"], json!(["basic"]));
        assert_eq!(request["hash_algo"], "sha256");
        assert_eq!(request["objects"].as_array().unwrap().len(), 1);
        let o = &request["objects"][0];
        let oid = o["oid"].as_str().unwrap();
        let n = o["size"].as_u64().unwrap();
        let st = state.lock().unwrap();
        let action = |kind: &str| json!({"href":format!("{base}/actions/{kind}/{oid}"),"header":{"Authorization":"Bearer fixture-action"},"expires_in":if mode==Mode::Expired {-1} else {86400}});
        let mut row = json!({"oid":if mode==Mode::WrongOid {digest("wrong").as_str().to_owned()} else {oid.to_owned()},"size":n,"authenticated":true});
        match request["operation"].as_str().unwrap() {
            "download" => {
                if st.objects.contains_key(oid) {
                    row["actions"] = json!({"download":action("download")});
                } else {
                    row["error"] = json!({"code":404,"message":"missing"});
                }
            }
            "upload" => {
                if !st.objects.contains_key(oid) {
                    row["actions"] = json!({"upload":action("upload"),"verify":action("verify")});
                } else if !st.verified.contains(oid) {
                    row["actions"] = json!({"verify":action("verify")});
                }
            }
            _ => panic!("wrong operation"),
        }
        if mode == Mode::BadScope && row.get("actions").is_some() {
            for (_, a) in row["actions"].as_object_mut().unwrap() {
                a["href"] = json!(format!("{base}/outside/{oid}"));
            }
        }
        if mode == Mode::BadHeader && row.get("actions").is_some() {
            for (_, a) in row["actions"].as_object_mut().unwrap() {
                a["header"]["Host"] = json!("different.invalid");
            }
        }
        if let Some(actions) = row.get_mut("actions").and_then(Value::as_object_mut) {
            for a in actions.values_mut() {
                match mode {
                    Mode::ScopedCredentials => {
                        a["header"] = json!({});
                    }
                    Mode::DuplicateHeader => {
                        a["header"]["AUTHORIZATION"] = json!("Bearer fixture-action");
                    }
                    Mode::BadContentType => {
                        a["header"]["Content-Type"] = json!("text/plain");
                    }
                    Mode::PastExpiry | Mode::InvalidExpiry | Mode::FutureExpiry => {
                        a.as_object_mut().unwrap().remove("expires_in");
                        a["expires_at"] = json!(match mode {
                            Mode::PastExpiry => "2000-01-01T00:00:00Z",
                            Mode::InvalidExpiry => "2099-02-30T00:00:00Z",
                            _ => "2099-01-01T00:00:00+01:00",
                        });
                    }
                    _ => {}
                }
            }
        }
        if mode == Mode::ScopedCredentials {
            row["authenticated"] = json!(false);
        }
        let response = json!({"transfer":if mode==Mode::Unsupported {"unknown"} else {"basic"},"hash_algo":"sha256","objects":[row]});
        drop(st);
        respond(
            &mut s,
            200,
            &serde_json::to_vec(&response).unwrap(),
            "Content-Type: application/vnd.git-lfs+json\r\n",
        );
        return;
    }
    assert_eq!(
        headers.get("authorization").map(String::as_str),
        Some("Bearer fixture-action")
    );
    let parts = path.split('/').collect::<Vec<_>>();
    assert_eq!(parts.len(), 4);
    let kind = parts[2];
    let oid = parts[3];
    match (method, kind) {
        ("PUT", "upload") => {
            assert_eq!(headers.get("if-none-match").map(String::as_str), Some("*"));
            assert_eq!(Object::from_bytes(&body).sha256.as_str(), oid);
            let mut st = state.lock().unwrap();
            st.puts += 1;
            if st.objects.contains_key(oid) {
                respond(&mut s, 412, b"exists", "");
                return;
            }
            st.objects.insert(oid.into(), body);
            let hold = mode == Mode::HoldPut && st.puts == 1;
            drop(st);
            if hold {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while state.lock().unwrap().mode == Mode::HoldPut {
                    if std::time::Instant::now() >= deadline {
                        return;
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
            if mode != Mode::LostPut {
                respond(&mut s, 200, b"", "");
            }
        }
        ("POST", "verify") => {
            let v: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(v["oid"], oid);
            let mut st = state.lock().unwrap();
            st.verifies += 1;
            if mode == Mode::VerifyDenied {
                respond(&mut s, 403, b"verify denied", "");
                return;
            }
            assert_eq!(
                v["size"].as_u64().unwrap(),
                st.objects.get(oid).unwrap().len() as u64
            );
            st.verified.insert(oid.into());
            drop(st);
            if mode != Mode::LostVerify {
                respond(&mut s, 200, b"", "");
            }
        }
        ("GET", "download") => {
            let b = state.lock().unwrap().objects.get(oid).cloned();
            match (mode, b) {
                (Mode::MissingGet, _) | (_, None) => respond(&mut s, 404, b"missing", ""),
                (Mode::CorruptGet, Some(b)) => respond(&mut s, 200, &vec![b'x'; b.len()], ""),
                (_, Some(b)) => respond(&mut s, 200, &b, ""),
            }
        }
        _ => panic!("unexpected mutation"),
    }
}
struct Fixture {
    root: tempfile::TempDir,
    repo: std::path::PathBuf,
    server: Server,
    profile: Value,
    objects: Vec<Object>,
    bytes: Vec<Vec<u8>>,
    minimum: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("git-asset upload fixture ")
            .tempdir()
            .unwrap();
        let repo = root.path().join("source");
        fs::create_dir(&repo).unwrap();
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Fixture"]);
        git(&repo, &["config", "user.email", "fixture@example.invalid"]);
        git(&repo, &["lfs", "install", "--local", "--skip-smudge"]);
        fs::write(
            repo.join(".gitattributes"),
            "*.bin filter=lfs diff=lfs merge=lfs -text\n",
        )
        .unwrap();
        let bytes = vec![
            b"first shared upload bytes".to_vec(),
            vec![17; 128 * 1024 + 19],
        ];
        let objects = bytes
            .iter()
            .map(|b| Object::from_bytes(b))
            .collect::<Vec<_>>();
        for (i, b) in bytes.iter().enumerate() {
            fs::write(repo.join(format!("asset-{i}.bin")), b).unwrap();
        }
        git(&repo, &["add", "."]);
        git(
            &repo,
            &[
                "-c",
                "core.hooksPath=NUL",
                "commit",
                "-m",
                "synthetic assets",
            ],
        );
        let remote = root.path().join("origin.git");
        git(root.path(), &["init", "--bare", remote.to_str().unwrap()]);
        git(
            &repo,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        for (i, o) in objects.iter().enumerate() {
            assert_eq!(
                Pointer::parse(&git(&repo, &["show", &format!("HEAD:asset-{i}.bin")]).stdout)
                    .unwrap()
                    .object(),
                o
            );
        }
        let nodes = root.path().join("nodes");
        fs::create_dir(&nodes).unwrap();
        let bindings = objects
            .iter()
            .map(|o| (LogicalPath::parse(o.sha256.as_str()).unwrap(), o.clone()))
            .collect();
        let mut memory = MemoryNodes::default();
        let (tree, _) = Tree::build(&bindings, &mut memory, TreeLimits::default()).unwrap();
        for (id, b) in memory.objects() {
            fs::write(nodes.join(id.as_str()), b).unwrap();
        }
        let witness = LocalStore::create(&root.path().join("witness"), key("witness")).unwrap();
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
        let job = ledger
            .reserve(key("job"), key("owner"), 128 * 1024 * 1024)
            .unwrap()
            .reference()
            .clone();
        let minimum = ledger.checkpoint();
        let server = Server::new();
        let profile = json!({"schema":"tig.lfs-upload.design-1","source_root":repo.join(".git/lfs"),"source_identity":digest("source"),"source_generation":1,"source_protection":if cfg!(windows) {RequiredProtection::PreventMutation} else {RequiredProtection::CooperativeImmutable},
            "authority_root":tree.root(),"authority_nodes":nodes,"authority_checkpoint":null,"max_authority_entries":500000,
            "witness_root":root.path().join("witness"),"witness":Place::local(&witness,1).unwrap(),"ledger_root":root.path().join("ledger"),"minimum":minimum,"capacity":ledger.identity(),"job":job,
            "owner":"owner","context":digest("policy"),"operation_scope":"fixture-upload","expected_remote":"origin","ref_name":"refs/heads/main",
            "remote":{"instance":digest("remote"),"generation":1,"batch_url":format!("{}/repo/objects/batch",server.origin),"headers":{"Authorization":"Bearer fixture-batch"},"scopes":[{"origin":server.origin,"prefix":"/actions/","headers":["Authorization"]}],"loopback_fixture":true},
            "budget":{"object_bytes":8*1024*1024,"total_source_bytes":32*1024*1024,"objects":32,"http_requests":12,"transfer_bytes":32*1024*1024,"millis":5000},"allow_upload":true});
        write(root.path(), "profile.json", &profile);
        write(root.path(), "object.json", &json!(objects[0]));
        let f = Self {
            root,
            repo,
            server,
            profile,
            objects,
            bytes,
            minimum,
        };
        f.plan_file();
        f
    }
    fn backend(&self) -> Backend {
        Backend::new(Profile::from_json(&serde_json::to_vec(&self.profile).unwrap()).unwrap())
            .unwrap()
    }
    fn plan_file(&self) {
        let p = self.backend().plan(self.objects[0].clone()).unwrap();
        fs::write(self.root.path().join("plan.json"), p.to_json().unwrap()).unwrap();
    }
    fn path(&self, i: usize) -> std::path::PathBuf {
        Layout::GitLfs.object_path(&self.repo.join(".git/lfs"), &self.objects[i].sha256)
    }
    fn cli(&self, action: &str) -> Output {
        Command::new(env!("CARGO_BIN_EXE_git-asset-lfs"))
            .args(["upload", action])
            .arg(self.root.path().join("profile.json"))
            .arg(self.root.path().join(if action == "plan" {
                "object.json"
            } else {
                "plan.json"
            }))
            .output()
            .unwrap()
    }
    fn seq(&self) -> u64 {
        CapacityLedger::open(&self.root.path().join("ledger"), &self.minimum)
            .unwrap()
            .checkpoint()
            .sequence()
    }
    fn preserved(&self) {
        for (i, b) in self.bytes.iter().enumerate() {
            assert_eq!(fs::read(self.path(i)).unwrap(), *b);
        }
    }
    fn protocol(&self, frames: &[Value]) -> Output {
        let mut p = Command::new(env!("CARGO_BIN_EXE_git-asset-lfs"))
            .arg("--upload-profile")
            .arg(self.root.path().join("profile.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        {
            let mut input = p.stdin.take().unwrap();
            for frame in frames {
                serde_json::to_writer(&mut input, frame).unwrap();
                input.write_all(b"\n").unwrap();
            }
        }
        p.wait_with_output().unwrap()
    }
    fn frame(&self, i: usize) -> Value {
        json!({"event":"upload","oid":self.objects[i].sha256,"size":self.objects[i].bytes,"path":self.path(i),"action":null})
    }
}
fn write(p: &Path, name: &str, v: &Value) {
    fs::write(p.join(name), serde_json::to_vec(v).unwrap()).unwrap();
}
fn success(o: Output) -> Value {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(
        o.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&o.stderr)
    );
    serde_json::from_slice(&o.stdout).unwrap()
}
fn failure(o: Output, code: &str) {
    assert!(!o.status.success());
    assert!(o.stderr.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&o.stdout).unwrap()["code"],
        code
    );
}
fn init() -> Value {
    json!({"event":"init","operation":"upload","remote":"origin","concurrent":true,"concurrenttransfers":2})
}
#[test]
fn actual_git_lfs_push_uploads_verifies_reuses_and_preserves_source() {
    let f = Fixture::new();
    let profile = f.root.path().join("profile.json");
    let arg = format!(
        "--upload-profile '{}'",
        profile
            .to_str()
            .unwrap()
            .replace('\\', "/")
            .replace('\'', "'\\''")
    );
    for (k, v) in [
        (
            "lfs.customtransfer.git-asset-upload.path",
            env!("CARGO_BIN_EXE_git-asset-lfs"),
        ),
        ("lfs.customtransfer.git-asset-upload.args", arg.as_str()),
        ("lfs.customtransfer.git-asset-upload.direction", "upload"),
        ("lfs.customtransfer.git-asset-upload.concurrent", "true"),
        ("lfs.concurrenttransfers", "2"),
        ("lfs.locksverify", "false"),
    ] {
        git(&f.repo, &["config", k, v]);
    }
    git(
        &f.repo,
        &[
            "-c",
            "lfs.standalonetransferagent=git-asset-upload",
            "lfs",
            "push",
            "--all",
            "origin",
            "main",
        ],
    );
    f.preserved();
    let s = f.server.state.lock().unwrap();
    for (i, o) in f.objects.iter().enumerate() {
        assert_eq!(s.objects.get(o.sha256.as_str()).unwrap(), &f.bytes[i]);
        assert!(s.verified.contains(o.sha256.as_str()));
    }
    assert_eq!(s.puts, 2);
    drop(s);
    git(
        &f.repo,
        &[
            "-c",
            "lfs.standalonetransferagent=git-asset-upload",
            "lfs",
            "push",
            "--all",
            "origin",
            "main",
        ],
    );
    assert_eq!(f.server.counts().1, 2);
    f.preserved();
}
#[test]
fn cli_plan_apply_status_and_exact_retry_have_no_extra_writes() {
    let f = Fixture::new();
    let before = f.seq();
    let plan = success(f.cli("plan"));
    assert!(plan.get("object").is_some());
    assert_eq!(f.server.counts(), (0, 0, 0));
    assert_eq!(f.seq(), before);
    success(f.cli("peak"));
    let result = success(f.cli("apply"));
    assert_eq!(result["retained_custody"], false);
    assert_eq!(result["removal_authority"], false);
    let after = f.seq();
    let status = success(f.cli("status"));
    assert_eq!(status["historical_completion"], true);
    assert_eq!(status["verified_present"], true);
    assert_eq!(status["accounting_settled"], true);
    success(f.cli("resume"));
    assert_eq!(f.seq(), after);
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
}
#[test]
fn lost_put_and_verify_replies_resume_without_reupload_or_skipped_registration() {
    for mode in [Mode::LostPut, Mode::LostVerify, Mode::VerifyDenied] {
        let f = Fixture::new();
        f.server.mode(mode);
        failure(
            f.cli("apply"),
            if mode == Mode::VerifyDenied {
                "access_denied"
            } else {
                "remote_unavailable"
            },
        );
        f.preserved();
        f.server.mode(Mode::Normal);
        success(f.cli("resume"));
        assert_eq!(f.server.counts().1, 1);
        let st = f.server.state.lock().unwrap();
        assert!(st.verified.contains(f.objects[0].sha256.as_str()));
        drop(st);
        f.preserved();
    }
}
#[test]
fn bad_remote_scope_headers_oid_transfer_expiry_and_denial_preserve_source() {
    for (mode, code) in [
        (Mode::BadScope, "authority"),
        (Mode::BadHeader, "authority"),
        (Mode::WrongOid, "integrity"),
        (Mode::Unsupported, "unsupported"),
        (Mode::Expired, "access_denied"),
        (Mode::Denied, "access_denied"),
        (Mode::Redirect, "remote_unavailable"),
    ] {
        let f = Fixture::new();
        f.server.mode(mode);
        failure(f.cli("apply"), code);
        assert_eq!(f.server.counts().1, 0);
        f.preserved();
    }
}
#[test]
fn corrupt_or_missing_readback_cannot_complete_and_acknowledged_loss_is_not_reuploaded() {
    for mode in [Mode::CorruptGet, Mode::MissingGet] {
        let f = Fixture::new();
        f.server.mode(mode);
        failure(f.cli("apply"), "integrity");
        f.preserved();
        f.server.mode(Mode::Normal);
        success(f.cli("resume"));
        assert_eq!(f.server.counts().1, 1);
    }
    let f = Fixture::new();
    success(f.cli("apply"));
    f.server.state.lock().unwrap().objects.clear();
    let s = success(f.cli("status"));
    assert_eq!(s["requires_recovery"], true);
    failure(f.cli("resume"), "missing_acknowledged_remote");
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
}
#[test]
fn standalone_protocol_rejects_wrong_remote_path_and_injected_action_then_continues() {
    let f = Fixture::new();
    let mut bad = f.frame(0);
    bad["path"] = json!(f.root.path().join("outside"));
    let mut action = f.frame(0);
    action["action"] = json!({"href":"http://127.0.0.1:1/exfil"});
    let out = f.protocol(&[
        init(),
        bad,
        action,
        f.frame(0),
        json!({"event":"terminate"}),
    ]);
    assert!(out.status.success());
    assert!(out.stderr.is_empty());
    let rows = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str::<Value>(s).unwrap())
        .collect::<Vec<_>>();
    assert!(rows[1].get("error").is_some());
    assert!(rows[2].get("error").is_some());
    assert_eq!(rows.last().unwrap()["event"], "complete");
    assert!(rows.last().unwrap().get("path").is_none());
    assert_eq!(f.server.counts().1, 1);
    f.preserved();
    let g = Fixture::new();
    let mut wrong = init();
    wrong["remote"] = json!("other");
    let out = g.protocol(&[wrong]);
    assert!(
        serde_json::from_slice::<Value>(&out.stdout)
            .unwrap()
            .get("error")
            .is_some()
    );
    assert_eq!(g.server.counts(), (0, 0, 0));
}
#[test]
fn disabled_stale_unauthorized_and_unknown_source_profiles_do_not_expand_access() {
    let f = Fixture::new();
    let mut p = f.profile.clone();
    p.as_object_mut().unwrap().remove("allow_upload");
    write(f.root.path(), "profile.json", &p);
    failure(f.cli("apply"), "upload_disabled");
    assert_eq!(f.server.counts(), (0, 0, 0));
    let mut p = f.profile.clone();
    p["context"] = json!(digest("changed"));
    write(f.root.path(), "profile.json", &p);
    failure(f.cli("apply"), "authority");
    assert_eq!(f.server.counts(), (0, 0, 0));
    write(f.root.path(), "profile.json", &f.profile);
    let mut frame = f.frame(0);
    frame["oid"] = json!(digest("unauthorized"));
    let out = f.protocol(&[init(), frame, json!({"event":"terminate"})]);
    assert!(out.status.success());
    assert_eq!(f.server.counts(), (0, 0, 0));
    for field in ["source_root", "witness_root", "ledger_root"] {
        let mut profile = f.profile.clone();
        let absent = f.root.path().join(format!("absent-{field}"));
        profile[field] = json!(absent);
        write(f.root.path(), "profile.json", &profile);
        failure(
            f.cli("apply"),
            if field == "source_root" {
                "authority"
            } else {
                "needs_recovery"
            },
        );
        assert!(!absent.exists());
        assert_eq!(f.server.counts(), (0, 0, 0));
    }
    f.preserved();
}
#[test]
fn witness_corruption_holds_without_repair_and_status_needs_no_source_payload() {
    let f = Fixture::new();
    success(f.cli("apply"));
    fs::rename(f.repo.join(".git/lfs"), f.repo.join(".git/offline-lfs")).unwrap();
    let before = f.seq();
    assert_eq!(success(f.cli("status"))["verified_present"], true);
    assert_eq!(f.seq(), before);
    let mut removed = None;
    for entry in fs::read_dir(f.root.path().join("witness/objects")).unwrap() {
        let path = entry.unwrap().path();
        let v: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if v["schema"] == "tig.lfs-upload-completion.design-1" {
            fs::remove_file(&path).unwrap();
            removed = Some(path);
        }
    }
    let before = f.server.counts();
    failure(f.cli("status"), "needs_recovery");
    assert_eq!(f.server.counts(), before);
    assert!(!removed.unwrap().exists());
}
#[test]
fn strict_plan_and_bounded_http_refuse_without_unbounded_work() {
    let f = Fixture::new();
    let mut plan = f
        .backend()
        .plan(f.objects[0].clone())
        .unwrap()
        .to_json()
        .unwrap();
    plan.push(b'\n');
    assert!(Plan::from_json(&plan).is_err());
    let mut p = f.profile.clone();
    p["budget"]["http_requests"] = json!(1);
    let mut backend =
        Backend::new(Profile::from_json(&serde_json::to_vec(&p).unwrap()).unwrap()).unwrap();
    let plan = backend.plan(f.objects[0].clone()).unwrap();
    assert_eq!(
        backend.apply(&plan, &f.path(0)).unwrap_err(),
        git_asset::upload::Error::Budget
    );
    assert_eq!(f.server.counts().1, 0);
    f.preserved();
}
#[test]
fn private_action_credentials_are_scoped_refreshable_and_absent_from_plans() {
    let f = Fixture::new();
    let mut profile = f.profile.clone();
    profile["remote"]["scopes"][0]["credentials"] =
        json!({"Authorization":"Bearer fixture-action"});
    let p = Profile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap();
    let mut b = Backend::new(p).unwrap();
    let plan = b.plan(f.objects[0].clone()).unwrap();
    assert_eq!(
        plan.to_json().unwrap(),
        f.backend()
            .plan(f.objects[0].clone())
            .unwrap()
            .to_json()
            .unwrap()
    );
    assert!(
        !String::from_utf8(plan.to_json().unwrap())
            .unwrap()
            .contains("Bearer")
    );
    f.server.mode(Mode::ScopedCredentials);
    b.apply(&plan, &f.path(0)).unwrap();
    profile["remote"]["scopes"][0]["credentials"]["Authorization"] =
        json!("Bearer renewed-private-token");
    let b =
        Backend::new(Profile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap()).unwrap();
    assert_eq!(
        b.plan(f.objects[0].clone()).unwrap().to_json().unwrap(),
        plan.to_json().unwrap()
    );
    f.preserved();
}
#[test]
fn conflicting_duplicate_and_fixed_action_headers_refuse_before_upload() {
    for mode in [Mode::DuplicateHeader, Mode::BadContentType] {
        let f = Fixture::new();
        f.server.mode(mode);
        failure(f.cli("apply"), "authority");
        assert_eq!(f.server.counts().1, 0);
        f.preserved();
    }
    let f = Fixture::new();
    let mut profile = f.profile.clone();
    profile["remote"]["scopes"][0]["credentials"] =
        json!({"Authorization":"Bearer conflicting-token"});
    let mut b =
        Backend::new(Profile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap()).unwrap();
    let plan = b.plan(f.objects[0].clone()).unwrap();
    assert_eq!(
        b.apply(&plan, &f.path(0)).unwrap_err(),
        git_asset::upload::Error::Authority
    );
    assert_eq!(f.server.counts().1, 0);
    f.preserved();
}
#[test]
fn absolute_expiry_and_http_deadline_are_enforced() {
    for (mode, code) in [
        (Mode::PastExpiry, "access_denied"),
        (Mode::InvalidExpiry, "invalid_document"),
    ] {
        let f = Fixture::new();
        f.server.mode(mode);
        failure(f.cli("apply"), code);
        assert_eq!(f.server.counts().1, 0);
    }
    let f = Fixture::new();
    f.server.mode(Mode::FutureExpiry);
    success(f.cli("apply"));
    let f = Fixture::new();
    f.server.mode(Mode::Slow);
    let mut p = f.profile.clone();
    p["budget"]["millis"] = json!(50);
    let mut b =
        Backend::new(Profile::from_json(&serde_json::to_vec(&p).unwrap()).unwrap()).unwrap();
    let plan = b.plan(f.objects[0].clone()).unwrap();
    let start = std::time::Instant::now();
    assert_eq!(
        b.apply(&plan, &f.path(0)).unwrap_err(),
        git_asset::upload::Error::Budget
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    assert_eq!(f.server.counts().1, 0);
    f.preserved();
}
#[test]
fn killed_upload_holds_its_peer_allows_another_job_and_resumes_without_reupload() {
    let f = Fixture::new();
    let witness = LocalStore::open(&f.root.path().join("witness")).unwrap();
    let mut ledger = CapacityLedger::open(&f.root.path().join("ledger"), &f.minimum).unwrap();
    LocalCapacityProbe::bind(&witness, &mut ledger)
        .unwrap()
        .observe(
            &witness,
            &mut ledger,
            key("other-probe"),
            key("other-sample"),
        )
        .unwrap();
    let other_job = ledger
        .reserve(key("other-job"), key("owner"), 128 * 1024 * 1024)
        .unwrap()
        .reference()
        .clone();
    let mut profile = f.profile.clone();
    profile["job"] = json!(other_job);
    profile["operation_scope"] = json!("other-upload");
    profile["minimum"] = json!(ledger.checkpoint());
    let other =
        Backend::new(Profile::from_json(&serde_json::to_vec(&profile).unwrap()).unwrap()).unwrap();
    let plan = other.plan(f.objects[1].clone()).unwrap();
    write(f.root.path(), "other-profile.json", &profile);
    fs::write(
        f.root.path().join("other-plan.json"),
        plan.to_json().unwrap(),
    )
    .unwrap();
    f.server.mode(Mode::HoldPut);
    let mut first = Command::new(env!("CARGO_BIN_EXE_git-asset-lfs"))
        .args(["upload", "apply"])
        .arg(f.root.path().join("profile.json"))
        .arg(f.root.path().join("plan.json"))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(4);
    while f.server.counts().1 == 0 {
        assert!(
            first.try_wait().unwrap().is_none(),
            "upload unexpectedly exited"
        );
        assert!(
            std::time::Instant::now() < deadline,
            "upload did not reach PUT"
        );
        thread::sleep(Duration::from_millis(2));
    }
    failure(f.cli("apply"), "busy");
    // The first PUT stays paused; another job uses the same ledger/store.
    success(
        Command::new(env!("CARGO_BIN_EXE_git-asset-lfs"))
            .args(["upload", "apply"])
            .arg(f.root.path().join("other-profile.json"))
            .arg(f.root.path().join("other-plan.json"))
            .output()
            .unwrap(),
    );
    assert!(first.try_wait().unwrap().is_none());
    first.kill().unwrap();
    let result = first.wait_with_output().unwrap();
    assert!(!result.status.success());
    f.server.mode(Mode::Normal);
    assert_eq!(f.server.counts().1, 2);
    success(f.cli("resume"));
    assert_eq!(f.server.counts().1, 2);
    f.preserved();
}
