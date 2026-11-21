use git_asset_core::{Digest, Object, identity};
use git_asset_store::{
    AckLevel, Key, Request,
    ledger::{CapacityLedger, Checkpoint},
    local::LocalStore,
    local_capacity::LocalCapacityProbe,
    orchestration::{Basis, Budget, Copy, Place, Plan, PlanRequest, ProtectionScope},
    publish,
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Cursor,
    path::Path,
    process::{Command, Output},
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn digest(s: &str) -> Digest {
    Digest::blob(s.as_bytes())
}
struct Fixture {
    root: tempfile::TempDir,
    request: PlanRequest,
    minimum: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut source = LocalStore::create(&root.path().join("source"), key("source")).unwrap();
        let destination =
            LocalStore::create(&root.path().join("destination"), key("destination")).unwrap();
        let mut ledger = CapacityLedger::create(
            &root.path().join("ledger"),
            key("capacity"),
            key("epoch"),
            0,
        )
        .unwrap();
        let minimum = ledger.checkpoint();
        let mut copies = Vec::new();
        for (index, bytes) in [b"synthetic-cad-model".as_slice(), b"synthetic-report-input"]
            .iter()
            .enumerate()
        {
            let request = Request {
                operation: key(&format!("source-{index}")),
                owner: key("original-owner"),
                object: Object::from_bytes(bytes),
                required: AckLevel::ProcessRestart,
            };
            publish(&mut source, &request, &mut Cursor::new(bytes)).unwrap();
            copies.push(Copy {
                source: request,
                operation: key(&format!("copy-{index}")),
            });
        }
        let request = PlanRequest {
            source: Place::local(&source, 1).unwrap(),
            destination: Place::local(&destination, 1).unwrap(),
            capacity: ledger.identity().clone(),
            owner: key("owner"),
            basis: Basis {
                source: digest("source"),
                policy: digest("policy"),
                claims: digest("claims"),
                authority: digest("authority"),
                protection: ProtectionScope::Unknown {
                    gaps: vec![key("jobs")],
                },
            },
            budget: Budget {
                max_objects: 32,
                max_payload_bytes: 1024,
                max_peak_bytes: 64 * 1024 * 1024,
            },
            copies,
        };
        let plan = Plan::new(request.clone()).unwrap();
        LocalCapacityProbe::bind(&destination, &mut ledger)
            .unwrap()
            .observe(&destination, &mut ledger, key("probe"), key("sample"))
            .unwrap();
        let job = ledger
            .reserve(
                key("job"),
                key("owner"),
                plan.local_peak(&destination).unwrap(),
            )
            .unwrap()
            .reference()
            .clone();
        fs::write(
            root.path().join("request.json"),
            identity::canonical_json(&serde_json::to_value(&request).unwrap()).unwrap(),
        )
        .unwrap();
        let profile = json!({"schema":"tig.storage-profile.design-1","source_root":root.path().join("source"),
            "destination_root":root.path().join("destination"),"ledger_root":root.path().join("ledger"),"minimum":minimum,
            "job":job,"source":request.source,"destination":request.destination,"basis":request.basis});
        write(root.path(), "profile.json", &profile);
        Self {
            root,
            request,
            minimum,
        }
    }
    fn run(&self, action: &str, index: Option<usize>) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_git-asset-storage"));
        command.arg(action);
        if action == "plan" {
            command.arg(self.root.path().join("request.json"));
        } else if action != "describe" {
            command
                .arg(self.root.path().join("profile.json"))
                .arg(self.root.path().join("plan.json"));
            if let Some(index) = index {
                command.arg(index.to_string());
            }
        }
        command.output().unwrap()
    }
    fn make_plan(&self) {
        let output = self.run("plan", None);
        assert!(output.status.success());
        Plan::from_json(&output.stdout).unwrap();
        fs::write(self.root.path().join("plan.json"), output.stdout).unwrap();
    }
    fn sequence(&self) -> u64 {
        CapacityLedger::open(&self.root.path().join("ledger"), &self.minimum)
            .unwrap()
            .checkpoint()
            .sequence()
    }
}
fn write(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn success(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn separate_process_plan_apply_status_retry_and_partial_batch_preserve_source() {
    let f = Fixture::new();
    let before = f.sequence();
    f.make_plan();
    assert_eq!(f.sequence(), before);
    assert_eq!(success(f.run("status", Some(0)))["phase"], "not_started");
    assert_eq!(f.sequence(), before);
    assert!(
        success(f.run("peak", None))["peak_file_data_bytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert_eq!(f.sequence(), before);
    let result = success(f.run("apply", Some(0)));
    assert_eq!(result["source_removed"], false);
    assert_eq!(result["removal_authority"], false);
    assert_eq!(result["phase"], "retained");
    assert_eq!(success(f.run("status", Some(1)))["phase"], "not_started");
    let after = f.sequence();
    assert_eq!(success(f.run("status", Some(0)))["phase"], "retained");
    assert_eq!(success(f.run("resume", Some(0)))["phase"], "retained");
    assert_eq!(f.sequence(), after);
    assert_eq!(success(f.run("apply", Some(1)))["phase"], "retained");
    for copy in &f.request.copies {
        let s = LocalStore::open(&f.root.path().join("source")).unwrap();
        assert!(s.read_object(&copy.source.object).is_ok());
    }
}
#[test]
fn changed_plan_context_same_operation_refuses_in_fresh_process() {
    let f = Fixture::new();
    f.make_plan();
    success(f.run("apply", Some(0)));
    let before = f.sequence();
    let mut changed = f.request.clone();
    changed.basis.policy = digest("changed");
    write(
        f.root.path(),
        "request.json",
        &serde_json::to_value(&changed).unwrap(),
    );
    f.make_plan();
    let mut profile: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("profile.json")).unwrap()).unwrap();
    profile["basis"] = serde_json::to_value(&changed.basis).unwrap();
    write(f.root.path(), "profile.json", &profile);
    let result = f.run("apply", Some(0));
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["code"],
        "operation_conflict"
    );
    assert_eq!(f.sequence(), before);
}
#[test]
fn stale_profile_and_bad_index_refuse_without_initializing_unknown_roots() {
    let f = Fixture::new();
    f.make_plan();
    let before = f.sequence();
    let mut profile: Value =
        serde_json::from_slice(&fs::read(f.root.path().join("profile.json")).unwrap()).unwrap();
    let unknown = f.root.path().join("must-not-create");
    profile["source_root"] = json!(unknown);
    profile["basis"]["claims"] = json!(digest("changed"));
    write(f.root.path(), "profile.json", &profile);
    let result = f.run("apply", Some(0));
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["code"],
        "stale_admission"
    );
    assert!(!unknown.exists());
    assert_eq!(f.sequence(), before);
    fs::remove_file(f.root.path().join("profile.json")).unwrap();
    let result = f.run("apply", Some(32));
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["code"],
        "invalid_index"
    );
}
#[test]
fn corrupt_acknowledged_bytes_report_redacted_error_without_repair() {
    let f = Fixture::new();
    f.make_plan();
    success(f.run("apply", Some(0)));
    let before = f.sequence();
    let path = f
        .root
        .path()
        .join("destination/objects")
        .join(f.request.copies[0].source.object.sha256.as_str());
    fs::write(&path, b"damaged").unwrap();
    let output = f.run("status", Some(0));
    assert!(!output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(!text.contains(&f.root.path().to_string_lossy().to_string()));
    assert_eq!(
        serde_json::from_str::<Value>(&text).unwrap()["code"],
        "operation_failed"
    );
    assert_eq!(fs::read(path).unwrap(), b"damaged");
    assert_eq!(f.sequence(), before);
}
#[test]
fn discovery_has_no_provider_effects_and_no_removal_operation() {
    let output = Command::new(env!("CARGO_BIN_EXE_git-asset-storage"))
        .arg("describe")
        .output()
        .unwrap();
    let result = success(output);
    assert_eq!(result["copy_only"], true);
    assert_eq!(result["removal_authority"], false);
    assert_eq!(result["max_actions"], 32);
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-storage"))
        .arg("delete")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["code"],
        "usage"
    );
}
