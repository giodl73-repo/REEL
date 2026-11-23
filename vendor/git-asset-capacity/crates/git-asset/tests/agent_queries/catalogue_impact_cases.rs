use super::*;
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    process::{Child, Stdio},
    time::Duration,
};

pub(super) struct Server {
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
    pub(super) fn new(f: &Fixture) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_git-asset-catalogue"))
            .args(["serve", f.enrollment.to_str().unwrap(), "0"])
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
    pub(super) fn get(&self, path: &str, session: bool) -> (u16, String) {
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
    pub(super) fn json(&self, path: &str) -> (u16, Value) {
        let (code, body) = self.get(path, true);
        (code, serde_json::from_str(&body).unwrap())
    }
}
fn path(f: &Fixture) -> String {
    let profile: Value =
        serde_json::from_slice(&fs::read(f.temp.path().join("profile.json")).unwrap()).unwrap();
    format!(
        "/api/impact?project=project&id=cel-a&snapshot={}&kinds=production,composition,publication",
        profile["commit"].as_str().unwrap()
    )
}

#[test]
fn catalogue_impact_endpoint_reuses_agent_basis_and_exact_selected_navigation_without_payload_io() {
    let f = Fixture::new();
    let server = Server::new(&f);
    let (code, report) = server.json(&path(&f));
    assert_eq!(code, 200, "{report}");
    let agent = f.evaluate(&f.request(impact("cel-a")));
    assert_eq!(report["basis_sha256"], agent["basis_sha256"]);
    assert_eq!(report["data"]["total_known"], 3);
    assert_eq!(
        report["data"]["items"][0]["path"],
        agent["data"]["items"][0]["path"]
    );
    for row in report["data"]["items"].as_array().unwrap() {
        assert_eq!(row["navigation"]["id"], row["asset"]["id"]);
        assert_eq!(row["navigation"]["project"], "project");
    }
    assert_eq!(report["coverage"]["payloads_opened"], 0);
    assert_eq!(report["authorizes_eviction"], false);
    assert_eq!(server.get(&path(&f), false).0, 403);
    assert!(server.get("/", false).1.contains("/impact.js"));
    assert!(
        server
            .get("/impact.js", false)
            .1
            .contains("function drawImpact")
    );
}

#[test]
fn catalogue_impact_pages_25_rows_and_refuses_stale_or_nonmetadata_operations() {
    let f = Fixture::new();
    for n in 0..60 {
        f.record(&format!("render-{n:03}"), "render", json!([{"id":"scene"}]));
    }
    f.advance();
    let server = Server::new(&f);
    let url = path(&f);
    let (_, first) = server.json(&url);
    assert_eq!(first["paging"]["returned"], 25);
    assert_eq!(first["paging"]["next_offset"], 25);
    assert_eq!(first["data"]["total_known"], 63);
    let basis = first["basis_sha256"].as_str().unwrap();
    let (_, second) = server.json(&format!("{url}&offset=25&basis={basis}"));
    assert_eq!(second["paging"]["returned"], 25);
    assert!(first["data"]["items"].as_array().unwrap().iter().all(|a| {
        second["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|b| a["asset"] != b["asset"])
    }));
    let (_, third) = server.json(&format!("{url}&offset=50&basis={basis}"));
    assert_eq!(third["paging"]["returned"], 13);
    assert!(third["paging"]["next_offset"].is_null());
    for invalid in [
        format!("{url}&offset=25"),
        format!("{url}&offset=25&basis={}", "0".repeat(64)),
        format!("{url}&operation=manifest_impact"),
        url.replace("project=project", "project=all"),
        url.replace(
            "production,composition,publication",
            "production,production",
        ),
        url.replace("production,composition,publication", "invented"),
        url.replace("production,composition,publication", ""),
        url.replace(first["commit"].as_str().unwrap(), &"0".repeat(40)),
    ] {
        assert_eq!(server.json(&invalid).0, 400, "{invalid}");
    }
}

#[test]
fn catalogue_impact_partial_zero_exposes_missing_definition_pins() {
    let f = Fixture::new();
    let mut config: Value = serde_json::from_slice(&fs::read(&f.enrollment).unwrap()).unwrap();
    config["projects"][0]["graph_profiles"] = json!([]);
    fs::write(&f.enrollment, serde_json::to_vec(&config).unwrap()).unwrap();
    let server = Server::new(&f);
    let (code, report) = server.json(&path(&f));
    assert_eq!(code, 200);
    assert_eq!(report["status"], "partial");
    assert_eq!(report["data"]["total_known"], 0);
    assert_eq!(report["data"]["unadmitted_definition_count"], 4);
    assert!(report["data"]["gap_count"].as_u64().unwrap() > 0);
    assert_eq!(report["coverage"]["complete_project_scope"], false);
}
