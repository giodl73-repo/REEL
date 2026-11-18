//! Frontier protocol fixtures, not native protection or storage qualification.
use reel_workflow::{
    graph::{self, Job, Observer, Status},
    handoff::{State, Ticket},
    *,
};
use std::collections::BTreeMap;
fn plan() -> Plan {
    let mut tasks: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|id| Task {
            id: id.into(),
            native_inputs: None,
            operation: Operation::TextProposal,
            tool: None,
            language: Some("es".into()),
            prompt: "Synthetic private prompt.".into(),
            inputs: vec![],
            depends_on: vec![],
            outputs: BTreeMap::from([(
                "note".into(),
                OutputContract {
                    media_type: "text/plain".into(),
                    max_bytes: 100,
                    required: true,
                },
            )]),
        })
        .collect();
    tasks[2].depends_on = vec!["a".into()];
    Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic".into(),
        source_commit: "a".repeat(40),
        tasks,
    }
}
fn artifact(id: &str) -> Artifact {
    Artifact {
        logical_id: id.into(),
        sha256: digest(b"synthetic"),
        bytes: 9,
        media_type: "application/json".into(),
    }
}
fn job(packet: Packet, state: State) -> Job {
    let result = matches!(state, State::Retained | State::ImportPending).then(|| ReturnedResult {
        schema: RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: "attempt-1".into(),
        worker_receipt: None,
        artifacts: BTreeMap::from([(
            "note".into(),
            Artifact {
                media_type: "text/plain".into(),
                ..artifact("candidate/note")
            },
        )]),
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
    });
    Job {
        ticket: Ticket {
            packet,
            attempt_id: "attempt-1".into(),
            native_binding: artifact("fixture/binding"),
            lifecycle_tool: artifact("fixture/tool"),
        },
        state,
        result,
        acknowledgement: (state == State::Retained).then(|| artifact("fixture/ack")),
    }
}
#[derive(Default)]
struct Fixture {
    jobs: BTreeMap<String, Job>,
    refused: BTreeMap<String, Error>,
    verified: Vec<String>,
}
impl Observer for Fixture {
    fn observe(&mut self, id: &str) -> Result<Option<Job>> {
        Ok(self.jobs.get(id).cloned())
    }
    fn verify(&mut self, job: &Job) -> Result<()> {
        let id = job.ticket.packet.task.id.clone();
        self.verified.push(id.clone());
        if let Some(error) = self.refused.get(&id) {
            Err(error.clone())
        } else {
            Ok(())
        }
    }
}
#[test]
fn only_verified_retained_results_unlock_children_with_exact_dependency_fingerprints() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let expected = DependencyResult {
        result_sha256: canonical_digest(a.result.as_ref().unwrap()).unwrap(),
        artifacts: a.result.as_ref().unwrap().artifacts.clone(),
    };
    fixture.jobs.insert("a".into(), a);
    fixture.jobs.insert(
        "b".into(),
        job(
            make_packet(&plan, "b", BTreeMap::new()).unwrap(),
            State::Waiting,
        ),
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.waiting.len(), 1);
    assert_eq!(frontier.needs_admission.len(), 1);
    assert_eq!(frontier.needs_admission["c"].dependencies["a"], expected);
    assert_eq!(fixture.verified, vec!["a", "b"]);
    assert_eq!(frontier.rows[0].status, Status::RetainedObserved);
}
#[test]
fn storage_refusal_blocks_descendants_but_keeps_independent_work_and_redacts_reasons() {
    let plan = plan();
    let mut fixture = Fixture::default();
    fixture.jobs.insert(
        "a".into(),
        job(
            make_packet(&plan, "a", BTreeMap::new()).unwrap(),
            State::Retained,
        ),
    );
    fixture.refused.insert(
        "a".into(),
        Error {
            kind: ErrorKind::Missing,
            code: "private/path?secret=hidden".into(),
        },
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.rows[0].status, Status::Refused);
    assert_eq!(frontier.rows[2].status, Status::Blocked);
    assert!(frontier.needs_admission.contains_key("b"));
    assert!(!frontier.needs_admission.contains_key("c"));
    let json = serde_json::to_string(&frontier.rows).unwrap();
    assert!(!json.contains("secret"));
    assert!(!json.contains("prompt"));
}
#[test]
fn changed_parent_attempt_invalidates_an_already_started_child_without_retargeting_it() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let mut a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    let old = DependencyResult {
        result_sha256: canonical_digest(a.result.as_ref().unwrap()).unwrap(),
        artifacts: a.result.as_ref().unwrap().artifacts.clone(),
    };
    let child = job(
        make_packet(&plan, "c", BTreeMap::from([("a".into(), old)])).unwrap(),
        State::Waiting,
    );
    fixture.jobs.insert("c".into(), child.clone());
    a.ticket.attempt_id = "attempt-2".into();
    a.result.as_mut().unwrap().attempt_id = "attempt-2".into();
    fixture.jobs.insert("a".into(), a);
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(
        frontier.rows[2].error.as_ref().unwrap().kind,
        ErrorKind::Stale
    );
    assert_eq!(fixture.jobs["c"], child);
    assert!(!fixture.verified.contains(&"c".into()));
}
#[test]
fn pending_and_cancelled_jobs_are_never_dispatched_or_automatically_reconciled() {
    let plan = plan();
    let mut fixture = Fixture::default();
    fixture.jobs.insert(
        "a".into(),
        job(
            make_packet(&plan, "a", BTreeMap::new()).unwrap(),
            State::ImportPending,
        ),
    );
    fixture.jobs.insert(
        "b".into(),
        job(
            make_packet(&plan, "b", BTreeMap::new()).unwrap(),
            State::Cancelled,
        ),
    );
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert_eq!(frontier.rows[0].status, Status::NeedsRecovery);
    assert_eq!(frontier.rows[1].status, Status::Cancelled);
    assert!(frontier.waiting.is_empty());
    assert!(fixture.verified.is_empty());
}
#[test]
fn local_retained_marker_with_missing_result_or_another_attempt_is_not_completion() {
    let plan = plan();
    let mut fixture = Fixture::default();
    let mut a = job(
        make_packet(&plan, "a", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    a.result = None;
    fixture.jobs.insert("a".into(), a);
    let mut b = job(
        make_packet(&plan, "b", BTreeMap::new()).unwrap(),
        State::Retained,
    );
    b.result.as_mut().unwrap().attempt_id = "another".into();
    fixture.jobs.insert("b".into(), b);
    let frontier = graph::inspect(&plan, &mut fixture).unwrap();
    assert!(
        frontier.rows[..2]
            .iter()
            .all(|r| r.status == Status::Refused)
    );
    assert!(fixture.verified.is_empty());
}
