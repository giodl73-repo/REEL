use reel_workflow::{Error, ErrorKind};
use reel_workflow_http::*;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};
fn config(endpoint: String) -> Config {
    Config {
        schema: "reel.workflow-http.v1".into(),
        endpoint,
        timeout_seconds: 2,
        request_byte_limit: 1024,
        response_byte_limit: 1024,
        secret_headers: BTreeMap::new(),
        allow_loopback_http: true,
    }
}
fn server(reply: Vec<u8>, recorded: Arc<AtomicBool>) -> (String, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}/call", listener.local_addr().unwrap());
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(recorded.load(Ordering::SeqCst));
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut bytes = Vec::new();
        let mut byte = [0u8; 1];
        while !bytes.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            bytes.push(byte[0]);
            assert!(bytes.len() < 32768);
        }
        let header = String::from_utf8(bytes.clone()).unwrap();
        let count = header
            .lines()
            .find_map(|s| {
                s.to_ascii_lowercase()
                    .strip_prefix("content-length: ")
                    .map(|n| n.parse::<usize>().unwrap())
            })
            .unwrap();
        let mut body = vec![0u8; count];
        stream.read_exact(&mut body).unwrap();
        bytes.extend(body);
        stream.write_all(&reply).unwrap();
        bytes
    });
    (address, worker)
}
#[test]
fn post_records_original_identity_before_network_and_returns_exact_private_bytes() {
    let recorded = Arc::new(AtomicBool::new(false));
    let (address,worker)=server(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}".to_vec(),recorded.clone());
    let mut config = config(address);
    config.secret_headers.insert(
        "authorization".into(),
        SecretHeader {
            variable: "SYNTHETIC_KEY".into(),
            prefix: "Bearer ".into(),
        },
    );
    let prepared = config
        .prepare("original-op", br#"{"prompt":"synthetic"}"#, |name| {
            assert_eq!(name, "SYNTHETIC_KEY");
            Some("synthetic-token".into())
        })
        .unwrap();
    let response = prepared
        .send(|intent| {
            assert_eq!(intent.operation_id, "original-op");
            assert_eq!(
                intent.request_sha256,
                reel_workflow::digest(br#"{"prompt":"synthetic"}"#)
            );
            recorded.store(true, Ordering::SeqCst);
            Ok(())
        })
        .unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(response.body, br#"{"ok":true}"#);
    assert_eq!(response.body_sha256, reel_workflow::digest(&response.body));
    let request = String::from_utf8(worker.join().unwrap())
        .unwrap()
        .to_lowercase();
    assert!(request.contains("x-reel-operation-id: original-op"));
    assert!(request.contains("authorization: bearer synthetic-token"));
}
#[test]
fn failed_intent_and_invalid_config_never_connect_or_expose_secrets() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let mut cfg = config(format!("http://{}/call", listener.local_addr().unwrap()));
    let error = cfg
        .prepare("operation", b"{}", |_| None)
        .unwrap()
        .send(|_| {
            Err(Error {
                kind: ErrorKind::NeedsRecovery,
                code: "intent-not-durable".into(),
            })
        })
        .err()
        .unwrap();
    assert_eq!(error.code, "intent-not-durable");
    assert!(listener.accept().is_err());
    cfg.secret_headers.insert(
        "authorization".into(),
        SecretHeader {
            variable: "KEY".into(),
            prefix: "Bearer ".into(),
        },
    );
    let error = cfg
        .prepare("operation", b"{}", |_| Some("PRIVATE\r\nINJECT".into()))
        .err()
        .unwrap();
    assert!(!error.to_string().contains("PRIVATE"));
    assert!(listener.accept().is_err());
    cfg.secret_headers.clear();
    cfg.allow_loopback_http = false;
    assert_eq!(
        cfg.prepare("operation", b"{}", |_| None)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Denied
    );
    cfg.endpoint = "https://user:PRIVATE@example.invalid/".into();
    assert_eq!(
        cfg.prepare("operation", b"{}", |_| None)
            .err()
            .unwrap()
            .code,
        "http-endpoint-policy"
    );
}
#[test]
fn redirects_are_returned_without_contacting_the_target_and_statuses_are_not_retried() {
    let target = TcpListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let reply = format!(
        "HTTP/1.1 307 Temporary Redirect\r\nLocation: http://{}/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        target.local_addr().unwrap()
    );
    let recorded = Arc::new(AtomicBool::new(true));
    let (address, worker) = server(reply.into_bytes(), recorded);
    let response = config(address)
        .prepare("operation", b"{}", |_| None)
        .unwrap()
        .send(|_| Ok(()))
        .unwrap();
    assert_eq!(response.status, 307);
    worker.join().unwrap();
    assert!(target.accept().is_err());
    let (address,worker)=server(b"HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}".to_vec(),Arc::new(AtomicBool::new(true)));
    assert_eq!(
        config(address)
            .prepare("operation", b"{}", |_| None)
            .unwrap()
            .send(|_| Ok(()))
            .unwrap()
            .status,
        429
    );
    worker.join().unwrap();
}
#[test]
fn oversized_chunked_or_truncated_responses_require_original_operation_reconciliation() {
    for reply in [b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n".to_vec(),b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\nshort".to_vec()] {
        let (address,worker)=server(reply,Arc::new(AtomicBool::new(true)));let mut cfg=config(address);cfg.response_byte_limit=8;
        let error=cfg.prepare("original",b"{}",|_|None).unwrap().send(|_|Ok(())).err().unwrap();
        assert_eq!(error.kind,ErrorKind::NeedsRecovery);worker.join().unwrap();
    }
}

