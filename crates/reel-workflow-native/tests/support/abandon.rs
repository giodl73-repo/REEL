//! Native metadata/claim handoff fixtures. No payload availability is inferred.
use super::*;
use reel_workflow::handoff::{Journal, State};
use reel_workflow_native::{abandon, admission::Prepared, handoff, retention::capacity::Policy};

fn policy() -> Policy {
    Policy {
        schema: "reel.workflow-retained-owner-policy.v1".into(),
        objects: 4096,
        logical_bytes: 8 * 1024 * 1024 * 1024,
    }
}

#[test]
fn abandoned_waiting_job_recovers_original_handoff_and_fences_reused_actor() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let activation = receipt.activation.clone();
    let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let journal = case.temp.path().join("abandoned-journal");
    handoff::create(&journal, &fixture.packet, &profile).unwrap();
    let scratch = case.temp.path().join("preserved-scratch.bin");
    fs::write(&scratch, b"original scratch").unwrap();
    let (ticket, preview) = abandon::prepare(
        &journal,
        profile,
        retained_target(),
        policy(),
        "invalid-source-control",
    )
    .unwrap();
    let alternative_profile =
        serde_json::from_slice(&serde_json::to_vec(&ticket.profile).unwrap()).unwrap();
    let (alternative, _) = abandon::prepare(
        &journal,
        alternative_profile,
        retained_target(),
        policy(),
        "different-close-request",
    )
    .unwrap();
    assert!(!preview.applied);
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Waiting);
    let path = case.temp.path().join("abandon-ticket.json");
    let pin = ticket.export(&path).unwrap();
    assert!(abandon::read_pinned(&path, &"0".repeat(64)).is_err());
    let staged = ticket
        .stage_to(&case.temp.path().join("staged.json"))
        .unwrap();
    assert!(!staged.slot_idle);
    assert!(staged.handoff.is_some());
    assert!(staged.clear.is_none());
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Cancelled);
    let conflict = alternative
        .execute_to(&case.temp.path().join("different-operation.json"), true)
        .err()
        .unwrap();
    assert_eq!(conflict.code, "abandon-original-operation-required");
    let intent = ticket
        .profile
        .admission
        .scope
        .coordination_root
        .join("workflow-abandon-intents")
        .join(format!(
            "{}.json",
            ticket
                .profile
                .binding
                .admission
                .operation_identity()
                .unwrap()
        ));
    let saved = intent.with_extension("preserved");
    fs::rename(&intent, &saved).unwrap();
    let missing = ticket
        .execute_to(&case.temp.path().join("missing-intent.json"), true)
        .err()
        .unwrap();
    assert_eq!(missing.code, "abandon-original-intent-missing");
    fs::rename(&saved, &intent).unwrap();
    drop(ticket);
    let (ticket, _held) = abandon::read_pinned(&path, &pin).unwrap();
    let recovered = ticket
        .execute_to(&case.temp.path().join("recovered.json"), true)
        .unwrap();
    assert!(recovered.slot_idle);
    assert!(recovered.original_clear_complete);
    assert_eq!(recovered.handoff, staged.handoff);
    assert_eq!(
        case.protected(&activation),
        vec![tig_store::Key::parse("retained").unwrap()]
    );
    assert!(Journal::open(&journal).unwrap().result().is_none());
    assert_eq!(fs::read(&scratch).unwrap(), b"original scratch");
    assert_eq!(
        fs::read_dir(&ticket.profile.acknowledgements)
            .unwrap()
            .count(),
        0
    );
    let mut next_profile = case.profile.clone();
    next_profile.scope.operation = tig_store::Key::parse("after-abandon").unwrap();
    let next = Prepared::open(&fixture.packet, &case.source, &next_profile, "attempt-2")
        .unwrap()
        .accept()
        .unwrap();
    let replay = ticket
        .execute_to(&case.temp.path().join("old-close-after-reuse.json"), true)
        .unwrap();
    assert!(!replay.slot_idle);
    assert!(replay.original_clear_complete);
    assert_eq!(
        case.protected(&next.activation),
        vec![tig_store::Key::parse("slot").unwrap()]
    );
    assert_eq!(fs::read(&scratch).unwrap(), b"original scratch");
}

#[test]
fn abandoned_preview_checks_capacity_before_recording_cancellation() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let journal = case.temp.path().join("journal");
    handoff::create(&journal, &fixture.packet, &profile).unwrap();
    let mut small = policy();
    small.objects = 1;
    small.logical_bytes = 1;
    let error = abandon::prepare(
        &journal,
        profile,
        retained_target(),
        small,
        "invalid-source-control",
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(Journal::open(&journal).unwrap().state(), State::Waiting);
}

struct Refuse;
fn refused<T>() -> Result<T> {
    Err(Error {
        kind: ErrorKind::Busy,
        code: "fixture-stop-before-publication".into(),
    })
}
impl reel_workflow::handoff::ProtectedJob for Refuse {
    fn register(
        &mut self,
        _: &reel_workflow::handoff::Ticket,
        _: &ReturnedResult,
    ) -> Result<Artifact> {
        refused()
    }
    fn reconcile(
        &mut self,
        _: &reel_workflow::handoff::Ticket,
        _: &ReturnedResult,
    ) -> Result<Artifact> {
        refused()
    }
    fn verify_retained(
        &mut self,
        _: &reel_workflow::handoff::Ticket,
        _: &ReturnedResult,
        _: &Artifact,
    ) -> Result<()> {
        refused()
    }
}
impl reel_workflow::handoff::Lifecycle for Refuse {
    type Guard<'a> = Refuse;
    fn reopen<'a>(&'a mut self, _: &reel_workflow::handoff::Ticket) -> Result<Self::Guard<'a>> {
        refused()
    }
}
#[test]
fn uncertain_import_is_not_cancellable_or_reclassified_as_abandoned() {
    let fixture = Fixture::new();
    let case = AdmissionCase::new(&fixture);
    let receipt = Prepared::open(&fixture.packet, &case.source, &case.profile, "attempt-1")
        .unwrap()
        .accept()
        .unwrap();
    let profile = handoff_profile(&case, receipt, std::env::current_exe().unwrap());
    let journal = case.temp.path().join("journal");
    handoff::create(&journal, &fixture.packet, &profile).unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: fixture.packet.task.id.clone(),
        task_key: fixture.packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: fixture
            .packet
            .task
            .outputs
            .iter()
            .map(|(name, c)| {
                (
                    name.clone(),
                    Artifact {
                        logical_id: format!("fixture/{name}"),
                        sha256: digest(b"x"),
                        bytes: 1,
                        media_type: c.media_type.clone(),
                    },
                )
            })
            .collect(),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    assert!(
        Journal::open(&journal)
            .unwrap()
            .import(result, &mut Refuse)
            .is_err()
    );
    assert_eq!(
        Journal::open(&journal).unwrap().state(),
        State::ImportPending
    );
    let error = abandon::prepare(&journal, profile, retained_target(), policy(), "cancel")
        .err()
        .unwrap();
    assert_eq!(error.code, "abandon-original-waiting-job-required");
    assert_eq!(
        Journal::open(&journal).unwrap().state(),
        State::ImportPending
    );
}
