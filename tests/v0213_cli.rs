use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde_json::Value;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

const SERIES: &str = "manifests/templates/episodic-series.yaml";

#[test]
fn independent_records_preserve_disagreement_and_explicit_resolution() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("comparison.mp4");
    fs::write(&target, b"synthetic exact review target").unwrap();
    let target_hash = sha256(&fs::read(&target).unwrap());

    let finding_a = dir.path().join("finding-a.yaml");
    let finding_b = dir.path().join("finding-b.yaml");
    write_finding(
        &finding_a,
        "finding-a",
        "reviewer-a",
        "selection",
        Some("B"),
        "private reason from reviewer A",
        "advisory",
        &[],
        false,
    );
    write_finding(
        &finding_b,
        "finding-b",
        "reviewer-b",
        "selection",
        Some("C"),
        "private reason from reviewer B",
        "advisory",
        &[],
        false,
    );
    let record_a = dir.path().join("record-a.json");
    let record_b = dir.path().join("record-b.json");
    record(&target, &finding_a, &record_a);
    record(&target, &finding_b, &record_b);
    let record_a_hash = sha256(&fs::read(&record_a).unwrap());
    let record_b_hash = sha256(&fs::read(&record_b).unwrap());
    let a: Value = serde_json::from_slice(&fs::read(&record_a).unwrap()).unwrap();
    assert_eq!(a["target_sha256"], target_hash);
    assert_eq!(a["claims"]["approval"], false);

    let index = dir.path().join("review-index.yaml");
    write_index(
        &index,
        &target_hash,
        &[(&record_a, &record_a_hash), (&record_b, &record_b_hash)],
    );
    let disagreement = queue(&index);
    assert_eq!(
        disagreement["decision_status_by_episode"]["S1E01"],
        "disagreement"
    );
    assert!(
        disagreement["decision_release_gates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|episode| episode == "S1E01")
    );
    assert!(
        disagreement["release_blocked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|episode| episode == "S1E01")
    );

    let resolution_finding = dir.path().join("resolution.yaml");
    write_finding(
        &resolution_finding,
        "resolution-final",
        "final-authority",
        "resolution",
        Some("B"),
        "private final-authority resolution reason",
        "final",
        &[record_a.clone(), record_b.clone()],
        false,
    );
    let resolution = dir.path().join("resolution.json");
    record(&target, &resolution_finding, &resolution);
    let resolution_hash = sha256(&fs::read(&resolution).unwrap());
    assert_eq!(sha256(&fs::read(&record_a).unwrap()), record_a_hash);
    assert_eq!(sha256(&fs::read(&record_b).unwrap()), record_b_hash);
    write_index(
        &index,
        &target_hash,
        &[
            (&record_a, &record_a_hash),
            (&record_b, &record_b_hash),
            (&resolution, &resolution_hash),
        ],
    );
    let resolved = queue(&index);
    assert_eq!(resolved["decision_status_by_episode"]["S1E01"], "resolved");
    assert!(
        resolved["explicit_resolutions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|episode| episode == "S1E01")
    );
    assert!(
        resolved["decision_release_gates"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        resolved["release_blocked"]
            .as_array()
            .unwrap()
            .iter()
            .any(|episode| episode == "S1E01")
    );
    let queue_json = serde_json::to_string(&resolved).unwrap();
    assert!(!queue_json.contains("private reason"));
    assert!(!queue_json.contains("final-authority resolution reason"));

    let overwrite = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("review-record")
        .arg(&target)
        .arg(&finding_a)
        .arg("--output")
        .arg(&record_a)
        .output()
        .unwrap();
    assert!(!overwrite.status.success());
    assert!(String::from_utf8_lossy(&overwrite.stderr).contains("refusing to overwrite"));

    let false_approval = dir.path().join("false-approval.yaml");
    write_finding(
        &false_approval,
        "false-approval",
        "reviewer-name-only",
        "selection",
        Some("B"),
        "a name is not approval",
        "advisory",
        &[],
        true,
    );
    let rejected = dir.path().join("false-approval.json");
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("review-record")
        .arg(&target)
        .arg(&false_approval)
        .arg("--output")
        .arg(&rejected)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot claim"));
    assert!(!rejected.exists());
}

fn record(target: &Path, finding: &Path, output: &Path) {
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("review-record")
        .arg(target)
        .arg(finding)
        .arg("--output")
        .arg(output)
        .args(["--format", "json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[allow(clippy::too_many_arguments)]
fn write_finding(
    path: &Path,
    record_id: &str,
    reviewer: &str,
    kind: &str,
    selected: Option<&str>,
    reason: &str,
    authority: &str,
    cites: &[PathBuf],
    approval: bool,
) {
    let selected = selected
        .map(|value| format!("'{value}'"))
        .unwrap_or_else(|| "null".to_string());
    let citations = if cites.is_empty() {
        "[]".to_string()
    } else {
        format!(
            "[{}]",
            cites
                .iter()
                .map(|path| format!("'{}'", yaml_path(path)))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    fs::write(
        path,
        format!(
            "schema: reel.review-finding.v0.1\nrecord_id: {record_id}\nreviewer_key: {reviewer}\ntarget_kind: video\nkind: {kind}\nselected_option: {selected}\nreason: '{reason}'\ntimestamp: '2026-08-06T15:00:00Z'\nscope: S1E01\nauthority: {authority}\ncites: {citations}\nclaims:\n  authenticated: false\n  signed: false\n  consent: false\n  approval: {approval}\n"
        ),
    )
    .unwrap();
}

fn write_index(path: &Path, target_hash: &str, records: &[(&PathBuf, &String)]) {
    let mut yaml = format!(
        "schema: reel.review-index.v0.1\nseries_sha256: {}\nepisodes:\n  - episode_id: S1E01\n    target_sha256: {target_hash}\n    required_reviewers: [reviewer-a, reviewer-b]\n    records:\n",
        sha256(&fs::read(SERIES).unwrap())
    );
    for (record, hash) in records {
        writeln!(
            &mut yaml,
            "      - path: '{}'\n        sha256: {hash}",
            yaml_path(record)
        )
        .unwrap();
    }
    fs::write(path, yaml).unwrap();
}

fn queue(index: &Path) -> Value {
    let result = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["series-review-queue", SERIES])
        .arg("--decision-index")
        .arg(index)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}

fn yaml_path(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "/")
        .replace('\'', "''")
}

fn sha256(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(&mut output, "{byte:02x}").unwrap();
    }
    output
}
