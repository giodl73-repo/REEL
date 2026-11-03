use reel_assets::review_ids::{self, Request, Source};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command, sync::Arc, thread};

fn baseline() -> Vec<u8> {
    format!(r#"{{"schema":"caminos.private-review-short-ids.v1","by_source":{{"C:{}":"C0001"}},"next":{{"C":2,"V":1,"P":1}}}}"#, "a".repeat(64)).into_bytes()
}

fn adopt(root: &Path) -> std::path::PathBuf {
    let source = root.join("original.json");
    let raw = baseline();
    fs::write(&source, &raw).unwrap();
    let state = root.join("state");
    review_ids::adopt(
        &source,
        &format!("{:x}", Sha256::digest(&raw)),
        raw.len() as u64,
        &state,
    )
    .unwrap();
    state
}

fn request(digest: &str) -> Request {
    Request {
        schema: "reel.review-id-request.v1".into(),
        sources: vec![Source {
            kind: "C".into(),
            sha256: digest.into(),
        }],
    }
}

#[test]
fn adopts_exact_bytes_and_allocates_idempotently_without_original_source() {
    let dir = tempfile::tempdir().unwrap();
    let state = adopt(dir.path());
    fs::remove_file(dir.path().join("original.json")).unwrap();
    let first = review_ids::inspect(&state).unwrap();
    assert_eq!(first.registry.by_source.len(), 1);
    let same = review_ids::allocate(&state, &request(&"a".repeat(64))).unwrap();
    assert_eq!(same.new_assignments, 0);
    assert_eq!(same.checkpoint, first.checkpoint);
    let result = review_ids::allocate(&state, &request(&"b".repeat(64))).unwrap();
    assert_eq!(result.assignments[0].id, "C0002");
    assert_eq!(result.new_assignments, 1);
    let retry = review_ids::allocate(&state, &request(&"b".repeat(64))).unwrap();
    assert_eq!(retry.assignments[0].id, "C0002");
    assert_eq!(retry.new_assignments, 0);
    assert_eq!(retry.checkpoint, result.checkpoint);
    assert_eq!(review_ids::inspect(&state).unwrap().registry.next["C"], 3);
    assert_eq!(fs::read(state.join("baseline.json")).unwrap(), baseline());
}

#[test]
fn late_invalid_source_and_oversized_batch_leave_journal_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let state = adopt(dir.path());
    let mut req = request(&"b".repeat(64));
    req.sources.push(Source {
        kind: "UNKNOWN".into(),
        sha256: "c".repeat(64),
    });
    assert!(review_ids::allocate(&state, &req).is_err());
    req.sources = vec![req.sources[0].clone(); 513];
    assert!(review_ids::allocate(&state, &req).is_err());
    assert!(
        fs::read(state.join("assignments.ndjson"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(review_ids::inspect(&state).unwrap().registry.next["C"], 2);
}

#[test]
fn rejects_bad_imports_before_creating_state() {
    let dir = tempfile::tempdir().unwrap();
    let original = String::from_utf8(baseline()).unwrap();
    let key = format!("C:{}", "a".repeat(64));
    let invalid = [
        original.replace("\"C\":2", "\"C\":1"),
        original.replace("\"C\":2", "\"C\":2,\"C\":3"),
        original.replace(
            &format!("\"{key}\":\"C0001\""),
            &format!("\"{key}\":\"C0001\",\"{key}\":\"C0001\""),
        ),
        original.replace(
            &format!("\"{key}\":\"C0001\""),
            &format!("\"{key}\":\"C0001\",\"C:{}\":\"C0001\"", "b".repeat(64)),
        ),
        original.replace("C0001", "V0001"),
        original.replace("C0001", "C0000"),
        original.replace("private-review-short-ids.v1", "unknown.v1"),
    ];
    for (i, raw) in invalid.iter().enumerate() {
        let source = dir.path().join(format!("source-{i}"));
        fs::write(&source, raw).unwrap();
        let state = dir.path().join(format!("state-{i}"));
        assert!(
            review_ids::adopt(
                &source,
                &format!("{:x}", Sha256::digest(raw)),
                raw.len() as u64,
                &state
            )
            .is_err()
        );
        assert!(!state.exists());
    }
    let source = dir.path().join("good");
    fs::write(&source, baseline()).unwrap();
    let state = dir.path().join("wrong-pin");
    assert!(review_ids::adopt(&source, &"0".repeat(64), baseline().len() as u64, &state).is_err());
    assert!(!state.exists());
}

#[test]
fn truncated_corrupt_reordered_or_extra_journal_lines_refuse_without_repair() {
    let dir = tempfile::tempdir().unwrap();
    let state = adopt(dir.path());
    review_ids::allocate(&state, &request(&"b".repeat(64))).unwrap();
    let original = fs::read(state.join("assignments.ndjson")).unwrap();
    let mut blank = original.clone();
    blank.push(b'\n');
    for bad in [
        original[..original.len() - 1].to_vec(),
        b"not-json\n".to_vec(),
        blank,
        [original.clone(), original.clone()].concat(),
    ] {
        fs::write(state.join("assignments.ndjson"), &bad).unwrap();
        assert!(review_ids::inspect(&state).is_err());
        assert!(review_ids::allocate(&state, &request(&"c".repeat(64))).is_err());
        assert_eq!(fs::read(state.join("assignments.ndjson")).unwrap(), bad);
    }
}

#[test]
fn concurrent_workers_share_one_counter_and_reuse_shared_sources() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(adopt(dir.path()));
    let workers: Vec<_> = (0..12)
        .map(|i| {
            let state = state.clone();
            thread::spawn(move || {
                let mut req = request(&format!("{i:064x}"));
                req.sources.push(Source {
                    kind: "P".into(),
                    sha256: "f".repeat(64),
                });
                review_ids::allocate(&state, &req).unwrap()
            })
        })
        .collect();
    let reports: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(reports.iter().all(|r| r.assignments[1].id == "P0001"));
    let recovered = review_ids::inspect(&state).unwrap();
    assert_eq!(recovered.registry.by_source.len(), 14);
    assert_eq!(recovered.registry.next["C"], 14);
    assert_eq!(recovered.registry.next["P"], 2);
}

#[test]
fn baseline_tamper_and_reimport_never_overwrite_current_history() {
    let dir = tempfile::tempdir().unwrap();
    let state = adopt(dir.path());
    review_ids::allocate(&state, &request(&"b".repeat(64))).unwrap();
    let journal = fs::read(state.join("assignments.ndjson")).unwrap();
    let source = dir.path().join("original.json");
    assert!(
        review_ids::adopt(
            &source,
            &format!("{:x}", Sha256::digest(baseline())),
            baseline().len() as u64,
            &state
        )
        .is_err()
    );
    assert_eq!(fs::read(state.join("assignments.ndjson")).unwrap(), journal);
    fs::write(state.join("baseline.json"), b"{}").unwrap();
    assert!(review_ids::inspect(&state).is_err());
}

#[test]
fn real_cli_reopens_and_refuses_existing_output_before_allocation() {
    let dir = tempfile::tempdir().unwrap();
    let state = adopt(dir.path());
    let binary = env!("CARGO_BIN_EXE_reel-review-ids");
    let req = dir.path().join("request.json");
    fs::write(&req, serde_json::to_vec(&request(&"b".repeat(64))).unwrap()).unwrap();
    let report = dir.path().join("report.json");
    fs::write(&report, b"existing").unwrap();
    assert!(
        !Command::new(binary)
            .args(["allocate"])
            .arg(&state)
            .arg(&req)
            .arg(&report)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        fs::read(state.join("assignments.ndjson"))
            .unwrap()
            .is_empty()
    );
    let new_report = dir.path().join("new-report.json");
    let output = Command::new(binary)
        .arg("allocate")
        .arg(&state)
        .arg(&req)
        .arg(&new_report)
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains(&"b".repeat(64))
    );
    assert_eq!(review_ids::inspect(&state).unwrap().registry.next["C"], 3);
}

#[test]
fn separate_cli_processes_recover_one_shared_registry() {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(adopt(dir.path()));
    let binary = env!("CARGO_BIN_EXE_reel-review-ids");
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let request_path = dir.path().join(format!("request-{i}.json"));
            let report_path = dir.path().join(format!("report-{i}.json"));
            let mut req = request(&format!("{i:064x}"));
            req.sources.push(Source {
                kind: "V".into(),
                sha256: "e".repeat(64),
            });
            fs::write(&request_path, serde_json::to_vec(&req).unwrap()).unwrap();
            let state = state.clone();
            thread::spawn(move || {
                let output = Command::new(binary)
                    .arg("allocate")
                    .arg(state.as_ref())
                    .arg(request_path)
                    .arg(report_path)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    let report = review_ids::inspect(&state).unwrap();
    assert_eq!(report.registry.by_source.len(), 10);
    assert_eq!(report.registry.next["C"], 10);
    assert_eq!(report.registry.next["V"], 2);
}

#[test]
fn mixed_historical_padding_survives_but_numeric_aliases_refuse() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("mixed.json");
    let raw = format!(
        r#"{{"schema":"reel.review-short-ids.v1","by_source":{{"C:{}":"C001","C:{}":"C1000","P:{}":"P23"}},"next":{{"C":1001,"P":24}}}}"#,
        "a".repeat(64),
        "b".repeat(64),
        "c".repeat(64)
    );
    fs::write(&source, &raw).unwrap();
    let state = dir.path().join("mixed-state");
    let recovered = review_ids::adopt(
        &source,
        &format!("{:x}", Sha256::digest(&raw)),
        raw.len() as u64,
        &state,
    )
    .unwrap();
    assert_eq!(
        recovered.registry.by_source[&format!("C:{}", "a".repeat(64))],
        "C001"
    );
    assert_eq!(
        recovered.registry.by_source[&format!("P:{}", "c".repeat(64))],
        "P23"
    );
    let req = Request {
        schema: "reel.review-id-request.v1".into(),
        sources: vec![Source {
            kind: "P".into(),
            sha256: "d".repeat(64),
        }],
    };
    assert_eq!(
        review_ids::allocate(&state, &req).unwrap().assignments[0].id,
        "P0024"
    );
    let alias = raw.replace("C1000", "C0001");
    fs::write(&source, &alias).unwrap();
    assert!(
        review_ids::adopt(
            &source,
            &format!("{:x}", Sha256::digest(&alias)),
            alias.len() as u64,
            &dir.path().join("alias-state")
        )
        .is_err()
    );
}
