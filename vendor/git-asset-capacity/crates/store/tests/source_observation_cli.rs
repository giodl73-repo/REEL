use git_asset_core::{Digest, Object};
use git_asset_store::{
    Key,
    source_claims::{Command as Change, Selection, SourceJournal},
};
use serde_json::{Value, json};
use std::{fs, process::Command};
fn key(value: &str) -> Key {
    Key::parse(value).unwrap()
}

#[test]
fn batch_cli_reports_one_private_snapshot_and_refuses_invalid_or_overwritten_reports() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let authority = root.join("journal");
    let mut journal = SourceJournal::create(&authority, key("project"), key("epoch")).unwrap();
    let minimum = journal.checkpoint();
    journal
        .apply(
            key("private-operation"),
            Change::Prepare {
                source: key("private-source"),
                owner: key("private-owner"),
                expected_revision: 0,
                selection: Selection {
                    root: Object::from_bytes(b"root"),
                    context: Digest::blob(b"context"),
                },
            },
        )
        .unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let before = fs::read(authority.join("claims.sqlite")).unwrap();
    let input = root.join("profile.json");
    let output = root.join("report.json");
    let profile = json!({"schema":"tig.source-batch-observation-profile.experimental-1",
        "journal_root":authority,"minimum":minimum,"sources":["private-source","private-absent"]});
    let invoke = |candidate: &Value, target: &std::path::Path| {
        fs::write(&input, serde_json::to_vec(candidate).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
            .arg("inspect-many")
            .arg(&input)
            .arg(target)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let checked = invoke(&profile, &output);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["requested_sources"], 2);
    assert_eq!(summary["present_sources"], 1);
    assert_eq!(summary["pending_roots"], 1);
    assert_eq!(summary["single_checkpoint"], true);
    let bytes = fs::read(&output).unwrap();
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(report["checkpoint"], serde_json::to_value(latest).unwrap());
    assert_eq!(report["sources"]["private-source"]["revision"], 1);
    assert!(
        report["sources"]
            .as_object()
            .unwrap()
            .contains_key("private-absent")
    );
    assert!(report["sources"]["private-absent"].is_null());
    for field in [
        "process_liveness_verified",
        "recovery_authorized",
        "complete_claim_coverage",
        "authorizes_eviction",
    ] {
        assert_eq!(report[field], false);
        assert_eq!(summary[field], false);
    }
    let stdout = String::from_utf8_lossy(&checked.stdout);
    for private in [
        "private-source",
        "private-absent",
        "private-owner",
        "sha256",
        &root.display().to_string(),
    ] {
        assert!(!stdout.contains(private));
    }
    assert!(!invoke(&profile, &output).status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    let refused = root.join("refused.json");
    for sources in [
        json!([]),
        json!(["private-source", "private-source"]),
        json!((0..65).map(|n| format!("source-{n}")).collect::<Vec<_>>()),
    ] {
        let mut changed = profile.clone();
        changed["sources"] = sources;
        let result = invoke(&changed, &refused);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!refused.exists());
        let stderr = String::from_utf8_lossy(&result.stderr);
        assert!(!stderr.contains("private-source"));
        assert!(!stderr.contains(&root.display().to_string()));
    }
    let mut wrong_schema = profile.clone();
    wrong_schema["schema"] = json!("tig.source-observation-profile.experimental-1");
    assert!(!invoke(&wrong_schema, &refused).status.success());
    let mut extra = profile.clone();
    extra["unknown"] = json!(true);
    assert!(!invoke(&extra, &refused).status.success());
    assert!(
        !invoke(&profile, &authority.join("forbidden.json"))
            .status
            .success()
    );
    assert_eq!(fs::read(authority.join("claims.sqlite")).unwrap(), before);
    assert_eq!(fs::read_dir(authority).unwrap().count(), 2);
    assert!(!refused.exists());
}

