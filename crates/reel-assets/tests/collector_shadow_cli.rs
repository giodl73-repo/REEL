//! Opt-in combined protection through the existing binary; no live cache paths.
use reel_assets::{
    inventory_evidence::sidecar::file_store::{FileStore, Limits},
    versions,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    profile: PathBuf,
    subject: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        for name in [".local", "registry", "hot", "warm"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        let hot = root.join("hot");
        for name in ["staging", "retained"] {
            fs::create_dir(hot.join(name)).unwrap();
        }
        fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        fs::write(root.join(".local/storage-rust.json"),json!({"path_policy":"configured-local-roots","cache_roots":[hot,root.join("warm")],
            "archive_roots":[root.join("UNAVAILABLE_ARCHIVE")],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
        fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"PRIVATE_PROJECT_ID",
            "storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":100,"lfs_namespaces":[]}).to_string()).unwrap();
        let data = b"fixture previously used";
        let subject = hash(data);
        for cache in [&hot, &root.join("warm")] {
            let path = reel_assets::object(cache, &subject);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
        fs::write(
            root.join("registry/objects.tsv"),
            format!("{subject}\t{}\n", data.len()),
        )
        .unwrap();
        let mut state = versions::State {
            coverage_complete: true,
            ..versions::State::default()
        };
        state
            .register(versions::Version {
                logical_id: "old".into(),
                version_id: "v1".into(),
                manifest_sha256: subject.clone(),
                payloads: BTreeSet::from([subject.clone()]),
                dependencies: vec![],
                supersedes: None,
                deprecated: false,
                dependency_contract: None,
            })
            .unwrap();
        fs::write(
            root.join("registry/versions.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        // Legitimate partial staging intent, deliberately not verified custody.
        let writer =
            FileStore::begin(&hot, &hot.join("staging"), "job", Limits::default()).unwrap();
        fs::write(
            writer.staging_path().join("objects").join(&subject),
            b"partial",
        )
        .unwrap();
        drop(writer);
        let census = reel_assets::accounting::report(&root).unwrap();
        let raw = serde_json::to_vec(&census).unwrap();
        fs::write(root.join("baseline.json"), &raw).unwrap();
        fs::write(root.join("policy.json"),json!({"schema":"reel.asset-tier-policy.v1","project_id":"PRIVATE_PROJECT_ID",
            "baseline":{"sha256":hash(&raw),"bytes":raw.len()},"tiers":[{"id":"hot","storage":"active","index":0},
                {"id":"warm","storage":"active","index":1},{"id":"cold","storage":"archive","index":0}],
            "selected_tier":"hot","history_tier":"warm","cold_tier":"cold","history_versions":[{"logical_id":"old","version_id":"v1","manifest_sha256":subject}],
            "tier_pins":{},"max_objects":100,"max_plan_seconds":60,"expires_after_seconds":120,"max_batch_bytes":1048576,"max_batch_objects":100,
            "allow_same_volume_copies":true}).to_string()).unwrap();
        let profile = root.join("private-shadow.json");
        fs::write(&profile,serde_json::to_vec_pretty(&json!({"schema":"reel.collector-shadow-job.v1",
            "policy":"policy.json","census":"baseline.json","protection":{"schema":"reel.collector-protection-scope.v1",
                "caches":[{"cache":hot,"staging_parent":hot.join("staging"),"retained_parent":hot.join("retained"),"catalogs":[]}],
                "consumers":[{"root":root,"policy":root.join("policy.json")}],"limits":{}}})).unwrap()).unwrap();
        Self {
            _temp: temp,
            root,
            profile,
            subject,
        }
    }
    fn command(&self) -> Command {
        self.named_command("collector-shadow")
    }
    fn named_command(&self, name: &str) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_reel-assets"));
        c.current_dir(&self.root)
            .arg("--root")
            .arg(&self.root)
            .arg(name)
            .arg(&self.profile)
            .env("PATH", "")
            .env_remove("CAMINOS_TRANSFER_CACHE");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000);
        }
        c
    }
    fn run(&self) -> Output {
        self.command().output().unwrap()
    }
    fn edit(&self, change: impl FnOnce(&mut Value)) {
        let mut value: Value = serde_json::from_slice(&fs::read(&self.profile).unwrap()).unwrap();
        change(&mut value);
        fs::write(&self.profile, serde_json::to_vec(&value).unwrap()).unwrap();
    }
    fn verification(&self, remove_hold: bool) {
        if remove_hold {
            fs::remove_file(
                self.root
                    .join("hot/staging/job/objects")
                    .join(&self.subject),
            )
            .unwrap();
        }
        self.edit(|v| {
            v["schema"] = json!("reel.collector-verify-job.v1");
            v["sha256s"] = json!([self.subject]);
            v["max_verify_bytes"] = json!(1048576);
            v["max_seconds"] = json!(60);
        });
    }
    fn verify(&self) -> Output {
        self.named_command("collector-verify").output().unwrap()
    }
}
fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, dir: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files)
            } else {
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn custody_preflight_hashes_named_pairs_without_writes_or_private_export() {
    let f = Fixture::new();
    f.verification(true);
    let before = files(&f.root);
    let out = f.verify();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let summary: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(summary["verified_pairs"], 1);
    assert_eq!(
        summary["verified_payload_bytes"],
        2 * b"fixture previously used".len()
    );
    assert_eq!(summary["same_volume_pairs"], 1);
    assert_eq!(summary["different_volume_pairs"], 0);
    assert_eq!(summary["removal_authorized"], false);
    assert_eq!(summary["reusable_custody_proof"], false);
    assert_eq!(summary["files_removed"], 0);
    assert_eq!(summary["payload_bytes_copied"], 0);
    for (key, value) in summary.as_object().unwrap() {
        assert!(key == "schema" || value.is_number() || value.is_boolean());
    }
    let text = String::from_utf8(out.stdout).unwrap();
    for private in [&f.subject, "PRIVATE_PROJECT_ID", "fixture previously used"] {
        assert!(!text.contains(private));
    }
    assert_eq!(files(&f.root), before);
    assert!(!f.root.join("UNAVAILABLE_ARCHIVE").exists());
}

#[test]
fn custody_preflight_protection_precedes_payload_access() {
    let f = Fixture::new();
    f.verification(false);
    fs::remove_file(reel_assets::object(&f.root.join("warm"), &f.subject)).unwrap();
    let before = files(&f.root);
    let out = f.verify();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("combined protection"));
    assert_eq!(files(&f.root), before);
}

