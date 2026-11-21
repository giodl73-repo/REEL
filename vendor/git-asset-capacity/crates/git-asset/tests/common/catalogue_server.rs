//! Owned loopback-only test child; never launches a desktop viewer.
use serde_json::Value;
use std::process::Command;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Stdio},
    time::Duration,
};

pub(crate) struct Server {
    child: Child,
    address: String,
    token: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Server {
    pub(crate) fn new(enrollment: &std::path::Path) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_git-asset-catalogue"))
            .args(["serve", enrollment.to_str().unwrap(), "0"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut server = Self {
            child,
            address: String::new(),
            token: String::new(),
        };
        let stdout = server.child.stdout.take().unwrap();
        let (send, receive) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let line = BufReader::new(stdout.take(4096)).lines().next();
            let _ = send.send(line);
        });
        let line = receive
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
            .unwrap()
            .unwrap();
        server.address = line
            .strip_prefix("git-asset catalogue: http://")
            .unwrap()
            .trim_end_matches('/')
            .into();
        assert!(server.address.starts_with("127.0.0.1:"));
        let (code, html) = server.get("/", false);
        assert_eq!(code, 200);
        server.token = html
            .split("name=\"git-asset-session\" content=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .into();
        server
    }
    pub(crate) fn get(&self, path: &str, session: bool) -> (u16, String) {
        let mut stream =
            TcpStream::connect_timeout(&self.address.parse().unwrap(), Duration::from_secs(5))
                .unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(15)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let token = if session {
            format!("X-Git-Asset-Session: {}\r\n", self.token)
        } else {
            String::new()
        };
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: {}\r\n{token}\r\n",
            self.address
        )
        .unwrap();
        let mut raw = String::new();
        stream
            .take(4 * 1024 * 1024 + 8192)
            .read_to_string(&mut raw)
            .unwrap();
        let (headers, body) = raw.split_once("\r\n\r\n").unwrap();
        let code = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        assert!(!body.contains("DO_NOT_EXPORT"));
        (code, body.into())
    }
    pub(crate) fn json(&self, path: &str) -> (u16, Value) {
        let (code, body) = self.get(path, true);
        (code, serde_json::from_str(&body).unwrap())
    }
}
