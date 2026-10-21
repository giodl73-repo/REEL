//! Real independent processes sharing one cache, with isolated producer metadata.
use reel_assets::{admission, registry, versions};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Command, sync::Arc};

fn fixture(root: &Path, shared: &Path) {
    fs::create_dir_all(root.join(".local")).unwrap();
    fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
    fs::write(
        root.join(".local/storage-rust.json"),
        json!({
            "cache_roots":[shared.join("hot")], "coordination_root":shared.join("control"),
            "capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}
        })
        .to_string(),
    )
    .unwrap();
}

#[test]
fn worktree_registration_and_versions_do_not_take_the_shared_capacity_lock() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir(root.join("hot")).unwrap();
    let a = root.join("a");
    let b = root.join("b");
    fixture(&a, &root);
    fixture(&b, &root);
    let cfg = reel_assets::config(&a).unwrap();
    let control = admission::control(&a, &cfg).unwrap();
    let _old_client = admission::lock(&control).unwrap();
    let _other_writer = admission::worktree_lock(&a).unwrap();
    let h = "a".repeat(64);
    let manifest = b.join("manifest.json");
    fs::write(
        &manifest,
        json!({"assets":[{"sha256":h,"bytes":5,
        "provenance":"fixture","rights":"fixture","selection_state":"retained"}]})
        .to_string(),
    )
    .unwrap();
    registry::register(&b, &manifest).unwrap();
    versions::transact(&b, |_| Ok(())).unwrap();
    assert_eq!(reel_assets::selected(&b).unwrap()[&h], 5);
}

#[test]
fn independent_cli_producers_publish_duplicate_and_unique_objects_without_lost_rows() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().canonicalize().unwrap();
    fs::create_dir(root.join("hot")).unwrap();
    for name in ["a", "b"] {
        fixture(&root.join(name), &root);
    }
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|i| {
            let root = root.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let producer = root.join(if i % 2 == 0 { "a" } else { "b" });
                let payload = if i < 4 {
                    b"shared duplicate".to_vec()
                } else {
                    format!("unique {i}").into_bytes()
                };
                let h = format!("{:x}", Sha256::digest(&payload));
                let source = root.join(format!("input-{i}"));
                fs::write(&source, &payload).unwrap();
                let manifest = root.join(format!("manifest-{i}.json"));
                fs::write(
                    &manifest,
                    json!({"assets":[{"sha256":h,"bytes":payload.len(),
                "provenance":"fixture","rights":"fixture","selection_state":"retained"}]})
                    .to_string(),
                )
                .unwrap();
                barrier.wait();
                for (command, path) in [("ingest", source), ("register", manifest)] {
                    let output = Command::new(env!("CARGO_BIN_EXE_reel-assets"))
                        .arg("--root")
                        .arg(&producer)
                        .arg(command)
                        .arg(path)
                        .output()
                        .unwrap();
                    assert!(
                        output.status.success(),
                        "{command}: {}",
                        String::from_utf8_lossy(&output.stderr)
                    );
                }
                reel_assets::verify(
                    &reel_assets::object(&root.join("hot"), &h),
                    &h,
                    payload.len() as u64,
                )
                .unwrap();
                (producer, h, payload.len() as u64)
            })
        })
        .collect();
    for handle in handles {
        let (producer, h, bytes) = handle.join().unwrap();
        assert_eq!(reel_assets::selected(&producer).unwrap()[&h], bytes);
    }
    assert_eq!(
        fs::read_dir(root.join("control/admissions"))
            .unwrap()
            .count(),
        0
    );
    let receipts: usize = ["a", "b"]
        .iter()
        .map(|name| {
            fs::read_to_string(root.join(name).join(".local/cache-promotions.jsonl"))
                .unwrap_or_default()
                .lines()
                .map(|line| {
                    serde_json::from_str::<serde_json::Value>(line).unwrap();
                    1
                })
                .sum::<usize>()
        })
        .sum();
    assert!(receipts >= 5);
}