#[test]
fn custody_preflight_rejects_corrupt_or_missing_source_and_retained_copies() {
    for cache in ["hot", "warm"] {
        for missing in [false, true] {
            let f = Fixture::new();
            f.verification(true);
            let path = reel_assets::object(&f.root.join(cache), &f.subject);
            if missing {
                fs::remove_file(path).unwrap();
            } else {
                fs::write(path, vec![b'x'; b"fixture previously used".len()]).unwrap();
            }
            let before = files(&f.root);
            let out = f.verify();
            assert!(!out.status.success());
            assert!(out.stdout.is_empty());
            assert_eq!(files(&f.root), before);
        }
    }
}

#[test]
fn custody_preflight_never_counts_hardlink_alias_as_retained_copy() {
    let f = Fixture::new();
    f.verification(true);
    let source = reel_assets::object(&f.root.join("hot"), &f.subject);
    let retained = reel_assets::object(&f.root.join("warm"), &f.subject);
    fs::remove_file(&retained).unwrap();
    fs::hard_link(source, retained).unwrap();
    let before = files(&f.root);
    let out = f.verify();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8_lossy(&out.stderr).contains("one physical file"));
    assert_eq!(files(&f.root), before);
}

#[test]
fn custody_preflight_requires_explicit_same_volume_exception() {
    let f = Fixture::new();
    f.verification(true);
    let path = f.root.join("policy.json");
    let mut policy: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    policy["allow_same_volume_copies"] = json!(false);
    fs::write(path, serde_json::to_vec(&policy).unwrap()).unwrap();
    let before = files(&f.root);
    let out = f.verify();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(files(&f.root), before);
}

#[test]
fn custody_preflight_rejects_bad_scopes_duplicates_budgets_and_execution_fields() {
    for case in 0..10 {
        let f = Fixture::new();
        f.verification(true);
        f.edit(|v| match case {
            0 => v["sha256s"] = json!([]),
            1 => v["sha256s"] = json!([f.subject, f.subject]),
            2 => v["sha256s"] = json!(["bad"]),
            3 => v["max_verify_bytes"] = json!(1),
            4 => v["max_seconds"] = json!(0),
            5 => v["apply"] = json!(true),
            6 => v["sha256s"] = json!(["e".repeat(64)]),
            7 => v["protection"]["caches"] = json!([]),
            8 => v["protection"]["consumers"] = json!([]),
            _ => v["schema"] = json!("unknown"),
        });
        let before = files(&f.root);
        let out = f.verify();
        assert!(!out.status.success(), "case {case}");
        assert!(out.stdout.is_empty());
        assert_eq!(files(&f.root), before);
    }
}

