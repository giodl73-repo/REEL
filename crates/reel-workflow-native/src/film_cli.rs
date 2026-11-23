//! Owned film CLI orchestration over the existing capacity and native lifecycle.
use crate::{
    NativeInputs, NativeProfile, Tool, admission, error, film_inputs::Programs, film_task, io_error,
};
use reel_assets::{
    admission as capacity,
    jobs::{WorkspaceDirectory, open_workspace_directory},
};
use reel_workflow::{ErrorKind, Packet, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const CONTROL_RESERVE: u64 = 4 * 1024 * 1024;
fn capacity_error(value: Box<dyn std::error::Error>) -> reel_workflow::Error {
    let kind = if value.downcast_ref::<capacity::MetadataBusy>().is_some() {
        ErrorKind::Busy
    } else if value.downcast_ref::<capacity::AdmissionDenied>().is_some() {
        ErrorKind::Budget
    } else if let Some(io) = value.downcast_ref::<std::io::Error>() {
        match io.kind() {
            std::io::ErrorKind::NotFound => ErrorKind::Missing,
            std::io::ErrorKind::PermissionDenied => ErrorKind::Denied,
            std::io::ErrorKind::WouldBlock => ErrorKind::Busy,
            _ if io.raw_os_error() == Some(33) => ErrorKind::Busy,
            _ => ErrorKind::NeedsRecovery,
        }
    } else {
        ErrorKind::NeedsRecovery
    };
    error(kind, "film-cli-capacity-state")
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceProfile {
    pub parent: PathBuf,
    pub maximum_bytes: u64,
    pub capacity_policy: capacity::Policy,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub packet: Packet,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub receipt: admission::Receipt,
    pub runtime: Tool,
    pub programs: Programs,
    pub workspace: WorkspaceProfile,
    pub retained: crate::retention::Target,
    pub close_operation: tig_store::Key,
    /// Optional exact still-repair intent. Old requests serialize unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_derivation: Option<crate::film_controls::Derivation>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    schema: String,
    request_sha256: String,
    operation_identity: String,
    parent: PathBuf,
    parent_identity: String,
    root_name: String,
    root_identity: String,
    work_identity: String,
    reservation: String,
    volume: String,
    maximum_bytes: u64,
    intent_sha256: String,
}
impl Ticket {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        admission::encode(self)
    }
}
pub fn read_ticket(path: &Path) -> Result<(Vec<u8>, File)> {
    crate::staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)
}
pub fn read(path: &Path, pin: &str) -> Result<(Request, File)> {
    let (raw, held) = crate::staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "film-cli-request-pin"));
    }
    let request: Request = reel_workflow::decode(&raw)?;
    if request.schema != "reel.workflow-film-cli.v1" {
        return Err(error(ErrorKind::Invalid, "film-cli-request-schema"));
    }
    reel_workflow::check_packet(&request.packet)?;
    Ok((request, held))
}
fn config(request: &Request, parent: &Path) -> Result<reel_assets::Config> {
    let profile = &request.workspace;
    profile
        .capacity_policy
        .validate()
        .map_err(|_| error(ErrorKind::Invalid, "film-cli-capacity-policy"))?;
    if profile.maximum_bytes < CONTROL_RESERVE
        || profile.maximum_bytes > 64 * 1024 * 1024 * 1024 + CONTROL_RESERVE
        || profile.maximum_bytes > profile.capacity_policy.max_object_bytes
    {
        return Err(error(ErrorKind::Budget, "film-cli-workspace-bounds"));
    }
    let mut policy = profile.capacity_policy.clone();
    if let Some(minimum) = policy.tier_minimum_free_bytes.get(&profile.parent) {
        policy.minimum_free_bytes = policy.minimum_free_bytes.max(*minimum);
    }
    Ok(reel_assets::Config {
        cache_roots: vec![parent.into()],
        coordination_root: Some(request.admission.scope.coordination_root.clone()),
        capacity: policy,
        ..reel_assets::Config::default()
    })
}
fn parent(request: &Request) -> Result<WorkspaceDirectory> {
    let parent = open_workspace_directory(&request.workspace.parent)
        .map_err(|_| error(ErrorKind::Denied, "film-cli-parent-namespace"))?;
    for managed in [
        &request.source.source.pool.repository,
        &request.admission.index_root,
        &request.admission.metadata_root,
        &request.admission.scope.coordination_root,
    ] {
        let managed = managed.canonicalize().map_err(io_error)?;
        if parent.path().starts_with(&managed) {
            return Err(error(ErrorKind::Denied, "film-cli-parent-managed"));
        }
    }
    Ok(parent)
}
fn persist_new(path: &Path, raw: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| error(ErrorKind::NeedsRecovery, "film-cli-control-preserve"))?;
    file.write_all(raw)
        .and_then(|_| file.sync_all())
        .map_err(io_error)
}
/// Explicit new owned workspace only. A preserved ticket is required on retry.
pub fn prepare(request: &Request, request_pin: &str) -> Result<Ticket> {
    let tool = request
        .packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "film-cli-film-tool"))?;
    for (pin, runtime) in [
        (Some(&tool.executable), &request.runtime),
        (
            tool.auxiliary_executables.get("ffmpeg"),
            &request.programs.ffmpeg,
        ),
        (
            tool.auxiliary_executables.get("ffprobe"),
            &request.programs.ffprobe,
        ),
    ] {
        if pin.is_none_or(|pin| pin.sha256 != runtime.sha256 || pin.bytes != runtime.bytes) {
            return Err(error(ErrorKind::Conflict, "film-cli-runtime-pin"));
        }
        drop(crate::hold_tool(runtime)?);
    }
    let claims = admission::Prepared::open(
        &request.packet,
        &request.source,
        &request.admission,
        &request.receipt.attempt_id,
    )?
    .reopen(&request.receipt)?;
    let mut native = NativeInputs::from_claims(&request.packet, &request.source, claims)?;
    let control_verification = request
        .control_derivation
        .as_ref()
        .map(|spec| crate::film_controls::verify(&request.packet, &mut native, spec))
        .transpose()?;
    let bound = film_task::bind(&request.packet, &mut native)?;
    let maximum = bound
        .request()
        .limits
        .workspace_bytes
        .checked_add(CONTROL_RESERVE)
        .ok_or_else(|| error(ErrorKind::Budget, "film-cli-workspace-overflow"))?;
    if maximum > request.workspace.maximum_bytes {
        return Err(error(
            ErrorKind::Budget,
            "film-cli-process-workspace-budget",
        ));
    }
    let parent = parent(request)?;
    let config = config(request, parent.path())?;
    let operation_identity = request.receipt.operation_identity()?;
    let name = format!("film-{operation_identity}");
    let root_path = parent.path().join(&name);
    if fs::symlink_metadata(&root_path).is_ok() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "film-cli-original-ticket-required",
        ));
    }
    let reservation = capacity::reserve_on(
        parent.path(),
        &config,
        parent.path(),
        request.workspace.maximum_bytes,
    )
    .map_err(capacity_error)?;
    let reservation_id = reservation.id.clone();
    let volume = reservation.volume_id.clone();
    // End the writer lease, preserving its conservative record even if directory
    // creation or ticket persistence fails. No implicit release/delete on error.
    reservation.retain_for_recovery();
    fs::create_dir(&root_path).map_err(io_error)?;
    let root = open_workspace_directory(&root_path)
        .map_err(|_| error(ErrorKind::Denied, "film-cli-root-namespace"))?;
    fs::create_dir(root.path().join("work")).map_err(io_error)?;
    let work = open_workspace_directory(&root.path().join("work"))
        .map_err(|_| error(ErrorKind::Denied, "film-cli-work-namespace"))?;
    let mut intent = serde_json::json!({"schema":"reel.workflow-film-owned-workspace.v1","request_sha256":request_pin,
        "operation_identity":operation_identity,"parent_identity":parent.identity(),"root_identity":root.identity(),"work_identity":work.identity()});
    if let Some(verified) = control_verification {
        intent["control_verification"] = serde_json::to_value(verified)
            .map_err(|_| error(ErrorKind::Invalid, "film-cli-control-verification-json"))?;
    }
    let intent = admission::encode(&intent)?;
    persist_new(&root.path().join("intent.json"), &intent)?;
    let ticket = Ticket {
        schema: "reel.workflow-film-workspace-ticket.v1".into(),
        request_sha256: request_pin.into(),
        operation_identity,
        parent: parent.path().into(),
        parent_identity: parent.identity().into(),
        root_name: name,
        root_identity: root.identity().into(),
        work_identity: work.identity().into(),
        reservation: reservation_id,
        volume,
        maximum_bytes: request.workspace.maximum_bytes,
        intent_sha256: reel_workflow::digest(&intent),
    };
    persist_new(&root.path().join("ticket.json"), &ticket.bytes()?)?;
    Ok(ticket)
}
struct Owned {
    _parent: WorkspaceDirectory,
    root: WorkspaceDirectory,
    work: WorkspaceDirectory,
    _controls: Vec<File>,
    _lease: capacity::Reservation,
}
fn open(
    request: &Request,
    request_pin: &str,
    ticket_raw: &[u8],
    ticket_pin: &str,
) -> Result<Owned> {
    if reel_workflow::digest(ticket_raw) != ticket_pin {
        return Err(error(ErrorKind::Integrity, "film-cli-ticket-pin"));
    }
    let ticket: Ticket = reel_workflow::decode(ticket_raw)?;
    if ticket.schema != "reel.workflow-film-workspace-ticket.v1"
        || ticket.request_sha256 != request_pin
        || ticket.operation_identity != request.receipt.operation_identity()?
        || ticket.root_name != format!("film-{}", ticket.operation_identity)
        || ticket.maximum_bytes != request.workspace.maximum_bytes
    {
        return Err(error(ErrorKind::Conflict, "film-cli-ticket-binding"));
    }
    let parent = parent(request)?;
    if parent.path() != ticket.parent || parent.identity() != ticket.parent_identity {
        return Err(error(ErrorKind::Conflict, "film-cli-parent-replaced"));
    }
    let root = open_workspace_directory(&parent.path().join(&ticket.root_name))
        .map_err(|_| error(ErrorKind::Denied, "film-cli-root-namespace"))?;
    let work = open_workspace_directory(&root.path().join("work"))
        .map_err(|_| error(ErrorKind::Denied, "film-cli-work-namespace"))?;
    if root.identity() != ticket.root_identity || work.identity() != ticket.work_identity {
        return Err(error(ErrorKind::Conflict, "film-cli-workspace-replaced"));
    }
    for entry in fs::read_dir(root.path()).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if !["intent.json", "ticket.json", "work", "close-outcome.json"]
            .iter()
            .any(|n| entry.file_name() == *n)
        {
            return Err(error(
                ErrorKind::Conflict,
                "film-cli-unexpected-owner-entry",
            ));
        }
    }
    let (intent, intent_file) = crate::staging::read_held(
        &root.path().join("intent.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if reel_workflow::digest(&intent) != ticket.intent_sha256 {
        return Err(error(ErrorKind::Integrity, "film-cli-owner-intent"));
    }
    let (stored, ticket_file) = crate::staging::read_held(
        &root.path().join("ticket.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if stored != ticket_raw {
        return Err(error(ErrorKind::Conflict, "film-cli-original-ticket"));
    }
    let lease = capacity::resume_preserved_on(
        &config(request, parent.path())?,
        parent.path(),
        &ticket.reservation,
        &ticket.volume,
        ticket.maximum_bytes,
    )
    .map_err(capacity_error)?;
    Ok(Owned {
        _parent: parent,
        root,
        work,
        _controls: vec![intent_file, ticket_file],
        _lease: lease,
    })
}
#[derive(Clone, Copy)]
pub enum Action {
    Run,
    Resume,
    Verify,
    PreviewClose,
    StageClose,
    Close,
    RecoverClose,
}
pub fn execute(
    request: &Request,
    request_pin: &str,
    ticket_raw: &[u8],
    ticket_pin: &str,
    action: Action,
) -> Result<serde_json::Value> {
    let owned = open(request, request_pin, ticket_raw, ticket_pin)?;
    match action {
        Action::Run | Action::Resume => {
            let invocation = film_task::Invocation {
                source: &request.source,
                receipt: &request.receipt,
                runtime: &request.runtime,
                programs: request.programs.clone(),
                workspace: owned.work.path(),
                action: film_task::Action::Render,
            };
            let generated = if matches!(action, Action::Run) {
                let claims = admission::Prepared::open(
                    &request.packet,
                    &request.source,
                    &request.admission,
                    &request.receipt.attempt_id,
                )?
                .reopen(&request.receipt)?;
                let mut native =
                    NativeInputs::from_claims(&request.packet, &request.source, claims)?;
                if let Some(spec) = &request.control_derivation {
                    crate::film_controls::verify(&request.packet, &mut native, spec)?;
                }
                film_task::run(&request.packet, native, invocation)?
            } else {
                film_task::resume(
                    &request.packet,
                    &request.admission,
                    invocation,
                    Some(&request.retained),
                )?
            };
            generated.ingest(&request.source, &request.admission)?;
            generated.retain(&request.source, &request.admission, &request.retained)?;
            Ok(
                serde_json::json!({"ok":true,"task_completed":true,"retained_owner_acknowledged":true,"selected":false,"scratch_retired":false,"capacity_released":false}),
            )
        }
        _ => {
            let result: reel_workflow::ReturnedResult =
                crate::read_control(&owned.work.path().join("workflow-result.json"))?;
            let ack = crate::read_control(&owned.work.path().join("retained.json"))?;
            if matches!(action, Action::Verify) {
                let _proof = film_task::verify_retained(
                    &request.packet,
                    &request.source,
                    &request.admission,
                    &request.receipt,
                    &result,
                    &ack,
                )?;
                return Ok(
                    serde_json::json!({"ok":true,"retained_verified":true,"selected":false,"scratch_retired":false}),
                );
            }
            let close_action = match action {
                Action::PreviewClose => crate::closing::Action::Preview,
                Action::StageClose => crate::closing::Action::Stage,
                Action::Close => crate::closing::Action::Apply,
                _ => crate::closing::Action::Recover,
            };
            let outcome = film_task::close(
                film_task::Close {
                    packet: &request.packet,
                    source: &request.source,
                    profile: &request.admission,
                    receipt: &request.receipt,
                    result: &result,
                    acknowledgement: &ack,
                    operation: &request.close_operation,
                },
                close_action,
            )?;
            if outcome.slot_idle {
                let raw = admission::encode(&outcome)?;
                let path = owned.root.path().join("close-outcome.json");
                if !path.exists() {
                    persist_new(&path, &raw)?;
                } else {
                    let (stored, _guard) =
                        crate::staging::read_held(&path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
                    let previous: crate::closing::Outcome = reel_workflow::decode(&stored)?;
                    if previous.operation != outcome.operation
                        || previous.admission_identity != outcome.admission_identity
                        || previous.activation != outcome.activation
                        || previous.handoff != outcome.handoff
                        || previous.clear != outcome.clear
                        || !previous.slot_idle
                        || !previous.original_clear_complete
                    {
                        return Err(error(
                            ErrorKind::Conflict,
                            "film-cli-original-close-outcome",
                        ));
                    }
                }
            }
            Ok(
                serde_json::json!({"ok":true,"slot_idle":outcome.slot_idle,"original_clear_complete":outcome.original_clear_complete,"scratch_retired":false,"capacity_released":false}),
            )
        }
    }
}