#[test]
fn batch_cli_refuses_oversized_history_report_without_truncation_or_mutation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let authority = root.join("journal");
    let mut journal = SourceJournal::create(&authority, key("project"), key("epoch")).unwrap();
    let minimum = journal.checkpoint();
    let sources: Vec<_> = (0..6).map(|n| format!("private-source-{n}")).collect();
    let mut expected = serde_json::Map::new();
    for id in &sources {
        for n in 0..65 {
            let prepared = journal
                .apply(
                    key(&format!("p-{id}-{n}")),
                    Change::Prepare {
                        source: key(id),
                        owner: key("private-owner"),
                        expected_revision: n * 2,
                        selection: Selection {
                            root: Object::from_bytes(format!("root-{n}").as_bytes()),
                            context: Digest::blob(b"context"),
                        },
                    },
                )
                .unwrap();
            journal
                .apply(
                    key(&format!("a-{id}-{n}")),
                    Change::Adopt {
                        source: key(id),
                        owner: key("private-owner"),
                        expected_revision: n * 2 + 1,
                        prepared: prepared.reference,
                    },
                )
                .unwrap();
        }
        expected.insert(
            id.clone(),
            serde_json::to_value(journal.source(&key(id)).unwrap()).unwrap(),
        );
    }
    assert!(serde_json::to_vec(&expected).unwrap().len() > 65_536);
    drop(journal);
    let before = fs::read(authority.join("claims.sqlite")).unwrap();
    let profile = root.join("profile.json");
    let output = root.join("report.json");
    fs::write(
        &profile,
        serde_json::to_vec(
            &json!({"schema":"tig.source-batch-observation-profile.experimental-1",
        "journal_root":authority,"minimum":minimum,"sources":sources}),
        )
        .unwrap(),
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
        .arg("inspect-many")
        .arg(&profile)
        .arg(&output)
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(!output.exists());
    assert_eq!(fs::read(authority.join("claims.sqlite")).unwrap(), before);
    assert_eq!(fs::read_dir(root).unwrap().count(), 2);
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!stderr.contains("private-source"));
    assert!(!stderr.contains(&root.display().to_string()));
}

#[test]
fn acknowledge_cli_recovers_only_exact_recorded_receipts_with_no_mutation_or_disclosure() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let authority = root.join("journal");
    let mut journal = SourceJournal::create(&authority, key("project"), key("epoch")).unwrap();
    let minimum = journal.checkpoint();
    let command = Change::Prepare {
        source: key("private-source"),
        owner: key("private-owner"),
        expected_revision: 0,
        selection: Selection {
            root: Object::from_bytes(b"root"),
            context: Digest::blob(b"context"),
        },
    };
    let original = journal
        .apply(key("private-operation"), command.clone())
        .unwrap();
    drop(journal);
    let before = fs::read(authority.join("claims.sqlite")).unwrap();
    let input = root.join("profile.json");
    let output = root.join("receipt.json");
    let profile = json!({"schema":"tig.source-acknowledgment-profile.experimental-1",
        "journal_root":authority,"minimum":minimum,"operation":"private-operation","command":command});
    let invoke = |profile: &Value, target: &std::path::Path| {
        fs::write(&input, serde_json::to_vec(profile).unwrap()).unwrap();
        Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
            .arg("acknowledge")
            .arg(&input)
            .arg(target)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let result = invoke(&profile, &output);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["recorded"], true);
    assert_eq!(summary["transition_executed"], false);
    let bytes = fs::read(&output).unwrap();
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        report["reference"],
        serde_json::to_value(original.reference).unwrap()
    );
    assert_eq!(
        report["checkpoint"],
        serde_json::to_value(original.checkpoint).unwrap()
    );
    assert_eq!(report["historical_receipt"], true);
    assert_eq!(report["producer_success_verified"], false);
    assert_eq!(report["authorizes_eviction"], false);
    let mut unknown = profile.clone();
    unknown["operation"] = json!("typo-operation");
    let mut changed = profile.clone();
    changed["command"]["owner"] = json!("changed-owner");
    let refused = root.join("refused.json");
    for candidate in [&unknown, &changed] {
        let result = invoke(candidate, &refused);
        assert!(!result.status.success());
        assert!(!refused.exists());
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        for secret in [
            "private-source",
            "private-owner",
            "private-operation",
            "typo-operation",
            "changed-owner",
            &root.display().to_string(),
        ] {
            assert!(!diagnostics.contains(secret));
        }
    }
    assert!(!invoke(&profile, &output).status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert!(
        !invoke(&profile, &authority.join("refused.json"))
            .status
            .success()
    );
    assert_eq!(fs::read(authority.join("claims.sqlite")).unwrap(), before);
    assert_eq!(fs::read_dir(&authority).unwrap().count(), 2);
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
    let stdout = String::from_utf8_lossy(&result.stdout);
    for secret in [
        "private-source",
        "private-owner",
        "private-operation",
        "sha256",
        &root.display().to_string(),
    ] {
        assert!(!stdout.contains(secret));
    }
}

