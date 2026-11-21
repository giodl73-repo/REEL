use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: u64,
    sha256: String,
}
fn checked(path: &Path, hash: &str) -> Vec<u8> {
    let raw = fs::read(path).unwrap();
    assert_eq!(digest(&raw), hash, "source pin: {}", path.display());
    raw
}
fn walk(root: &Path, directory: &Path, result: &mut BTreeSet<String>) {
    for entry in fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        let kind = fs::symlink_metadata(&path).unwrap().file_type();
        assert!(!kind.is_symlink(), "linked source: {}", path.display());
        if kind.is_dir() {
            walk(root, &path, result);
        } else {
            assert!(kind.is_file());
            result.insert(
                path.strip_prefix(root)
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .replace('\\', "/"),
            );
        }
    }
}
fn check_tree(root: &Path, entries: &[Entry], extras: &[&str]) {
    let mut declared = BTreeSet::new();
    for entry in entries {
        assert!(!entry.path.starts_with('/') && !entry.path.contains('\\'));
        assert!(
            entry
                .path
                .split('/')
                .all(|p| !p.is_empty() && p != "." && p != ".." && !p.contains(':'))
        );
        assert!(
            declared.insert(entry.path.clone()),
            "duplicate source: {}",
            entry.path
        );
        let path = root.join(&entry.path);
        let raw = checked(&path, &entry.sha256);
        assert_eq!(
            raw.len() as u64,
            entry.bytes,
            "source length: {}",
            entry.path
        );
    }
    for extra in extras {
        assert!(declared.insert((*extra).into()));
    }
    let mut actual = BTreeSet::new();
    walk(root, root, &mut actual);
    assert_eq!(actual, declared, "unlisted or missing vendored source");
}
fn vendor(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../vendor")
        .join(name)
}

#[test]
fn lifecycle_sdk_preserves_all_247_exact_sources_and_original_manifest() {
    let root = vendor("tig-workflow");
    let raw = checked(
        &root.join("source-pin.json"),
        "52fa04da281f55a494280f461eed7c94ca66336cccd2b6547bf15a1b0f832c20",
    );
    let pin: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        pin["source_manifest_sha256"],
        reel_workflow_native::LIFECYCLE_SOURCE_MANIFEST
    );
    assert_eq!(pin["working_tree"], true);
    let entries: Vec<Entry> = serde_json::from_value(pin["files"].clone()).unwrap();
    assert_eq!(entries.len(), 247);
    check_tree(&root, &entries, &["source-pin.json"]);
}

#[test]
fn original_slot_module_tests_and_license_are_preserved_without_compilation_claim() {
    let root = vendor("reel-workflow-lifecycle");
    let raw = checked(
        &root.join("source-pin.json"),
        "c255d5b592e7aae469b091c1982dd2bdf892bf5d6d258884c79cb9d0165c7c77",
    );
    let pin: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        pin["qualified_combined_source_digest"],
        reel_workflow_native::LIFECYCLE_QUALIFICATION
    );
    let entries: Vec<Entry> = serde_json::from_value(pin["files"].clone()).unwrap();
    assert_eq!(entries.len(), 2);
    checked(
        &root.join("LICENSE"),
        "163a1b56cf59de3caa662b2111a28063e4f810f10d020bc8ee63c34e957e63aa",
    );
    check_tree(&root, &entries, &["source-pin.json", "LICENSE"]);
}

#[test]
fn merged_capacity_sdk_preserves_all_379_sources_manifest_and_upstream_license() {
    let root = vendor("git-asset-capacity");
    let raw = checked(
        &root.join("source-pin.json"),
        "af092e8fd94e5980c317057165ebaf48854f4c536613a60ec64e0f074d92864a",
    );
    let pin: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(
        pin["source_commit"],
        reel_workflow_native::CAPACITY_SOURCE_COMMIT
    );
    assert_eq!(
        pin["source_manifest_sha256"],
        reel_workflow_native::CAPACITY_SOURCE_MANIFEST
    );
    let raw = checked(
        &root.join("qualified-manifest.json"),
        reel_workflow_native::CAPACITY_SOURCE_MANIFEST,
    );
    let entries: Vec<Entry> = serde_json::from_slice(&raw).unwrap();
    assert_eq!(entries.len(), 379);
    checked(
        &root.join("LICENSE"),
        "ea9e81bc2d37f6ea5cadfeb461f68500d8cc29bcc4fdd3053b532129387ce1c8",
    );
    check_tree(
        &root,
        &entries,
        &["source-pin.json", "qualified-manifest.json", "LICENSE"],
    );
}
