//! Protected input staging, manual handoff or existing deterministic worker.
use crate::{
    advancement::{persist, raw},
    error, graph, handoff, io_error, staging,
};
use reel_workflow::{ErrorKind, Operation, Plan, Result, ToolCapability, handoff::State};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub input_staging: staging::Profile,
    pub worker_staging: Option<staging::Profile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<crate::provider::Config>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub auxiliary_programs: std::collections::BTreeMap<String, crate::Tool>,
}
impl Config {
    pub(crate) fn check_operation(
        &self,
        operation: Operation,
        capability: Option<ToolCapability>,
    ) -> Result<()> {
        if self.schema != "reel.workflow-dispatch.v1"
            || self.worker_staging.is_some() != (operation == Operation::DeterministicTool)
            || (self.provider.is_some() && self.worker_staging.is_some())
            || (!self.auxiliary_programs.is_empty() && self.worker_staging.is_none())
        {
            return Err(error(ErrorKind::Invalid, "dispatch-operation-mode"));
        }
        if capability.is_some_and(|c| {
            !(matches!(
                c,
                ToolCapability::ComicRender
                    | ToolCapability::ComicIssueAssembly
                    | ToolCapability::ComicIssueControl
                    | ToolCapability::ComicReviewControl
                    | ToolCapability::FilmSceneControl
            ) || (cfg!(all(windows, feature = "film")) && c == ToolCapability::FilmRender))
        }) {
            return Err(error(
                ErrorKind::Unsupported,
                "dispatch-qualified-tool-required",
            ));
        }
        Ok(())
    }
}
#[derive(Serialize)]
pub struct Summary {
    pub task_id: String,
    pub state: State,
    pub batch: Option<graph::PinnedFile>,
    pub already_ready: bool,
    pub provider_called: bool,
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let (bytes, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    Ok(bytes)
}
fn pin(path: &Path) -> Result<graph::PinnedFile> {
    Ok(graph::PinnedFile {
        path: path.into(),
        sha256: reel_workflow::digest(&read(path)?),
    })
}
fn saved_pin(path: &Path, control: &Path) -> Result<graph::PinnedFile> {
    let pin = pin(path)?;
    persist(control, &raw(&pin)?)?;
    Ok(pin)
}
fn check_config(
    packet: &reel_workflow::Packet,
    profile: &handoff::Profile,
    config: &Config,
) -> Result<()> {
    config.check_operation(
        packet.task.operation,
        packet.task.tool.as_ref().map(|t| t.capability),
    )?;
    staging::worker::validate_programs(packet, &config.auxiliary_programs)?;
    if let Some(provider) = &config.provider {
        crate::provider::check(packet, provider)?;
    }
    if let Some(tool) = &packet.task.tool
        && (tool.executable.sha256 != profile.runtime.sha256
            || tool.executable.bytes != profile.runtime.bytes)
    {
        return Err(error(
            ErrorKind::Unsupported,
            "dispatch-qualified-tool-required",
        ));
    }
    Ok(())
}
fn guard(parent: &Path, profile: &handoff::Profile, journal: &Path) -> Result<()> {
    staging::check_parent(parent, &profile.source, &profile.admission)?;
    for root in [
        journal,
        &profile.acknowledgements,
        &profile.source.snapshot.repository,
    ] {
        if parent.starts_with(root.canonicalize().map_err(io_error)?) {
            return Err(error(ErrorKind::Denied, "dispatch-control-overlap"));
        }
    }
    Ok(())
}
/// A replay verifies original controls and work. It never creates a new attempt
/// after an uncertain reservation, producer result or native publication.
pub fn run(
    plan: &Plan,
    bindings: &graph::Bindings,
    id: &str,
    config: &Config,
    directory: &Path,
) -> Result<Summary> {
    let binding = bindings
        .tasks
        .get(id)
        .ok_or_else(|| error(ErrorKind::Missing, "dispatch-job-binding"))?;
    let (profile_raw, _profile_pin) = binding.profile.read()?;
    let profile: handoff::Profile = reel_workflow::decode(&profile_raw)?;
    let observed = handoff::observe(&binding.journal, &profile)?;
    let packet = &observed.ticket.packet;
    if reel_workflow::make_packet(plan, id, packet.dependencies.clone())? != *packet {
        return Err(error(ErrorKind::Stale, "dispatch-original-plan"));
    }
    check_config(packet, &profile, config)?;
    let _runtime = handoff::runtime(&profile.runtime)?;
    let frontier = graph::inspect(plan, bindings)?;
    let row = frontier
        .rows
        .iter()
        .find(|r| r.task_id == id)
        .ok_or_else(|| error(ErrorKind::Missing, "dispatch-frontier-task"))?;
    if row.status == reel_workflow::graph::Status::RetainedObserved {
        return Ok(Summary {
            task_id: id.into(),
            state: State::Retained,
            batch: None,
            already_ready: true,
            provider_called: false,
        });
    }
    if !matches!(
        row.status,
        reel_workflow::graph::Status::Waiting | reel_workflow::graph::Status::NeedsRecovery
    ) || !matches!(observed.state, State::Waiting | State::ImportPending)
    {
        return Err(error(ErrorKind::Conflict, "dispatch-original-job-state"));
    }
    if !directory.is_absolute()
        || directory
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(error(ErrorKind::Invalid, "dispatch-directory-path"));
    }
    let parent = reel_assets::jobs::open_workspace_directory(
        directory
            .parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "dispatch-directory-parent"))?,
    )
    .map_err(|_| error(ErrorKind::Denied, "dispatch-directory-parent"))?;
    let name = directory
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| staging::portable(n))
        .ok_or_else(|| error(ErrorKind::Invalid, "dispatch-directory-name"))?;
    let directory = parent.path().join(name);
    // Every run binding is an input/control namespace, not scratch space.
    let mut controls = Vec::new();
    let mut held = Vec::new();
    for binding in bindings.tasks.values() {
        let (bytes, file) = binding.profile.read()?;
        let p: handoff::Profile = reel_workflow::decode(&bytes)?;
        guard(parent.path(), &p, &binding.journal)?;
        controls.push((p, binding.journal.clone()));
        held.push(file);
    }
    let mut scratch_guards = Vec::new();
    for p in std::iter::once(&config.input_staging)
        .chain(config.worker_staging.iter())
        .chain(config.provider.iter().map(|p| &p.workspace))
    {
        let scratch = reel_assets::jobs::open_workspace_directory(&p.parent)
            .map_err(|_| error(ErrorKind::Denied, "dispatch-scratch-parent"))?;
        for (p, journal) in &controls {
            guard(scratch.path(), p, journal)?;
        }
        if scratch.path().starts_with(&directory) || directory.starts_with(scratch.path()) {
            return Err(error(ErrorKind::Denied, "dispatch-scratch-control-overlap"));
        }
        scratch_guards.push(scratch);
    }
    let _writer = reel_assets::admission::lock(
        &profile
            .admission
            .scope
            .coordination_root
            .join("workflow-dispatch")
            .join(reel_workflow::digest(
                directory.to_string_lossy().as_bytes(),
            )),
    )
    .map_err(|_| error(ErrorKind::Busy, "dispatch-writer-busy"))?;
    let created = match fs::create_dir(&directory) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(io_error(e)),
    };
    let owned = reel_assets::jobs::open_workspace_directory(&directory)
        .map_err(|_| error(ErrorKind::Denied, "dispatch-owned-directory"))?;
    let directory = owned.path();
    let intent = directory.join("intent.json");
    if !created && !intent.exists() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "dispatch-original-intent-missing",
        ));
    }
    persist(
        &intent,
        &raw(
            &serde_json::json!({"schema":"reel.workflow-dispatch-intent.v1","plan":reel_workflow::canonical_digest(plan)?,"bindings":reel_workflow::canonical_digest(bindings)?,"config":reel_workflow::canonical_digest(config)?,"task_key":packet.task_key,"attempt":observed.ticket.attempt_id,"directory_identity":owned.identity()}),
        )?,
    )?;
    let stage_path = directory.join("input-ticket.json");
    if !stage_path.exists() {
        if observed.state != State::Waiting {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "dispatch-original-input-ticket-missing",
            ));
        }
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&stage_path)
            .map_err(io_error)?;
        let ticket = staging::prepare(
            packet,
            &profile.source,
            &profile.admission,
            &profile.binding.admission,
            &config.input_staging,
        )?;
        file.write_all(&ticket.canonical_bytes()?)
            .and_then(|_| file.sync_all())
            .map_err(|_| error(ErrorKind::NeedsRecovery, "dispatch-input-ticket-preserve"))?;
    }
    let stage_pin = saved_pin(&stage_path, &directory.join("input-ticket-pin.json"))?;
    let (stage_raw, _stage_held) = stage_pin.read()?;
    let stage: staging::Ticket = reel_workflow::decode(&stage_raw)?;
    if !stage.matches(&profile.binding.admission)? {
        return Err(error(ErrorKind::Conflict, "dispatch-original-input-ticket"));
    }
    let mut staged: graph::Bindings = reel_workflow::decode(&raw(bindings)?)?;
    staged
        .tasks
        .get_mut(id)
        .ok_or_else(|| error(ErrorKind::Missing, "dispatch-job-binding"))?
        .staging_ticket = Some(stage_pin);
    persist(&directory.join("plan.json"), &raw(plan)?)?;
    persist(&directory.join("bindings.json"), &raw(&staged)?)?;
    let batch_path = directory.join("ready.json");
    let already_ready = batch_path.exists();
    if !already_ready {
        graph::export_ready(plan, &staged, &[id.into()], &batch_path)?;
    }
    let batch = saved_pin(&batch_path, &directory.join("ready-pin.json"))?;
    drop(graph::select(plan, &staged, &batch, id)?);
    let mut provider_called = false;
    let state = if let Some(worker) = &config.worker_staging {
        let path = directory.join("worker-ticket.json");
        if !path.exists() {
            staging::worker::prepare_file_with_programs(
                plan,
                &staged,
                &batch,
                id,
                worker,
                &path,
                &config.auxiliary_programs,
            )?;
        }
        let worker_pin = saved_pin(&path, &directory.join("worker-ticket-pin.json"))?;
        staging::worker::run_file(plan, &staged, &path, &worker_pin.sha256)?
    } else if let Some(provider) = &config.provider {
        let path = directory.join("provider-ticket.json");
        if !path.exists() {
            crate::provider::prepare_file(plan, &staged, &batch, id, provider, &path)?;
        }
        let provider_pin = saved_pin(&path, &directory.join("provider-ticket-pin.json"))?;
        let outcome = crate::provider::run_file(plan, &staged, &path, &provider_pin.sha256)?;
        provider_called = outcome.called;
        outcome.state
    } else {
        if observed.state != State::Waiting {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "dispatch-manual-response-pending",
            ));
        }
        // Recheck the original owned input export on a repeated manual handoff.
        staging::export(
            packet,
            &profile.source,
            &profile.admission,
            &stage_raw,
            &reel_workflow::digest(&stage_raw),
        )?;
        State::Waiting
    };
    Ok(Summary {
        task_id: id.into(),
        state,
        batch: Some(batch),
        already_ready,
        provider_called,
    })
}
