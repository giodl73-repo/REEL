#![cfg(feature = "tig-publication")]
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};
use tig_core::Object;

fn fixture(bytes: &[u8]) -> (tempfile::TempDir, Value) {
    let root = tempfile::tempdir().unwrap();
    for name in [
        "artifact",
        "coordination",
        "cache/objects/sha256",
        "reports",
    ] {
        fs::create_dir_all(root.path().join(name)).unwrap();
    }
    let source = root.path().join("source.bin");
    fs::write(&source, bytes).unwrap();
    let profile = json!({"schema":"reel.tig-file-publication.experimental-1",
        "artifact_root":root.path().join("artifact"),"namespace":"fixture",
        "storage":{"path_policy":"configured-local-roots","cache_roots":[root.path().join("cache")],
            "coordination_root":root.path().join("coordination"),
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0,"max_object_bytes":65536}},
        "destination":root.path().join("cache"),
        "reader_limits":{"object_bytes":65536,"total_bytes":1048576,"requests":16,"live_readers":1,"verify_millis":30000},
        "protection":if cfg!(windows) {"prevent-mutation"} else {"cooperative-immutable"},
        "source":source,"request":{"operation":"create-input","owner":"fixture-owner",
            "object":Object::from_bytes(bytes),"required":"process_restart"}});
    (root, profile)
}
fn run(root: &Path, profile: &Value, name: &str) -> std::process::Output {
    let path = root.join(format!("{name}-profile.json"));
    fs::write(&path, serde_json::to_vec(profile).unwrap()).unwrap();
    Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .args([
            "publish-file",
            path.to_str().unwrap(),
            root.join(format!("reports/{name}.json")).to_str().unwrap(),
        ])
        .output()
        .unwrap()
}
fn object_path(root: &Path, profile: &Value) -> std::path::PathBuf {
    reel_assets::object(
        &root.join("cache"),
        profile["request"]["object"]["sha256"].as_str().unwrap(),
    )
}
#[test]
fn file_publication_reopens_and_exact_retry_needs_no_source() {
    let bytes = b"small authored source; no renderer or script";
    let (root, profile) = fixture(bytes);
    let first = run(root.path(), &profile, "first");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(fs::read(object_path(root.path(), &profile)).unwrap(), bytes);
    let receipt: Value =
        serde_json::from_slice(&fs::read(root.path().join("reports/first.json")).unwrap()).unwrap();
    assert_eq!(receipt["request"], profile["request"]);
    assert_eq!(receipt["reopened_verified"], true);
    assert_eq!(receipt["source_may_be_retired"], false);
    assert_eq!(receipt["independent_custody_verified"], false);
    fs::remove_file(root.path().join("source.bin")).unwrap();
    let retry = run(root.path(), &profile, "retry");
    assert!(
        retry.status.success(),
        "{}",
        String::from_utf8_lossy(&retry.stderr)
    );
    let report: Value = serde_json::from_slice(&retry.stdout).unwrap();
    assert_eq!(report["reused_verified_object"], true);
    let private: Value =
        serde_json::from_slice(&fs::read(root.path().join("reports/retry.json")).unwrap()).unwrap();
    assert_eq!(private["source_opened"], false);
    assert!(!String::from_utf8_lossy(&retry.stdout).contains("source.bin"));
}
#[test]
fn receipt_conflict_refuses_before_any_publication_and_preserves_existing_file() {
    let (root, profile) = fixture(b"new bytes");
    fs::write(
        root.path().join("reports/existing.json"),
        b"keep this receipt",
    )
    .unwrap();
    assert!(!run(root.path(), &profile, "existing").status.success());
    assert_eq!(
        fs::read(root.path().join("reports/existing.json")).unwrap(),
        b"keep this receipt"
    );
    assert!(!object_path(root.path(), &profile).exists());
    assert_eq!(
        fs::read_dir(root.path().join("coordination"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn wrong_hash_length_and_missing_roots_never_acknowledge_or_create_a_cache() {
    for mode in [
        "hash",
        "length",
        "missing-root",
        "budget",
        "durability",
        "unknown",
    ] {
        let (root, mut profile) = fixture(b"expected source");
        match mode {
            "hash" => profile["request"]["object"]["sha256"] = json!("f".repeat(64)),
            "length" => profile["request"]["object"]["bytes"] = json!(99),
            "missing-root" => profile["destination"] = json!(root.path().join("not-created")),
            "budget" => profile["storage"]["capacity"]["max_object_bytes"] = json!(1),
            "durability" => profile["request"]["required"] = json!("provider_durable"),
            _ => profile["unknown"] = json!(true),
        }
        let output = run(root.path(), &profile, mode);
        assert!(!output.status.success(), "mode {mode}");
        assert!(!root.path().join(format!("reports/{mode}.json")).exists());
        assert!(!object_path(root.path(), &profile).exists());
        assert!(!root.path().join("not-created").exists());
        assert!(
            !String::from_utf8_lossy(&output.stderr)
                .contains(&root.path().to_string_lossy().to_string())
        );
    }
}
#[test]
fn acknowledged_missing_bytes_and_owner_conflicts_refuse_without_repair() {
    let (root, profile) = fixture(b"retain the exact subject");
    assert!(run(root.path(), &profile, "first").status.success());
    let mut foreign = profile.clone();
    foreign["request"]["owner"] = json!("other-owner");
    assert!(!run(root.path(), &foreign, "foreign").status.success());
    fs::remove_file(object_path(root.path(), &profile)).unwrap();
    assert!(!run(root.path(), &profile, "missing").status.success());
    assert!(!object_path(root.path(), &profile).exists());
    assert_eq!(
        fs::read(root.path().join("source.bin")).unwrap(),
        b"retain the exact subject"
    );
}
#[test]
fn managed_directory_receipt_refuses_and_unrelated_offline_root_does_not_block_publish() {
    let (root, mut profile) = fixture(b"explicit destination only");
    let path = root.path().join("profile.json");
    fs::write(&path, serde_json::to_vec(&profile).unwrap()).unwrap();
    let refused = Command::new(env!("CARGO_BIN_EXE_reel-tig-job"))
        .args([
            "publish-file",
            path.to_str().unwrap(),
            root.path().join("cache/receipt.json").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(!object_path(root.path(), &profile).exists());
    profile["storage"]["cache_roots"]
        .as_array_mut()
        .unwrap()
        .push(json!(root.path().join("offline-place")));
    assert!(run(root.path(), &profile, "available").status.success());
    assert!(!root.path().join("offline-place").exists());
}
