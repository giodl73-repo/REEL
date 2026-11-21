use git_asset_core::{Digest, Object};
use git_asset_store::{
    Key,
    source_claims::{Checkpoint, Command as Change, Selection, SourceJournal},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn key(s: &str) -> Key {
    Key::parse(s).unwrap()
}
fn selection(s: &str) -> Selection {
    Selection {
        root: Object::from_bytes(s.as_bytes()),
        context: Digest::blob(b"context"),
    }
}
fn prepare(source: &str, revision: u64, bytes: &str) -> Change {
    Change::Prepare {
        source: key(source),
        owner: key("private-owner"),
        expected_revision: revision,
        selection: selection(bytes),
    }
}
struct Fixture {
    temp: tempfile::TempDir,
    authority: PathBuf,
    minimum: Checkpoint,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let authority = temp.path().join("journal");
        let journal = SourceJournal::create(&authority, key("project"), key("epoch")).unwrap();
        Self {
            temp,
            authority,
            minimum: journal.checkpoint(),
        }
    }
    fn path(&self, name: &str) -> PathBuf {
        self.temp.path().join(name)
    }
    fn write(&self, name: &str, value: &Value) -> PathBuf {
        let path = self.path(name);
        fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
        path
    }
    fn invoke(&self, mode: &str, args: &[&Path]) -> Output {
        let output = Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
            .arg(mode)
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap();
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        for secret in [
            "private-owner",
            "private-main",
            "private-branch",
            "sha256",
            &self.temp.path().display().to_string(),
        ] {
            assert!(!diagnostics.contains(secret), "private data in CLI output");
        }
        output
    }
    fn plan(&self, op: &str, command: Change, name: &str) -> PathBuf {
        let profile = self.write(
            &format!("{name}-profile.json"),
            &json!({
            "schema":"tig.source-transition-profile.experimental-1", "journal_root":self.authority,
            "minimum":self.minimum,"operation":op,"command":command}),
        );
        let plan = self.path(&format!("{name}-plan.json"));
        let db = fs::read(self.authority.join("claims.sqlite")).unwrap();
        let result = self.invoke("plan", &[&profile, &plan]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(fs::read(self.authority.join("claims.sqlite")).unwrap(), db);
        assert_eq!(fs::read_dir(&self.authority).unwrap().count(), 2);
        plan
    }
    fn apply(&self, plan: &Path, name: &str) -> (Value, PathBuf) {
        let intent = self.path(&format!("{name}-intent.json"));
        let report = self.path(&format!("{name}-receipt.json"));
        let result = self.invoke("apply", &[plan, &intent, &report]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        (
            serde_json::from_slice(&fs::read(report).unwrap()).unwrap(),
            intent,
        )
    }
    fn change(&self, op: &str, command: Change, name: &str) -> (Value, PathBuf) {
        self.apply(&self.plan(op, command, name), name)
    }
}
fn reference(result: &Value) -> git_asset_store::source_claims::Reference {
    serde_json::from_value(result["receipt"]["reference"].clone()).unwrap()
}

#[test]
fn source_owner_guidance_refuses_before_any_write() {
    let f = Fixture::new();
    f.change("prepare", prepare("private-main", 0, "A"), "prepare");
    let mut wrong_owner = serde_json::to_value(prepare("private-main", 1, "B")).unwrap();
    wrong_owner["owner"] = json!("other-private-owner");
    let profile = f.write(
        "wrong-owner.json",
        &json!({
            "schema":"tig.source-transition-profile.experimental-1", "journal_root":f.authority,
            "minimum":f.minimum,"operation":"wrong-owner","command":wrong_owner
        }),
    );
    let report = f.path("refused-plan.json");
    let before = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let result = f.invoke("plan", &[&profile, &report]);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("code=source-owner-mismatch;"));
    assert!(!stderr.contains("other-private-owner"));
    assert!(!report.exists());
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
}
fn adopt(source: &str, revision: u64, result: &Value) -> Change {
    Change::Adopt {
        source: key(source),
        owner: key("private-owner"),
        expected_revision: revision,
        prepared: reference(result),
    }
}

#[test]
fn cli_lifecycle_preserves_another_branch_and_recovers_receipts_from_saved_requests() {
    let f = Fixture::new();
    let (a, _) = f.change("prepare-A", prepare("private-main", 0, "A"), "prepare-A");
    let (adopted, _) = f.change("adopt-A", adopt("private-main", 1, &a), "adopt-A");
    assert_eq!(adopted["receipt"]["source_revision"], 2);
    let (branch, _) = f.change("branch-A", prepare("private-branch", 0, "A"), "branch-A");
    f.change(
        "branch-adopt",
        adopt("private-branch", 1, &branch),
        "branch-adopt",
    );
    let (b, _) = f.change("prepare-B", prepare("private-main", 2, "B"), "prepare-B");
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    let pending = journal.source(&key("private-main")).unwrap().unwrap();
    assert_eq!(pending.current.unwrap().selection, selection("A"));
    assert_eq!(pending.pending.unwrap().selection, selection("B"));
    drop(journal);
    f.change("adopt-B", adopt("private-main", 3, &b), "adopt-B");
    let plan = f.plan(
        "retire-A",
        Change::Retire {
            source: key("private-main"),
            owner: key("private-owner"),
            expected_revision: 4,
            predecessor_revision: 2,
        },
        "retire-A",
    );
    let (retired, intent) = f.apply(&plan, "retire-A");
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    let roots = journal.obligations(10).unwrap();
    assert_eq!(roots.len(), 2);
    assert!(!roots.complete_claim_coverage());
    assert!(roots.iter().any(|root| root.selection == selection("A")));
    assert!(roots.iter().any(|root| root.selection == selection("B")));
    drop(journal);
    let recovered = f.path("recovered.json");
    assert!(
        f.invoke("acknowledge", &[&intent, &recovered])
            .status
            .success()
    );
    let recovered: Value = serde_json::from_slice(&fs::read(recovered).unwrap()).unwrap();
    assert_eq!(recovered["reference"], retired["receipt"]["reference"]);
    assert_eq!(recovered["checkpoint"], retired["receipt"]["checkpoint"]);
    let replay = f.apply(&plan, "retire-retry").0;
    assert_eq!(replay["receipt"]["replayed"], true);
    assert_eq!(replay["transition_executed"], false);
    let (c, _) = f.change("prepare-C", prepare("private-main", 5, "C"), "prepare-C");
    f.change(
        "abort-C",
        Change::Abort {
            source: key("private-main"),
            owner: key("private-owner"),
            expected_revision: 6,
            prepared: reference(&c),
        },
        "abort-C",
    );
    f.change(
        "withdraw-branch",
        Change::Withdraw {
            source: key("private-branch"),
            owner: key("private-owner"),
            expected_revision: 2,
        },
        "withdraw-branch",
    );
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    assert_eq!(
        journal
            .source(&key("private-main"))
            .unwrap()
            .unwrap()
            .current
            .unwrap()
            .selection,
        selection("B")
    );
    assert_eq!(
        journal
            .source(&key("private-branch"))
            .unwrap()
            .unwrap()
            .predecessors[0]
            .selection,
        selection("A")
    );
}

#[test]
fn stale_or_tampered_plans_refuse_before_intent_and_independent_sources_keep_progressing() {
    let f = Fixture::new();
    let plan = f.plan("main-A", prepare("private-main", 0, "A"), "main-A");
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    journal
        .apply(key("other"), prepare("private-branch", 0, "B"))
        .unwrap();
    drop(journal);
    let original: Value = serde_json::from_slice(&fs::read(&plan).unwrap()).unwrap();
    for mode in ["request", "preview", "schema", "eviction"] {
        let mut changed = original.clone();
        match mode {
            "request" => changed["request"]["command"]["owner"] = json!("changed-owner"),
            "preview" => {
                changed["preview"]["disposition"]["after"]["owner"] = json!("changed-owner")
            }
            "schema" => changed["schema"] = json!("unknown"),
            _ => changed["authorizes_eviction"] = json!(true),
        }
        let altered = f.write(&format!("altered-{mode}.json"), &changed);
        let intent = f.path(&format!("{mode}-intent.json"));
        let report = f.path(&format!("{mode}-report.json"));
        assert!(
            !f.invoke("apply", &[&altered, &intent, &report])
                .status
                .success()
        );
        assert!(!intent.exists());
        assert!(!report.exists());
    }
    let (applied, _) = f.apply(&plan, "independent-progress");
    assert_eq!(applied["receipt"]["checkpoint"]["sequence"], 2);
    let stale = f.plan(
        "next",
        Change::Abort {
            source: key("private-main"),
            owner: key("private-owner"),
            expected_revision: 1,
            prepared: reference(&applied),
        },
        "abort-plan",
    );
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    journal
        .apply(key("adopt-winner"), adopt("private-main", 1, &applied))
        .unwrap();
    drop(journal);
    let intent = f.path("stale-intent.json");
    let report = f.path("stale-report.json");
    let before = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let refused = f.invoke("apply", &[&stale, &intent, &report]);
    assert!(!refused.status.success());
    assert!(refused.stdout.is_empty());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("code=source-stale;"));
    assert!(!intent.exists());
    assert!(!report.exists());
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
}

