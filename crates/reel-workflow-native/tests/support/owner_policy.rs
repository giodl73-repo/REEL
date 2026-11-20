//! Finite automatic owner-history admission, separate from per-job budgets.
use super::*;
use reel_workflow_native::{admission::Prepared, handoff, retention};
use tig_core::tree::{NodeStore, Tree, TreeLimits};
use tig_store::{
    metadata::MetadataStore,
    source_claims::{
        Selection,
        protection::{self, Index, work},
    },
};

fn policy(objects: usize) -> retention::capacity::Policy {
    retention::capacity::Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects,
        logical_bytes: 1024 * 1024 * 1024,
    }
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}
fn success(root: &Path, name: &str, output: &std::process::Output) {
    save(
        root,
        name,
        &json!({"exit_code":output.status.code(), "stdout":String::from_utf8_lossy(&output.stdout), "stderr":String::from_utf8_lossy(&output.stderr)}),
    );
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn legacy_binding_bytes_remain_stable_and_new_policy_is_pinned_before_effects() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let mut profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let old = profile.binding.artifact().unwrap();
    let old_raw = serde_json::to_vec(&profile.binding).unwrap();
    assert!(!String::from_utf8_lossy(&old_raw).contains("retained_owner_policy"));
    let roundtrip: handoff::Binding = serde_json::from_slice(&old_raw).unwrap();
    assert_eq!(roundtrip.artifact().unwrap(), old);
    profile.binding.retained_owner_policy = Some(policy(1024));
    assert_eq!(
        profile.binding.artifact().unwrap_err().code,
        "handoff-binding-schema"
    );
    profile.binding.schema = "reel.workflow-native-handoff-binding.v2".into();
    let selected = profile.binding.artifact().unwrap();
    assert_ne!(selected, old);
    let journal = case.temp.path().join("owner-journal");
    handoff::create(&journal, &fixture.packet, &profile).unwrap();
    let checkpoint = Index::open(&case.profile.index_root, &case.profile.index_minimum)
        .unwrap()
        .checkpoint();
    profile
        .binding
        .retained_owner_policy
        .as_mut()
        .unwrap()
        .objects = 2048;
    assert_eq!(
        handoff::resume(&journal, &profile, None).unwrap_err().code,
        "handoff-ticket-native-binding"
    );
    assert_eq!(
        Index::open(&case.profile.index_root, &checkpoint)
            .unwrap()
            .checkpoint(),
        checkpoint
    );
    assert_eq!(
        reel_workflow::handoff::Journal::open(&journal)
            .unwrap()
            .ticket()
            .native_binding,
        selected
    );
    profile.binding.retained_owner_policy = None;
    assert_eq!(
        profile.binding.artifact().unwrap_err().code,
        "handoff-binding-schema"
    );
    profile.binding.retained_owner_policy = Some(policy(0));
    assert_eq!(
        profile.binding.artifact().unwrap_err().kind,
        ErrorKind::Budget
    );
}

// Only private synthetic metadata in this fixture's own Index. Existing native
// pool/media are neither enumerated nor initialized. Each phase remains <=128.
fn seed_history(case: &AdmissionCase) -> BTreeMap<LogicalPath, Object> {
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &case.profile.metadata_minimum).unwrap();
    let mut index = Index::open(&case.profile.index_root, &case.profile.index_minimum).unwrap();
    let target = retained_target();
    let mut history = Tree::from_root(
        index
            .actor(&mut nodes, &target.actor)
            .unwrap()
            .selection
            .root,
    )
    .entries(&mut nodes, TreeLimits::default())
    .unwrap()
    .0;
    for batch in 0..5 {
        let retained = index.actor(&mut nodes, &target.actor).unwrap();
        let mut changes = BTreeMap::new();
        for slot in batch * 112..(batch + 1) * 112 {
            let object = nodes
                .put(
                    &serde_json::to_vec(
                        &json!({"schema":"reel.synthetic-owner-history.v1","slot":slot}),
                    )
                    .unwrap(),
                )
                .unwrap();
            let path = LogicalPath::parse(&format!("objects/{}", object.sha256.as_str())).unwrap();
            history.insert(path.clone(), object.clone());
            changes.insert(path, Some(object));
        }
        assert_eq!(changes.len(), 112);
        let tree = Tree::build(&history, &mut nodes, TreeLimits::default())
            .unwrap()
            .0;
        let update = protection::Update {
            operation: tig_store::Key::parse(&format!("synthetic-history-{batch}")).unwrap(),
            actor: target.actor.clone(),
            owner: target.owner.clone(),
            expected_revision: retained.revision,
            selection: Selection {
                root: tree.root().clone(),
                context: tig_core::Digest::blob(format!("synthetic-history-{batch}").as_bytes()),
            },
            changes,
            ready: true,
        };
        let reference = work::stage(&mut index, &update, None, &mut nodes).unwrap();
        work::execute(&mut index, &reference, &mut nodes).unwrap();
    }
    nodes.finish().unwrap();
    history
}

