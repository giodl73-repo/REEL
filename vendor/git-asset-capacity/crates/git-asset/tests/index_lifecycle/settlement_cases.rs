use super::*;
use git_asset::lifecycle::{
    self,
    settlement::{self, Enrollment as Mode, Plan as SettlementPlan, Stage as SettlementStage},
};
use git_asset_store::source_claims::{Selection, admission::Select};
use serde_json::{Value, json};
use std::io::Read;
fn branch(f: &mut Fixture, source: &str, reference: &str) -> lifecycle::Enrollment {
    f.namespace
        .apply(
            key(&format!("enroll-{source}")),
            Command::Enroll {
                source: key(source),
                owner: key("owner"),
                class: Kind::Branch,
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    lifecycle::Enrollment {
        repository: f.enrollment.repository.clone(),
        authority: f.enrollment.authority.clone(),
        namespace: f.enrollment.namespace.clone(),
        policy: f.enrollment.policy.clone(),
        source: key(source),
        owner: key("owner"),
        class: Kind::Branch,
        ref_name: reference.into(),
        scope: f.enrollment.scope.clone(),
        budget: f.enrollment.budget.clone(),
    }
}
fn adopt_branch(f: &mut Fixture, b: &lifecycle::Enrollment) {
    let mut execution = lifecycle::Execution {
        enrollment: b,
        repository: &f.repository,
        namespace: &mut f.namespace,
        metadata: &mut f.metadata,
        allow_selection: true,
    };
    let plan = execution.plan(None).unwrap();
    execution.apply(&plan).unwrap();
}
fn prepare_draft(f: &mut Fixture) -> Mode {
    let b = branch(f, "main", "refs/heads/main");
    let staged = f.plan();
    f.execution().apply(&staged).unwrap();
    adopt_branch(f, &b);
    Mode::Draft {
        draft: f.enrollment.clone(),
        successor: Box::new(b),
    }
}
fn execution<'a>(f: &'a mut Fixture, mode: &'a Mode) -> settlement::Execution<'a> {
    settlement::Execution {
        enrollment: mode,
        repository: &f.repository,
        namespace: &mut f.namespace,
        metadata: &mut f.metadata,
        allow_withdraw: true,
    }
}
fn profile(f: &Fixture, mode: &Mode) -> PathBuf {
    let index = f.profile();
    let mut value: Value = serde_json::from_slice(&fs::read(index).unwrap()).unwrap();
    value["schema"] = json!("tig.source-settlement-profile.design-1");
    value["enrollment"] = json!(mode);
    value.as_object_mut().unwrap().remove("allow_selection");
    value["allow_withdraw"] = json!(true);
    let path = f.temp.path().join("settle-profile.json");
    fs::write(&path, identity::canonical_json(&value).unwrap()).unwrap();
    path
}
fn call(action: &str, args: &[&Path]) -> std::process::Output {
    Process::new(env!("CARGO_BIN_EXE_git-asset-lifecycle"))
        .args(["settle", action])
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn exact_absence_is_not_a_prefix_neighbor_a_blob_ref_or_an_unavailable_repository() {
    let mut f = Fixture::new();
    git(&f.repo(), &["branch", "feature"]);
    let b = branch(&mut f, "feature", "refs/heads/feature");
    adopt_branch(&mut f, &b);
    let mode = Mode::Branch { branch: b };
    assert!(matches!(execution(&mut f, &mode).plan(), Err(Error::Stale)));
    git(&f.repo(), &["branch", "-D", "feature"]);
    git(&f.repo(), &["branch", "feature/child"]);
    assert_eq!(
        f.repository
            .optional_named_commit("refs/heads/feature")
            .unwrap(),
        None
    );
    let oid =
        String::from_utf8(git(&f.repo(), &["rev-parse", "HEAD:assets/one.bin"]).stdout).unwrap();
    git(&f.repo(), &["update-ref", "refs/tags/blob", oid.trim()]);
    assert!(
        f.repository
            .optional_named_commit("refs/tags/blob")
            .is_err()
    );
    assert!(
        f.repository
            .optional_named_commit("refs/heads/main~1")
            .is_err()
    );
    fs::rename(f.repo(), f.temp.path().join("offline-repository")).unwrap();
    assert!(matches!(execution(&mut f, &mode).plan(), Err(Error::Git)));
    assert!(
        f.namespace
            .source(&key("feature"))
            .unwrap()
            .unwrap()
            .current
            .is_some()
    );
}
#[test]
fn branch_absence_withdraws_only_current_and_keeps_history_and_other_demand() {
    let mut f = Fixture::new();
    git(&f.repo(), &["branch", "feature"]);
    let b = branch(&mut f, "feature", "refs/heads/feature");
    adopt_branch(&mut f, &b);
    git(&f.repo(), &["branch", "-D", "feature"]);
    let mode = Mode::Branch { branch: b };
    let plan = execution(&mut f, &mode).plan().unwrap();
    let status = execution(&mut f, &mode).apply(&plan).unwrap();
    assert!(status.settled);
    assert_eq!(status.observed_evidence_current, Some(true));
    assert!(f.protected(b"base", "history"));
    assert!(!f.protected(b"base", "working"));
    let before = f.namespace.checkpoint();
    let noop = execution(&mut f, &mode).plan().unwrap();
    assert!(noop.no_op);
    execution(&mut f, &mode).apply(&noop).unwrap();
    assert_eq!(f.namespace.checkpoint(), before);
}
#[test]
fn committed_draft_settlement_retains_history_and_old_retry_cannot_withdraw_reactivation() {
    let mut f = Fixture::new();
    let mode = prepare_draft(&mut f);
    let plan = execution(&mut f, &mode).plan().unwrap();
    assert_eq!(
        SettlementPlan::from_json(&plan.to_json().unwrap()).unwrap(),
        plan
    );
    let status = execution(&mut f, &mode).apply(&plan).unwrap();
    assert!(status.settled);
    assert_eq!(status.conditions_current, Some(true));
    assert!(f.protected(b"base", "working"));
    assert!(
        f.namespace
            .source(&key("index"))
            .unwrap()
            .unwrap()
            .current
            .is_none()
    );
    let noop = execution(&mut f, &mode).plan().unwrap();
    assert!(noop.no_op);
    let checkpoint = f.namespace.checkpoint();
    execution(&mut f, &mode).apply(&noop).unwrap();
    assert_eq!(f.namespace.checkpoint(), checkpoint);
    stage(&f.repo(), b"new private draft");
    let next = f.plan();
    f.execution().apply(&next).unwrap();
    let before = f.namespace.source(&key("index")).unwrap().unwrap();
    let replay = execution(&mut f, &mode).apply(&plan).unwrap();
    assert!(replay.historical.unwrap().replayed);
    assert!(!replay.settled);
    assert_eq!(f.namespace.source(&key("index")).unwrap().unwrap(), before);
    assert!(f.protected(b"new private draft", "working"));
}
#[test]
fn staged_bytes_require_adopted_index_and_matching_adopted_successor() {
    let mut f = Fixture::new();
    let mode = prepare_draft(&mut f);
    stage(&f.repo(), b"staged replacement");
    assert!(matches!(
        execution(&mut f, &mode).plan(),
        Err(Error::Pending)
    ));
    let index = f.plan();
    f.execution().apply(&index).unwrap();
    assert!(matches!(
        execution(&mut f, &mode).plan(),
        Err(Error::Pending)
    ));
    commit(&f.repo());
    assert!(matches!(
        execution(&mut f, &mode).plan(),
        Err(Error::Pending)
    ));
    let Mode::Draft { successor, .. } = &mode else {
        unreachable!()
    };
    adopt_branch(&mut f, successor);
    assert!(execution(&mut f, &mode).plan().is_ok());
}
#[test]
fn successor_change_at_settlement_boundary_refuses_or_reports_changed_conditions() {
    for boundary in [SettlementStage::EvidenceVerified, SettlementStage::Settled] {
        let mut f = Fixture::new();
        let mode = prepare_draft(&mut f);
        let plan = execution(&mut f, &mode).plan().unwrap();
        let root = f.temp.path().join("namespace");
        let minimum = f.namespace.checkpoint();
        let metadata_root = f.temp.path().join("metadata");
        let metadata_minimum = f.metadata.checkpoint();
        let result = execution(&mut f, &mode).apply_with_progress(&plan, |stage| {
            if stage == boundary {
                let mut peer = Namespace::open(&root, &minimum).unwrap();
                let metadata = MetadataStore::open(&metadata_root, &metadata_minimum).unwrap();
                let source = peer.source(&key("main")).unwrap().unwrap();
                peer.apply(
                    key("successor-end"),
                    Command::Withdraw {
                        source: key("main"),
                        owner: key("owner"),
                        expected_revision: source.revision,
                    },
                    &metadata.finished_reader().unwrap(),
                    &limits(),
                )
                .unwrap();
            }
        });
        if boundary == SettlementStage::EvidenceVerified {
            assert!(matches!(result, Err(Error::Stale)));
            assert!(
                f.namespace
                    .source(&key("index"))
                    .unwrap()
                    .unwrap()
                    .current
                    .is_some()
            );
        } else {
            let status = result.unwrap();
            assert!(status.settled);
            assert_eq!(status.observed_evidence_current, Some(true));
            assert_eq!(status.conditions_current, Some(false));
        }
    }
}
#[test]
fn branch_reappearance_before_withdrawal_holds_and_afterwards_is_observed() {
    for boundary in [SettlementStage::EvidenceVerified, SettlementStage::Settled] {
        let mut f = Fixture::new();
        git(&f.repo(), &["branch", "feature"]);
        let b = branch(&mut f, "feature", "refs/heads/feature");
        adopt_branch(&mut f, &b);
        git(&f.repo(), &["branch", "-D", "feature"]);
        let mode = Mode::Branch { branch: b };
        let plan = execution(&mut f, &mode).plan().unwrap();
        let repo = f.repo();
        let result = execution(&mut f, &mode).apply_with_progress(&plan, |stage| {
            if stage == boundary {
                git(&repo, &["branch", "feature"]);
            }
        });
        if boundary == SettlementStage::EvidenceVerified {
            assert!(matches!(result, Err(Error::Stale)));
            assert!(
                f.namespace
                    .source(&key("feature"))
                    .unwrap()
                    .unwrap()
                    .current
                    .is_some()
            );
        } else {
            let status = result.unwrap();
            assert!(status.settled);
            assert_eq!(status.observed_evidence_current, Some(false));
        }
    }
}
#[test]
fn missing_successor_metadata_holds_instead_of_treating_it_as_empty() {
    let mut f = Fixture::new();
    let b = branch(&mut f, "main", "refs/heads/main");
    let index = f.plan();
    f.execution().apply(&index).unwrap();
    let branch_plan = lifecycle::Execution {
        enrollment: &b,
        repository: &f.repository,
        namespace: &mut f.namespace,
        metadata: &mut f.metadata,
        allow_selection: false,
    }
    .plan(None)
    .unwrap();
    let commit = f.repository.named_commit(&b.ref_name).unwrap();
    let snapshot = f.repository.commit(commit).unwrap();
    let extracted = f
        .repository
        .extract(
            &snapshot,
            git_asset::source::Scope::new(b.scope.clone()).unwrap(),
        )
        .unwrap();
    let mut nodes = MemoryNodes::default();
    let tree = extracted
        .write_tree(&mut nodes, TreeLimits::default())
        .unwrap();
    assert_eq!(tree.root(), &branch_plan.selection.root);
    f.namespace
        .select(
            &Select {
                prepare: key("unpublished-prepare"),
                adopt: key("unpublished-adopt"),
                source: b.source.clone(),
                owner: b.owner.clone(),
                expected_revision: 1,
                selection: branch_plan.selection,
            },
            &nodes,
            &limits(),
        )
        .unwrap();
    let mode = Mode::Draft {
        draft: f.enrollment.clone(),
        successor: Box::new(b),
    };
    let before = f.namespace.checkpoint();
    assert!(matches!(
        execution(&mut f, &mode).plan(),
        Err(Error::Metadata)
    ));
    assert_eq!(f.namespace.checkpoint(), before);
}
#[test]
fn operator_permissions_canonical_plan_and_offline_receipts_are_independent() {
    let mut f = Fixture::new();
    let mode = prepare_draft(&mut f);
    let profile = profile(&f, &mode);
    let plan = success(call("plan", &[&profile]));
    let path = f.temp.path().join("settle-plan.json");
    fs::write(&path, identity::canonical_json(&plan).unwrap()).unwrap();
    let mut document: Value = serde_json::from_slice(&fs::read(&profile).unwrap()).unwrap();
    document.as_object_mut().unwrap().remove("allow_withdraw");
    fs::write(&profile, identity::canonical_json(&document).unwrap()).unwrap();
    let result = call("apply", &[&profile, &path]);
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["code"],
        "withdrawal_disabled"
    );
    document["allow_withdraw"] = json!(true);
    fs::write(&profile, identity::canonical_json(&document).unwrap()).unwrap();
    let selected = success(call("apply", &[&profile, &path]));
    assert_eq!(selected["settled"], true);
    let absent = f.temp.path().join("must-not-create");
    for field in ["git_executable", "repository_root", "metadata_root"] {
        document[field] = json!(absent.join(field));
    }
    fs::write(&profile, identity::canonical_json(&document).unwrap()).unwrap();
    let status = success(call("status", &[&profile, &path]));
    assert_eq!(status["historical"]["replayed"], true);
    assert_eq!(status["observed_evidence_current"], Value::Null);
    assert!(!absent.exists());
    let mut bytes = identity::canonical_json(&plan).unwrap();
    bytes.push(b'\n');
    assert!(SettlementPlan::from_json(&bytes).is_err());
}
#[test]
fn actual_cli_draft_commit_then_branch_end_demotes_without_deleting_retained_backup() {
    let mut f = Fixture::new();
    git(&f.repo(), &["branch", "feature"]);
    let b = branch(&mut f, "feature", "refs/heads/feature");
    adopt_branch(&mut f, &b);
    let storage = super::store_cases::Storage::new(&f);
    let index_profile = f.profile();
    let staged = success(cli(&[&index_profile, &storage.path], "reconcile"));
    assert_eq!(staged["storage_converged"], true);
    assert!(storage.has(0, b"base"));
    let draft_mode = Mode::Draft {
        draft: f.enrollment.clone(),
        successor: Box::new(b.clone()),
    };
    let draft_profile = profile(&f, &draft_mode);
    let settled = success(call("reconcile", &[&draft_profile, &storage.path]));
    assert_eq!(settled["settlement"]["settled"], true);
    assert_eq!(settled["storage_converged"], true);
    assert!(storage.has(0, b"base"));
    f.namespace
        .apply(
            key("settlement-job-enroll"),
            Command::Enroll {
                source: key("job"),
                owner: key("owner"),
                class: Kind::Job,
            },
            &MemoryNodes::default(),
            &limits(),
        )
        .unwrap();
    let active = f
        .namespace
        .source(&key("feature"))
        .unwrap()
        .unwrap()
        .current
        .unwrap()
        .selection;
    f.namespace
        .select(
            &Select {
                prepare: key("settlement-job-prepare"),
                adopt: key("settlement-job-adopt"),
                source: key("job"),
                owner: key("owner"),
                expected_revision: 1,
                selection: Selection {
                    root: active.root,
                    context: Digest::blob(b"frozen active job"),
                },
            },
            &f.metadata.finished_reader().unwrap(),
            &limits(),
        )
        .unwrap();
    git(&f.repo(), &["branch", "-D", "feature"]);
    let branch_mode = Mode::Branch { branch: b };
    let branch_profile = profile(&f, &branch_mode);
    let ended = success(call("reconcile", &[&branch_profile, &storage.path]));
    assert_eq!(ended["storage_converged"], true);
    assert!(storage.has(0, b"base"));
    assert!(!storage.has(1, b"base"));
    let job = f.namespace.source(&key("job")).unwrap().unwrap();
    f.namespace
        .apply(
            key("settlement-job-end"),
            Command::Withdraw {
                source: key("job"),
                owner: key("owner"),
                expected_revision: job.revision,
            },
            &f.metadata.finished_reader().unwrap(),
            &limits(),
        )
        .unwrap();
    let ended = success(call("reconcile", &[&branch_profile, &storage.path]));
    assert_eq!(ended["storage_converged"], true);
    assert!(!storage.has(0, b"base"));
    assert!(storage.has(1, b"base"));
    assert!(storage.has(2, b"base"));
    let mut restored = Vec::new();
    storage.stores[2]
        .read_object(&Object::from_bytes(b"base"))
        .unwrap()
        .read_to_end(&mut restored)
        .unwrap();
    assert_eq!(restored, b"base");
    let retry = success(call("reconcile", &[&branch_profile, &storage.path]));
    assert_eq!(retry["storage_converged"], true);
    assert_eq!(retry["placement"]["stream_bytes"], 0);
}
#[test]
fn unresolved_stage_alternatives_are_not_eligible_for_commit_settlement() {
    let mut f = Fixture::new();
    let mode = prepare_draft(&mut f);
    git(&f.repo(), &["checkout", "-b", "side"]);
    stage(&f.repo(), b"side");
    commit(&f.repo());
    git(&f.repo(), &["checkout", "main"]);
    stage(&f.repo(), b"main");
    commit(&f.repo());
    assert!(
        !command(&f.repo(), &["-c", "core.hooksPath=NUL", "merge", "side"])
            .output()
            .unwrap()
            .status
            .success()
    );
    let index = f.plan();
    f.execution().apply(&index).unwrap();
    let before = f.namespace.checkpoint();
    assert!(matches!(
        execution(&mut f, &mode).plan(),
        Err(Error::Pending)
    ));
    assert_eq!(f.namespace.checkpoint(), before);
    for bytes in [b"base".as_slice(), b"main", b"side"] {
        assert!(f.protected(bytes, "working"));
    }
}
#[test]
fn settlement_interruption_child() {
    let Ok(profile) = std::env::var("GIT_ASSET_SETTLEMENT_PROFILE") else {
        return;
    };
    let value: Value = serde_json::from_slice(&fs::read(profile).unwrap()).unwrap();
    let mode: Mode = serde_json::from_value(value["enrollment"].clone()).unwrap();
    let repository = Repository::new(
        serde_json::from_value(value["git_executable"].clone()).unwrap(),
        serde_json::from_value(value["repository_root"].clone()).unwrap(),
        Limits::default(),
    )
    .unwrap();
    let mut namespace = Namespace::open(
        &serde_json::from_value::<PathBuf>(value["namespace_root"].clone()).unwrap(),
        &serde_json::from_value(value["namespace_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let mut metadata = MetadataStore::open(
        &serde_json::from_value::<PathBuf>(value["metadata_root"].clone()).unwrap(),
        &serde_json::from_value(value["metadata_minimum"].clone()).unwrap(),
    )
    .unwrap();
    let plan = SettlementPlan::from_json(
        &fs::read(std::env::var("GIT_ASSET_SETTLEMENT_PLAN").unwrap()).unwrap(),
    )
    .unwrap();
    let boundary = if std::env::var("GIT_ASSET_SETTLEMENT_BOUNDARY").unwrap() == "before" {
        SettlementStage::EvidenceVerified
    } else {
        SettlementStage::Settled
    };
    settlement::Execution {
        enrollment: &mode,
        repository: &repository,
        namespace: &mut namespace,
        metadata: &mut metadata,
        allow_withdraw: true,
    }
    .apply_with_progress(&plan, |stage| {
        if stage == boundary {
            std::process::exit(74)
        }
    })
    .unwrap();
    panic!("interruption boundary not reached");
}
#[test]
fn actual_child_exits_resume_exact_settlement_without_losing_protected_history() {
    for boundary in ["before", "after"] {
        let mut f = Fixture::new();
        let mode = prepare_draft(&mut f);
        let plan = execution(&mut f, &mode).plan().unwrap();
        let prior = f.plan().selection.root;
        let profile = profile(&f, &mode);
        let path = f.temp.path().join("settlement-exact.json");
        fs::write(&path, plan.to_json().unwrap()).unwrap();
        let status = Process::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "settlement_cases::settlement_interruption_child",
                "--nocapture",
            ])
            .env("GIT_ASSET_SETTLEMENT_PROFILE", profile)
            .env("GIT_ASSET_SETTLEMENT_PLAN", path)
            .env("GIT_ASSET_SETTLEMENT_BOUNDARY", boundary)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(74));
        let status = execution(&mut f, &mode).apply(&plan).unwrap();
        assert!(status.settled);
        assert!(status.historical.is_some());
        assert!(f.protected(b"base", "working"));
        assert!(
            f.namespace
                .source(&key("index"))
                .unwrap()
                .unwrap()
                .predecessors
                .iter()
                .any(|p| p.selection.root == prior)
        );
    }
}
