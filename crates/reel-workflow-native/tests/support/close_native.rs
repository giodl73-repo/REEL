//! Actual retained bytes, original close work and actor generation fencing.
use super::*;
use reel_workflow_native::{
    admission::{Prepared, Receipt},
    closing::Outcome,
};

fn run(executable: &Path, args: &[&std::ffi::OsStr]) -> Vec<u8> {
    let output = Command::new(executable).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout)
    );
    output.stdout
}
#[allow(clippy::too_many_arguments)]
pub(super) fn exercise_native_close(
    case: &AdmissionCase,
    fixture: &Fixture,
    profile: &reel_workflow_native::handoff::Profile,
    journal: &Path,
    profile_file: &Path,
    executable: &Path,
    result: &ReturnedResult,
    receipt: &Receipt,
    ack_path: &Path,
) {
    let root = case.temp.path();
    let target = root.join("close-target.json");
    fs::write(
        &target,
        serde_json::to_vec(&profile.binding.target).unwrap(),
    )
    .unwrap();
    let ticket = root.join("close-ticket.json");
    let before = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let preview = run(
        executable,
        &[
            "prepare-close".as_ref(),
            journal.as_os_str(),
            profile_file.as_os_str(),
            target.as_os_str(),
            ticket.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    assert_eq!(
        before,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    let pin = digest(&fs::read(&ticket).unwrap());
    let staged_file = root.join("close-staged.json");
    run(
        executable,
        &[
            "stage-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            staged_file.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let staged: Outcome = serde_json::from_slice(&fs::read(&staged_file).unwrap()).unwrap();
    assert!(!staged.slot_idle && !staged.original_clear_complete);
    {
        use tig_store::{
            metadata::MetadataStore,
            source_claims::protection::{self, Index, work},
        };
        let mut nodes =
            MetadataStore::open(&case.profile.metadata_root, &staged.metadata_checkpoint).unwrap();
        let mut index = Index::open(&case.profile.index_root, &staged.index_checkpoint).unwrap();
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
    let close_receipt = root.join("closed.json");
    let closed_report = run(
        executable,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            close_receipt.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let closed: Outcome = serde_json::from_slice(&fs::read(&close_receipt).unwrap()).unwrap();
    assert!(closed.slot_idle && closed.original_clear_complete && closed.applied);
    assert_eq!(
        case.protected(&Object::from_bytes(b"preview")),
        vec![profile.binding.target.actor.clone()]
    );
    let recovered_file = root.join("close-recovered.json");
    run(
        executable,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            recovered_file.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let recovered: Outcome = serde_json::from_slice(&fs::read(&recovered_file).unwrap()).unwrap();
    assert_eq!(recovered.handoff, closed.handoff);
    assert_eq!(recovered.clear, closed.clear);
    let mut next_profile = case.profile.clone();
    next_profile.scope.operation = tig_store::Key::parse("after-qualified-close").unwrap();
    let mut next_task = fixture.packet.task.clone();
    next_task
        .prompt
        .push_str(" New explicitly admitted generation.");
    let next_plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: fixture.packet.project.clone(),
        source_commit: fixture.packet.source_commit.clone(),
        tasks: vec![next_task],
    };
    let next_packet = make_packet(&next_plan, &fixture.packet.task.id, BTreeMap::new()).unwrap();
    let next = Prepared::open(&next_packet, &case.source, &next_profile, "attempt-2")
        .unwrap()
        .accept()
        .unwrap();
    assert_ne!(next.activation, receipt.activation);
    let before_replay = fs::read(case.profile.index_root.join("claims.json")).unwrap();
    let old_replay = root.join("close-after-reuse.json");
    run(
        executable,
        &[
            "recover-close".as_ref(),
            ticket.as_os_str(),
            pin.as_ref(),
            old_replay.as_os_str(),
            "--apply".as_ref(),
        ],
    );
    let after: Outcome = serde_json::from_slice(&fs::read(&old_replay).unwrap()).unwrap();
    assert!(after.original_clear_complete && !after.slot_idle);
    assert_eq!(after.handoff, closed.handoff);
    assert_eq!(after.clear, closed.clear);
    assert_eq!(
        before_replay,
        fs::read(case.profile.index_root.join("claims.json")).unwrap()
    );
    drop(
        Prepared::open(&next_packet, &case.source, &next_profile, "attempt-2")
            .unwrap()
            .reopen(&next)
            .unwrap(),
    );
    assert!(
        Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
            .unwrap()
            .reopen(receipt)
            .is_err()
    );
    let acknowledgement = serde_json::from_slice(&fs::read(ack_path).unwrap()).unwrap();
    drop(
        reel_workflow_native::retention::verify(
            &fixture.packet,
            &case.source,
            &case.profile,
            receipt,
            result,
            &acknowledgement,
        )
        .unwrap(),
    );
    if let Some(directory) = std::env::var_os("REEL_CLOSE_EVIDENCE_DIRECTORY") {
        let directory = PathBuf::from(directory);
        fs::write(directory.join("close-preview.json"), preview).unwrap();
        fs::write(directory.join("close-report.json"), closed_report).unwrap();
        fs::write(directory.join("qualification-result.json"),serde_json::to_vec(&json!({"root":root,"source":fixture.temp.path(),"old_slot_closed":true,"original_close_work_recovered":true,"new_generation_survived_old_close":true,"old_native_result_reopened":true,"next_admission":next,"payloads_deleted":false,"scratch_retired":false})).unwrap()).unwrap();
        for name in [
            "close-ticket.json",
            "closed.json",
            "close-recovered.json",
            "close-after-reuse.json",
        ] {
            fs::copy(root.join(name), directory.join(name)).unwrap();
        }
    }
}
