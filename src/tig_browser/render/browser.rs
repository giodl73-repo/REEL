//! Bounded loopback discovery/handshake for this invocation's owned profile.
use super::processes::{Task, native_path, pause};
use crate::{media_runtime::BoundTool, tig_browser::capture::WebSocketTransport};
use anyhow::{Context, Result, bail};
use reel_assets::jobs::QuietLimits;
use std::{
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    time::{Duration, Instant},
};
use tungstenite::{WebSocket, protocol::Role, stream::MaybeTlsStream};

fn write(stream: &mut TcpStream, mut bytes: &[u8], deadline: Instant) -> Result<()> {
    while !bytes.is_empty() {
        match stream.write(bytes) {
            Ok(0) => bail!("browser control write ended"),
            Ok(n) => bytes = &bytes[n..],
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => pause(deadline)?,
            Err(e) => return Err(e.into()),
        }
        if Instant::now() >= deadline {
            bail!("browser control write deadline elapsed");
        }
    }
    Ok(())
}
fn read(
    stream: &mut TcpStream,
    deadline: Instant,
    max: usize,
    headers_only: bool,
) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 2048];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Ok(bytes),
            Ok(n) => {
                bytes.extend_from_slice(&buffer[..n]);
                if bytes.len() > max {
                    bail!("browser discovery response exceeds bound");
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => pause(deadline)?,
            Err(e) => return Err(e.into()),
        }
        if let Some(end) = bytes
            .windows(4)
            .position(|v| v == b"\r\n\r\n")
            .map(|n| n + 4)
        {
            if headers_only {
                return Ok(bytes);
            }
            let head = std::str::from_utf8(&bytes[..end])?;
            let mut length = None;
            for line in head.lines().skip(1).filter(|v| !v.is_empty()) {
                let (name, value) = line
                    .split_once(':')
                    .context("invalid browser HTTP header")?;
                if name.eq_ignore_ascii_case("transfer-encoding") {
                    bail!("unsupported browser discovery transfer encoding");
                }
                if name.eq_ignore_ascii_case("content-length") {
                    if length.is_some() {
                        bail!("duplicate browser discovery length");
                    }
                    length = Some(value.trim().parse::<usize>()?);
                }
            }
            let length = length.context("browser discovery length missing")?;
            if end.checked_add(length).is_none_or(|n| n > max) {
                bail!("browser discovery body exceeds bound");
            }
            if bytes.len() > end + length {
                bail!("excess browser discovery bytes");
            }
            if bytes.len() == end + length {
                return Ok(bytes);
            }
        }
        if Instant::now() >= deadline {
            bail!("browser discovery deadline elapsed");
        }
    }
}
fn connection(port: u16, deadline: Instant) -> Result<TcpStream> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .context("browser connect deadline elapsed")?;
    let stream = TcpStream::connect_timeout(
        &SocketAddr::from(([127, 0, 0, 1], port)),
        remaining.min(Duration::from_secs(1)),
    )?;
    stream.set_nonblocking(true)?;
    Ok(stream)
}
fn headers(raw: &[u8]) -> Result<(&str, &[u8])> {
    let end = raw
        .windows(4)
        .position(|v| v == b"\r\n\r\n")
        .context("browser HTTP headers incomplete")?
        + 4;
    Ok((std::str::from_utf8(&raw[..end])?, &raw[end..]))
}
fn page(port: u16, deadline: Instant) -> Result<String> {
    let mut stream = connection(port, deadline)?;
    write(
        &mut stream,
        format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
        deadline,
    )?;
    let raw = read(&mut stream, deadline, 65536, false)?;
    let (head, body) = headers(&raw)?;
    if !head.starts_with("HTTP/1.1 200 ") {
        bail!("browser page discovery refused");
    }
    let targets = crate::tig_browser::capture::wire_value(std::str::from_utf8(body)?)?;
    let targets = targets
        .as_array()
        .filter(|v| v.len() <= 32)
        .context("browser target list exceeds bound")?;
    let candidates = targets
        .iter()
        .filter(|v| v["type"] == "page" && v["url"] == "about:blank")
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        bail!("browser did not expose one owned initial page");
    }
    let url = candidates[0]["webSocketDebuggerUrl"]
        .as_str()
        .context("browser target endpoint missing")?;
    let prefix = format!("ws://127.0.0.1:{port}/devtools/page/");
    let id = url
        .strip_prefix(&prefix)
        .filter(|v| {
            !v.is_empty()
                && v.len() <= 128
                && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .context("browser target endpoint escapes owned loopback scope")?;
    Ok(format!("/devtools/page/{id}"))
}
fn connect(port: u16, path: &str, deadline: Instant, max: usize) -> Result<WebSocketTransport> {
    let mut stream = connection(port, deadline)?;
    let key = tungstenite::handshake::client::generate_key();
    write(&mut stream,format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n\r\n").as_bytes(),deadline)?;
    let raw = read(&mut stream, deadline, 8192, true)?;
    let (head, tail) = headers(&raw)?;
    let mut fields = std::collections::BTreeMap::new();
    for line in head.lines().skip(1).filter(|v| !v.is_empty()) {
        let (name, value) = line
            .split_once(':')
            .context("invalid browser upgrade header")?;
        if fields
            .insert(name.to_ascii_lowercase(), value.trim())
            .is_some()
        {
            bail!("duplicate browser upgrade header");
        }
    }
    if !head.starts_with("HTTP/1.1 101 ")
        || fields.get("sec-websocket-accept").copied()
            != Some(tungstenite::handshake::derive_accept_key(key.as_bytes()).as_str())
        || !fields
            .get("upgrade")
            .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        || !fields
            .get("connection")
            .is_some_and(|v| v.eq_ignore_ascii_case("upgrade"))
    {
        bail!("browser WebSocket upgrade differs");
    }
    WebSocketTransport::new(
        WebSocket::from_partially_read(
            MaybeTlsStream::Plain(stream),
            tail.to_vec(),
            Role::Client,
            None,
        ),
        max,
    )
}

pub(super) struct Browser {
    pub task: Task,
    pub transport: WebSocketTransport,
    profile: tempfile::TempDir,
    max_message: usize,
    io_wait: Duration,
}
impl Browser {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        program: &BoundTool,
        workspace: &Path,
        budget: u64,
        timeout: Duration,
        limits: QuietLimits,
        max_message: usize,
        io_wait: Duration,
    ) -> Result<Self> {
        let profile = tempfile::Builder::new()
            .prefix("browser-runtime-")
            .tempdir_in(workspace)?;
        let user_profile = profile.path().join("profile");
        fs::create_dir(&user_profile)?;
        let args = vec![
            "--headless=new".into(),
            "--disable-gpu".into(),
            "--no-first-run".into(),
            "--no-default-browser-check".into(),
            "--disable-background-networking".into(),
            "--disable-component-update".into(),
            "--disable-extensions".into(),
            "--disable-sync".into(),
            "--metrics-recording-only".into(),
            "--mute-audio".into(),
            "--hide-scrollbars".into(),
            "--disable-breakpad".into(),
            "--disable-crash-reporter".into(),
            "--no-pings".into(),
            "--disk-cache-size=0".into(),
            "--media-cache-size=0".into(),
            "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE localhost".into(),
            "--remote-debugging-port=0".into(),
            format!("--user-data-dir={}", native_path(&user_profile)?),
            "about:blank".into(),
        ];
        let mut task = Task::start(
            program.path(),
            args,
            profile.path(),
            budget,
            timeout,
            limits,
        )?;
        let deadline = Instant::now() + io_wait;
        let port = loop {
            task.check_running()?;
            if let Ok(file) = fs::File::open(user_profile.join("DevToolsActivePort")) {
                let mut bytes = Vec::new();
                file.take(4097).read_to_end(&mut bytes)?;
                if bytes.len() > 4096 {
                    bail!("browser endpoint observation exceeds bound");
                }
                if let Ok(text) = std::str::from_utf8(&bytes)
                    && let Some(port) = text
                        .lines()
                        .next()
                        .and_then(|v| v.parse::<u16>().ok())
                        .filter(|p| *p != 0)
                {
                    break port;
                }
            }
            pause(deadline)?;
        };
        let path = page(port, deadline)?;
        let transport = connect(port, &path, deadline, max_message)?;
        Ok(Self {
            task,
            transport,
            profile,
            max_message,
            io_wait,
        })
    }
    pub fn close(mut self, deadline: Instant) -> Result<()> {
        use crate::tig_browser::capture::Transport;
        let deadline = deadline.min(Instant::now() + self.io_wait);
        // Chromium command IDs are signed 32-bit integers. Capture's bounded
        // command budget cannot reach this reserved final ID.
        let close_id = i32::MAX;
        self.transport.send(
            &serde_json::to_string(&serde_json::json!({"id":close_id,"method":"Browser.close"}))?,
            deadline
                .checked_duration_since(Instant::now())
                .context("browser close deadline elapsed")?,
        )?;
        for _ in 0..256 {
            if self.task.ended() {
                break;
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .context("browser close response deadline elapsed")?;
            match self.transport.receive(self.max_message, remaining) {
                Ok(raw) => {
                    let value = crate::tig_browser::capture::wire_value(&raw)?;
                    if value["id"].as_i64() == Some(i64::from(close_id)) {
                        if value.get("error").is_some_and(|v| !v.is_null()) {
                            bail!("browser refused owned shutdown");
                        }
                        break;
                    }
                }
                // Closing the control connection is normal during Browser.close;
                // successful process-tree exit is still mandatory below.
                Err(_) => break,
            }
        }
        self.task.wait(deadline)?;
        self.profile.close()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(raw: &[u8]) -> Result<Vec<u8>> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
        let mut client = TcpStream::connect(listener.local_addr()?)?;
        let (mut server, _) = listener.accept()?;
        client.set_nonblocking(true)?;
        server.write_all(raw)?;
        // Keep server alive: completion must follow Content-Length, not EOF.
        read(
            &mut client,
            Instant::now() + Duration::from_millis(100),
            1024,
            false,
        )
    }
    #[test]
    fn discovery_obeys_body_length_without_waiting_for_connection_close() {
        let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: keep-alive\r\n\r\n[]";
        assert_eq!(response(raw).unwrap(), raw);
    }
    #[test]
    fn discovery_refuses_ambiguous_oversized_and_excess_body_lengths() {
        for raw in [
            b"HTTP/1.1 200 OK\r\nContent-Length: 90000\r\n\r\n".as_slice(),
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Length: 3\r\n\r\n[]",
            b"HTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\n[]",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n",
        ] {
            assert!(response(raw).is_err());
        }
    }
}
