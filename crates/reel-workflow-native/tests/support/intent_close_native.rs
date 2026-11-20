//! Guarded close and explicit reuse of the retained synthetic v2 control actor.
use super::*;
use reel_workflow_native::{admission, closing, handoff, retention, upstream};
use tig_store::source_claims::protection::{self, Index, work};

fn load<T: serde::de::DeserializeOwned>(path: &Path) -> T {
    let raw = fs::read(path).unwrap();
    assert!(raw.len() <= MAX_CONTROL_BYTES);
    serde_json::from_slice(&raw).unwrap()
}
fn save(path: &Path, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
    file.sync_all().unwrap();
}
fn run(executable: &Path, args: &[&std::ffi::OsStr]) -> Vec<u8> {
    let output = Command::new(executable).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}: {} {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn checkpoint(
    profile: &admission::Profile,
    minimum: &tig_store::source_claims::Checkpoint,
) -> String {
    let index = Index::open(&profile.index_root, minimum).unwrap();
    canonical_digest(&index.checkpoint()).unwrap()
}

#[test]
#[ignore = "Explicit retained v2 control and pinned close ticket only; interrupted original close, explicit same-slot generation reuse, fenced old replay and native history; preserves all scratch and bytes"]
fn retained_v2_control_close_reuses_the_slot_without_retargeting_original_history() {
    let root = PathBuf::from(
        std::env::var_os("REEL_INTENT_CLOSE_EVIDENCE").expect("explicit close evidence"),
    );
    assert!(root.is_absolute() && root.is_dir());
    let raw = fs::read(root.join("close-ticket.json")).unwrap();
    let ticket: closing::Ticket = serde_json::from_slice(&raw).unwrap();
    assert_eq!(ticket.original.ticket.packet.project, "synthetic-comic");
    assert_eq!(ticket.original.ticket.packet.task.id, "issue-control");
    assert!(
        ticket
            .original
            .ticket
            .packet
            .task
            .tool
            .as_ref()
            .unwrap()
            .source_bindings
            .contains_key("intent.json")
    );
    let profile = &ticket.profile;
    let executable = &profile.runtime.path;
    let program = fs::read(executable).unwrap();
    assert_eq!(digest(&program), profile.runtime.sha256);
    assert_eq!(program.len() as u64, profile.runtime.bytes);
    assert_eq!(ticket.runtime.sha256, profile.runtime.sha256);
    let case = ticket.journal.parent().unwrap();
    let profile_path = case.join("control-profile.json");
    let original_profile: handoff::Profile = load(&profile_path);
    assert_eq!(
        canonical_digest(&original_profile).unwrap(),
        canonical_digest(profile).unwrap()
    );
    for name in [
        "checked-preview-ticket.json",
        "staged.json",
        "closed.json",
        "next-packet.json",
        "next-receipt.json",
    ] {
        assert!(
            !root.join(name).exists(),
            "existing close checkpoint: {name}"
        );
    }
    let before = checkpoint(
        &profile.admission,
        &profile.binding.admission.index_checkpoint,
    );
    let preview = run(
        executable,
        &[
            "prepare-close".as_ref(),
            ticket.journal.as_os_str(),
            profile_path.as_os_str(),
            root.join("target.json").as_os_str(),
            root.join("checked-preview-ticket.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(
        before,
        checkpoint(
            &profile.admission,
            &profile.binding.admission.index_checkpoint
        )
    );
    assert_eq!(
        digest(&fs::read(root.join("checked-preview-ticket.json")).unwrap()),
        digest(&raw)
    );
    save(
        &root.join("verified-preview.json"),
        &json!({"index_checkpoint_sha256":before,"actual_checkpoint_unchanged":true,"same_close_ticket":true,"stdout":serde_json::from_slice::<serde_json::Value>(&preview).unwrap()}),
    );
    let pin = digest(&raw);
    run(
        executable,
        &[
            "stage-close".as_ref(),
            root.join("close-ticket.json").as_os_str(),
            pin.as_ref(),
            root.join("staged.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let staged: closing::Outcome = load(&root.join("staged.json"));
    assert!(!staged.slot_idle && !staged.original_clear_complete);
    // Inject the same bounded pre-commit interruption as the qualified close
    // fixture. The source slot is not cleared before retained coverage exists.
    {
        let mut nodes = tig_store::metadata::MetadataStore::open(
            &profile.admission.metadata_root,
            &staged.metadata_checkpoint,
        )
        .unwrap();
        let mut index =
            Index::open(&profile.admission.index_root, &staged.index_checkpoint).unwrap();
        let record = work::read(&mut index, staged.handoff.as_ref().unwrap(), &nodes).unwrap();
        let mut interrupted = false;
        assert!(
            index
                .update_hook(&record.update, &mut nodes, |stage| {
                    if stage == protection::Stage::Prepared {
                        interrupted = true;
                        Err(protection::Error::Unknown)
                    } else {
                        Ok(())
                    }
                })
                .is_err()
        );
        assert!(interrupted);
        nodes.finish().unwrap();
    }
    run(
        executable,
        &[
            "recover-close".as_ref(),
            root.join("close-ticket.json").as_os_str(),
            pin.as_ref(),
            root.join("closed.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let closed: closing::Outcome = load(&root.join("closed.json"));
    assert!(closed.applied && closed.slot_idle && closed.original_clear_complete);
    assert_eq!(closed.handoff, staged.handoff);
    run(
        executable,
        &[
            "recover-close".as_ref(),
            root.join("close-ticket.json").as_os_str(),
            pin.as_ref(),
            root.join("closed-replayed.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let replay: closing::Outcome = load(&root.join("closed-replayed.json"));
    assert_eq!(replay.clear, closed.clear);
    assert_eq!(replay.handoff, closed.handoff);
    reuse_closed_slot(&root, &ticket, &closed);
}

#[test]
#[ignore = "Explicit completed original v2 close only; continues the same closed checkpoint into a new attempt, no repeated close or payload/scratch deletion"]
fn continue_original_closed_v2_control_into_explicit_slot_reuse() {
    let root = PathBuf::from(
        std::env::var_os("REEL_INTENT_CLOSE_EVIDENCE").expect("original close evidence"),
    );
    assert!(root.is_absolute() && root.is_dir());
    for name in [
        "next-packet.json",
        "next-admission-profile.json",
        "next-receipt.json",
        "next-journal",
        "qualification-result.json",
    ] {
        assert!(
            !root.join(name).exists(),
            "later reuse checkpoint exists: {name}"
        );
    }
    let ticket: closing::Ticket = load(&root.join("close-ticket.json"));
    let closed: closing::Outcome = load(&root.join("closed.json"));
    let replay: closing::Outcome = load(&root.join("closed-replayed.json"));
    assert!(closed.applied && closed.slot_idle && closed.original_clear_complete);
    assert_eq!(closed.operation, ticket.operation);
    assert_eq!(
        closed.activation,
        ticket.profile.binding.admission.activation
    );
    assert_eq!(closed.clear, replay.clear);
    assert_eq!(closed.handoff, replay.handoff);
    assert_eq!(
        checkpoint(&ticket.profile.admission, &replay.index_checkpoint),
        canonical_digest(&replay.index_checkpoint).unwrap()
    );
    reuse_closed_slot(&root, &ticket, &closed);
}

fn reuse_closed_slot(root: &Path, ticket: &closing::Ticket, closed: &closing::Outcome) {
    let profile = &ticket.profile;
    assert_eq!(ticket.original.ticket.packet.project, "synthetic-comic");
    assert_eq!(ticket.original.ticket.packet.task.id, "issue-control");
    let executable = &profile.runtime.path;
    assert_eq!(
        digest(&fs::read(executable).unwrap()),
        profile.runtime.sha256
    );
    let case = ticket.journal.parent().unwrap();
    let pin = digest(&fs::read(root.join("close-ticket.json")).unwrap());
    let frozen = profile.source.snapshot.upstream.as_ref().unwrap();
    let mut tasks = frozen
        .parents
        .values()
        .map(|p| p.packet.task.clone())
        .collect::<Vec<_>>();
    // Tool prompts stay empty. A fresh explicit attempt/operation after a
    // completed close is a new actor generation even for an identical task.
    tasks.push(ticket.original.ticket.packet.task.clone());
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        task_snapshots: Default::default(),
        project: ticket.original.ticket.packet.project.clone(),
        source_commit: ticket.original.ticket.packet.source_commit.clone(),
        tasks,
    };
    let next_packet = make_packet(
        &plan,
        "issue-control",
        ticket.original.ticket.packet.dependencies.clone(),
    )
    .unwrap();
    assert_eq!(next_packet.task_key, ticket.original.ticket.packet.task_key);
    let mut next_profile: admission::Profile =
        serde_json::from_slice(&serde_json::to_vec(&profile.admission).unwrap()).unwrap();
    next_profile.scope.operation =
        tig_store::Key::parse("intent-control-after-guarded-close").unwrap();
    save(&root.join("next-packet.json"), &next_packet);
    save(&root.join("next-admission-profile.json"), &next_profile);
    let mut parent_controls = Vec::new();
    for (id, evidence) in &frozen.parents {
        let parent: handoff::Profile = load(&case.join(format!("{id}-profile.json")));
        let ack: retention::Acknowledgement = load(&parent.acknowledgements.join(format!(
            "retained-{}-{}.json",
            evidence.packet.task_key, evidence.result.attempt_id
        )));
        parent_controls.push((id.clone(), parent, ack));
    }
    let parents = parent_controls
        .iter()
        .map(|(id, p, ack)| {
            let evidence = &frozen.parents[id];
            (
                id.clone(),
                upstream::Parent {
                    packet: &evidence.packet,
                    result: &evidence.result,
                    source: &p.source,
                    admission: &p.admission,
                    receipt: &p.binding.admission,
                    acknowledgement: ack,
                },
            )
        })
        .collect();
    let prepared = upstream::Admission::prepare(
        &next_packet,
        &profile.source,
        &next_profile,
        "reuse-attempt-1",
        parents,
    )
    .unwrap();
    save(
        &root.join("next-preview.json"),
        &prepared.preview().unwrap(),
    );
    let next = prepared.accept().unwrap();
    save(&root.join("next-receipt.json"), &next);
    assert_ne!(next.activation, profile.binding.admission.activation);
    let mut next_handoff: handoff::Profile =
        serde_json::from_slice(&serde_json::to_vec(profile).unwrap()).unwrap();
    next_handoff.admission = next_profile.clone();
    next_handoff.binding.admission = next.clone();
    next_handoff.acknowledgements = root.join("next-acks");
    fs::create_dir(&next_handoff.acknowledgements).unwrap();
    let next_file = root.join("next-handoff-profile.json");
    save(&next_file, &next_handoff);
    run(
        executable,
        &[
            "create-handoff".as_ref(),
            root.join("next-packet.json").as_os_str(),
            next_file.as_os_str(),
            root.join("next-journal").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let before_old_replay = checkpoint(&next_profile, &next.index_checkpoint);
    run(
        executable,
        &[
            "recover-close".as_ref(),
            root.join("close-ticket.json").as_os_str(),
            pin.as_ref(),
            root.join("close-after-reuse.json").as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let after: closing::Outcome = load(&root.join("close-after-reuse.json"));
    assert!(after.original_clear_complete && !after.slot_idle);
    assert_eq!(after.handoff, closed.handoff);
    assert_eq!(after.clear, closed.clear);
    assert_eq!(
        before_old_replay,
        checkpoint(&next_profile, &next.index_checkpoint)
    );
    drop(
        admission::Prepared::open(
            &next_packet,
            &profile.source,
            &next_profile,
            "reuse-attempt-1",
        )
        .unwrap()
        .reopen(&next)
        .unwrap(),
    );
    let old_refused = match admission::Prepared::open(
        &ticket.original.ticket.packet,
        &profile.source,
        &profile.admission,
        &ticket.original.ticket.attempt_id,
    ) {
        Ok(old) => match old.reopen(&profile.binding.admission) {
            Ok(_) => false,
            Err(e) => matches!(
                e.kind,
                ErrorKind::Stale | ErrorKind::Conflict | ErrorKind::Denied
            ),
        },
        Err(e) => matches!(
            e.kind,
            ErrorKind::Stale | ErrorKind::Conflict | ErrorKind::Denied
        ),
    };
    assert!(old_refused);
    drop(
        retention::verify(
            &ticket.original.ticket.packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &ticket.original.result,
            &ticket.acknowledgement,
        )
        .unwrap(),
    );
    let history = run(
        executable,
        &[
            "resume-handoff".as_ref(),
            case.join("issue-journal").as_os_str(),
            case.join("issue-profile.json").as_os_str(),
            "-".as_ref(),
            "--apply".as_ref(),
        ],
    );
    let observed: serde_json::Value = serde_json::from_slice(&history).unwrap();
    assert_eq!(observed["task_completed"], true);
    assert_eq!(observed["state"], "retained");
    assert_eq!(
        before_old_replay,
        checkpoint(&next_profile, &next.index_checkpoint)
    );
    save(&root.join("post-reuse-issue-history.json"), &observed);
    save(
        &root.join("qualification-result.json"),
        &json!({"schema":"reel.native-v2-control-close-qualification.v1","case_root":case,
        "old_task_key":ticket.original.ticket.packet.task_key,"new_task_key":next_packet.task_key,
        "old_attempt":ticket.original.ticket.attempt_id,"new_attempt":"reuse-attempt-1",
        "old_slot_closed":true,"original_close_recovered":true,"new_generation_waiting":true,
        "old_close_preserves_actual_index_checkpoint":true,"old_admission_fenced":true,
        "old_control_native_result_verified":true,"original_issue_native_history_verified":true,
        "new_admission":next,"payloads_deleted":false,"scratch_retired":false,"selected":false}),
    );
}
