//! Process/socket checks over synthetic controls. No private media or GUI launch.
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn fixture() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    fs::write(
        d.path().join("index.html"),
        include_bytes!("../../reel-music/src/note_editor.html"),
    )
    .unwrap();
    fs::write(
        d.path().join("editor.js"),
        include_bytes!("../../reel-music/src/note_editor.js"),
    )
    .unwrap();
    fs::write(
        d.path().join("session.json"),
        serde_json::to_vec(&json!({"schema":"reel.music-note-session.v1","synthetic":true}))
            .unwrap(),
    )
    .unwrap();
    fs::write(d.path().join("private.txt"), b"not an editor resource").unwrap();
    d
}
fn start(root: &Path, report: &Path, idle: &str, max: &str) -> (Running, Value) {
    let child = Command::new(env!("CARGO_BIN_EXE_reel-editor-host"))
        .arg("serve")
        .arg(root)
        .args(["--idle-seconds", idle, "--max-seconds", max, "--report"])
        .arg(report)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut running = Running(child);
    let out = running.0.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(out.take(8192));
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            let _ = tx.send(line);
        }
        let mut rest = vec![];
        let _ = reader.read_to_end(&mut rest);
    });
    let first = rx
        .recv_timeout(Duration::from_secs(10))
        .expect("bounded startup");
    let value = serde_json::from_str(&first).expect("startup JSON");
    (running, value)
}
fn finish(child: &mut Running) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            assert!(status.success());
            return;
        }
        assert!(Instant::now() < deadline, "host exceeded expiry budget");
        thread::sleep(Duration::from_millis(10));
    }
}
fn get(authority: &str, path: &str, method: &str, extra: &str) -> Vec<u8> {
    let mut socket = TcpStream::connect(authority).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        socket,
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\n{extra}\r\n"
    )
    .unwrap();
    let mut data = vec![];
    socket.read_to_end(&mut data).unwrap();
    data
}
#[test]
fn actual_cli_serves_only_locked_resources_and_exits_at_hard_deadline() {
    let d = fixture();
    let report = d.path().join("private-report.json");
    let original = fs::read(d.path().join("session.json")).unwrap();
    let (mut child, start) = start(d.path(), &report, "3600", "3");
    let url = start["url"].as_str().unwrap();
    let (authority, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    assert!(authority.starts_with("127.0.0.1:"));
    let token = path.split('/').next().unwrap();
    assert_eq!(token.len(), 32);
    assert!(token.bytes().all(|b| b.is_ascii_hexdigit()));
    let route = format!("/{token}/session.json");
    let full = get(authority, &route, "GET", "");
    assert!(full.starts_with(b"HTTP/1.1 200"));
    assert!(full.ends_with(&original));
    assert!(get(authority, &route, "HEAD", "").ends_with(b"\r\n\r\n"));
    let range = get(authority, &route, "GET", "Range: bytes=0-2\r\n");
    assert!(range.starts_with(b"HTTP/1.1 206"));
    assert!(range.ends_with(&original[..3]));
    assert!(get(authority, &route, "POST", "").starts_with(b"HTTP/1.1 405"));
    assert!(
        get(
            authority,
            &route,
            "GET",
            "Origin: https://outside.invalid\r\n"
        )
        .starts_with(b"HTTP/1.1 403")
    );
    assert!(
        get(authority, &format!("/{token}/private.txt"), "GET", "").starts_with(b"HTTP/1.1 404")
    );
    assert!(
        get(
            authority,
            &format!("/{token}/private-report.json"),
            "GET",
            ""
        )
        .starts_with(b"HTTP/1.1 404")
    );
    finish(&mut child);
    assert!(TcpStream::connect(authority).is_err());
    assert_eq!(fs::read(d.path().join("session.json")).unwrap(), original);
    let receipt: Value = serde_json::from_slice(&fs::read(report).unwrap()).unwrap();
    assert_eq!(receipt["summary"]["reason"], "max-seconds");
    assert_eq!(receipt["summary"]["served_requests"], 3);
    assert_eq!(receipt["summary"]["rejected_requests"], 4);
}
#[test]
fn an_idle_browser_connection_does_not_block_a_ready_resource_request() {
    let d = fixture();
    let report = d.path().join("parallel.json");
    let (mut child, start) = start(d.path(), &report, "5", "5");
    let url = start["url"].as_str().unwrap();
    let (authority, path) = url
        .strip_prefix("http://")
        .unwrap()
        .split_once('/')
        .unwrap();
    let mut idle = TcpStream::connect(authority).unwrap();
    idle.write_all(b"GET ").unwrap();
    let began = Instant::now();
    let response = get(authority, &format!("/{path}"), "GET", "");
    assert!(response.starts_with(b"HTTP/1.1 200"));
    assert!(
        began.elapsed() < Duration::from_millis(1500),
        "idle connection blocked browser resource loading"
    );
    drop(idle);
    finish(&mut child);
}
#[test]
fn idle_exit_and_existing_report_refusal_leave_source_untouched() {
    let d = fixture();
    let report = d.path().join("idle.json");
    let (mut child, _) = start(d.path(), &report, "1", "5");
    finish(&mut child);
    let receipt: Value = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert_eq!(receipt["summary"]["reason"], "idle-seconds");
    let original = fs::read(&report).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_reel-editor-host"))
        .arg("serve")
        .arg(d.path())
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(report).unwrap(), original);
}