#[test]
fn insufficient_known_history_refuses_before_provider_or_journal_admission() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let history = seed_history(&case);
    assert_eq!(history.len(), 560);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let mut profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    profile.binding.schema = "reel.workflow-native-handoff-binding.v2".into();
    profile.binding.retained_owner_policy = Some(policy(512));
    let before = Index::open(&case.profile.index_root, &case.profile.index_minimum)
        .unwrap()
        .checkpoint();
    let journal = case.temp.path().join("refused-owner-journal");
    let error = handoff::create(&journal, &fixture.packet, &profile).unwrap_err();
    assert_eq!(error.code, "retained-owner-policy-known-input-bound");
    assert_eq!(
        Index::open(&case.profile.index_root, &before)
            .unwrap()
            .checkpoint(),
        before
    );
    assert!(!journal.join("00000.json").exists());
    assert_eq!(
        case.protected(&Object::from_bytes(b"synthetic input")),
        vec![case.profile.scope.actor.clone()]
    );
}

#[test]
#[ignore = "Existing declared synthetic native payload only; own Index with 560 metadata history objects, binding-v2 automatic finite policy, retained import, lost-completion reconciliation and historical resume after close; preserves all roots"]
fn automatic_owner_policy_retains_beyond_legacy_job_limit_and_reopens_after_close() {
    automatic_case(false);
}

#[test]
#[ignore = "Existing declared synthetic native payload only; original binding-v2 policy/native retention Prepare interrupted and reconciled by frozen CLI without new work identity; preserves all roots"]
fn interrupted_owner_policy_prepare_recovers_exact_original_work_before_completion() {
    automatic_case(true);
}

