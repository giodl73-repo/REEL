use git_asset_core::tree::NodeStore;
use git_asset_store::metadata::{Checkpoint, Limits, MetadataReader, MetadataStore};
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn bounds(nodes: u64) -> Limits {
    Limits {
        nodes,
        bytes: 4 * 1024 * 1024,
        database_bytes: 16 * 1024 * 1024,
        min_free_bytes: 65536,
    }
}
fn source(root: &Path) -> Checkpoint {
    let mut store = MetadataStore::create(root, bounds(100)).unwrap();
    for n in 0..24 {
        store
            .put(format!("synthetic-object-{n}").as_bytes())
            .unwrap();
    }
    store.finish().unwrap()
}
fn write(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec(value).unwrap()).unwrap();
}
fn profile(a: &Path, b: &Path, minimum: &Checkpoint) -> Value {
    json!({"schema":"tig.metadata-provider-copy-profile.experimental-1","source_root":a,
        "source_minimum":minimum,"target_root":b,"target_limits":bounds(200),
        "budget":{"nodes":100,"bytes":1048576,"millis":60000}})
}
fn run(args: &[&Path], action: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_git-asset-metadata"))
        .arg(action)
        .args(args)
        .output()
        .unwrap()
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn explicit_plan_apply_and_fresh_process_verification_preserve_source() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let minimum = source(&a);
    let original = fs::read(a.join("metadata.json")).unwrap();
    let p = root.path().join("profile.json");
    let plan = root.path().join("plan.json");
    let report = root.path().join("report.json");
    let verified = root.path().join("verified.json");
    write(&p, &profile(&a, &b, &minimum));
    let summary = success(run(&[&p, &plan], "plan"));
    assert_eq!(summary["dry_run"], true);
    assert!(!b.exists());
    let summary = success(run(&[&p, &plan, &report], "apply"));
    assert_eq!(summary["copy_complete"], true);
    assert_eq!(summary["source_journal_accessed"], false);
    assert_eq!(summary["provider_selected"], false);
    assert_eq!(fs::read(a.join("metadata.json")).unwrap(), original);
    assert_eq!(
        MetadataReader::open(&a, &minimum).unwrap().checkpoint(),
        &minimum
    );
    // Verification needs the admitted copy observation and target, not source access.
    fs::rename(&a, root.path().join("source-offline")).unwrap();
    let summary = success(run(&[&report, &verified], "verify"));
    assert_eq!(summary["target_matches_admitted_copy_observation"], true);
    assert!(!b.join("journal").exists());
    assert!(
        !run(&[&p, &plan, &root.path().join("again.json")], "apply")
            .status
            .success()
    );
}

#[test]
fn changed_profile_or_source_refuses_before_target_creation() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let minimum = source(&a);
    let p = root.path().join("profile.json");
    let plan = root.path().join("plan.json");
    let value = profile(&a, &b, &minimum);
    write(&p, &value);
    success(run(&[&p, &plan], "plan"));
    let mut altered = value.clone();
    altered["budget"]["nodes"] = json!(101);
    write(&p, &altered);
    assert!(
        !run(&[&p, &plan, &root.path().join("altered.json")], "apply")
            .status
            .success()
    );
    assert!(!b.exists());
    write(&p, &value);
    let mut writer = MetadataStore::open(&a, &minimum).unwrap();
    writer.put(b"new-source-node").unwrap();
    writer.finish().unwrap();
    drop(writer);
    assert!(
        !run(&[&p, &plan, &root.path().join("stale.json")], "apply")
            .status
            .success()
    );
    assert!(!b.exists());
}

#[test]
fn existing_targets_and_provider_namespace_reports_are_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let minimum = source(&a);
    let p = root.path().join("profile.json");
    write(&p, &profile(&a, &b, &minimum));
    assert!(!run(&[&p, &a.join("report.json")], "plan").status.success());
    assert!(!a.join("report.json").exists());
    let occupied = root.path().join("occupied.json");
    fs::write(&occupied, b"preserve").unwrap();
    assert!(!run(&[&p, &occupied], "plan").status.success());
    assert_eq!(fs::read(&occupied).unwrap(), b"preserve");
    fs::create_dir(&b).unwrap();
    fs::write(b.join("unrelated"), b"preserve").unwrap();
    assert!(
        !run(&[&p, &root.path().join("plan.json")], "plan")
            .status
            .success()
    );
    assert_eq!(fs::read(b.join("unrelated")).unwrap(), b"preserve");
}

#[test]
fn corruption_blocks_apply_and_unknown_profile_fields_are_not_ignored() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    let minimum = source(&a);
    let p = root.path().join("profile.json");
    let plan = root.path().join("plan.json");
    let mut value = profile(&a, &b, &minimum);
    value["unexpected"] = json!(true);
    write(&p, &value);
    assert!(!run(&[&p, &plan], "plan").status.success());
    assert!(!b.exists());
    value.as_object_mut().unwrap().remove("unexpected");
    write(&p, &value);
    success(run(&[&p, &plan], "plan"));
    let connection = rusqlite::Connection::open(a.join("nodes.sqlite")).unwrap();
    connection
        .execute(
            "UPDATE nodes SET body=?1 WHERE oid=(SELECT min(oid) FROM nodes)",
            [b"bad".as_slice()],
        )
        .unwrap();
    drop(connection);
    let output = run(&[&p, &plan, &root.path().join("report.json")], "apply");
    assert!(!output.status.success());
    let public = String::from_utf8_lossy(&output.stderr);
    assert!(!public.contains(&a.to_string_lossy().to_string()));
    assert!(!root.path().join("report.json").exists());
}
