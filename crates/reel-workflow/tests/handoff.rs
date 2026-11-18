//! Coordinator fault injection, not qualification of native protection/storage.
use reel_workflow::{handoff::*, *};
use std::{collections::BTreeMap, path::PathBuf};

fn pin(id: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(id.as_bytes()),
        bytes: id.len() as u64,
        media_type: "application/json".into(),
    }
}
fn fixture() -> (Ticket, ReturnedResult) {
    let task = Task {
        native_inputs: None,
        id: "review".into(),
        operation: Operation::ReviewFinding,
        tool: None,
        language: Some("es".into()),
        prompt: "Review synthetic page.".into(),
        inputs: vec![],
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "note".into(),
            OutputContract {
                media_type: "application/json".into(),
                max_bytes: 100,
                required: true,
            },
        )]),
    };
    let packet = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic-comic".into(),
            source_commit: "a".repeat(40),
            tasks: vec![task],
        },
        "review",
        BTreeMap::new(),
    )
    .unwrap();
    let result = ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        artifacts: BTreeMap::from([("note".into(), pin("candidate/note"))]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    };
    (
        Ticket {
            packet,
            attempt_id: result.attempt_id.clone(),
            native_binding: pin("native/original-admission"),
            lifecycle_tool: pin("tool/qualified-lifecycle"),
        },
        result,
    )
}
fn failure(kind: ErrorKind) -> Error {
    Error {
        kind,
        code: "injected-native-observation".into(),
    }
}
struct Native {
    ticket: Ticket,
    journal: PathBuf,
    opens: usize,
    registrations: usize,
    reconciliations: usize,
    verifications: usize,
    published: bool,
    fail_after_publication: bool,
    stale: bool,
    missing_retained: bool,
}
impl Native {
    fn new(ticket: Ticket, journal: PathBuf) -> Self {
        Self {
            ticket,
            journal,
            opens: 0,
            registrations: 0,
            reconciliations: 0,
            verifications: 0,
            published: false,
            fail_after_publication: false,
            stale: false,
            missing_retained: false,
        }
    }
}
struct Guard<'a>(&'a mut Native);
impl Lifecycle for Native {
    type Guard<'a> = Guard<'a>;
    fn reopen<'a>(&'a mut self, ticket: &Ticket) -> Result<Self::Guard<'a>> {
        self.opens += 1;
        if self.stale || ticket != &self.ticket {
            return Err(failure(ErrorKind::Stale));
        }
        Ok(Guard(self))
    }
}
impl ProtectedJob for Guard<'_> {
    fn register(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        assert_eq!(ticket, &self.0.ticket);
        assert_eq!(result.attempt_id, ticket.attempt_id);
        // The actual flushed intent is observable before the first effect.
        assert_eq!(
            Journal::open(&self.0.journal).unwrap().state(),
            State::ImportPending
        );
        self.0.registrations += 1;
        self.0.published = true;
        if self.0.fail_after_publication {
            return Err(failure(ErrorKind::NeedsRecovery));
        }
        Ok(pin("native/retained-ack"))
    }
    fn reconcile(&mut self, ticket: &Ticket, result: &ReturnedResult) -> Result<Artifact> {
        assert_eq!(ticket, &self.0.ticket);
        assert_eq!(result.attempt_id, ticket.attempt_id);
        self.0.reconciliations += 1;
        if !self.0.published {
            return Err(failure(ErrorKind::NeedsRecovery));
        }
        Ok(pin("native/retained-ack"))
    }
    fn verify_retained(
        &mut self,
        _: &Ticket,
        _: &ReturnedResult,
        acknowledgement: &Artifact,
    ) -> Result<()> {
        self.0.verifications += 1;
        if self.0.missing_retained || acknowledgement != &pin("native/retained-ack") {
            return Err(failure(ErrorKind::Missing));
        }
        Ok(())
    }
}

#[test]
fn import_intent_precedes_effect_and_ack_is_reverified_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket, &mut native).unwrap();
    journal.import(result, &mut native).unwrap();
    assert_eq!(journal.state(), State::Retained);
    let mut restarted = Journal::open(&path).unwrap();
    restarted.resume(&mut native).unwrap();
    assert_eq!(
        (
            native.registrations,
            native.reconciliations,
            native.verifications
        ),
        (1, 0, 2)
    );
    native.missing_retained = true;
    assert_eq!(
        restarted.resume(&mut native).unwrap_err().kind,
        ErrorKind::Missing
    );
}

#[test]
fn publication_before_response_reconciles_original_identity_without_reimport() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket.clone(), &mut native).unwrap();
    native.fail_after_publication = true;
    assert_eq!(
        journal
            .import(result.clone(), &mut native)
            .unwrap_err()
            .kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(Journal::open(&path).unwrap().state(), State::ImportPending);
    assert!(journal.cancel().is_err());
    assert!(journal.import(result, &mut native).is_err());
    let mut restarted = Journal::open(&path).unwrap();
    restarted.resume(&mut native).unwrap();
    assert_eq!(restarted.ticket(), &ticket);
    assert_eq!((native.registrations, native.reconciliations), (1, 1));
    assert_eq!(restarted.state(), State::Retained);
}

