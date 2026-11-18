//! Native-backed frontier inspection and explicit protected-input batch export.
//! No automatic admission, provider/worker invocation, recovery, selection or close.
use crate::{admission, error, handoff, io_error, staging};
use reel_workflow::{
    ErrorKind, Plan, Result,
    graph::{self, Job, Observer},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedFile {
    pub path: PathBuf,
    pub sha256: String,
}
impl PinnedFile {
    pub(crate) fn read(&self) -> Result<(Vec<u8>, File)> {
        if !self.path.is_absolute()
            || self
                .path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir))
        {
            return Err(error(ErrorKind::Invalid, "graph-explicit-control-path"));
        }
        let (raw, held) = staging::read_held(&self.path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
        if reel_workflow::digest(&raw) != self.sha256 {
            return Err(error(ErrorKind::Integrity, "graph-control-pin"));
        }
        Ok((raw, held))
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub journal: PathBuf,
    pub profile: PinnedFile,
    pub staging_ticket: Option<PinnedFile>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub schema: String,
    pub tasks: BTreeMap<String, Binding>,
}
struct Loaded {
    binding: Binding,
    profile: handoff::Profile,
    _held: File,
}
struct NativeObserver<'a> {
    bindings: &'a Bindings,
    loaded: BTreeMap<String, Loaded>,
}
impl NativeObserver<'_> {
    fn load(&mut self, id: &str) -> Result<Option<&Loaded>> {
        if !self.loaded.contains_key(id) {
            let Some(binding) = self.bindings.tasks.get(id) else {
                return Ok(None);
            };
            let (raw, held) = binding.profile.read()?;
            let profile: handoff::Profile = reel_workflow::decode(&raw)?;
            self.loaded.insert(
                id.into(),
                Loaded {
                    binding: binding.clone(),
                    profile,
                    _held: held,
                },
            );
        }
        Ok(self.loaded.get(id))
    }
}
impl Observer for NativeObserver<'_> {
    fn observe(&mut self, id: &str) -> Result<Option<Job>> {
        self.load(id)?
            .map(|job| handoff::observe(&job.binding.journal, &job.profile))
            .transpose()
    }
    fn verify(&mut self, observed: &Job) -> Result<()> {
        let job = self
            .load(&observed.ticket.packet.task.id)?
            .ok_or_else(|| error(ErrorKind::Missing, "graph-job-binding"))?;
        handoff::verify_observation(&job.binding.journal, &job.profile, observed)
    }
}
fn observer<'a>(plan: &Plan, bindings: &'a Bindings) -> Result<NativeObserver<'a>> {
    reel_workflow::check_plan(plan)?;
    if bindings.schema != "reel.workflow-run-bindings.v1"
        || bindings.tasks.len() > 128
        || bindings
            .tasks
            .keys()
            .any(|id| !plan.tasks.iter().any(|t| &t.id == id))
    {
        return Err(error(ErrorKind::Invalid, "graph-run-bindings"));
    }
    if serde_json::to_vec(bindings)
        .map_err(|_| error(ErrorKind::Invalid, "graph-bindings-json"))?
        .len()
        > reel_workflow::MAX_CONTROL_BYTES
    {
        return Err(error(ErrorKind::Budget, "graph-bindings-bytes"));
    }
    Ok(NativeObserver {
        bindings,
        loaded: BTreeMap::new(),
    })
}
pub fn inspect(plan: &Plan, bindings: &Bindings) -> Result<graph::Frontier> {
    graph::inspect(plan, &mut observer(plan, bindings)?)
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReadyTask {
    pub ticket: reel_workflow::handoff::Ticket,
    pub runtime: crate::Tool,
    pub journal: PathBuf,
    pub profile: PinnedFile,
    pub workspace: PathBuf,
    pub input_bindings: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Batch {
    pub schema: String,
    pub plan_sha256: String,
    pub tasks: Vec<ReadyTask>,
}
/// Return one manual result to its exact original job. No replacement attempt:
/// matching pending/retained results reconcile through the existing lifecycle.
/// Effects require execution by the ticket's original pinned lifecycle runtime.
pub fn import_ready(
    plan: &Plan,
    bindings: &Bindings,
    batch_pin: &PinnedFile,
    id: &str,
    result: reel_workflow::ReturnedResult,
    returns: &crate::response::Returns,
) -> Result<reel_workflow::handoff::State> {
    let selected = select(plan, bindings, batch_pin, id)?;
    if result.attempt_id != selected.task.ticket.attempt_id {
        return Err(error(ErrorKind::Conflict, "graph-original-batch-attempt"));
    }
    reel_workflow::check_result(&selected.task.ticket.packet, &result)?;
    if let Some(prior) = &selected.observed.result {
        reel_workflow::check_replay(prior, &result)?;
        handoff::resume(&selected.task.journal, &selected.profile, Some(returns))
    } else {
        handoff::import(&selected.task.journal, &selected.profile, result, returns)
    }
}
pub(crate) struct Selection {
    pub(crate) task: ReadyTask,
    pub(crate) profile: handoff::Profile,
    pub(crate) observed: Job,
    _pins: Vec<File>,
}
pub(crate) fn select(
    plan: &Plan,
    bindings: &Bindings,
    batch_pin: &PinnedFile,
    id: &str,
) -> Result<Selection> {
    let (raw, _batch_file) = batch_pin.read()?;
    let batch: Batch = reel_workflow::decode(&raw)?;
    if batch.schema != "reel.workflow-ready-batch.v1"
        || batch.plan_sha256 != reel_workflow::canonical_digest(plan)?
        || batch.tasks.is_empty()
        || batch.tasks.len() > 16
        || batch
            .tasks
            .iter()
            .map(|t| &t.ticket.packet.task.id)
            .collect::<BTreeSet<_>>()
            .len()
            != batch.tasks.len()
    {
        return Err(error(ErrorKind::Stale, "graph-original-batch-plan"));
    }
    let task = batch
        .tasks
        .iter()
        .find(|t| t.ticket.packet.task.id == id)
        .ok_or_else(|| error(ErrorKind::Missing, "graph-batch-task"))?;
    let mut observer = observer(plan, bindings)?;
    let frontier = graph::inspect(plan, &mut observer)?;
    let row = frontier
        .rows
        .iter()
        .find(|r| r.task_id == id)
        .ok_or_else(|| error(ErrorKind::Missing, "graph-run-task"))?;
    if !matches!(
        row.status,
        graph::Status::Waiting | graph::Status::NeedsRecovery | graph::Status::RetainedObserved
    ) || row.task_key.as_ref() != Some(&task.ticket.packet.task_key)
    {
        return Err(error(
            ErrorKind::Stale,
            "graph-batch-task-no-longer-current",
        ));
    }
    let job = observer
        .loaded
        .remove(id)
        .ok_or_else(|| error(ErrorKind::Missing, "graph-job-binding"))?;
    if job.binding.profile != task.profile
        || job.binding.journal != task.journal
        || job.profile.runtime.sha256 != task.runtime.sha256
        || job.profile.runtime.bytes != task.runtime.bytes
    {
        return Err(error(ErrorKind::Conflict, "graph-batch-job-binding"));
    }
    let observed = handoff::observe(&job.binding.journal, &job.profile)?;
    if observed.ticket != task.ticket {
        return Err(error(ErrorKind::Conflict, "graph-original-batch-ticket"));
    }
    let stage_pin = job
        .binding
        .staging_ticket
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "graph-original-staging-ticket"))?;
    let (stage_raw, stage_file) = stage_pin.read()?;
    let stage: staging::Ticket = reel_workflow::decode(&stage_raw)?;
    if !stage.matches(&job.profile.binding.admission)?
        || stage.directory() != task.workspace
        || task.input_bindings != task.workspace.join("input-bindings.json")
    {
        return Err(error(ErrorKind::Conflict, "graph-original-batch-workspace"));
    }
    Ok(Selection {
        task: task.clone(),
        profile: job.profile,
        observed,
        _pins: vec![_batch_file, job._held, stage_file],
    })
}
/// Explicit task selection, at most 16. Reopens original claims and checks all
/// named native bytes via owned staging before writing one exclusive private batch.
/// Batch bytes are instructions/evidence of this call, never a live custody lease.
pub fn export_ready(
    plan: &Plan,
    bindings: &Bindings,
    tasks: &[String],
    output: &Path,
) -> Result<usize> {
    if tasks.is_empty()
        || tasks.len() > 16
        || tasks.iter().collect::<BTreeSet<_>>().len() != tasks.len()
    {
        return Err(error(ErrorKind::Budget, "graph-export-task-count"));
    }
    if !output.is_absolute()
        || output
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(error(ErrorKind::Invalid, "graph-explicit-batch-path"));
    }
    let parent = reel_assets::jobs::open_workspace_directory(
        output
            .parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "graph-batch-parent"))?,
    )
    .map_err(|_| error(ErrorKind::Denied, "graph-batch-namespace"))?;
    let name = output
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| staging::portable(n))
        .ok_or_else(|| error(ErrorKind::Invalid, "graph-batch-name"))?;
    let output = parent.path().join(name);
    if std::fs::symlink_metadata(&output).is_ok() {
        return Err(error(ErrorKind::Conflict, "graph-existing-batch"));
    }
    let mut observer = observer(plan, bindings)?;
    let frontier = graph::inspect(plan, &mut observer)?;
    let mut batch = Batch {
        schema: "reel.workflow-ready-batch.v1".into(),
        plan_sha256: reel_workflow::canonical_digest(plan)?,
        tasks: Vec::new(),
    };
    let mut stage_bytes = Vec::new();
    let mut held = Vec::new();
    let mut controls = Vec::new();
    for id in tasks {
        let packet = frontier
            .waiting
            .get(id)
            .ok_or_else(|| error(ErrorKind::Conflict, "graph-task-not-verified-waiting"))?;
        let job = observer
            .loaded
            .get(id)
            .ok_or_else(|| error(ErrorKind::Missing, "graph-job-binding"))?;
        staging::check_parent(parent.path(), &job.profile.source, &job.profile.admission)?;
        for root in [
            &job.binding.journal,
            &job.profile.acknowledgements,
            &job.profile.source.snapshot.repository,
        ] {
            if parent
                .path()
                .starts_with(root.canonicalize().map_err(io_error)?)
            {
                return Err(error(
                    ErrorKind::Denied,
                    "graph-batch-overlaps-input-control",
                ));
            }
        }
        let pin = job
            .binding
            .staging_ticket
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "graph-staging-ticket-required"))?;
        let (raw, file) = pin.read()?;
        let ticket: staging::Ticket = reel_workflow::decode(&raw)?;
        if !ticket.matches(&job.profile.binding.admission)? {
            return Err(error(
                ErrorKind::Conflict,
                "graph-original-staging-admission",
            ));
        }
        let workspace = ticket.directory();
        if parent.path().starts_with(&workspace) {
            return Err(error(ErrorKind::Denied, "graph-batch-inside-workspace"));
        }
        let waiting = handoff::lock_waiting(&job.binding.journal, &job.profile, packet)?;
        batch.tasks.push(ReadyTask {
            ticket: waiting.ticket.clone(),
            runtime: job.profile.runtime.clone(),
            journal: job.binding.journal.clone(),
            profile: job.binding.profile.clone(),
            input_bindings: workspace.join("input-bindings.json"),
            workspace,
        });
        controls.push(waiting);
        stage_bytes.push((id.clone(), raw, pin.sha256.clone()));
        held.push(file);
    }
    let raw = serde_json::to_vec_pretty(&batch)
        .map_err(|_| error(ErrorKind::Invalid, "graph-batch-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "graph-batch-bytes"));
    }
    let mut claims = Vec::new();
    for (id, raw, pin) in &stage_bytes {
        let job = &observer.loaded[id];
        staging::export(
            &frontier.waiting[id],
            &job.profile.source,
            &job.profile.admission,
            raw,
            pin,
        )?;
        claims.push(
            admission::Prepared::open(
                &frontier.waiting[id],
                &job.profile.source,
                &job.profile.admission,
                &job.profile.binding.admission.attempt_id,
            )?
            .reopen(&job.profile.binding.admission)?,
        );
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
        .map_err(io_error)?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "graph-batch-write-preserve"))?;
    drop(file);
    let (written, _guard) = staging::read_held(&output, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if written != raw {
        return Err(error(ErrorKind::Integrity, "graph-batch-readback"));
    }
    Ok(batch.tasks.len())
}
