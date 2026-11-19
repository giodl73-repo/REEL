//! Owned deterministic comic dispatch. No arbitrary commands or provider calls.
//! The original pinned executable must host both production and lifecycle effects.
use super::*;
use crate::{graph, handoff};
use reel_workflow::handoff::State;
use reel_workflow::{Mode, Plan as WorkflowPlan, Provenance, ReturnedResult, ToolCapability};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    profile: Profile,
    reservation: Reservation,
    owner: Owner,
    admission: Receipt,
    batch: graph::PinnedFile,
    task_id: String,
    packet: Packet,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    auxiliary_programs: BTreeMap<String, crate::Tool>,
}
pub fn validate_programs(packet: &Packet, programs: &BTreeMap<String, crate::Tool>) -> Result<()> {
    reel_workflow::check_packet(packet)?;
    let Some(tool) = &packet.task.tool else {
        return if programs.is_empty() {
            Ok(())
        } else {
            Err(error(ErrorKind::Invalid, "worker-auxiliary-tool-required"))
        };
    };
    if tool.auxiliary_executables.len() != programs.len()
        || tool.auxiliary_executables.iter().any(|(role, a)| {
            programs
                .get(role)
                .is_none_or(|p| p.sha256 != a.sha256 || p.bytes != a.bytes)
        })
    {
        return Err(error(
            ErrorKind::Conflict,
            "worker-auxiliary-program-binding",
        ));
    }
    Ok(())
}
struct ProgramGuards {
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
fn hold_programs(
    packet: &Packet,
    programs: &BTreeMap<String, crate::Tool>,
) -> Result<ProgramGuards> {
    validate_programs(packet, programs)?;
    let mut files = Vec::new();
    let mut directories = Vec::new();
    for program in programs.values() {
        if !program.path.is_absolute()
            || program.path.components().count() > 128
            || program.path.as_os_str().len() > 4096
        {
            return Err(error(ErrorKind::Invalid, "worker-auxiliary-path-bound"));
        }
        directories.push(
            open_workspace_directory(
                program
                    .path
                    .parent()
                    .ok_or_else(|| error(ErrorKind::Invalid, "worker-auxiliary-parent"))?,
            )
            .map_err(|_| error(ErrorKind::Denied, "worker-auxiliary-namespace"))?,
        );
        files.push(crate::hold_tool(program)?);
    }
    Ok(ProgramGuards {
        _files: files,
        _directories: directories,
    })
}
impl Ticket {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        json(self)
    }
    pub fn directory(&self) -> PathBuf {
        self.profile.parent.join(&self.owner.name)
    }
}
fn context(selected: &graph::Selection) -> Result<&reel_workflow::ToolContract> {
    let packet = &selected.task.ticket.packet;
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Unsupported, "worker-deterministic-tool-required"))?;
    if packet.task.operation != reel_workflow::Operation::DeterministicTool
        || !matches!(
            tool.capability,
            ToolCapability::ComicRender
                | ToolCapability::ComicIssueAssembly
                | ToolCapability::ComicIssueControl
                | ToolCapability::FilmSceneControl
        )
        || tool.executable.sha256 != selected.profile.runtime.sha256
        || tool.executable.bytes != selected.profile.runtime.bytes
    {
        return Err(error(
            ErrorKind::Unsupported,
            "worker-current-qualified-comic-runtime-required",
        ));
    }
    Ok(tool)
}
fn intent(
    packet: &Packet,
    receipt: &Receipt,
    batch: &graph::PinnedFile,
    programs: &BTreeMap<String, crate::Tool>,
) -> Result<Plan> {
    let mut identity = serde_json::json!({"schema":"reel.workflow-worker-operation.v1",
        "admission":receipt.operation_identity()?,"task_key":packet.task_key,"attempt":receipt.attempt_id,
        "executable":packet.task.tool.as_ref().map(|t|&t.executable)});
    let mut invocation = serde_json::json!({"schema":"reel.workflow-worker-invocation.v1","packet":packet,"admission":receipt,"batch":batch});
    if !programs.is_empty() {
        let value = serde_json::to_value(programs)
            .map_err(|_| error(ErrorKind::Invalid, "worker-auxiliary-json"))?;
        identity["auxiliary_programs"] = value.clone();
        invocation["auxiliary_programs"] = value;
    }
    let operation = reel_workflow::digest(&json(&identity)?);
    let controls: BTreeMap<String, Vec<u8>> =
        BTreeMap::from([("invocation.json".into(), json(&invocation)?)]);
    let files = controls
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                Entry {
                    object: Object::from_bytes(raw),
                    input: None,
                },
            )
        })
        .collect();
    Ok(Plan {
        manifest: Manifest {
            schema: "reel.workflow-worker-intent.v1".into(),
            operation,
            files,
        },
        controls,
    })
}
pub fn prepare(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    batch: &graph::PinnedFile,
    id: &str,
    profile: &Profile,
) -> Result<Ticket> {
    prepare_with_programs(plan, bindings, batch, id, profile, &BTreeMap::new())
}
pub fn prepare_with_programs(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    batch: &graph::PinnedFile,
    id: &str,
    profile: &Profile,
    programs: &BTreeMap<String, crate::Tool>,
) -> Result<Ticket> {
    let selected = graph::select(plan, bindings, batch, id)?;
    context(&selected)?;
    let _runtime = handoff::runtime(&selected.profile.runtime)?;
    let _programs = hold_programs(&selected.task.ticket.packet, programs)?;
    if selected.observed.state != State::Waiting {
        return Err(error(
            ErrorKind::Conflict,
            "worker-original-waiting-job-required",
        ));
    }
    check_parent(
        &profile.parent,
        &selected.profile.source,
        &selected.profile.admission,
    )?;
    let actual_parent = profile.parent.canonicalize().map_err(io_error)?;
    for root in [
        &selected.task.journal,
        &selected.profile.acknowledgements,
        &selected.profile.source.snapshot.repository,
        &selected.task.workspace,
    ] {
        if actual_parent.starts_with(root.canonicalize().map_err(io_error)?) {
            return Err(error(
                ErrorKind::Denied,
                "worker-parent-overlaps-input-control",
            ));
        }
    }
    let _control = handoff::lock_waiting(
        &selected.task.journal,
        &selected.profile,
        &selected.task.ticket.packet,
    )?;
    let claims = admission::Prepared::open(
        &selected.task.ticket.packet,
        &selected.profile.source,
        &selected.profile.admission,
        &selected.task.ticket.attempt_id,
    )?
    .reopen(&selected.profile.binding.admission)?;
    let mut inputs = NativeInputs::from_claims(
        &selected.task.ticket.packet,
        &selected.profile.source,
        claims,
    )?;
    let output_bytes = crate::producer::output_budget(&selected.task.ticket.packet, &mut inputs)?;
    let required = output_bytes
        .checked_mul(2)
        .and_then(|n| n.checked_add(10 * reel_workflow::MAX_CONTROL_BYTES as u64))
        .ok_or_else(|| error(ErrorKind::Budget, "worker-reservation-overflow"))?;
    if required > profile.maximum_bytes {
        return Err(error(
            ErrorKind::Budget,
            "worker-output-and-evidence-reservation",
        ));
    }
    let base = intent(
        &selected.task.ticket.packet,
        &selected.profile.binding.admission,
        batch,
        programs,
    )?;
    let (profile, reservation, owner) = prepare_owned(
        profile,
        &selected.profile.admission.scope.coordination_root,
        &base,
    )?;
    Ok(Ticket {
        schema: "reel.workflow-worker-ticket.v1".into(),
        profile,
        reservation,
        owner,
        admission: selected.profile.binding.admission.clone(),
        batch: batch.clone(),
        task_id: id.into(),
        packet: selected.task.ticket.packet.clone(),
        auxiliary_programs: programs.clone(),
    })
}
fn result_plan(base: &Plan, mut controls: BTreeMap<String, Vec<u8>>) -> Result<Plan> {
    let files: BTreeMap<_, _> = controls
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                Entry {
                    object: Object::from_bytes(raw),
                    input: None,
                },
            )
        })
        .collect();
    check_entries(&files)?;
    let manifest = Manifest {
        schema: "reel.workflow-worker-result-manifest.v1".into(),
        operation: base.manifest.operation.clone(),
        files,
    };
    controls.insert("result-manifest.json".into(), json(&manifest)?);
    let files = controls
        .iter()
        .map(|(name, raw)| {
            (
                name.clone(),
                Entry {
                    object: Object::from_bytes(raw),
                    input: None,
                },
            )
        })
        .collect();
    check_entries(&files)?;
    Ok(Plan {
        manifest: Manifest { files, ..manifest },
        controls,
    })
}
fn completed(
    view: &mut View,
    ticket: &Ticket,
    base: &Plan,
    pin: &str,
) -> Result<(ReturnedResult, crate::response::Returns)> {
    let (raw, held) = read_held(
        &view.root.path().join("result-manifest.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let manifest: Manifest = reel_workflow::decode(&raw)?;
    if manifest.schema != "reel.workflow-worker-result-manifest.v1"
        || manifest.operation != base.manifest.operation
        || manifest.files.get("invocation.json") != base.manifest.files.get("invocation.json")
        || !manifest.files.contains_key("result.json")
        || !manifest.files.contains_key("returns.json")
        || manifest.files.iter().any(|(name, entry)| {
            entry.input.is_some()
                || !(name == "invocation.json"
                    || name == "result.json"
                    || name == "returns.json"
                    || name.starts_with("packet/"))
        })
    {
        return Err(error(
            ErrorKind::Integrity,
            "worker-result-manifest-binding",
        ));
    }
    let mut manifest = manifest;
    manifest.files.insert(
        "result-manifest.json".into(),
        Entry {
            object: Object::from_bytes(&raw),
            input: None,
        },
    );
    check_entries(&manifest.files)?;
    let plan = Plan {
        manifest,
        controls: BTreeMap::new(),
    };
    checked_budget(&ticket.profile, &plan)?;
    if !scan(view, &ticket.profile, &plan, &completion(&plan, pin)?)? {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "worker-materialization-incomplete",
        ));
    }
    view.files.push(held);
    let (raw, result_guard) = read_held(
        &view.root.path().join("result.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let result: ReturnedResult = reel_workflow::decode(&raw)?;
    reel_workflow::check_result(&ticket.packet, &result)?;
    if result.attempt_id != ticket.admission.attempt_id {
        return Err(error(ErrorKind::Conflict, "worker-original-result-attempt"));
    }
    let (raw, returns_guard) = read_held(
        &view.root.path().join("returns.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    let returns: crate::response::Returns = reel_workflow::decode(&raw)?;
    if returns.schema != "reel.workflow-returns.v1"
        || returns.root != view.root.path().join("packet")
    {
        return Err(error(ErrorKind::Integrity, "worker-owned-return-binding"));
    }
    let files = plan
        .manifest
        .files
        .iter()
        .filter_map(|(path, entry)| {
            path.strip_prefix("packet/")
                .map(|relative| (relative.to_owned(), entry.object.clone()))
        })
        .collect();
    crate::output_layout::check(
        &result.artifacts,
        result
            .worker_receipt
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Missing, "worker-produced-receipt"))?,
        &returns.files,
        &files,
    )?;
    view.files.extend([result_guard, returns_guard]);
    Ok((result, returns))
}
/// Pure production may be repeated under the same immutable intent after an
/// interruption. Pending native effects reconcile existing materialized bytes;
/// they never launch a replacement attempt or silently repair incomplete output.
pub fn run(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    ticket_raw: &[u8],
    pin: &str,
) -> Result<State> {
    execute(plan, bindings, ticket_raw, pin, true)
}
fn execute(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    ticket_raw: &[u8],
    pin: &str,
    finish: bool,
) -> Result<State> {
    if reel_workflow::digest(ticket_raw) != pin {
        return Err(error(ErrorKind::Integrity, "worker-ticket-pin"));
    }
    let ticket: Ticket = reel_workflow::decode(ticket_raw)?;
    if ticket.schema != "reel.workflow-worker-ticket.v1" {
        return Err(error(ErrorKind::Invalid, "worker-ticket-schema"));
    }
    let selected = graph::select(plan, bindings, &ticket.batch, &ticket.task_id)?;
    let tool = context(&selected)?;
    let _runtime = handoff::runtime(&selected.profile.runtime)?;
    let _programs = hold_programs(&ticket.packet, &ticket.auxiliary_programs)?;
    if selected.task.ticket.packet != ticket.packet
        || selected.profile.binding.admission.operation_identity()?
            != ticket.admission.operation_identity()?
    {
        return Err(error(ErrorKind::Stale, "worker-original-job-context"));
    }
    check_parent(
        &ticket.profile.parent,
        &selected.profile.source,
        &selected.profile.admission,
    )?;
    let base = intent(
        &ticket.packet,
        &ticket.admission,
        &ticket.batch,
        &ticket.auxiliary_programs,
    )?;
    let config = configuration(
        &ticket.profile,
        &selected.profile.admission.scope.coordination_root,
    )?;
    let mut view = open_owner(&ticket.profile, &ticket.owner, &base)?;
    let _reservation = capacity::resume_preserved_on(
        &config,
        &ticket.profile.parent,
        &ticket.reservation.id,
        &ticket.reservation.volume,
        ticket.profile.maximum_bytes,
    )
    .map_err(scoped_error)?;
    let incomplete = match fs::symlink_metadata(view.root.path().join("complete.json")) {
        Ok(_) => false,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(e) => return Err(io_error(e)),
    };
    if selected.observed.state == State::Waiting && incomplete {
        let control =
            handoff::lock_waiting(&selected.task.journal, &selected.profile, &ticket.packet)?;
        let claims = admission::Prepared::open(
            &ticket.packet,
            &selected.profile.source,
            &selected.profile.admission,
            &ticket.admission.attempt_id,
        )?
        .reopen(&ticket.admission)?;
        let native = NativeInputs::from_claims(&ticket.packet, &selected.profile.source, claims)?;
        let generated = crate::producer::generate(&ticket.packet, native)?;
        let receipt_raw = generated
            .files()
            .get("receipt.json")
            .ok_or_else(|| error(ErrorKind::Missing, "worker-produced-receipt"))?;
        let result = ReturnedResult {
            schema: reel_workflow::RESULT_SCHEMA.into(),
            task_id: ticket.task_id.clone(),
            task_key: ticket.packet.task_key.clone(),
            attempt_id: ticket.admission.attempt_id.clone(),
            artifacts: generated.outputs().clone(),
            provenance: Provenance {
                mode: Mode::Tool,
                provider: None,
                model: None,
                executable: Some(tool.executable.clone()),
            },
            worker_receipt: Some(Artifact {
                logical_id: format!(
                    "workflow/worker/{}/{}/receipt",
                    ticket.packet.task_key, ticket.admission.attempt_id
                ),
                sha256: reel_workflow::digest(receipt_raw),
                bytes: receipt_raw.len() as u64,
                media_type: "application/json".into(),
            }),
        };
        reel_workflow::check_result(&ticket.packet, &result)?;
        let returns = crate::response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: view.root.path().join("packet"),
            files: generated.paths()?,
        };
        let mut controls = base.controls.clone();
        controls.extend(
            generated
                .files()
                .iter()
                .map(|(name, raw)| (format!("packet/{name}"), raw.clone())),
        );
        controls.insert("result.json".into(), json(&result)?);
        controls.insert("returns.json".into(), json(&returns)?);
        let produced = result_plan(&base, controls)?;
        checked_budget(&ticket.profile, &produced)?;
        materialize(
            &mut view,
            &ticket.profile,
            &produced,
            &Buffers(BTreeMap::new()),
            pin,
            &mut |_| Ok(()),
        )?;
        drop(generated);
        drop(control);
    }
    let (result, returns) = completed(&mut view, &ticket, &base, pin)?;
    // Keep owned output/file guards and capacity through native import, but drop
    // all source/object gates before the original lifecycle admits new work.
    if !finish {
        return Ok(selected.observed.state);
    }
    graph::import_ready(
        plan,
        bindings,
        &ticket.batch,
        &ticket.task_id,
        result,
        &returns,
    )
}
pub fn materialize_file(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    path: &Path,
    pin: &str,
) -> Result<()> {
    let (raw, _held) = read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    execute(plan, bindings, &raw, pin, false).map(|_| ())
}
pub fn run_file(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    path: &Path,
    pin: &str,
) -> Result<State> {
    let (raw, _held) = read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    run(plan, bindings, &raw, pin)
}
pub fn prepare_file(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    batch: &graph::PinnedFile,
    id: &str,
    profile: &Profile,
    path: &Path,
) -> Result<()> {
    prepare_file_with_programs(plan, bindings, batch, id, profile, path, &BTreeMap::new())
}
#[allow(clippy::too_many_arguments)]
pub fn prepare_file_with_programs(
    plan: &WorkflowPlan,
    bindings: &graph::Bindings,
    batch: &graph::PinnedFile,
    id: &str,
    profile: &Profile,
    path: &Path,
    programs: &BTreeMap<String, crate::Tool>,
) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(error(ErrorKind::Invalid, "worker-explicit-ticket-path"));
    }
    let selected = graph::select(plan, bindings, batch, id)?;
    let _programs = hold_programs(&selected.task.ticket.packet, programs)?;
    let parent = open_workspace_directory(
        path.parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "worker-ticket-parent"))?,
    )
    .map_err(scoped_error)?;
    check_parent(
        parent.path(),
        &selected.profile.source,
        &selected.profile.admission,
    )?;
    for root in [
        &selected.task.journal,
        &selected.profile.acknowledgements,
        &selected.profile.source.snapshot.repository,
        &selected.task.workspace,
    ] {
        if parent
            .path()
            .starts_with(root.canonicalize().map_err(io_error)?)
        {
            return Err(error(
                ErrorKind::Denied,
                "worker-ticket-overlaps-input-control",
            ));
        }
    }
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| portable(n))
        .ok_or_else(|| error(ErrorKind::Invalid, "worker-ticket-name"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(parent.path().join(name))
        .map_err(io_error)?;
    let ticket = prepare_with_programs(plan, bindings, batch, id, profile, programs)?;
    file.write_all(&ticket.canonical_bytes()?)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "worker-ticket-write-preserve"))
}
