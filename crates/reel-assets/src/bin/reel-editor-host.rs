//! Explicit, short-lived loopback hosting for a single native editor export.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    io::{Read, Seek, SeekFrom, Write},
    net::{Ipv4Addr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
const CONTROL_LIMIT: u64 = 2 * 1024 * 1024;
const MEDIA_LIMIT: u64 = 1024 * 1024 * 1024;
const HEADER_LIMIT: usize = 8192;

struct Asset {
    body: Body,
    bytes: u64,
    sha256: String,
    mime: &'static str,
}
enum Body {
    Frozen(Vec<u8>),
    Media(Mutex<fs::File>, Option<SystemTime>),
}
struct Bundle {
    assets: BTreeMap<String, Asset>,
    schema: String,
}
fn regular(path: &Path) -> Result<()> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err("Editor resources must be regular files".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err("Editor resources cannot be reparse points".into());
        }
    }
    Ok(())
}
fn control(root: &Path, name: &str, mime: &'static str) -> Result<Asset> {
    let path = root.join(name);
    regular(&path)?;
    let mut data = vec![];
    fs::File::open(path)?
        .take(CONTROL_LIMIT + 1)
        .read_to_end(&mut data)?;
    if data.is_empty() || data.len() as u64 > CONTROL_LIMIT {
        return Err("Editor control resource exceeds its budget or is empty".into());
    }
    Ok(Asset {
        bytes: data.len() as u64,
        sha256: format!("{:x}", Sha256::digest(&data)),
        body: Body::Frozen(data),
        mime,
    })
}
fn media(
    root: &Path,
    name: &str,
    expected: Option<&str>,
    mime: &'static str,
    budget: u64,
    deadline: Option<Instant>,
) -> Result<Asset> {
    let path = root.join(name);
    regular(&path)?;
    let mut file = fs::File::open(path)?;
    let m = file.metadata()?;
    if m.len() > budget.min(MEDIA_LIMIT) {
        return Err("Editor media exceeds its budget".into());
    }
    let mut digest = Sha256::new();
    let mut copied = 0u64;
    let mut buf = [0; 65536];
    let mut bounded = (&mut file).take(m.len() + 1);
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err("Bundle verification exhausted the time budget".into());
        }
        let n = bounded.read(&mut buf)?;
        if n == 0 {
            break;
        }
        copied += n as u64;
        digest.update(&buf[..n]);
    }
    let sha256 = format!("{:x}", digest.finalize());
    if copied != m.len() || expected.is_some_and(|h| h != sha256) {
        return Err("Editor media identity changed or does not match its name".into());
    }
    file.seek(SeekFrom::Start(0))?;
    Ok(Asset {
        body: Body::Media(Mutex::new(file), m.modified().ok()),
        bytes: copied,
        sha256,
        mime,
    })
}
impl Bundle {
    #[cfg(test)]
    fn open(root: &Path) -> Result<Self> {
        Self::open_until(root, None)
    }
    fn open_until(root: &Path, deadline: Option<Instant>) -> Result<Self> {
        let root = root.canonicalize()?;
        let mut assets = BTreeMap::new();
        for (name, mime) in [
            ("index.html", "text/html; charset=utf-8"),
            ("editor.js", "application/javascript; charset=utf-8"),
            ("session.json", "application/json; charset=utf-8"),
        ] {
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return Err("Bundle verification exhausted the time budget".into());
            }
            assets.insert(name.into(), control(&root, name, mime)?);
        }
        let Body::Frozen(bytes) = &assets["session.json"].body else {
            unreachable!()
        };
        let session: Value = serde_json::from_slice(bytes)?;
        let schema = session["schema"]
            .as_str()
            .ok_or("Missing native editor session schema")?
            .to_owned();
        let (html, script): (&[u8], &[u8]) = match schema.as_str() {
            "reel.music-note-session.v1" => (
                include_bytes!("../../../reel-music/src/note_editor.html"),
                include_bytes!("../../../reel-music/src/note_editor.js"),
            ),
            "reel.music-underlay-session.v1" => (
                include_bytes!("../../../reel-music/src/underlay_editor.html"),
                include_bytes!("../../../reel-music/src/underlay_editor.js"),
            ),
            "reel.cel-sorter-session.v1"
            | "reel.cel-sorter-session.v2"
            | "reel.cel-sorter-session.v3"
            | "reel.cel-sorter-session.v4" => (
                include_bytes!("../cel_sorter.html"),
                include_bytes!("../cel_sorter.js"),
            ),
            _ => return Err("Unsupported native editor session".into()),
        };
        for (name, expected) in [("index.html", html), ("editor.js", script)] {
            let Body::Frozen(actual) = &assets[name].body else {
                unreachable!()
            };
            let normalize = |bytes: &[u8]| -> Result<String> {
                Ok(std::str::from_utf8(bytes)?.replace("\r\n", "\n"))
            };
            if normalize(actual)? != normalize(expected)? {
                return Err("Frontend differs from this launcher version; re-export with its matching native editor".into());
            }
        }
        match schema.as_str() {
            "reel.music-note-session.v1" | "reel.music-underlay-session.v1" => (),
            "reel.cel-sorter-session.v1"
            | "reel.cel-sorter-session.v2"
            | "reel.cel-sorter-session.v3"
            | "reel.cel-sorter-session.v4" => {
                let images = session["images"]
                    .as_object()
                    .ok_or("Missing native image map")?;
                if session["media_mode"] == "controls-only" {
                    if !images.is_empty() || !session["movie"].is_null() {
                        return Err("Controls-only session declares media".into());
                    }
                } else {
                    if !session["media_mode"].is_null() || session["movie"] != "performance.mp4" {
                        return Err("Unsupported cel media route".into());
                    }
                    let folder = root.join("media");
                    if !images.is_empty() {
                        let m = fs::symlink_metadata(&folder)?;
                        if !m.is_dir() || m.file_type().is_symlink() {
                            return Err("Media directory must be local and regular".into());
                        }
                        #[cfg(windows)]
                        {
                            use std::os::windows::fs::MetadataExt;
                            if m.file_attributes() & 0x400 != 0 {
                                return Err("Media directory cannot be a reparse point".into());
                            }
                        }
                    }
                    let mut total = 0u64;
                    for route in images.values() {
                        let name = route.as_str().ok_or("Invalid image route")?;
                        let hash = name
                            .strip_prefix("media/")
                            .and_then(|n| n.strip_suffix(".png"))
                            .ok_or("Unsupported image route")?;
                        if hash.len() != 64
                            || !hash
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        {
                            return Err("Image route requires a SHA-256 filename".into());
                        }
                        if !assets.contains_key(name) {
                            if assets.len() >= 515 {
                                return Err("Editor resource count exceeds budget".into());
                            }
                            let resource = media(
                                &root,
                                name,
                                Some(hash),
                                "image/png",
                                MEDIA_LIMIT - total,
                                deadline,
                            )?;
                            total = total
                                .checked_add(resource.bytes)
                                .ok_or("Media budget overflow")?;
                            if total > MEDIA_LIMIT {
                                return Err("Editor media set exceeds budget".into());
                            }
                            assets.insert(name.into(), resource);
                        }
                    }
                    let movie = media(
                        &root,
                        "performance.mp4",
                        None,
                        "video/mp4",
                        MEDIA_LIMIT - total,
                        deadline,
                    )?;
                    if total
                        .checked_add(movie.bytes)
                        .is_none_or(|n| n > MEDIA_LIMIT)
                    {
                        return Err("Editor media set exceeds budget".into());
                    }
                    assets.insert("performance.mp4".into(), movie);
                }
            }
            _ => return Err("Unsupported native editor session".into()),
        }
        Ok(Self { assets, schema })
    }
}

