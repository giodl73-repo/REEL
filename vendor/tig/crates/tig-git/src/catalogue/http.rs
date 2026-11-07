//! Loopback-only application host. Static UI plus authenticated read operations.
use super::*;
use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
const HTML: &str = include_str!("web/index.html");
const CSS: &str = include_str!("web/app.css");
const JS: &str = include_str!("web/app.js");
const WORKS_JS: &str = include_str!("web/works.js");
const STORAGE_JS: &str = include_str!("web/storage.js");
const STORAGE_GLOBAL_JS: &str = include_str!("web/storage-global.js");
const STORAGE_OBJECTS_JS: &str = include_str!("web/storage-objects.js");
const REVIEW_JS: &str = include_str!("web/review.js");
const BROWSE_JS: &str = include_str!("web/browse.js");
pub struct Host {
    pub catalogue: Mutex<Catalogue>,
    secret: String,
    authority: String,
}
#[derive(Debug)]
pub struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
}
fn decode(s: &str) -> Result<String> {
    let raw = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'%' => {
                if i + 2 >= raw.len() {
                    return Err("Invalid query encoding".into());
                }
                let digits = std::str::from_utf8(&raw[i + 1..i + 3])
                    .map_err(|_| "Invalid query encoding")?;
                out.push(u8::from_str_radix(digits, 16).map_err(|_| "Invalid query encoding")?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    let text = String::from_utf8(out).map_err(|_| "Invalid query text")?;
    if text.chars().any(char::is_control) {
        return Err("Invalid query text".into());
    }
    Ok(text)
}
fn query(path: &str) -> Result<BTreeMap<String, String>> {
    let mut map = BTreeMap::new();
    if let Some((_, q)) = path.split_once('?') {
        for item in q.split('&').filter(|s| !s.is_empty()) {
            let (k, v) = item.split_once('=').ok_or("Invalid query")?;
            if map.insert(decode(k)?, decode(v)?).is_some() {
                return Err("Duplicate query key".into());
            }
        }
    }
    Ok(map)
}
fn parse(raw: &str) -> Result<Request> {
    let mut lines = raw.split("\r\n");
    let first = lines.next().ok_or("Missing request")?;
    let words: Vec<_> = first.split(' ').collect();
    if words.len() != 3
        || words[2] != "HTTP/1.1"
        || !words[1].starts_with('/')
        || words[1].len()
            > if words[1].starts_with("/api/storage/batch-preview?") {
                8192
            } else {
                4096
            }
    {
        return Err("Invalid request line".into());
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (k, v) = line.split_once(':').ok_or("Invalid header")?;
        let k = k.to_ascii_lowercase();
        if k.is_empty()
            || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || headers.insert(k, v.trim().into()).is_some()
        {
            return Err("Invalid or duplicate header".into());
        }
    }
    if headers.contains_key("transfer-encoding")
        || headers.get("content-length").is_some_and(|v| v != "0")
    {
        return Err("Request bodies are not supported".into());
    }
    Ok(Request {
        method: words[0].into(),
        path: words[1].into(),
        headers,
    })
}
impl Host {
    pub fn new(catalogue: Catalogue, port: u16) -> Result<Self> {
        let mut random = [0; 32];
        getrandom::fill(&mut random).map_err(|_| "Session entropy unavailable")?;
        Ok(Self {
            catalogue: Mutex::new(catalogue),
            secret: random.iter().map(|b| format!("{b:02x}")).collect(),
            authority: format!("127.0.0.1:{port}"),
        })
    }
    fn admit(&self, r: &Request) -> Result<()> {
        if r.method != "GET" {
            return Err("Only read requests are supported".into());
        }
        if r.headers.get("host") != Some(&self.authority) {
            return Err("Host refused".into());
        }
        if r.headers
            .get("origin")
            .is_some_and(|v| v != &format!("http://{}", self.authority))
        {
            return Err("Cross-origin request refused".into());
        }
        if r.path.starts_with("/api/") {
            let token = r.headers.get("x-tig-session").ok_or("Session required")?;
            if token.len() != self.secret.len()
                || token
                    .bytes()
                    .zip(self.secret.bytes())
                    .fold(0u8, |v, (a, b)| v | (a ^ b))
                    != 0
            {
                return Err("Session refused".into());
            }
        }
        Ok(())
    }
    fn api(&self, path: &str) -> Result<Value> {
        let q = query(path)?;
        let get = |key: &str| q.get(key).map(String::as_str).unwrap_or("");
        let route = path.split('?').next().unwrap_or(path);
        if route == "/api/storage/compare" {
            let request = {
                let c = self.catalogue.lock().map_err(|_| "Catalogue unavailable")?;
                c.comparison(
                    get("project"),
                    get("snapshot"),
                    get("collection"),
                    get("work"),
                    get("from"),
                    get("to"),
                )?
            };
            let result = storage::review::execute(request)?;
            return self
                .catalogue
                .lock()
                .map_err(|_| "Catalogue unavailable")?
                .finish_comparison(result);
        }
        if route == "/api/storage/expand" {
            let request = {
                let c = self.catalogue.lock().map_err(|_| "Catalogue unavailable")?;
                c.nested_request(
                    get("project"),
                    get("snapshot"),
                    get("collection"),
                    get("work"),
                )?
            };
            let loaded = storage::nested::execute(&request)?;
            return self
                .catalogue
                .lock()
                .map_err(|_| "Catalogue unavailable")?
                .adopt_nested(&request, loaded);
        }
        if route == "/api/storage/verify-copy" {
            let (object, profile) = {
                let c = self.catalogue.lock().map_err(|_| "Catalogue unavailable")?;
                c.object_copy_request(get("project"), get("sha"), get("cache"), get("basis"))?
            };
            let result = super::verify(object, profile)?;
            return Ok(
                json!({"result":result,"cache_id":get("cache"),"independent_custody_verified":false,"removal_authorized":false,"scope":"Exact guarded read at verification time; not a durable retention acknowledgment"}),
            );
        }
        if route == "/api/verify" {
            let (object, profile) = {
                let c = self.catalogue.lock().map_err(|_| "Catalogue unavailable")?;
                let p = c.projects.get(get("project")).ok_or("Unknown project")?;
                p.check_snapshot(get("snapshot"))?;
                p.verification(get("id"), q.get("slot").map(String::as_str))?
            };
            return super::verify(object, profile);
        }
        let mut c = self.catalogue.lock().map_err(|_| "Catalogue unavailable")?;
        match route {
            "/api/storage/coverage" => c.coverage(
                get("project"),
                get("collection"),
                get("state"),
                get("q"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/projects" => Ok(c.summaries()),
            "/api/storage/batch-preview" => c.object_batch_preview(
                get("project"),
                get("selection"),
                get("cache"),
                get("action"),
            ),
            "/api/storage/objects" => c.work_object_inventory(
                &storage::objects::Query {
                    project: get("project"),
                    kind: get("kind"),
                    usage: get("usage"),
                    anomaly: get("anomaly"),
                    primary: get("primary"),
                    observed: get("cache"),
                    q: get("q"),
                    sort: if get("sort").is_empty() {
                        "bytes"
                    } else {
                        get("sort")
                    },
                    offset: if get("offset").is_empty() {
                        0
                    } else {
                        get("offset").parse().map_err(|_| "Invalid offset")?
                    },
                    sha: get("sha"),
                },
                get("collection"),
                get("work"),
            ),
            "/api/storage/preview" => {
                c.object_preview(get("project"), get("sha"), get("basis"), get("action"))
            }
            "/api/storage/scope" => {
                c.select_storage_scope(get("project"), get("snapshot"), get("family"))
            }
            "/api/storage/global" => c.global_storage(
                get("project"),
                get("domain"),
                get("measure") == "true",
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/storage/retained" => c.use_retained_inventory(get("project"), get("snapshot")),
            "/api/storage/observe" => {
                c.observe_registered(get("project"), get("snapshot"), get("family"))
            }
            "/api/storage" => c.storage(
                get("project"),
                get("snapshot"),
                &storage::Query {
                    cache: get("cache"),
                    family: get("family"),
                    owner: get("owner"),
                    version: get("version"),
                    state: get("state"),
                    pivot: if get("pivot").is_empty() {
                        "works"
                    } else {
                        get("pivot")
                    },
                    q: get("q"),
                    metric: if get("metric").is_empty() {
                        "recorded_file_bytes"
                    } else {
                        get("metric")
                    },
                    offset: if get("offset").is_empty() {
                        0
                    } else {
                        get("offset").parse().map_err(|_| "Invalid offset")?
                    },
                },
            ),
            "/api/space" => c.space(get("project")),
            "/api/search" => c.search(
                get("project"),
                get("kind"),
                get("q"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/children" => c.children(
                (get("project"), get("collection")),
                get("id"),
                get("snapshot"),
                get("kind"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/works" => c.work_list(
                (get("project"), get("collection")),
                get("state"),
                get("facet"),
                get("q"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/work" => c.work_detail(
                (get("project"), get("collection")),
                get("id"),
                get("snapshot"),
                get("category"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
            ),
            "/api/assets" => c.list(
                if get("project").is_empty() {
                    "all"
                } else {
                    get("project")
                },
                get("category"),
                get("q"),
                if get("offset").is_empty() {
                    0
                } else {
                    get("offset").parse().map_err(|_| "Invalid offset")?
                },
                100,
            ),
            "/api/asset" | "/api/dependencies" => {
                let p = c
                    .projects
                    .get_mut(get("project"))
                    .ok_or("Unknown project")?;
                p.check_snapshot(get("snapshot"))?;
                if route == "/api/asset" {
                    p.inspect(get("id"))
                } else {
                    p.dependencies(get("id"))
                }
            }
            "/api/snapshot" => {
                let p = c
                    .projects
                    .get_mut(get("project"))
                    .ok_or("Unknown project")?;
                p.change_snapshot(get("commit"))?;
                Ok(p.summary())
            }
            _ => Err("Unknown read operation".into()),
        }
    }
    fn response(&self, r: &Request) -> (u16, &'static str, Vec<u8>) {
        if let Err(e) = self.admit(r) {
            return (
                403,
                "application/json",
                json!({"error":e}).to_string().into_bytes(),
            );
        }
        match r.path.as_str() {
            "/" => (
                200,
                "text/html; charset=utf-8",
                HTML.replace("__TIG_SESSION__", &self.secret).into_bytes(),
            ),
            "/app.css" => (200, "text/css; charset=utf-8", CSS.as_bytes().to_vec()),
            "/storage-objects.js" => (
                200,
                "text/javascript; charset=utf-8",
                STORAGE_OBJECTS_JS.as_bytes().to_vec(),
            ),
            "/storage-global.js" => (
                200,
                "text/javascript; charset=utf-8",
                STORAGE_GLOBAL_JS.as_bytes().to_vec(),
            ),
            "/storage.js" => (
                200,
                "text/javascript; charset=utf-8",
                STORAGE_JS.as_bytes().to_vec(),
            ),
            "/works.js" => (
                200,
                "text/javascript; charset=utf-8",
                WORKS_JS.as_bytes().to_vec(),
            ),
            "/browse.js" => (
                200,
                "text/javascript; charset=utf-8",
                BROWSE_JS.as_bytes().to_vec(),
            ),
            "/app.js" => (
                200,
                "text/javascript; charset=utf-8",
                JS.as_bytes().to_vec(),
            ),
            "/review.js" => (
                200,
                "text/javascript; charset=utf-8",
                REVIEW_JS.as_bytes().to_vec(),
            ),
            _ => match self.api(&r.path) {
                Ok(v) => match serde_json::to_vec(&v) {
                    Ok(raw) if raw.len() <= 4 * 1024 * 1024 => (200, "application/json", raw),
                    _ => (
                        413,
                        "application/json",
                        b"{\"error\":\"Response budget exceeded\"}".to_vec(),
                    ),
                },
                Err(e) => (
                    400,
                    "application/json",
                    json!({"error":e}).to_string().into_bytes(),
                ),
            },
        }
    }
}
fn connection(mut stream: TcpStream, host: &Host) -> std::io::Result<()> {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    stream.set_write_timeout(Some(Duration::from_secs(15)))?;
    let mut raw = Vec::new();
    let mut byte = [0; 1];
    while raw.len() < 16384 {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        stream.set_read_timeout(Some(remaining))?;
        if stream.read(&mut byte)? == 0 {
            return Ok(());
        }
        raw.push(byte[0]);
        if raw.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    let (status, mime, body) = if raw.ends_with(b"\r\n\r\n") {
        match std::str::from_utf8(&raw).ok().and_then(|s| parse(s).ok()) {
            Some(r) => host.response(&r),
            None => (400, "text/plain", b"Invalid bounded HTTP request".to_vec()),
        }
    } else {
        (431, "text/plain", b"Header budget exceeded".to_vec())
    };
    write!(
        stream,
        "HTTP/1.1 {status} Response\r\nContent-Type: {mime}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' blob:; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n\r\n",
        body.len()
    )?;
    stream.write_all(&body)
}
pub fn serve(catalogue: Catalogue, port: u16) -> Result<()> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .map_err(|_| "Cannot bind requested loopback port")?;
    let port = listener
        .local_addr()
        .map_err(|_| "Cannot inspect local listener")?
        .port();
    let host = Arc::new(Host::new(catalogue, port)?);
    let active = Arc::new(AtomicUsize::new(0));
    println!("Tig catalogue: http://127.0.0.1:{port}/");
    std::io::stdout()
        .flush()
        .map_err(|_| "Cannot report local URL")?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if active.fetch_add(1, Ordering::SeqCst) >= 8 {
            active.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let host = host.clone();
        let active = active.clone();
        std::thread::spawn(move || {
            let _ = connection(stream, &host);
            active.fetch_sub(1, Ordering::SeqCst);
        });
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hostile_http_never_reaches_the_application() {
        let selection = "a".repeat(6800);
        assert!(
            parse(&format!(
                "GET /api/storage/batch-preview?selection={selection} HTTP/1.1\r\n\r\n"
            ))
            .is_ok()
        );
        assert!(parse(&format!("GET /api/assets?q={selection} HTTP/1.1\r\n\r\n")).is_err());
        assert!(
            parse(&format!(
                "GET /api/storage/batch-preview?selection={} HTTP/1.1\r\n\r\n",
                "a".repeat(8192)
            ))
            .is_err()
        );
        assert!(parse("GET / HTTP/1.1\r\nHost: a\r\nHost: b\r\n\r\n").is_err());
        assert!(parse("GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n").is_err());
        assert!(query("/api/assets?q=%00").is_err());
        assert!(query("/api/assets?q=a&q=b").is_err());
        let host = Host::new(
            Catalogue {
                projects: BTreeMap::new(),
                storage_domains: vec![],
                default_project: None,
            },
            8799,
        )
        .unwrap();
        for request in [
            "GET /api/projects HTTP/1.1\r\nHost: 127.0.0.1:8799\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: evil.invalid\r\n\r\n",
            "POST / HTTP/1.1\r\nHost: 127.0.0.1:8799\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: 127.0.0.1:8799\r\nOrigin: https://foreign.invalid\r\n\r\n",
            "GET /api/projects HTTP/1.1\r\nHost: 127.0.0.1:8799\r\nX-Tig-Session: wrong\r\n\r\n",
        ] {
            assert_eq!(host.response(&parse(request).unwrap()).0, 403);
        }
        let r = parse(&format!(
            "GET /api/projects HTTP/1.1\r\nHost: 127.0.0.1:8799\r\nX-Tig-Session: {}\r\n\r\n",
            host.secret
        ))
        .unwrap();
        assert_eq!(host.response(&r).0, 200);
    }
}
