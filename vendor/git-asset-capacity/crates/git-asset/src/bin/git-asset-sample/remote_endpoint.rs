//! Bounded literal-loopback teaching adapter over actual accounted LocalStore.
//! Not a production LFS server or retained-custody capability.
use super::{Result, key};
use git_asset_core::Object;
use git_asset_store::{
    AckLevel, Request, accounted,
    ledger::{CapacityLedger, JournalRef},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
struct State {
    store: LocalStore,
    ledger: CapacityLedger,
    job: JournalRef,
    admitted: BTreeMap<String, Object>,
    published: BTreeSet<String>,
    registered: BTreeSet<String>,
    requests: u64,
    puts: u64,
    put_bytes: u64,
    get_bytes: u64,
    corrupt: bool,
}
impl State {
    fn bytes(&self, oid: &str) -> Result<Vec<u8>> {
        let object = self.admitted.get(oid).ok_or("unknown receiver object")?;
        let mut bytes = Vec::new();
        self.store
            .read_object(object)?
            .take(65)
            .read_to_end(&mut bytes)?;
        if &Object::from_bytes(&bytes) != object {
            return Err("receiver integrity".into());
        }
        Ok(bytes)
    }
}
pub(super) struct Endpoint {
    pub origin: String,
    state: Arc<Mutex<State>>,
    stop: Arc<AtomicBool>,
    worker: Option<thread::JoinHandle<std::result::Result<(), String>>>,
}
impl Endpoint {
    pub fn new(root: &Path, objects: &[Object]) -> Result<Self> {
        let admitted = objects
            .iter()
            .map(|o| (o.sha256.as_str().to_owned(), o.clone()))
            .collect::<BTreeMap<_, _>>();
        if admitted.len() != 2 || objects.iter().any(|o| o.bytes > 64 || o.bytes == 0) {
            return Err("receiver scope".into());
        }
        let store =
            LocalStore::create(&root.join("remote-receiver"), key("sample-remote-receiver"))?;
        let mut ledger = CapacityLedger::create(
            &root.join("remote-receiver-ledger"),
            key("sample-remote-capacity"),
            key("sample-remote-epoch"),
            0,
        )?;
        LocalCapacityProbe::bind(&store, &mut ledger)?.observe(
            &store,
            &mut ledger,
            key("remote-probe"),
            key("sample-remote"),
        )?;
        let job = ledger
            .reserve(key("remote-job"), key("sample"), 64 * 1024 * 1024)?
            .reference()
            .clone();
        let state = Arc::new(Mutex::new(State {
            store,
            ledger,
            job,
            admitted,
            published: BTreeSet::new(),
            registered: BTreeSet::new(),
            requests: 0,
            puts: 0,
            put_bytes: 0,
            get_bytes: 0,
            corrupt: false,
        }));
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let origin = format!("http://{}", listener.local_addr()?);
        let stop = Arc::new(AtomicBool::new(false));
        let halted = stop.clone();
        let copy = state.clone();
        let base = origin.clone();
        let worker = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(120);
            while !halted.load(Ordering::SeqCst) {
                if Instant::now() >= deadline {
                    return Err("receiver lifetime exhausted".into());
                }
                match listener.accept() {
                    Ok((mut stream, peer)) => {
                        if !peer.ip().is_loopback() {
                            return Err("non-loopback receiver peer".into());
                        }
                        let mut st = copy.lock().map_err(|_| "receiver state poisoned")?;
                        if st.requests >= 64 {
                            return Err("receiver request budget".into());
                        }
                        st.requests += 1;
                        serve(&mut stream, &base, &mut st).map_err(|e| e.to_string())?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => return Err(e.to_string()),
                }
            }
            Ok(())
        });
        Ok(Self {
            origin,
            state,
            stop,
            worker: Some(worker),
        })
    }
    pub fn corrupt(&self, value: bool) -> Result<()> {
        self.state
            .lock()
            .map_err(|_| "receiver state poisoned")?
            .corrupt = value;
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Value> {
        let st = self.state.lock().map_err(|_| "receiver state poisoned")?;
        let mut objects = Vec::new();
        for oid in &st.published {
            st.bytes(oid)?;
            objects.push(st.admitted[oid].clone());
        }
        Ok(
            json!({"store":st.store.identity(),"capacity":st.ledger.identity(),"checkpoint":st.ledger.checkpoint(),"objects":objects,"registered":st.registered,"requests":st.requests,"put_count":st.puts,"put_payload_bytes":st.put_bytes,"get_payload_bytes":st.get_bytes,"corrupt_response_injected":st.corrupt,"retained_custody":false,"removal_authority":false}),
        )
    }
    pub fn finish(mut self) -> Result<Value> {
        self.stop.store(true, Ordering::SeqCst);
        self.worker
            .take()
            .ok_or("receiver already stopped")?
            .join()
            .map_err(|_| "receiver thread panicked")?
            .map_err(std::io::Error::other)?;
        self.snapshot()
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn reply(s: &mut TcpStream, code: u16, body: &[u8], json_body: bool) -> Result<()> {
    let kind = if json_body {
        "Content-Type: application/vnd.git-lfs+json\r\n"
    } else {
        ""
    };
    write!(
        s,
        "HTTP/1.1 {code} fixture\r\nContent-Length: {}\r\nConnection: close\r\n{kind}\r\n",
        body.len()
    )?;
    s.write_all(body)?;
    Ok(())
}
fn read_bounded(s: &mut TcpStream, n: usize, deadline: Instant) -> Result<Vec<u8>> {
    if n > 16384 {
        return Err("receiver body budget".into());
    }
    let mut out = vec![0; n];
    let mut at = 0;
    while at < n {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or("receiver IO deadline")?;
        s.set_read_timeout(Some(left.min(Duration::from_secs(3))))?;
        let got = s.read(&mut out[at..])?;
        if got == 0 {
            return Err("receiver truncated request".into());
        }
        at += got;
    }
    Ok(out)
}
fn serve(s: &mut TcpStream, base: &str, st: &mut State) -> Result<()> {
    s.set_nonblocking(false)?;
    s.set_write_timeout(Some(Duration::from_secs(3)))?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut raw = Vec::new();
    while !raw.ends_with(b"\r\n\r\n") {
        if raw.len() >= 16384 {
            return Err("receiver header budget".into());
        }
        raw.extend(read_bounded(s, 1, deadline)?);
    }
    let text = std::str::from_utf8(&raw)?;
    let mut lines = text.split("\r\n");
    let line = lines.next().ok_or("missing request")?;
    let fields = line.split(' ').collect::<Vec<_>>();
    if fields.len() != 3 || fields[2] != "HTTP/1.1" {
        return Err("receiver request line".into());
    }
    let (method, path) = (fields[0], fields[1]);
    let mut headers = BTreeMap::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let (name, value) = line.split_once(':').ok_or("receiver header")?;
        if headers
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("receiver duplicate header".into());
        }
    }
    if headers.contains_key("transfer-encoding") {
        return Err("receiver transfer encoding".into());
    }
    let size = headers
        .get("content-length")
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(0);
    let body = read_bounded(s, size, deadline)?;
    if method == "POST" && path == "/repo/objects/batch" {
        let v: Value = git_asset_core::identity::parse_json(&body)?;
        let rows = v["objects"].as_array().ok_or("receiver batch objects")?;
        if rows.len() != 1
            || v["hash_algo"] != "sha256"
            || v["transfers"] != json!(["basic"])
            || v["ref"]["name"] != "refs/heads/main"
        {
            return Err("receiver batch scope".into());
        }
        let oid = rows[0]["oid"].as_str().ok_or("receiver oid")?;
        let object = st.admitted.get(oid).ok_or("unadmitted receiver oid")?;
        if rows[0]["size"] != json!(object.bytes) {
            return Err("receiver length".into());
        }
        let action = |kind: &str| json!({"href":format!("{base}/actions/{kind}/{oid}"),"header":{},"expires_in":60});
        let mut row = json!({"oid":oid,"size":object.bytes,"authenticated":true});
        match v["operation"].as_str() {
            Some("download") if st.published.contains(oid) => {
                row["actions"] = json!({"download":action("download")})
            }
            Some("download") => {
                row["error"] = json!({"code":404,"message":"missing fixture payload"})
            }
            Some("upload") if !st.published.contains(oid) => {
                row["actions"] = json!({"upload":action("upload"),"verify":action("verify")})
            }
            Some("upload") if !st.registered.contains(oid) => {
                row["actions"] = json!({"verify":action("verify")})
            }
            Some("upload") => {}
            _ => return Err("receiver batch operation".into()),
        }
        return reply(
            s,
            200,
            &serde_json::to_vec(&json!({"transfer":"basic","hash_algo":"sha256","objects":[row]}))?,
            true,
        );
    }
    let parts = path.split('/').collect::<Vec<_>>();
    if parts.len() != 4 || !parts[0].is_empty() || parts[1] != "actions" {
        return Err("receiver action route".into());
    }
    let oid = parts[3];
    let object = st.admitted.get(oid).ok_or("unadmitted action oid")?.clone();
    match (method, parts[2]) {
        ("PUT", "upload") => {
            if headers.get("if-none-match").map(String::as_str) != Some("*")
                || Object::from_bytes(&body) != object
            {
                return Err("receiver immutable upload".into());
            }
            if st.published.contains(oid) {
                return reply(s, 412, b"exists", false);
            }
            let request = Request {
                operation: key(&format!("receive-{oid}")),
                owner: key("sample"),
                object,
                required: AckLevel::ProcessRestart,
            };
            let plan = accounted::plan(&st.store, &request)?;
            let receipt = accounted::publish_accounted(
                &mut st.ledger,
                &st.store,
                &st.job,
                &plan,
                &mut std::io::Cursor::new(&body),
            )?;
            if receipt.publication().request() != &request
                || receipt.publication().level() != AckLevel::ProcessRestart
            {
                return Err("receiver acknowledgement".into());
            }
            st.bytes(oid)?;
            st.puts += 1;
            st.put_bytes += body.len() as u64;
            st.published.insert(oid.to_owned());
            reply(s, 200, b"", false)
        }
        ("POST", "verify") => {
            let v: Value = git_asset_core::identity::parse_json(&body)?;
            if !st.published.contains(oid) || v["oid"] != oid || v["size"] != json!(object.bytes) {
                return Err("receiver verification scope".into());
            }
            st.bytes(oid)?;
            st.registered.insert(oid.to_owned());
            reply(s, 200, b"", false)
        }
        ("GET", "download") => {
            if !st.published.contains(oid) {
                return reply(s, 404, b"missing", false);
            }
            let mut bytes = st.bytes(oid)?;
            st.get_bytes += bytes.len() as u64;
            if st.corrupt {
                bytes[0] ^= 1;
            }
            reply(s, 200, &bytes, false)
        }
        _ => Err("receiver unsupported action".into()),
    }
}
