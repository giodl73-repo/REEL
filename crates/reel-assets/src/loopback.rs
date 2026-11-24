//! Shared read-only capability-scoped loopback transport for REEL editors/views.
//! Callers supply explicit admitted resources; this module resolves no assets.
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpStream,
    time::{Duration, Instant},
};
#[cfg(windows)]
use std::{env, path::PathBuf};
use std::{
    process::{Command, Stdio},
    thread,
};
pub const HEADER_LIMIT: usize = 8192;
#[derive(Debug)]
pub struct Request {
    pub head: bool,
    pub path: String,
    pub range: Option<String>,
}
pub fn request(raw: &[u8], host: &str, token: &str) -> std::result::Result<Request, u16> {
    if raw.len() > HEADER_LIMIT {
        return Err(400);
    }
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
/// Read a bounded HTTP header; Windows accepted sockets may inherit nonblocking.
pub fn read_request(
    stream: &mut TcpStream,
    host: &str,
    token: &str,
    deadline: Instant,
) -> std::result::Result<Request, u16> {
    stream.set_nonblocking(false).map_err(|_| 400u16)?;
    let timeout = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_secs(5));
    if timeout.is_zero() {
        return Err(503);
    }
    stream.set_read_timeout(Some(timeout)).map_err(|_| 400u16)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|_| 400u16)?;
    let mut raw = Vec::new();
    let mut buffer = [0; 1024];
    loop {
        if raw.len() >= HEADER_LIMIT || Instant::now() >= deadline {
            return Err(400);
        }
        let count = stream.read(&mut buffer).map_err(|_| 400u16)?;
        if count == 0 {
            return Err(400);
        }
        raw.extend_from_slice(&buffer[..count]);
        if raw.len() > HEADER_LIMIT {
            return Err(400);
        }
        if raw.windows(4).any(|w| w == b"\r\n\r\n") {
            return request(&raw, host, token);
        }
    }
}
pub fn byte_range(value: Option<&str>, size: u64) -> std::result::Result<(u64, u64, bool), u16> {
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
pub fn response(
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
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {mime}\r\nContent-Length: {bytes}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\nReferrer-Policy: no-referrer\r\nX-Frame-Options: DENY\r\nContent-Security-Policy: default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'\r\n{extra}\r\n"
    )
}
pub fn browser_command(url: &str) -> crate::Result<Command> {
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
pub fn open_browser(url: &str) -> crate::Result<()> {
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