#[test]
fn custody_preflight_byte_budget_is_checked_before_payload_reads() {
    let f = Fixture::new();
    f.verification(true);
    f.edit(|v| v["max_verify_bytes"] = json!(1));
    fs::remove_file(reel_assets::object(&f.root.join("warm"), &f.subject)).unwrap();
    let out = f.verify();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("byte budget exceeded before payload I/O")
    );
}

#[test]
fn custody_preflight_verifies_only_named_cold_candidate_without_registering_it() {
    let f = Fixture::new();
    f.verification(true);
    let data = b"unregistered prior candidate";
    let subject = hash(data);
    for root in [f.root.join("hot"), f.root.join("UNAVAILABLE_ARCHIVE")] {
        let path = reel_assets::object(&root, &subject);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, data).unwrap();
    }
    let raw = serde_json::to_vec(&reel_assets::accounting::report(&f.root).unwrap()).unwrap();
    fs::write(f.root.join("baseline.json"), &raw).unwrap();
    let policy = f.root.join("policy.json");
    let mut value: Value = serde_json::from_slice(&fs::read(&policy).unwrap()).unwrap();
    value["baseline"] = json!({"sha256":hash(&raw),"bytes":raw.len()});
    fs::write(policy, serde_json::to_vec(&value).unwrap()).unwrap();
    f.edit(|v| v["sha256s"] = json!([subject]));
    let before = files(&f.root);
    let out = f.verify();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["verified_pairs"], 1);
    assert_eq!(value["verified_payload_bytes"], data.len() * 2);
    assert_eq!(files(&f.root), before);
    // No fallback to the warm tier when a named cold copy disappears.
    fs::remove_file(reel_assets::object(
        &f.root.join("UNAVAILABLE_ARCHIVE"),
        &subject,
    ))
    .unwrap();
    let out = f.verify();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
#[test]
fn actual_reference_planner_shadow_adds_a_hold_without_writes_or_private_output() {
    let f = Fixture::new();
    let before = files(&f.root);
    let out = f.run();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["held_proposals"], 1);
    assert_eq!(report["objects"], 1);
    assert_eq!(report["reference_parity"], false);
    assert_eq!(report["dry_run"], true);
    assert_eq!(report["removal_authorized"], false);
    assert_eq!(report["payload_bytes_copied"], 0);
    assert_eq!(report["declared_scope_only"], true);
    for (key, value) in report.as_object().unwrap() {
        assert!(key == "schema" || value.is_number() || value.is_boolean());
    }
    assert!(!report.to_string().contains("PRIVATE_PROJECT_ID"));
    assert!(!report.to_string().contains(&f.subject));
    assert!(before == files(&f.root));
    assert!(!f.root.join("UNAVAILABLE_ARCHIVE").exists());
    assert!(!f.root.join("WRONG_UNUSED_TRANSFER").exists());
}
#[test]
fn broken_protection_or_unknown_coverage_fails_without_a_partial_report() {
    for case in ["session", "coverage", "consumer", "scope", "budget"] {
        let f = Fixture::new();
        match case {
            "session" => {
                fs::write(f.root.join("hot/staging/job/session.json"), b"partial").unwrap()
            }
            "coverage" => {
                let path = f.root.join("registry/versions.json");
                let mut state: versions::State =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                state.coverage_complete = false;
                fs::write(path, serde_json::to_vec(&state).unwrap()).unwrap();
            }
            "consumer" => f.edit(|v| v["protection"]["consumers"] = json!([])),
            "scope" => f.edit(|v| {
                v["protection"]["caches"][0]["retained_parent"] =
                    v["protection"]["caches"][0]["staging_parent"].clone()
            }),
            _ => f.edit(|v| v["protection"]["limits"]["data_bytes"] = json!(1)),
        }
        let before = files(&f.root);
        let out = f.run();
        assert!(!out.status.success(), "{case}");
        assert!(out.stdout.is_empty());
        assert!(before == files(&f.root));
    }
}
#[test]
fn no_apply_or_execution_like_profile_fields_are_supported() {
    let f = Fixture::new();
    assert!(
        !f.command()
            .arg("--apply")
            .output()
            .unwrap()
            .status
            .success()
    );
    f.edit(|v| v["execute"] = json!(true));
    let before = files(&f.root);
    assert!(!f.run().status.success());
    assert!(before == files(&f.root));
}