#[test]
fn publication_refusals_do_not_overwrite_outputs_or_mutate_journal() {
    let f = Fixture::new();
    let plan = f.plan("op", prepare("private-main", 0, "A"), "op");
    let before = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let existing = f.path("existing.json");
    fs::write(&existing, b"keep").unwrap();
    let new = f.path("new.json");
    assert!(
        !f.invoke("apply", &[&plan, &existing, &new])
            .status
            .success()
    );
    assert!(
        !f.invoke("apply", &[&plan, &new, &existing])
            .status
            .success()
    );
    assert!(!f.invoke("apply", &[&plan, &new, &new]).status.success());
    assert!(
        !f.invoke("apply", &[&plan, &f.authority.join("inside.json"), &new])
            .status
            .success()
    );
    assert_eq!(fs::read(&existing).unwrap(), b"keep");
    assert!(!new.exists());
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
    assert_eq!(fs::read_dir(&f.authority).unwrap().count(), 2);
}

#[test]
fn busy_apply_keeps_exact_intent_and_acknowledgment_never_executes_unknown_transition() {
    let f = Fixture::new();
    let plan = f.plan("op", prepare("private-main", 0, "A"), "op");
    let before = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let blocker = rusqlite::Connection::open(f.authority.join("claims.sqlite")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let intent = f.path("busy-intent.json");
    let report = f.path("busy-report.json");
    assert!(
        !f.invoke("apply", &[&plan, &intent, &report])
            .status
            .success()
    );
    assert!(intent.exists());
    assert!(!report.exists());
    blocker.execute_batch("ROLLBACK").unwrap();
    drop(blocker);
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
    let unknown = f.path("unknown.json");
    assert!(
        !f.invoke("acknowledge", &[&intent, &unknown])
            .status
            .success()
    );
    assert!(!unknown.exists());
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
    let explicit = f.apply(&plan, "explicit-retry").0;
    assert_eq!(explicit["receipt"]["source_revision"], 1);
}

#[test]
fn committed_transition_survives_receipt_publication_failure_and_saved_request_recovers_it() {
    let f = Fixture::new();
    let plan = f.plan("op", prepare("private-main", 0, "A"), "op");
    let intent = f.path("saved-intent.json");
    // NTFS/ext4 reject a 300-character component at final hard-link publication;
    // its trusted parent exists and intent publication still succeeds.
    let impossible = f.path(&"x".repeat(300));
    assert!(
        !f.invoke("apply", &[&plan, &intent, &impossible])
            .status
            .success()
    );
    assert!(intent.exists());
    assert!(!impossible.exists());
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    let source = journal.source(&key("private-main")).unwrap().unwrap();
    assert_eq!(source.revision, 1);
    assert_eq!(source.pending.unwrap().selection, selection("A"));
    drop(journal);
    let before = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let recovered = f.path("recovered.json");
    assert!(
        f.invoke("acknowledge", &[&intent, &recovered])
            .status
            .success()
    );
    let recovered: Value = serde_json::from_slice(&fs::read(recovered).unwrap()).unwrap();
    assert_eq!(recovered["source_revision"], 1);
    assert_eq!(recovered["replayed"], true);
    assert_eq!(fs::read(f.authority.join("claims.sqlite")).unwrap(), before);
    assert!(!fs::read_dir(f.temp.path()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".git-asset-observation-")
    }));
}

#[test]
fn apply_uses_observed_plan_checkpoint_instead_of_weaker_original_floor() {
    let f = Fixture::new();
    let initial = fs::read(f.authority.join("claims.sqlite")).unwrap();
    let mut journal = SourceJournal::open(&f.authority, &f.minimum).unwrap();
    journal
        .apply(key("other"), prepare("private-branch", 0, "B"))
        .unwrap();
    drop(journal);
    let plan = f.plan("op", prepare("private-main", 0, "A"), "op");
    fs::write(f.authority.join("claims.sqlite"), &initial).unwrap();
    let intent = f.path("rollback-intent.json");
    let report = f.path("rollback-report.json");
    assert!(
        !f.invoke("apply", &[&plan, &intent, &report])
            .status
            .success()
    );
    assert!(!intent.exists());
    assert!(!report.exists());
    assert_eq!(
        fs::read(f.authority.join("claims.sqlite")).unwrap(),
        initial
    );
}