#[derive(Debug)]
struct Request {
    head: bool,
    path: String,
    range: Option<String>,
}
fn request(raw: &[u8], host: &str, token: &str) -> std::result::Result<Request, u16> {
    let text = std::str::from_utf8(raw).map_err(|_| 400u16)?;
    let (header, body) = text.split_once("\r\n\r\n").ok_or(400u16)?;
    if !body.is_empty() {
        return Err(400);
    }
    let mut lines = header.split("\r\n");
    let line: Vec<_> = lines.next().ok_or(400u16)?.split(' ').collect();
    if line.len() != 3 || !["HTTP/1.1", "HTTP/1.0"].contains(&line[2]) {
        return Err(400);
    }
    if !["GET", "HEAD"].contains(&line[0]) {
        return Err(405);
    }
    let mut headers = BTreeMap::new();
    for (i, line) in lines.enumerate() {
        if i >= 64 {
            return Err(400);
        }
        let (key, value) = line.split_once(':').ok_or(400u16)?;
        if key.is_empty()
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.bytes().any(|b| b < 32 && b != b'\t' || b == 127)
        {
            return Err(400);
        }
        let key = key.to_ascii_lowercase();
        if headers.insert(key.clone(), value.trim()).is_some()
            && [
                "host",
                "origin",
                "range",
                "content-length",
                "transfer-encoding",
            ]
            .contains(&key.as_str())
        {
            return Err(400);
        }
    }
    if headers.get("host") != Some(&host) {
        return Err(403);
    }
    let origin = format!("http://{host}");
    if headers.get("origin").is_some_and(|v| *v != origin) {
        return Err(403);
    }
    if headers.contains_key("transfer-encoding")
        || headers.get("content-length").is_some_and(|v| *v != "0")
    {
        return Err(400);
    }
    let path = line[1].strip_prefix(&format!("/{token}/")).ok_or(404u16)?;
    if path.is_empty()
        || path.contains(['%', '?', '#', '\\'])
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(404);
    }
    Ok(Request {
        head: line[0] == "HEAD",
        path: path.into(),
        range: headers.get("range").map(|s| s.to_string()),
    })
}
fn byte_range(value: Option<&str>, size: u64) -> std::result::Result<(u64, u64, bool), u16> {
    let Some(value) = value else {
        return Ok((0, size, false));
    };
    let value = value.strip_prefix("bytes=").ok_or(416u16)?;
    let (a, b) = value.split_once('-').ok_or(416u16)?;
    if !a.bytes().all(|c| c.is_ascii_digit()) || !b.bytes().all(|c| c.is_ascii_digit()) {
        return Err(416);
    }
    if size == 0 {
        return Err(416);
    }
    let (start, end) = if a.is_empty() {
        let suffix: u64 = b.parse().map_err(|_| 416u16)?;
        if suffix == 0 {
            return Err(416);
        }
        (size.saturating_sub(suffix), size - 1)
    } else {
        let start: u64 = a.parse().map_err(|_| 416u16)?;
        let end = if b.is_empty() {
            size - 1
        } else {
            b.parse::<u64>().map_err(|_| 416u16)?.min(size - 1)
        };
        if start >= size || end < start {
            return Err(416);
        }
        (start, end)
    };
    Ok((start, end - start + 1, true))
}
fn response(
    stream: &mut TcpStream,
    status: u16,
    mime: &str,
    bytes: u64,
    extra: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        206 => "Partial Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        416 => "Range Not Satisfiable",
        _ => "Unavailable",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {bytes}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n{extra}\r\n"
    )
}
fn handle(
    stream: &mut TcpStream,
    bundle: &Bundle,
    host: &str,
    token: &str,
    deadline: Instant,
) -> Result<bool> {
    // Accepted sockets can inherit the listener's nonblocking mode on Windows.
    stream.set_nonblocking(false)?;
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(5));
    if timeout.is_zero() {
        return Ok(false);
    }
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    let mut raw = vec![];
    let mut buf = [0; 1024];
    let result = loop {
        if raw.len() >= HEADER_LIMIT || Instant::now() >= deadline {
            break Err(400);
        }
        let n = match stream.read(&mut buf) {
            Ok(0) | Err(_) => break Err(400),
            Ok(n) => n,
        };
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > HEADER_LIMIT {
            break Err(400);
        }
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            break request(&raw, host, token);
        }
    };
    let req = match result {
        Ok(req) => req,
        Err(status) => {
            response(stream, status, "text/plain", 0, "")?;
            return Ok(false);
        }
    };
    let Some(asset) = bundle.assets.get(&req.path) else {
        response(stream, 404, "text/plain", 0, "")?;
        return Ok(false);
    };
    // Only requests for this same media handle serialize its seek position.
    // Frozen controls and other assets remain independent of the stream.
    let mut media_file = match &asset.body {
        Body::Media(file, _) => Some(file.lock().map_err(|_| "Media reader failed")?),
        Body::Frozen(_) => None,
    };
    if Instant::now() >= deadline {
        return Ok(false);
    }
    if let Body::Media(_, modified) = &asset.body {
        let file = media_file.as_ref().ok_or("Missing media handle")?;
        let m = file.metadata()?;
        if m.len() != asset.bytes || m.modified().ok() != *modified {
            response(stream, 409, "text/plain", 0, "")?;
            return Ok(false);
        }
    }
    let (start, length, partial) = match byte_range(req.range.as_deref(), asset.bytes) {
        Ok(range) => range,
        Err(status) => {
            response(
                stream,
                status,
                "text/plain",
                0,
                &format!("Content-Range: bytes */{}\r\n", asset.bytes),
            )?;
            return Ok(false);
        }
    };
    let extra = if partial {
        format!(
            "Accept-Ranges: bytes\r\nContent-Range: bytes {}-{}/{}\r\n",
            start,
            start + length - 1,
            asset.bytes
        )
    } else {
        "Accept-Ranges: bytes\r\n".into()
    };
    response(
        stream,
        if partial { 206 } else { 200 },
        asset.mime,
        length,
        &extra,
    )?;
    if !req.head {
        match &asset.body {
            Body::Frozen(data) => {
                stream.write_all(&data[start as usize..(start + length) as usize])?
            }
            Body::Media(_, _) => {
                let file = media_file.as_mut().ok_or("Missing media handle")?;
                file.seek(SeekFrom::Start(start))?;
                let mut remaining = length;
                let mut buf = [0; 65536];
                while remaining > 0 {
                    if Instant::now() >= deadline {
                        return Ok(false);
                    }
                    let n = file.read(&mut buf[..remaining.min(65536) as usize])?;
                    if n == 0 {
                        return Err("Media changed during streaming".into());
                    }
                    stream.write_all(&buf[..n])?;
                    remaining -= n as u64;
                }
            }
        }
    }
    Ok(true)
}
struct Options {
    root: PathBuf,
    port: u16,
    idle: u64,
    max: u64,
    open: bool,
    report: Option<PathBuf>,
}
fn options(args: &[String]) -> Result<Options> {
    if args.len() < 2 || args[0] != "serve" {
        return Err("usage: reel-editor-host serve <export-directory> [--open] [--port 0] [--idle-seconds 900] [--max-seconds 1800] [--report <new-private-report.json>]".into());
    }
    let mut result = Options {
        root: PathBuf::from(&args[1]),
        port: 0,
        idle: 900,
        max: 1800,
        open: false,
        report: None,
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut i = 2;
    while i < args.len() {
        if !seen.insert(args[i].as_str()) {
            return Err("Duplicate launcher option".into());
        }
        if args[i] == "--open" {
            result.open = true;
            i += 1;
            continue;
        }
        let value = args.get(i + 1).ok_or("Missing launcher option value")?;
        match args[i].as_str() {
            "--port" => result.port = value.parse()?,
            "--idle-seconds" => result.idle = value.parse()?,
            "--max-seconds" => result.max = value.parse()?,
            "--report" => result.report = Some(value.into()),
            _ => return Err("Unknown launcher option".into()),
        }
        i += 2;
    }
    if !(1..=3600).contains(&result.idle) || !(1..=3600).contains(&result.max) {
        return Err("Launcher time limits must be between 1 and 3600 seconds".into());
    }
    Ok(result)
}
fn browser_command(url: &str) -> Result<Command> {
    #[cfg(windows)]
    let mut command = {
        let system = PathBuf::from(
            env::var_os("SystemRoot").ok_or("Windows system directory is unavailable")?,
        );
        if !system.is_absolute() {
            return Err("Windows system directory must be absolute".into());
        }
        let mut c = Command::new(system.join("System32").join("rundll32.exe"));
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("/usr/bin/open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    command.arg(url);
    Ok(command)
}
fn open_browser(url: &str) -> Result<()> {
    let mut child = browser_command(url)?
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
fn run(args: &[String]) -> Result<()> {
    let opts = options(args)?;
    let start = Instant::now();
    let deadline = start + Duration::from_secs(opts.max);
    let bundle = Arc::new(Bundle::open_until(&opts.root, Some(deadline))?);
    if Instant::now() >= deadline {
        return Err("Bundle verification exhausted the time budget".into());
    }
    let report = opts
        .report
        .as_ref()
        .map(|p| fs::OpenOptions::new().write(true).create_new(true).open(p))
        .transpose()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, opts.port))?;
    listener.set_nonblocking(true)?;
    let host = listener.local_addr()?.to_string();
    let mut entropy = [0u8; 16];
    getrandom::fill(&mut entropy).map_err(|_| "OS randomness is unavailable")?;
    let token: String = entropy.iter().map(|b| format!("{b:02x}")).collect();
    let url = format!("http://{host}/{token}/index.html");
    println!(
        "{}",
        json!({"schema":"reel.editor-host-start.v1","url":url,"resources":bundle.assets.len(),"idle_seconds":opts.idle,"max_seconds":opts.max,"read_only":true})
    );
    std::io::stdout().flush()?;
    let browser_launch_started = opts.open
        && match open_browser(&url) {
            Ok(()) => true,
            Err(_) => {
                eprintln!("Default browser could not be opened; use the printed loopback URL.");
                false
            }
        };
    let activity_epoch = Instant::now();
    let last = Arc::new(AtomicU64::new(0));
    let served = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let mut workers: Vec<thread::JoinHandle<()>> = vec![];
    loop {
        let mut i = 0;
        while i < workers.len() {
            if workers[i].is_finished() {
                workers
                    .swap_remove(i)
                    .join()
                    .map_err(|_| "Editor reader failed")?;
            } else {
                i += 1;
            }
        }
        let idle_deadline = activity_epoch
            + Duration::from_millis(last.load(Ordering::Relaxed))
            + Duration::from_secs(opts.idle);
        if Instant::now() >= deadline || Instant::now() >= idle_deadline {
            break;
        }
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if !peer.ip().is_loopback() {
                    rejected.fetch_add(1, Ordering::Relaxed);
                    continue;
                }
                if workers.len() >= 16 {
                    rejected.fetch_add(1, Ordering::Relaxed);
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
                    let _ = response(&mut stream, 503, "text/plain", 0, "");
                    continue;
                }
                let bundle = Arc::clone(&bundle);
                let host = host.clone();
                let token = token.clone();
                let last = Arc::clone(&last);
                let served = Arc::clone(&served);
                let rejected = Arc::clone(&rejected);
                let request_deadline = deadline.min(idle_deadline);
                workers.push(
                    thread::Builder::new()
                        .name("reel-editor-reader".into())
                        .spawn(move || {
                            if matches!(
                                handle(&mut stream, &bundle, &host, &token, request_deadline),
                                Ok(true)
                            ) {
                                served.fetch_add(1, Ordering::Relaxed);
                                last.fetch_max(
                                    activity_epoch.elapsed().as_millis() as u64,
                                    Ordering::Relaxed,
                                );
                            } else {
                                rejected.fetch_add(1, Ordering::Relaxed);
                            }
                        })?,
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
    drop(listener);
    for worker in workers {
        worker.join().map_err(|_| "Editor reader failed")?;
    }
    let served = served.load(Ordering::Relaxed);
    let rejected = rejected.load(Ordering::Relaxed);
    let reason = if Instant::now() >= deadline {
        "max-seconds"
    } else {
        "idle-seconds"
    };
    let summary = json!({"schema":"reel.editor-host-stop.v1","served_requests":served,"rejected_requests":rejected,"reason":reason,"read_only":true,"schema_hosted":bundle.schema});
    if let Some(mut file) = report {
        let resources: BTreeMap<_, _> = bundle
            .assets
            .iter()
            .map(|(k, a)| (k, json!({"sha256":a.sha256,"bytes":a.bytes})))
            .collect();
        serde_json::to_writer(
            &mut file,
            &json!({"schema":"reel.editor-host-private-report.v1","summary":summary,"resources":resources,"browser_open_requested":opts.open,"browser_launch_process_started":browser_launch_started}),
        )?;
        file.sync_all()?;
    }
    println!("{summary}");
    Ok(())
}
fn main() {
    if let Err(error) = run(&env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("reel-editor-host: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(schema: &str) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let (html, js): (&[u8], &[u8]) = match schema {
            "reel.music-note-session.v1" => (
                include_bytes!("../../../reel-music/src/note_editor.html"),
                include_bytes!("../../../reel-music/src/note_editor.js"),
            ),
            "reel.music-underlay-session.v1" => (
                include_bytes!("../../../reel-music/src/underlay_editor.html"),
                include_bytes!("../../../reel-music/src/underlay_editor.js"),
            ),
            _ => (
                include_bytes!("../cel_sorter.html"),
                include_bytes!("../cel_sorter.js"),
            ),
        };
        fs::write(d.path().join("index.html"), html).unwrap();
        fs::write(d.path().join("editor.js"), js).unwrap();
        fs::write(
            d.path().join("session.json"),
            serde_json::to_vec(
                &json!({"schema":schema,"media_mode":"controls-only","images":{},"movie":null}),
            )
            .unwrap(),
        )
        .unwrap();
        d
    }
    fn req(method: &str, path: &str, extra: &str) -> Vec<u8> {
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:1234\r\n{extra}\r\n").into_bytes()
    }
    #[test]
    fn routes_require_capability_host_origin_and_read_only_methods() {
        for method in ["GET", "HEAD"] {
            let r = request(
                &req(
                    method,
                    "/secret/session.json",
                    "Origin: http://127.0.0.1:1234\r\n",
                ),
                "127.0.0.1:1234",
                "secret",
            )
            .unwrap();
            assert_eq!(r.head, method == "HEAD");
            assert_eq!(r.path, "session.json");
        }
        for path in [
            "/session.json",
            "/wrong/session.json",
            "/secret/../session.json",
            "/secret/%2e%2e/session.json",
            "/secret/",
            "/secret//session.json",
            "/secret/..\\session.json",
            "/secret/session.json?x=1",
            "/secret/session.json#x",
        ] {
            assert_eq!(
                request(&req("GET", path, ""), "127.0.0.1:1234", "secret").unwrap_err(),
                404
            );
        }
        for method in ["POST", "PUT", "DELETE", "OPTIONS", "CONNECT"] {
            assert_eq!(
                request(
                    &req(method, "/secret/session.json", ""),
                    "127.0.0.1:1234",
                    "secret"
                )
                .unwrap_err(),
                405
            );
        }
        assert_eq!(
            request(
                &req(
                    "GET",
                    "/secret/session.json",
                    "Origin: https://outside.invalid\r\n"
                ),
                "127.0.0.1:1234",
                "secret"
            )
            .unwrap_err(),
            403
        );
        assert_eq!(
            request(
                &req("GET", "/secret/session.json", ""),
                "rebound.invalid:1234",
                "secret"
            )
            .unwrap_err(),
            403
        );
        for extra in [
            "Host: 127.0.0.1:1234\r\n",
            "Range: bytes=1-2\r\nRange: bytes=2-3\r\n",
            "Content-Length: 1\r\n",
            "Transfer-Encoding: chunked\r\n",
            "Bad header: value\r\n",
        ] {
            assert_eq!(
                request(
                    &req("GET", "/secret/session.json", extra),
                    "127.0.0.1:1234",
                    "secret"
                )
                .unwrap_err(),
                400
            );
        }
        let mut pipelined = req("GET", "/secret/session.json", "");
        pipelined.extend(req("GET", "/secret/session.json", ""));
        assert_eq!(
            request(&pipelined, "127.0.0.1:1234", "secret").unwrap_err(),
            400
        );
    }
    #[test]
    fn ranges_are_exact_bounded_and_reject_multiple_or_unsatisfiable_requests() {
        for (value, expected) in [
            (None, (0, 10, false)),
            (Some("bytes=2-5"), (2, 4, true)),
            (Some("bytes=8-"), (8, 2, true)),
            (Some("bytes=-3"), (7, 3, true)),
            (Some("bytes=0-999"), (0, 10, true)),
            (Some("bytes=-99"), (0, 10, true)),
        ] {
            assert_eq!(byte_range(value, 10).unwrap(), expected);
        }
        for value in [
            "bytes=10-",
            "bytes=5-2",
            "bytes=-0",
            "bytes=1-2,4-5",
            "bytes=abc-4",
            "items=1-2",
            "bytes=18446744073709551616-",
            "bytes=0--1",
        ] {
            assert!(byte_range(Some(value), 10).is_err());
        }
        assert!(byte_range(Some("bytes=0-0"), 0).is_err());
        assert_eq!(byte_range(None, 0).unwrap(), (0, 0, false));
    }
    #[test]
    fn only_known_native_frontends_and_explicit_resources_are_hosted() {
        for schema in [
            "reel.cel-sorter-session.v1",
            "reel.cel-sorter-session.v2",
            "reel.cel-sorter-session.v3",
            "reel.cel-sorter-session.v4",
            "reel.music-note-session.v1",
            "reel.music-underlay-session.v1",
        ] {
            let d = fixture(schema);
            fs::write(d.path().join("private.txt"), b"not served").unwrap();
            let bundle = Bundle::open(d.path()).unwrap();
            assert_eq!(bundle.assets.len(), 3);
            assert!(!bundle.assets.contains_key("private.txt"));
        }
        let d = fixture("unknown");
        assert!(Bundle::open(d.path()).is_err());
        let d = fixture("reel.music-note-session.v1");
        fs::write(d.path().join("editor.js"), b"unrecognized script").unwrap();
        assert!(Bundle::open(d.path()).is_err());
        let d = fixture("reel.music-note-session.v1");
        fs::write(d.path().join("index.html"), b"unrecognized HTML").unwrap();
        assert!(Bundle::open(d.path()).is_err());
        let d = fixture("reel.music-note-session.v1");
        fs::write(
            d.path().join("session.json"),
            vec![b' '; CONTROL_LIMIT as usize + 1],
        )
        .unwrap();
        assert!(Bundle::open(d.path()).is_err());
    }
    #[test]
    fn media_is_declared_hashed_and_kept_behind_exact_routes() {
        let d = fixture("reel.cel-sorter-session.v4");
        fs::create_dir(d.path().join("media")).unwrap();
        let hash = format!("{:x}", Sha256::digest(b"synthetic picture bytes"));
        let name = format!("media/{hash}.png");
        fs::write(d.path().join(&name), b"synthetic picture bytes").unwrap();
        fs::write(d.path().join("performance.mp4"), b"synthetic movie bytes").unwrap();
        let session = json!({"schema":"reel.cel-sorter-session.v4","images":{"one":name,"same":name},"movie":"performance.mp4"});
        fs::write(
            d.path().join("session.json"),
            serde_json::to_vec(&session).unwrap(),
        )
        .unwrap();
        assert_eq!(Bundle::open(d.path()).unwrap().assets.len(), 5);
        fs::write(d.path().join(&name), b"tampered").unwrap();
        assert!(Bundle::open(d.path()).is_err());
        for route in [
            "../private.txt",
            "media/not-a-hash.png",
            "file:///private.txt",
        ] {
            let mut bad = session.clone();
            bad["images"] = json!({"one":route});
            fs::write(
                d.path().join("session.json"),
                serde_json::to_vec(&bad).unwrap(),
            )
            .unwrap();
            assert!(Bundle::open(d.path()).is_err());
        }
        let mut bad = session.clone();
        bad["media_mode"] = json!("controls-only");
        fs::write(
            d.path().join("session.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
        assert!(Bundle::open(d.path()).is_err());
    }
    fn exchange(bundle: &Bundle, method: &str, path: &str, extra: &str) -> Vec<u8> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let raw = format!("{method} /secret/{path} HTTP/1.1\r\nHost: {address}\r\n{extra}\r\n");
        let client = thread::spawn(move || {
            let mut s = TcpStream::connect(address).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            s.write_all(raw.as_bytes()).unwrap();
            let mut result = vec![];
            s.read_to_end(&mut result).unwrap();
            result
        });
        let (mut stream, _) = listener.accept().unwrap();
        handle(
            &mut stream,
            bundle,
            &address.to_string(),
            "secret",
            Instant::now() + Duration::from_secs(3),
        )
        .unwrap();
        drop(stream);
        client.join().unwrap()
    }
    #[test]
    fn socket_serving_freezes_controls_and_supports_head_ranges_and_missing_routes() {
        let d = fixture("reel.music-note-session.v1");
        let bundle = Bundle::open(d.path()).unwrap();
        let expected = fs::read(d.path().join("session.json")).unwrap();
        fs::write(d.path().join("session.json"), b"later change").unwrap();
        let full = exchange(&bundle, "GET", "session.json", "");
        assert!(full.ends_with(&expected));
        assert!(String::from_utf8_lossy(&full).contains("Cache-Control: no-store"));
        let head = exchange(&bundle, "HEAD", "session.json", "");
        assert!(head.ends_with(b"\r\n\r\n"));
        assert!(!head.ends_with(&expected));
        let range = exchange(&bundle, "GET", "session.json", "Range: bytes=0-3\r\n");
        assert!(range.starts_with(b"HTTP/1.1 206"));
        assert!(range.ends_with(&expected[..4]));
        assert!(exchange(&bundle, "GET", "private.txt", "").starts_with(b"HTTP/1.1 404"));
        assert!(
            exchange(&bundle, "GET", "session.json", "Range: bytes=999999-\r\n")
                .starts_with(b"HTTP/1.1 416")
        );
    }
    #[test]
    fn accepted_nonblocking_socket_waits_for_split_headers() {
        let d = fixture("reel.music-note-session.v1");
        let bundle = Bundle::open(d.path()).unwrap();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let raw =
            format!("GET /secret/session.json HTTP/1.1\r\nHost: {address}\r\n\r\n").into_bytes();
        let (tx, rx) = std::sync::mpsc::channel();
        let client = thread::spawn(move || {
            let mut s = TcpStream::connect(address).unwrap();
            s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let split = raw.len() - 2;
            s.write_all(&raw[..split]).unwrap();
            tx.send(()).unwrap();
            thread::sleep(Duration::from_millis(100));
            let _ = s.write_all(&raw[split..]);
            let mut result = vec![];
            let _ = s.read_to_end(&mut result);
            result
        });
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (mut stream, _) = listener.accept().unwrap();
        stream.set_nonblocking(true).unwrap();
        assert!(
            handle(
                &mut stream,
                &bundle,
                &address.to_string(),
                "secret",
                Instant::now() + Duration::from_secs(3)
            )
            .unwrap()
        );
        drop(stream);
        assert!(client.join().unwrap().starts_with(b"HTTP/1.1 200"));
    }
    #[test]
    fn changed_media_and_exhausted_verification_budgets_refuse() {
        let d = fixture("reel.cel-sorter-session.v4");
        fs::write(d.path().join("session.json"),serde_json::to_vec(&json!({"schema":"reel.cel-sorter-session.v4","images":{},"movie":"performance.mp4"})).unwrap()).unwrap();
        fs::write(d.path().join("performance.mp4"), b"synthetic movie").unwrap();
        assert!(media(d.path(), "performance.mp4", None, "video/mp4", 1, None).is_err());
        assert!(Bundle::open_until(d.path(), Some(Instant::now())).is_err());
        let bundle = Bundle::open(d.path()).unwrap();
        fs::write(
            d.path().join("performance.mp4"),
            b"changed synthetic movie bytes",
        )
        .unwrap();
        assert!(exchange(&bundle, "GET", "performance.mp4", "").starts_with(b"HTTP/1.1 409"));
    }
    #[test]
    fn options_are_explicit_finite_and_cannot_select_a_network_bind_address() {
        let parse =
            |args: &[&str]| options(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert!(parse(&[]).is_err());
        assert!(parse(&["serve", "export", "--host", "0.0.0.0"]).is_err());
        for args in [
            &["serve", "export", "--max-seconds", "0"][..],
            &["serve", "export", "--idle-seconds", "3601"][..],
            &["serve", "export", "--port", "1", "--port", "2"][..],
            &["serve", "export", "--open", "--open"][..],
            &["serve", "export", "--port"][..],
        ] {
            assert!(parse(args).is_err());
        }
        let opts = parse(&[
            "serve",
            "export",
            "--open",
            "--port",
            "0",
            "--max-seconds",
            "10",
        ])
        .unwrap();
        assert!(opts.open);
        assert_eq!(opts.max, 10);
        assert_eq!(opts.port, 0);
        let command = browser_command("http://127.0.0.1:1234/token/index.html").unwrap();
        assert_eq!(
            command.get_args().last().unwrap(),
            "http://127.0.0.1:1234/token/index.html"
        );
        #[cfg(any(windows, target_os = "macos"))]
        assert!(Path::new(command.get_program()).is_absolute());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_controls_and_media_directories_refuse() {
        use std::os::unix::fs::symlink;
        let d = fixture("reel.music-note-session.v1");
        let real = d.path().join("real.js");
        fs::rename(d.path().join("editor.js"), &real).unwrap();
        symlink(&real, d.path().join("editor.js")).unwrap();
        assert!(Bundle::open(d.path()).is_err());
    }
}
