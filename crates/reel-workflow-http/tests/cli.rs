use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::Path,
    process::Command,
    thread,
};
fn server(reply: &'static [u8]) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/api", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut byte = [0u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
        }
        let text = String::from_utf8(bytes).unwrap();
        let length = text
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        let mut bytes = vec![0u8; length];
        stream.read_exact(&mut bytes).unwrap();
        stream.write_all(reply).unwrap();
    });
    (endpoint, worker)
}
fn inputs(root: &Path, endpoint: String) {
    fs::write(root.join("config.json"),serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-http.v1","endpoint":endpoint,"timeout_seconds":2,"request_byte_limit":1024,"response_byte_limit":1024,"secret_headers":{},"allow_loopback_http":true})).unwrap()).unwrap();
    fs::write(root.join("body.json"), b"{}").unwrap();
}
fn invoke(root: &Path, metadata: &str, response: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-workflow-http"))
        .arg("post")
        .arg(root.join("config.json"))
        .arg("original-operation")
        .arg(root.join("body.json"))
        .arg(root.join("intent.json"))
        .arg(root.join(metadata))
        .arg(root.join(response))
        .arg("--apply")
        .output()
        .unwrap()
}
#[test]
fn cli_persists_private_intent_and_exact_response_then_refuses_another_call() {
    let root = tempfile::tempdir().unwrap();
    let (endpoint,worker)=server(b"HTTP/1.1 202 Accepted\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}");
    inputs(root.path(), endpoint);
    let output = invoke(root.path(), "metadata.json", "response.bin");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    worker.join().unwrap();
    let intent = fs::read(root.path().join("intent.json")).unwrap();
    let metadata: serde_json::Value =
        serde_json::from_slice(&fs::read(root.path().join("metadata.json")).unwrap()).unwrap();
    assert_eq!(metadata["status"], 202);
    assert_eq!(metadata["retained"], false);
    assert_eq!(fs::read(root.path().join("response.bin")).unwrap(), b"{}");
    assert_eq!(metadata["body_sha256"], reel_workflow::digest(b"{}"));
    let replay = invoke(root.path(), "new-metadata.json", "new-response.bin");
    assert!(!replay.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&replay.stdout).unwrap()["error"]["code"],
        "http-original-intent-exists"
    );
    assert!(!root.path().join("new-response.bin").exists());
    assert_eq!(fs::read(root.path().join("intent.json")).unwrap(), intent);
}
#[test]
fn cli_preserves_received_prefix_on_truncation_and_never_reissues_uncertain_request() {
    let root = tempfile::tempdir().unwrap();
    let (endpoint, worker) =
        server(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort");
    inputs(root.path(), endpoint);
    let output = invoke(root.path(), "metadata.json", "response.bin");
    assert!(!output.status.success());
    worker.join().unwrap();
    assert_eq!(
        fs::read(root.path().join("response.bin")).unwrap(),
        b"short"
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()["error"]["kind"],
        "needs-recovery"
    );
    assert!(root.path().join("intent.json").is_file());
    assert_eq!(
        fs::metadata(root.path().join("metadata.json"))
            .unwrap()
            .len(),
        0
    );
    let replay = invoke(root.path(), "new-metadata.json", "new-response.bin");
    assert!(!replay.status.success());
    assert!(!root.path().join("new-response.bin").exists());
}