#[test]
fn private_cli_reports_current_revision_without_changing_authority_or_overwriting_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let authority = root.join("journal");
    let mut journal = SourceJournal::create(&authority, key("project"), key("epoch")).unwrap();
    let initial = journal.checkpoint();
    journal
        .apply(
            key("prepare"),
            Change::Prepare {
                source: key("private-source"),
                owner: key("private-owner"),
                expected_revision: 0,
                selection: Selection {
                    root: Object::from_bytes(b"root"),
                    context: Digest::blob(b"context"),
                },
            },
        )
        .unwrap();
    let latest = journal.checkpoint();
    drop(journal);
    let original = fs::read(authority.join("claims.sqlite")).unwrap();
    let profile = json!({"schema":"tig.source-observation-profile.experimental-1","journal_root":authority,
        "minimum":initial,"source":"private-source"});
    let input = root.join("profile.json");
    let output = root.join("observation.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let invoke = |target: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
            .arg("inspect")
            .arg(&input)
            .arg(target)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    let checked = invoke(&output);
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["source_revision"], 1);
    assert_eq!(summary["pending_roots"], 1);
    let stdout = String::from_utf8_lossy(&checked.stdout);
    for private in [
        "private-source",
        "private-owner",
        "sha256",
        &root.display().to_string(),
    ] {
        assert!(!stdout.contains(private));
    }
    let bytes = fs::read(&output).unwrap();
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(report["checkpoint"], serde_json::to_value(latest).unwrap());
    assert_eq!(report["source"]["owner"], "private-owner");
    assert_eq!(report["process_liveness_verified"], false);
    assert_eq!(report["recovery_authorized"], false);
    assert!(!invoke(&output).status.success());
    assert_eq!(fs::read(&output).unwrap(), bytes);
    assert!(
        !invoke(&authority.join("forbidden-report.json"))
            .status
            .success()
    );
    assert_eq!(fs::read(authority.join("claims.sqlite")).unwrap(), original);
    assert_eq!(fs::read_dir(&authority).unwrap().count(), 2);
    assert_eq!(fs::read_dir(root).unwrap().count(), 3);
}

#[test]
fn foreign_and_missing_authorities_refuse_without_creating_report_or_leaking_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let one = SourceJournal::create(&root.join("one"), key("project"), key("epoch")).unwrap();
    let two = SourceJournal::create(&root.join("two"), key("project"), key("epoch")).unwrap();
    let floor = one.checkpoint();
    drop(one);
    drop(two);
    let input = root.join("profile.json");
    let output = root.join("refused.json");
    for authority in [root.join("two"), root.join("missing")] {
        let profile = json!({"schema":"tig.source-observation-profile.experimental-1","journal_root":authority,"minimum":floor,"source":"private-source"});
        fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
            .arg("inspect")
            .arg(&input)
            .arg(&output)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(!output.exists());
        assert!(!root.join("missing").exists());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(&root.display().to_string()));
    }
}

#[test]
fn absent_source_is_a_scoped_null_revision_not_a_new_owner_or_global_absence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let journal =
        SourceJournal::create(&root.join("journal"), key("project"), key("epoch")).unwrap();
    let profile = json!({"schema":"tig.source-observation-profile.experimental-1","journal_root":root.join("journal"),
        "minimum":journal.checkpoint(),"source":"not-enrolled"});
    drop(journal);
    let input = root.join("profile.json");
    let output = root.join("observation.json");
    fs::write(&input, serde_json::to_vec(&profile).unwrap()).unwrap();
    let checked = Command::new(env!("CARGO_BIN_EXE_git-asset-source"))
        .arg("inspect")
        .arg(&input)
        .arg(&output)
        .output()
        .unwrap();
    assert!(checked.status.success());
    let summary: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(summary["source_exists"], false);
    assert!(summary["source_revision"].is_null());
    assert_eq!(summary["complete_claim_coverage"], false);
    assert_eq!(summary["authorizes_eviction"], false);
    assert_eq!(summary["recovery_authorized"], false);
    let report: Value = serde_json::from_slice(&fs::read(output).unwrap()).unwrap();
    assert!(report["source"].is_null());
    assert_eq!(fs::read_dir(root.join("journal")).unwrap().count(), 2);
}
