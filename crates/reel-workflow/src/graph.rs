//! Verified, read-only graph frontier. Dependency references are planning metadata,
//! not byte admission. Observations are sequential, not an atomic custody snapshot.
use crate::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub ticket: handoff::Ticket,
    pub state: handoff::State,
    pub result: Option<ReturnedResult>,
    pub acknowledgement: Option<Artifact>,
}
/// Adapters must verify the exact observed job against actual protection/custody.
/// A local journal or deserialized acknowledgement alone cannot satisfy `verify`.
pub trait Observer {
    fn observe(&mut self, task: &str) -> Result<Option<Job>>;
    fn verify(&mut self, job: &Job) -> Result<()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    NeedsAdmission,
    Waiting,
    NeedsRecovery,
    RetainedObserved,
    Cancelled,
    Blocked,
    Refused,
}
#[derive(Debug, Serialize)]
pub struct Row {
    pub task_id: String,
    pub status: Status,
    pub task_key: Option<String>,
    pub waiting_on: Vec<String>,
    pub error: Option<Error>,
}
pub struct Frontier {
    pub rows: Vec<Row>,
    /// Only matching Waiting jobs with independently verified original claims.
    /// Export/dispatch must still recheck actual named input bytes and admission.
    pub waiting: BTreeMap<String, Packet>,
    /// Unadmitted planning packets; these are never safe to dispatch by themselves.
    pub needs_admission: BTreeMap<String, Packet>,
}
fn safe_error(mut error: Error) -> Error {
    if error.code.len() > 256
        || !error
            .code
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b))
    {
        error.code = "graph-observation-refused".into();
    }
    error
}
pub fn inspect(plan: &Plan, observer: &mut impl Observer) -> Result<Frontier> {
    let order = check_plan(plan)?;
    let mut frontier = Frontier {
        rows: Vec::new(),
        waiting: BTreeMap::new(),
        needs_admission: BTreeMap::new(),
    };
    let mut completed = BTreeMap::<String, DependencyResult>::new();
    let mut result_bytes = 0usize;
    for id in order {
        let task = plan.tasks.iter().find(|t| t.id == id).ok_or(Error {
            kind: ErrorKind::Missing,
            code: "graph-task".into(),
        })?;
        let waiting_on: Vec<_> = task
            .depends_on
            .iter()
            .filter(|id| !completed.contains_key(*id))
            .cloned()
            .collect();
        let mut row = Row {
            task_id: id.clone(),
            status: Status::Blocked,
            task_key: None,
            waiting_on,
            error: None,
        };
        if !row.waiting_on.is_empty() {
            frontier.rows.push(row);
            continue;
        }
        let dependencies = task
            .depends_on
            .iter()
            .map(|id| (id.clone(), completed[id].clone()))
            .collect();
        let packet = make_packet(plan, &id, dependencies)?;
        row.task_key = Some(packet.task_key.clone());
        let observed = (|| -> Result<Status> {
            let Some(job) = observer.observe(&id)? else {
                frontier.needs_admission.insert(id.clone(), packet.clone());
                return Ok(Status::NeedsAdmission);
            };
            if job.ticket.packet != packet || !token(&job.ticket.attempt_id) {
                return fail(ErrorKind::Stale, "graph-original-task-packet");
            }
            artifact(&job.ticket.native_binding)?;
            artifact(&job.ticket.lifecycle_tool)?;
            match job.state {
                handoff::State::Waiting => {
                    if job.result.is_some() || job.acknowledgement.is_some() {
                        return fail(ErrorKind::Integrity, "graph-waiting-control");
                    }
                    observer.verify(&job)?;
                    frontier.waiting.insert(id.clone(), packet.clone());
                    Ok(Status::Waiting)
                }
                handoff::State::ImportPending => {
                    let result = job.result.as_ref().ok_or(Error {
                        kind: ErrorKind::Missing,
                        code: "graph-pending-result".into(),
                    })?;
                    check_result(&packet, result)?;
                    if result.attempt_id != job.ticket.attempt_id || job.acknowledgement.is_some() {
                        return fail(ErrorKind::Integrity, "graph-pending-control");
                    }
                    Ok(Status::NeedsRecovery)
                }
                handoff::State::Cancelled => {
                    if job.result.is_some() || job.acknowledgement.is_some() {
                        return fail(ErrorKind::Integrity, "graph-cancelled-control");
                    }
                    Ok(Status::Cancelled)
                }
                handoff::State::Retained => {
                    let result = job.result.as_ref().ok_or(Error {
                        kind: ErrorKind::Missing,
                        code: "graph-original-result".into(),
                    })?;
                    let ack = job.acknowledgement.as_ref().ok_or(Error {
                        kind: ErrorKind::Missing,
                        code: "graph-retained-acknowledgement".into(),
                    })?;
                    artifact(ack)?;
                    check_result(&packet, result)?;
                    if result.attempt_id != job.ticket.attempt_id {
                        return fail(ErrorKind::Conflict, "graph-original-attempt");
                    }
                    let raw = serde_json::to_vec(result).map_err(|_| Error {
                        kind: ErrorKind::Invalid,
                        code: "graph-result-json".into(),
                    })?;
                    result_bytes = result_bytes.checked_add(raw.len()).ok_or(Error {
                        kind: ErrorKind::Budget,
                        code: "graph-result-overflow".into(),
                    })?;
                    if raw.len() > MAX_CONTROL_BYTES || result_bytes > 32 * MAX_CONTROL_BYTES {
                        return fail(ErrorKind::Budget, "graph-result-budget");
                    }
                    observer.verify(&job)?;
                    completed.insert(
                        id.clone(),
                        DependencyResult {
                            result_sha256: canonical_digest(result)?,
                            artifacts: result.artifacts.clone(),
                        },
                    );
                    Ok(Status::RetainedObserved)
                }
            }
        })();
        match observed {
            Ok(status) => row.status = status,
            Err(error) => {
                row.status = Status::Refused;
                row.error = Some(safe_error(error));
            }
        }
        frontier.rows.push(row);
    }
    Ok(frontier)
}
