//! Owned finite loopback HTTP/TLS fixture only; never a production server.
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};
use tig_core::Object;
pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub declared: Option<usize>,
    pub delay: Duration,
    pub body_delay: Duration,
    pub chunked: bool,
}
impl Reply {
    pub fn bytes(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            headers: vec![],
            body,
            declared: None,
            delay: Duration::ZERO,
            body_delay: Duration::ZERO,
            chunked: false,
        }
    }
    pub fn json(value: serde_json::Value) -> Self {
        let mut r = Self::bytes(serde_json::to_vec(&value).unwrap());
        r.headers
            .push(("Content-Type".into(), "application/vnd.git-lfs+json".into()));
        r
    }
}
pub fn batch(root: &str, object: &Object) -> serde_json::Value {
    serde_json::json!({"transfer":"basic","hash_algo":"sha256","objects":[{"oid":object.sha256.as_str(),"size":object.bytes,"authenticated":true,
        "actions":{"download":{"href":format!("{root}/download?token=synthetic-action"),"header":{"Authorization":"Bearer synthetic-download"},"expires_in":30}}}]})
}
type Handler = dyn Fn(&Request, &str) -> Reply + Send + Sync;
pub struct Server {
    pub root: String,
    pub cert: Option<String>,
    pub requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
trait Duplex: Read + Write + Send {
    fn finish(&mut self) {}
}
impl Duplex for TcpStream {}
impl Duplex for rustls::StreamOwned<rustls::ServerConnection, TcpStream> {
    fn finish(&mut self) {
        self.conn.send_close_notify();
        let _ = self.flush();
    }
}
impl Server {
    pub fn new(
        tls: bool,
        handler: impl Fn(&Request, &str) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let root = format!(
            "{}://{}",
            if tls { "https" } else { "http" },
            listener.local_addr().unwrap()
        );
        let (server_config, cert) = if tls {
            let rcgen::CertifiedKey { cert, signing_key } =
                rcgen::generate_simple_self_signed(vec!["localhost".into(), "127.0.0.1".into()])
                    .unwrap();
            let key = rustls::pki_types::PrivatePkcs8KeyDer::from(signing_key.serialize_der());
            let config = rustls::ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert.der().clone()], key.into())
                .unwrap();
            (Some(Arc::new(config)), Some(cert.pem()))
        } else {
            (None, None)
        };
        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = requests.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let done = stop.clone();
        let address = root.clone();
        let handler: Arc<Handler> = Arc::new(handler);
        let worker = thread::spawn(move || {
            while !done.load(Ordering::Acquire) {
                let stream = match listener.accept() {
                    Ok((s, _)) => s,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(1));
                        continue;
                    }
                    Err(_) => break,
                };
                // Accepted sockets can inherit the listener's nonblocking mode
                // on Windows; the finite connection worker uses blocking IO.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                stream
                    .set_write_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut stream: Box<dyn Duplex> = match &server_config {
                    Some(config) => Box::new(rustls::StreamOwned::new(
                        rustls::ServerConnection::new(config.clone()).unwrap(),
                        stream,
                    )),
                    None => Box::new(stream),
                };
                if let Some(request) = read_request(&mut *stream) {
                    let reply = handler(&request, &address);
                    observed.lock().unwrap().push(request);
                    send_reply(&mut *stream, reply);
                }
                stream.finish();
            }
        });
        Self {
            root,
            cert,
            requests,
            stop,
            worker: Some(worker),
        }
    }
    pub fn good(bytes: Vec<u8>, tls: bool) -> Self {
        let object = Object::from_bytes(&bytes);
        Self::new(tls, move |r, root| {
            if r.method == "POST" {
                Reply::json(batch(root, &object))
            } else {
                Reply::bytes(bytes.clone())
            }
        })
    }
    pub fn endpoint(&self) -> String {
        format!("{}/objects/batch", self.root)
    }
    pub fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker.join().unwrap();
        }
    }
}
fn read_request(stream: &mut dyn Duplex) -> Option<Request> {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        if bytes.len() >= 32_768 || stream.read(&mut byte).ok()? == 0 {
            return None;
        }
        bytes.push(byte[0]);
    }
    let text = String::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.into();
    let path = first.next()?.into();
    let mut headers = BTreeMap::new();
    for line in lines.filter(|s| !s.is_empty()) {
        let (name, value) = line.split_once(':')?;
        headers.insert(name.to_ascii_lowercase(), value.trim().into());
    }
    let length = headers
        .get("content-length")
        .map(|s: &String| s.parse::<usize>())
        .transpose()
        .ok()?
        .unwrap_or(0);
    if length > 4096 {
        return None;
    }
    let mut body = vec![0; length];
    stream.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body,
    })
}
fn send_reply(stream: &mut dyn Duplex, r: Reply) {
    thread::sleep(r.delay);
    let mut head = format!("HTTP/1.1 {} Fixture\r\nConnection: close\r\n", r.status);
    if r.chunked {
        head.push_str("Transfer-Encoding: chunked\r\n");
    } else {
        head.push_str(&format!(
            "Content-Length: {}\r\n",
            r.declared.unwrap_or(r.body.len())
        ));
    }
    for (name, value) in r.headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    head.push_str("\r\n");
    if stream.write_all(head.as_bytes()).is_err() {
        return;
    }
    let _ = stream.flush();
    thread::sleep(r.body_delay);
    if r.chunked {
        for chunk in r.body.chunks(4096) {
            if write!(stream, "{:x}\r\n", chunk.len())
                .and_then(|_| stream.write_all(chunk))
                .and_then(|_| stream.write_all(b"\r\n"))
                .is_err()
            {
                return;
            }
        }
        let _ = stream.write_all(b"0\r\n\r\n");
    } else {
        let _ = stream.write_all(&r.body);
    }
    let _ = stream.flush();
}