fn automatic_case(interrupted: bool) {
    use reel_workflow::handoff::{Journal, State};
    let descriptor =
        std::env::var_os("REEL_NATIVE_INPUT_FIXTURE").expect("REEL_NATIVE_INPUT_FIXTURE");
    let evidence_root = PathBuf::from(
        std::env::var_os("REEL_OWNER_POLICY_EVIDENCE").expect("REEL_OWNER_POLICY_EVIDENCE"),
    );
    let evidence = evidence_root.join(if interrupted {
        "interrupted"
    } else {
        "automatic"
    });
    fs::create_dir(&evidence).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&fs::read(descriptor).unwrap()).unwrap();
    let object: Object = serde_json::from_value(value["object"].clone()).unwrap();
    let native: tig_git::annex::workflow::reader::Profile =
        serde_json::from_value(value["source"].clone()).unwrap();
    let mut fixture = Fixture::with_object(object);
    fixture.temp.disable_cleanup(true);
    let git_bytes = fs::read(&native.pool.git).unwrap();
    fixture.profile.git = Tool {
        path: native.pool.git.clone(),
        sha256: digest(&git_bytes),
        bytes: git_bytes.len() as u64,
    };
    let mut case = AdmissionCase::with_store(&fixture, native.store.clone());
    case.temp.disable_cleanup(true);
    let control = case.source.source.pool.control.clone();
    case.source = reel_workflow_native::NativeProfile {
        snapshot: fixture.profile.clone(),
        source: native,
        annex: serde_json::from_value(value["annex"].clone()).unwrap(),
        movie_verifier: None,
    };
    case.source.source.pool.control = control;
    let root = case.temp.path();
    save(
        &evidence,
        "original-roots.json",
        &json!({"job":root,"source":fixture.temp.path()}),
    );
    let history = seed_history(&case);
    let receipt = Prepared::open(
        &fixture.packet,
        &case.source,
        &case.profile,
        "owner-attempt-1",
    )
    .unwrap()
    .accept()
    .unwrap();
    let executable = PathBuf::from(
        std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("REEL_NATIVE_HANDOFF_TOOL"),
    );
    let mut profile = handoff_profile(&case, receipt.clone(), executable.clone());
    profile.binding.schema = "reel.workflow-native-handoff-binding.v2".into();
    profile.binding.retained_owner_policy = Some(policy(1024));
    let (mut result, returns) = response_fixture(&fixture.packet, &root.join("returns"));
    result.attempt_id = receipt.attempt_id.clone();
    save(root, "packet.json", &fixture.packet);
    save(root, "profile.json", &profile);
    save(root, "result.json", &result);
    save(root, "returns.json", &returns);
    save(&evidence, "before-history.json", &history);
    let journal = root.join("journal");
    let out = Command::new(&executable)
        .arg("create-handoff")
        .arg(root.join("packet.json"))
        .arg(root.join("profile.json"))
        .arg(&journal)
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "create-result.json", &out);
    let mut staged_before_interruption = None;
    if interrupted {
        let mut missing = returns.clone();
        missing
            .files
            .insert("note".into(), "not-returned.txt".into());
        save(root, "missing-returns.json", &missing);
        let out = Command::new(&executable)
            .arg("import-handoff")
            .arg(&journal)
            .arg(root.join("profile.json"))
            .arg(root.join("result.json"))
            .arg(root.join("missing-returns.json"))
            .arg("--apply")
            .output()
            .unwrap();
        assert!(!out.status.success());
        save(
            &evidence,
            "original-pending-result.json",
            &json!({"exit_code":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout)}),
        );
        assert_eq!(
            Journal::open(&journal).unwrap().state(),
            State::ImportPending
        );
        reel_workflow_native::response::ingest(
            &fixture.packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &returns,
        )
        .unwrap();
        let staged = retention::capacity::stage_for_owner_policy(
            &fixture.packet,
            &profile.source,
            &profile.admission,
            &receipt,
            &result,
            &profile.binding.target,
            profile.binding.retained_owner_policy.as_ref().unwrap(),
        )
        .unwrap();
        save(&evidence, "staged-original.json", &staged);
        let mut nodes =
            MetadataStore::open(&case.profile.metadata_root, &staged.metadata_checkpoint).unwrap();
        let mut index = Index::open(&case.profile.index_root, &staged.index_checkpoint).unwrap();
        let record = work::read(&mut index, &staged.work, &nodes).unwrap();
        let mut reached = false;
        assert!(
            index
                .update_hook(&record.update, &mut nodes, |stage| {
                    if stage == protection::Stage::Prepared {
                        reached = true;
                        Err(protection::Error::Unknown)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(reached);
        assert!(
            Index::observe(&case.profile.index_root, &index.checkpoint(), &nodes)
                .unwrap()
                .pending
        );
        assert!(!work::complete(&mut index, &staged.work).unwrap());
        drop(index);
        drop(nodes);
        staged_before_interruption = Some(staged.work);
        let out = Command::new(&executable)
            .arg("resume-handoff")
            .arg(&journal)
            .arg(root.join("profile.json"))
            .arg("-")
            .arg("--apply")
            .output()
            .unwrap();
        success(&evidence, "interrupted-prepare-resume-result.json", &out);
    } else {
        let out = Command::new(&executable)
            .arg("import-handoff")
            .arg(&journal)
            .arg(root.join("profile.json"))
            .arg(root.join("result.json"))
            .arg(root.join("returns.json"))
            .arg("--apply")
            .output()
            .unwrap();
        success(&evidence, "import-result.json", &out);
    }
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Retained);
    let ack: retention::Acknowledgement = serde_json::from_slice(
        &fs::read(profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            result.task_key, result.attempt_id
        )))
        .unwrap(),
    )
    .unwrap();
    save(&evidence, "retained.json", &ack);
    if let Some(original) = staged_before_interruption {
        assert_eq!(original, ack.work);
    }
    let mut nodes =
        MetadataStore::open(&case.profile.metadata_root, &ack.metadata_checkpoint).unwrap();
    let mut index = Index::open(&case.profile.index_root, &ack.index_checkpoint).unwrap();
    let record = work::read(&mut index, &ack.work, &nodes).unwrap();
    let present = Tree::from_root(record.update.selection.root)
        .entries(&mut nodes, TreeLimits::default())
        .unwrap()
        .0;
    let named = present
        .get(
            &LogicalPath::parse(&format!(
                "retained-policies/{}",
                record.update.operation.as_str()
            ))
            .unwrap(),
        )
        .unwrap();
    let owner_raw = nodes.get(named).unwrap().unwrap();
    let owner: retention::capacity::Plan = serde_json::from_slice(&owner_raw).unwrap();
    assert_eq!(owner.schema, "reel.workflow-retained-owner-plan.v1");
    assert!(!owner.observed.resource_bound_ok);
    assert_eq!(owner.observed.object_limit, 512);
    assert!(owner.required_objects > 512 && owner.required_objects <= 1024);
    assert!(record.update.changes.len() <= 128);
    assert_eq!(owner.policy, policy(1024));
    for (path, pin) in &history {
        assert_eq!(present.get(path), Some(pin));
    }
    let checkpoint = index.checkpoint();
    drop(index);
    drop(nodes);
    fs::write(evidence.join("owner-plan.json"), owner_raw).unwrap();
    // A policy change cannot reinterpret an original task, even after success.
    let error = retention::capacity::stage_for_owner_policy(
        &fixture.packet,
        &profile.source,
        &profile.admission,
        &receipt,
        &result,
        &profile.binding.target,
        &policy(2048),
    )
    .unwrap_err();
    assert_eq!(error.code, "retained-owner-original-policy-binding");
    assert_eq!(
        Index::open(&case.profile.index_root, &checkpoint)
            .unwrap()
            .checkpoint(),
        checkpoint
    );
    let lost = root.join("lost-completion");
    fs::create_dir(&lost).unwrap();
    for name in ["00000.json", "00001.json"] {
        fs::copy(journal.join(name), lost.join(name)).unwrap();
    }
    let out = Command::new(&executable)
        .arg("resume-handoff")
        .arg(&lost)
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "lost-completion-result.json", &out);
    assert_eq!(
        Journal::open(&lost).unwrap().acknowledgement(),
        Journal::open(&journal).unwrap().acknowledgement()
    );
    assert_eq!(
        Index::open(&case.profile.index_root, &checkpoint)
            .unwrap()
            .checkpoint(),
        checkpoint
    );
    let close = root.join("close");
    fs::create_dir(&close).unwrap();
    save(&close, "target.json", &profile.binding.target);
    let out = Command::new(&executable)
        .arg("prepare-close")
        .arg(&journal)
        .arg(root.join("profile.json"))
        .arg(close.join("target.json"))
        .arg(close.join("ticket.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "prepare-close-result.json", &out);
    let ticket_pin = digest(&fs::read(close.join("ticket.json")).unwrap());
    let out = Command::new(&executable)
        .arg("close-job")
        .arg(close.join("ticket.json"))
        .arg(ticket_pin)
        .arg(close.join("closed.json"))
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "close-result.json", &out);
    let closed: reel_workflow_native::closing::Outcome =
        serde_json::from_slice(&fs::read(close.join("closed.json")).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete);
    let after_close = Index::open(&case.profile.index_root, &closed.index_checkpoint)
        .unwrap()
        .checkpoint();
    let out = Command::new(&executable)
        .arg("resume-handoff")
        .arg(&journal)
        .arg(root.join("profile.json"))
        .arg("-")
        .arg("--apply")
        .output()
        .unwrap();
    success(&evidence, "historical-resume-result.json", &out);
    assert_eq!(
        Index::open(&case.profile.index_root, &after_close)
            .unwrap()
            .checkpoint(),
        after_close
    );
    save(
        &evidence,
        "interruption-observation.json",
        &json!({"native_prepare_interrupted":interrupted,"same_original_retention_work_verified":true}),
    );
    save(
        &evidence,
        "qualification-result.json",
        &json!({"schema":"reel.automatic-owner-policy-qualification.v1","job":root,"source":fixture.temp.path(),"task_key":result.task_key,"attempt_id":result.attempt_id,"activation":receipt.activation,"work":receipt.work,"retention_work":ack.work,"runtime":profile.runtime,"policy":policy(1024),"history_objects":history.len(),"owner_objects":owner.required_objects,"changed_paths":record.update.changes.len(),"original_job_object_limit":512,"automatic_retained_completion":true,"wrong_policy_refused":true,"lost_completion_same_work_index_unchanged":true,"original_history_preserved":true,"slot_closed_into_retained_coverage":true,"historical_resume_index_unchanged":true,"after_close_checkpoint_sha256":canonical_digest(&after_close).unwrap(),"selected":false,"approved":false,"scratch_retired":false,"real_production_adopted":false}),
    );
}