#[test]
fn failed_retained_verification_preserves_pending_import_and_native_candidate() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket, &mut native).unwrap();
    native.missing_retained = true;
    assert!(journal.import(result, &mut native).is_err());
    assert!(native.published);
    assert_eq!(journal.state(), State::ImportPending);
    native.missing_retained = false;
    journal.resume(&mut native).unwrap();
    assert_eq!((native.registrations, native.reconciliations), (1, 1));
}

#[test]
fn waiting_and_cancellation_preserve_binding_and_never_import_or_close() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, _) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket.clone(), &mut native).unwrap();
    journal.resume(&mut native).unwrap();
    assert_eq!(journal.state(), State::Waiting);
    journal.cancel().unwrap();
    journal.cancel().unwrap();
    let opens = native.opens;
    journal.resume(&mut native).unwrap();
    assert_eq!(native.opens, opens);
    assert_eq!(journal.ticket(), &ticket);
    assert_eq!(journal.state(), State::Cancelled);
    assert_eq!((native.registrations, native.reconciliations), (0, 0));
}

#[test]
fn stale_original_generation_stops_before_publication_and_remains_recoverable() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket, &mut native).unwrap();
    native.stale = true;
    assert_eq!(
        journal.import(result, &mut native).unwrap_err().kind,
        ErrorKind::Stale
    );
    assert_eq!(native.registrations, 0);
    assert_eq!(journal.state(), State::ImportPending);
    native.stale = false;
    assert_eq!(
        journal.resume(&mut native).unwrap_err().kind,
        ErrorKind::NeedsRecovery
    );
    assert_eq!(native.registrations, 0); // Not-started is not permission to retry effects.
}

#[test]
fn another_attempt_and_stale_local_writer_cannot_overwrite_transitions() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, mut result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket, &mut native).unwrap();
    result.attempt_id = "attempt-2".into();
    assert_eq!(
        journal.import(result, &mut native).unwrap_err().kind,
        ErrorKind::Conflict
    );
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
    let mut stale = Journal::open(&path).unwrap();
    journal.cancel().unwrap();
    stale.cancel().unwrap();
    assert_eq!(std::fs::read_dir(&path).unwrap().count(), 2);
    assert_eq!(native.registrations, 0);
}

#[test]
fn partial_and_mutated_control_records_stop_without_repair_or_effects() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, _) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket.clone(), &mut native).unwrap();
    journal.cancel().unwrap();
    let tail = path.join("00001.json");
    let original = std::fs::read(&tail).unwrap();
    std::fs::write(&tail, b"{").unwrap();
    assert!(Journal::open(&path).is_err());
    assert_eq!(std::fs::read(&tail).unwrap(), b"{");
    std::fs::write(&tail, original).unwrap();
    let first = path.join("00000.json");
    let raw = std::fs::read(&first).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["event"]["ticket"]["attempt_id"] = "different-attempt".into();
    std::fs::write(&first, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Journal::open(&path).is_err());
    assert_eq!(native.registrations, 0);
}

#[test]
fn create_requires_existing_protection_and_refuses_existing_directories() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, _) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    native.stale = true;
    assert!(Journal::create(&path, ticket.clone(), &mut native).is_err());
    assert!(!path.exists());
    native.stale = false;
    Journal::create(&path, ticket.clone(), &mut native).unwrap();
    assert!(Journal::create(&path, ticket, &mut native).is_err());
    assert_eq!(std::fs::read_dir(path).unwrap().count(), 1);
}

#[test]
fn cli_inspection_and_cancel_are_local_only_and_emit_no_ticket_context() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ticket");
    let (ticket, result) = fixture();
    let mut native = Native::new(ticket.clone(), path.clone());
    let mut journal = Journal::create(&path, ticket.clone(), &mut native).unwrap();
    let call = |command: &str| {
        std::process::Command::new(env!("CARGO_BIN_EXE_reel-workflow"))
            .arg(command)
            .arg(&path)
            .output()
            .unwrap()
    };
    let checked = call("check-handoff");
    assert!(checked.status.success());
    let raw = String::from_utf8(checked.stdout).unwrap();
    assert!(!raw.contains(&ticket.packet.task.prompt));
    assert!(!raw.contains(&ticket.native_binding.sha256));
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(value["state"], "waiting");
    assert_eq!(value["native_state_verified"], false);
    journal.import(result, &mut native).unwrap();
    assert!(!call("cancel-handoff").status.success());
    assert_eq!(Journal::open(&path).unwrap().state(), State::Retained);
    assert!(path.join("00002.json").exists());
}
