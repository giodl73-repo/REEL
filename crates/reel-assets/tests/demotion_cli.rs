//! Exercise the actual executable stack/argv boundary, not a library test thread.
use reel_assets::{accounting, versions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path, process::Command};
fn write(root: &Path, name: &str, v: &Value) {
    fs::write(root.join(name), serde_json::to_vec(v).unwrap()).unwrap();
}
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_reel-assets"))
        .current_dir(root)
        .arg("--root")
        .arg(root)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn executable_verifies_on_its_real_main_stack_and_applies_only_on_supported_platforms() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for name in [".local", "registry", "hot", "history"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let payload = b"synthetic prior selected CLI payload";
    let hash = format!("{:x}", Sha256::digest(payload));
    for tier in ["hot", "history"] {
        let p = reel_assets::object(&root.join(tier), &hash);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, payload).unwrap();
    }
    write(
        &root,
        ".local/storage-rust.json",
        &json!({"cache_roots":[root.join("hot"),root.join("history")],"archive_roots":[root.join("cold-offline")],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}),
    );
    write(
        &root,
        "asset-project.json",
        &json!({"schema":"reel.asset-project.v1","project_id":"cli-fixture","storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":100,"lfs_namespaces":[]}),
    );
    write(&root, "catalog.json", &json!({"videos":[]}));
    fs::write(
        root.join("registry/objects.tsv"),
        format!("{hash}\t{}\n", payload.len()),
    )
    .unwrap();
    let mut state = versions::State::default();
    state
        .register(versions::Version {
            logical_id: "prior".into(),
            version_id: "v1".into(),
            manifest_sha256: hash.clone(),
            payloads: BTreeSet::from([hash.clone()]),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
        })
        .unwrap();
    write(
        &root,
        "registry/versions.json",
        &serde_json::to_value(state).unwrap(),
    );
    let mut census = accounting::report(&root).unwrap();
    // Model distinct tier volumes in the advisory baseline; the actual file
    // test intentionally opts into distinct copies on one synthetic volume.
    for (n, tier) in census["tiers"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        tier["volume_id"] = json!(format!("synthetic-{n}"));
    }
    let raw = serde_json::to_vec(&census).unwrap();
    fs::write(root.join("baseline.json"), &raw).unwrap();
    write(
        &root,
        "policy.json",
        &json!({"schema":"reel.asset-tier-policy.v1","project_id":"cli-fixture","baseline":{"sha256":format!("{:x}",Sha256::digest(&raw)),"bytes":raw.len()},"tiers":[{"id":"selected","storage":"active","index":0},{"id":"old","storage":"active","index":1},{"id":"cold","storage":"archive","index":0}],"selected_tier":"selected","history_tier":"old","cold_tier":"cold","history_versions":[{"logical_id":"prior","version_id":"v1","manifest_sha256":hash}],"tier_pins":{},"max_objects":100,"max_plan_seconds":60,"expires_after_seconds":600,"max_batch_bytes":1024,"max_batch_objects":10}),
    );
    let out = run(
        &root,
        &["tier-plan", "policy.json", "baseline.json", "plan.json"],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let plan: Value = serde_json::from_slice(&fs::read(root.join("plan.json")).unwrap()).unwrap();
    write(
        &root,
        "request.json",
        &json!({"schema":"reel.asset-demotion-request.v1","policy":root.join("policy.json"),"plan":root.join("plan.json"),"plan_sha256":plan["plan_sha256"],"consumer_roots":[root],"consumer_scope":"all-managed-cache-consumers","sha256s":[hash],"max_verify_bytes":1024,"max_seconds":60,"allow_same_volume_copies":true}),
    );
    let verify = root.join(".local/verify");
    let apply = root.join(".local/apply");
    let out = run(
        &root,
        &["tier-demote", "request.json", verify.to_str().unwrap()],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["apply"], false);
    assert!(reel_assets::object(&root.join("hot"), &hash).exists());
    let out = run(
        &root,
        &[
            "tier-demote",
            "request.json",
            apply.to_str().unwrap(),
            "--apply",
        ],
    );
    assert_eq!(
        out.status.success(),
        cfg!(windows),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        reel_assets::object(&root.join("hot"), &hash).exists(),
        !cfg!(windows)
    );
    assert_eq!(
        fs::read(reel_assets::object(&root.join("history"), &hash)).unwrap(),
        payload
    );
    assert!(!root.join("cold-offline").exists());
}
