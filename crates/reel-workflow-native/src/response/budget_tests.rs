use super::*;
use std::io::{self, Cursor};

fn allowance(bytes: u64) -> Bound {
    Bound {
        total: bytes,
        object: bytes,
        deadline: Some(Duration::from_secs(5)),
    }
}
fn artifact(raw: &[u8]) -> Artifact {
    Artifact {
        logical_id: "candidate/stream".into(),
        sha256: reel_workflow::digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/octet-stream".into(),
    }
}

#[test]
fn exact_stream_refuses_corruption_truncation_trailing_bytes_and_interrupted_io_is_retried() {
    struct Stream {
        source: Cursor<Vec<u8>>,
        interrupted: bool,
        largest: usize,
    }
    impl Read for Stream {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.largest = self.largest.max(buffer.len());
            if !self.interrupted {
                self.interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            self.source.read(buffer)
        }
    }
    let raw = vec![23u8; 2 * 65536 + 1];
    let pin = artifact(&raw);
    let bound = allowance(pin.bytes);
    let mut stream = Stream {
        source: Cursor::new(raw.clone()),
        interrupted: false,
        largest: 0,
    };
    bound.verify(&pin, &mut stream).unwrap();
    assert_eq!(stream.largest, 65536);
    for bytes in [
        vec![24; raw.len()],
        raw[..raw.len() - 1].to_vec(),
        [raw.as_slice(), &[0]].concat(),
    ] {
        assert_eq!(
            bound
                .verify(&pin, &mut Cursor::new(bytes))
                .unwrap_err()
                .kind,
            ErrorKind::Integrity
        );
    }
    struct Broken;
    impl Read for Broken {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::PermissionDenied.into())
        }
    }
    assert_eq!(
        bound.verify(&pin, &mut Broken).unwrap_err().kind,
        ErrorKind::Denied
    );
    assert_eq!(
        allowance(pin.bytes - 1)
            .verify(&pin, &mut Cursor::new(raw))
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
}

#[test]
fn deadline_is_checked_after_a_blocking_eof_and_before_any_further_read() {
    struct Late;
    impl Read for Late {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_millis(5));
            Ok(0)
        }
    }
    let bound = Bound {
        total: 1,
        object: 1,
        deadline: Some(Duration::from_millis(1)),
    };
    assert_eq!(
        bound.verify(&artifact(b""), &mut Late).unwrap_err().code,
        "response-verification-deadline"
    );
}

#[test]
fn response_accounting_includes_aliases_and_worker_receipt_and_never_wraps() {
    let pin = artifact(b"payload");
    let mut result = ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: "test".into(),
        task_key: "a".repeat(64),
        attempt_id: "original".into(),
        artifacts: BTreeMap::from([("first".into(), pin.clone()), ("alias".into(), pin.clone())]),
        provenance: reel_workflow::Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: Some(pin.clone()),
    };
    allowance(3 * pin.bytes).check(&result).unwrap();
    assert_eq!(
        allowance(3 * pin.bytes - 1)
            .check(&result)
            .unwrap_err()
            .kind,
        ErrorKind::Budget
    );
    result.artifacts.get_mut("first").unwrap().bytes = u64::MAX;
    assert_eq!(
        Bound {
            total: u64::MAX,
            object: u64::MAX,
            deadline: None
        }
        .check(&result)
        .unwrap_err()
        .kind,
        ErrorKind::Budget
    );
    let mut budget = Budget {
        schema: "reel.workflow-return-budget.v1".into(),
        total_bytes: 16 * 1024 * 1024 * 1024,
        object_bytes: 8 * 1024 * 1024 * 1024,
        verify_millis: 300_000,
    };
    budget.validate().unwrap();
    budget.verify_millis += 1;
    assert!(budget.validate().is_err());
    budget.verify_millis = 1;
    budget.total_bytes += 1;
    assert!(budget.validate().is_err());
}

#[cfg(windows)]
#[test]
#[ignore = "Read only the explicitly named existing reserved 3.6 GB synthetic source; no native admission, publication or new capacity"]
fn qualified_existing_large_return_stream_uses_exact_bytes_without_buffering_the_master() {
    let path = std::path::PathBuf::from(
        std::env::var_os("REEL_LARGE_RETURN_SOURCE").expect("Set original synthetic source path"),
    );
    assert!(path.is_absolute());
    let pin = Artifact {
        logical_id: "candidate/synthetic-master".into(),
        sha256: "1800c39a6a2baaedb3760b6be6035276d6ca7602430e0f91fc671be1ee22a509".into(),
        bytes: 3_600_000_001,
        media_type: "application/octet-stream".into(),
    };
    let mut held = crate::staging::open_held(&path, pin.bytes).unwrap();
    assert_eq!(
        Bound {
            total: MAX_BYTES,
            object: MAX_BYTES,
            deadline: None
        }
        .verify(&pin, &mut held)
        .unwrap_err()
        .kind,
        ErrorKind::Budget
    );
    let bound = Bound {
        total: 4 * 1024 * 1024 * 1024,
        object: 4 * 1024 * 1024 * 1024,
        deadline: Some(Duration::from_secs(300)),
    };
    bound.verify(&pin, &mut held).unwrap();
    assert!(std::fs::OpenOptions::new().write(true).open(&path).is_err());
}