#[test]
fn disconnected_post_is_not_retried_on_another_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/call", listener.local_addr().unwrap());
    let observer = listener.try_clone().unwrap();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let mut body = [0u8; 2];
        stream.read_exact(&mut body).unwrap();
    });
    let failure = config(endpoint)
        .prepare("original", b"{}", |_| None)
        .unwrap()
        .send(|_| Ok(()))
        .err()
        .unwrap();
    assert_eq!(failure.kind, ErrorKind::NeedsRecovery);
    worker.join().unwrap();
    observer.set_nonblocking(true).unwrap();
    assert!(
        observer.accept().is_err(),
        "an uncertain POST opened another connection"
    );
}

#[test]
fn timeout_preserves_original_operation_uncertainty_without_a_retry() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/call", listener.local_addr().unwrap());
    let (stop, wait) = std::sync::mpsc::channel();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            stream.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        let mut body = [0u8; 2];
        stream.read_exact(&mut body).unwrap();
        let _ = wait.recv_timeout(std::time::Duration::from_secs(3));
    });
    let mut cfg = config(endpoint);
    cfg.timeout_seconds = 1;
    let mut intents = 0;
    let failure = cfg
        .prepare("original", b"{}", |_| None)
        .unwrap()
        .send(|_| {
            intents += 1;
            Ok(())
        })
        .err()
        .unwrap();
    assert_eq!(failure.code, "http-call-outcome-uncertain");
    assert_eq!(intents, 1);
    stop.send(()).unwrap();
    worker.join().unwrap();
}

#[test]
fn response_identifiers_are_observed_before_a_truncated_body() {
    let recorded = Arc::new(AtomicBool::new(false));
    let (endpoint,worker)=server(b"HTTP/1.1 200 OK\r\nContent-Length: 20\r\nrequest-id: request-17\r\nhistory-item-id: history-9\r\nConnection: close\r\n\r\nshort".to_vec(),recorded.clone());
    let mut head = None;
    let mut prefix = Vec::new();
    let error = config(endpoint)
        .prepare("original", b"{}", |_| None)
        .unwrap()
        .send_observed(
            |_| {
                recorded.store(true, Ordering::SeqCst);
                Ok(())
            },
            |observed| {
                head = Some(serde_json::to_value(observed).unwrap());
                Ok(())
            },
            |bytes| {
                prefix.extend_from_slice(bytes);
                Ok(())
            },
        )
        .err()
        .unwrap();
    assert_eq!(error.kind, ErrorKind::NeedsRecovery);
    assert_eq!(head.as_ref().unwrap()["request_id"], "request-17");
    assert_eq!(head.as_ref().unwrap()["history_item_id"], "history-9");
    assert_eq!(prefix, b"short");
    worker.join().unwrap();
}
