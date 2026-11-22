//! Exact film task binding and durable unselected producer candidates.
//! Owned workspace/capacity admission remains with the caller.
use crate::{
    InputReader, NativeInputs, NativeProfile, Tool, admission, error,
    film_inputs::{Inputs, Programs},
    film_process::{self, Limits, Rendered},
    io_error,
};
use reel::scene_delivery::{self, FileRef};
use reel_workflow::{
    Artifact, ErrorKind, Mode, Packet, Provenance, Result, ReturnedResult, ToolCapability,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub job_path: String,
    pub production_path: String,
    pub output_paths: BTreeMap<String, String>,
    pub limits: Limits,
}
pub struct Bound {
    request: Request,
    job: Artifact,
    production: Artifact,
    task_key: String,
}
impl Bound {
    pub fn request(&self) -> &Request {
        &self.request
    }
}
fn control(reader: &mut impl InputReader, artifact: &Artifact) -> Result<Vec<u8>> {
    if artifact.bytes > reel_workflow::MAX_CONTROL_BYTES as u64 {
        return Err(error(ErrorKind::Budget, "film-task-control-bytes"));
    }
    let raw = reader.read(artifact, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if raw.len() as u64 != artifact.bytes || reel_workflow::digest(&raw) != artifact.sha256 {
        return Err(error(ErrorKind::Integrity, "film-task-control-pin"));
    }
    Ok(raw)
}
fn media(path: &str) -> Result<&'static str> {
    match path.rsplit('.').next() {
        Some("json") => Ok("application/json"),
        Some("mkv") => Ok("video/x-matroska"),
        Some("mp4") => Ok("video/mp4"),
        Some("wav") => Ok("audio/wav"),
        Some("png") => Ok("image/png"),
        Some("ttf") => Ok("font/ttf"),
        _ => Err(error(ErrorKind::Unsupported, "film-task-output-media")),
    }
}
/// Read exact controls only; grant no producer execution, admission or custody.
pub fn bind(packet: &Packet, reader: &mut impl InputReader) -> Result<Bound> {
    reel_workflow::check_packet(packet)?;
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "film-task-tool"))?;
    if tool.capability != ToolCapability::FilmRender || tool.part.is_some() {
        return Err(error(ErrorKind::Unsupported, "film-task-capability"));
    }
    if tool
        .auxiliary_executables
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["ffmpeg", "ffprobe"])
        || tool.source_bindings.values().collect::<BTreeSet<_>>()
            != packet.task.inputs.iter().map(|a| &a.logical_id).collect()
    {
        return Err(error(
            ErrorKind::Conflict,
            "film-task-input-program-closure",
        ));
    }
    if tool
        .source_bindings
        .keys()
        .any(|p| !crate::staging::portable(p))
    {
        return Err(error(ErrorKind::Invalid, "film-task-input-path"));
    }
    let request: Request = reel_workflow::decode(&control(reader, &tool.request)?)?;
    request.limits.validate()?;
    if request.schema != "reel.workflow-film-task.v1"
        || request.job_path == request.production_path
        || !crate::staging::portable(&request.job_path)
        || !crate::staging::portable(&request.production_path)
        || tool.source_bindings.get(&request.job_path) != Some(&tool.publication.logical_id)
        || request.output_paths.keys().ne(packet.task.outputs.keys())
        || packet.task.outputs.values().any(|c| !c.required)
    {
        return Err(error(ErrorKind::Conflict, "film-task-request-binding"));
    }
    let production = tool
        .source_bindings
        .get(&request.production_path)
        .and_then(|id| packet.task.inputs.iter().find(|a| &a.logical_id == id))
        .filter(|a| a.media_type == "application/json" && a.logical_id != tool.request.logical_id)
        .ok_or_else(|| error(ErrorKind::Missing, "film-task-production"))?;
    for (name, path) in [
        ("execution-request", "render-request.json"),
        ("execution-result", "render-result.json"),
        ("delivery-receipt", "delivery/receipt.json"),
    ] {
        if request.output_paths.get(name).map(String::as_str) != Some(path) {
            return Err(error(ErrorKind::Conflict, "film-task-required-evidence"));
        }
    }
    let mut folded = BTreeSet::new();
    for (name, path) in &request.output_paths {
        if !crate::staging::portable(path)
            || !folded.insert(path.to_ascii_lowercase())
            || !(path.starts_with("delivery/")
                || path == "render-request.json"
                || path == "render-result.json")
            || media(path)? != packet.task.outputs[name].media_type
        {
            return Err(error(ErrorKind::Conflict, "film-task-output-path"));
        }
    }
    for path in &folded {
        if path
            .match_indices('/')
            .any(|(at, _)| folded.contains(&path[..at]))
        {
            return Err(error(ErrorKind::Conflict, "film-task-output-prefix"));
        }
    }
    control(reader, &tool.publication)?;
    control(reader, production)?;
    Ok(Bound {
        request,
        job: tool.publication.clone(),
        production: production.clone(),
        task_key: packet.task_key.clone(),
    })
}
fn reference(path: &str, artifact: &Artifact) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: artifact.sha256.clone(),
        bytes: artifact.bytes,
    }
}
fn artifact(logical_id: String, raw: &[u8]) -> Artifact {
    Artifact {
        logical_id,
        sha256: reel_workflow::digest(raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkerReceipt {
    schema: String,
    task_key: String,
    attempt_id: String,
    request: Artifact,
    publication: Artifact,
    runtime: Artifact,
    auxiliary_executables: BTreeMap<String, Artifact>,
    output_paths: BTreeMap<String, String>,
    outputs: reel_workflow::ArtifactSet,
    human_approval: bool,
    retained_owner_acknowledged: bool,
}
/// Constructible only after exact task binding and actual contained production.
pub struct Generated {
    allowance: crate::output_bounds::Allowance,
    packet: Packet,
    source: String,
    receipt: admission::Receipt,
    result: ReturnedResult,
    returns: crate::response::Returns,
    _rendered: Rendered,
    _controls: Vec<File>,
    _workspace: reel_assets::jobs::WorkspaceDirectory,
    _intent_namespace: reel_assets::jobs::WorkspaceDirectory,
}
impl Generated {
    pub fn result(&self) -> &ReturnedResult {
        &self.result
    }
    pub fn returns(&self) -> &crate::response::Returns {
        &self.returns
    }
    fn context(&self, source: &NativeProfile) -> Result<()> {
        if reel_workflow::canonical_digest(source)? != self.source {
            return Err(error(ErrorKind::Conflict, "film-task-original-source"));
        }
        Ok(())
    }
    /// Publish only these actual guarded outputs, including the worker receipt.
    /// The generic external importer still refuses arbitrary tool responses.
    pub fn ingest(
        &self,
        source: &NativeProfile,
        profile: &admission::Profile,
    ) -> Result<crate::response::Candidate> {
        self.context(source)?;
        crate::response::ingest_verified(
            &self.packet,
            source,
            profile,
            &self.receipt,
            &self.result,
            crate::response::VerifiedFiles {
                returns: &self.returns,
                root: self._workspace.path(),
                worker_receipt: Some("worker-receipt.json"),
                allowance: self.allowance,
            },
        )
    }
    /// Freeze only the original registration WorkRef; not byte publication.
    pub fn stage(
        &self,
        source: &NativeProfile,
        profile: &admission::Profile,
    ) -> Result<crate::response::Candidate> {
        self.context(source)?;
        crate::response::stage_verified(&self.packet, source, profile, &self.receipt, &self.result)
    }
    /// Reopen native outputs and positively acknowledge retained ownership.
    /// No scratch retirement, actor close, selection or creative approval occurs.
    pub fn retain(
        &self,
        source: &NativeProfile,
        profile: &admission::Profile,
        target: &crate::retention::Target,
    ) -> Result<crate::retention::Acknowledgement> {
        self.context(source)?;
        let ack = crate::retention::retain_bounded(
            &self.packet,
            source,
            profile,
            &self.receipt,
            &self.result,
            target,
            self.allowance,
        )?;
        let _proof = crate::retention::verify_bounded(
            &self.packet,
            source,
            profile,
            &self.receipt,
            &self.result,
            &ack,
            self.allowance,
        )?;
        let raw = ack.canonical_bytes()?;
        let path = self._workspace.path().join("retained.json");
        let _ack = persist(&path, &raw, path.exists())?;
        let completed = admission::encode(
            &serde_json::json!({"schema":"reel.workflow-film-completed.v1", "task_key":self.result.task_key,
            "attempt_id":self.result.attempt_id, "result":artifact(format!("workflow/film/{}/result", self.result.task_key), &admission::encode(&self.result)?),
            "retained":ack.artifact()?, "human_approval":false,"selection_applied":false}),
        )?;
        let path = self._workspace.path().join("task-complete.json");
        let _completed = persist(&path, &completed, path.exists())?;
        Ok(ack)
    }
}
/// Historical readback requires actual retained work and native bytes. This
/// never renders, accepts a job, retargets a source or trusts receipt JSON alone.
pub fn verify_retained(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    acknowledgement: &crate::retention::Acknowledgement,
) -> Result<crate::retention::RetainedGuard> {
    use std::io::Read;
    reel_workflow::check_result(packet, result)?;
    let mut inputs = NativeInputs::open(packet, source)?;
    let bound = bind(packet, &mut inputs)?;
    let allowance = crate::output_bounds::Allowance::film(bound.request.limits.output_bytes)?;
    let tool = packet.task.tool.as_ref().unwrap();
    let worker = result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "film-task-worker-receipt"))?;
    if worker.bytes > 65536 {
        return Err(error(ErrorKind::Budget, "film-task-worker-receipt-bytes"));
    }
    let object = tig_core::Object {
        sha256: tig_core::Digest::parse(&worker.sha256)
            .map_err(|_| error(ErrorKind::Invalid, "film-task-worker-digest"))?,
        bytes: worker.bytes,
    };
    let held = tig_git::annex::workflow::reader::open(&source.source, &object).map_err(io_error)?;
    let mut raw = Vec::new();
    held.take(65537).read_to_end(&mut raw).map_err(io_error)?;
    let expected = WorkerReceipt {
        schema: "reel.workflow-film-worker.v1".into(),
        task_key: packet.task_key.clone(),
        attempt_id: receipt.attempt_id.clone(),
        request: tool.request.clone(),
        publication: tool.publication.clone(),
        runtime: tool.executable.clone(),
        auxiliary_executables: tool.auxiliary_executables.clone(),
        output_paths: bound.request.output_paths,
        outputs: result.artifacts.clone(),
        human_approval: false,
        retained_owner_acknowledged: false,
    };
    if admission::encode(&expected)? != raw
        || artifact(
            format!("workflow/film/{}/worker-receipt", packet.task_key),
            &raw,
        ) != *worker
    {
        return Err(error(
            ErrorKind::Integrity,
            "film-task-retained-worker-binding",
        ));
    }
    drop(inputs);
    crate::retention::verify_bounded(
        packet,
        source,
        profile,
        receipt,
        result,
        acknowledgement,
        allowance,
    )
}
/// Explicit close of the original job slot into retained coverage. Persist the
/// operation key before staging; recovery must reuse it. No workspace is retired.
pub struct Close<'a> {
    pub packet: &'a Packet,
    pub source: &'a NativeProfile,
    pub profile: &'a admission::Profile,
    pub receipt: &'a admission::Receipt,
    pub result: &'a ReturnedResult,
    pub acknowledgement: &'a crate::retention::Acknowledgement,
    pub operation: &'a tig_store::Key,
}
pub fn close(
    request: Close<'_>,
    action: crate::closing::Action,
) -> Result<crate::closing::Outcome> {
    reel_workflow::check_result(request.packet, request.result)?;
    if request
        .packet
        .task
        .tool
        .as_ref()
        .is_none_or(|t| t.capability != ToolCapability::FilmRender)
        || request.result.worker_receipt.is_none()
    {
        return Err(error(ErrorKind::Unsupported, "film-task-close-capability"));
    }
    if action != crate::closing::Action::Recover {
        drop(verify_retained(
            request.packet,
            request.source,
            request.profile,
            request.receipt,
            request.result,
            request.acknowledgement,
        )?);
    }
    // Original staged work owns recovery of a pending native Prepare. The
    // qualified handoff callback reopens all outputs before clearing the slot.
    admission::Prepared::open(
        request.packet,
        request.source,
        request.profile,
        &request.receipt.attempt_id,
    )?
    .close_retained(
        request.receipt,
        request.result,
        request.acknowledgement,
        &request.acknowledgement.target,
        request.operation,
        action,
        request.source,
        // Recovery reconciles original native work before reopening outputs in
        // the qualified close callback. This read ceiling admits no new output;
        // generation/publication used the checked original request allowance.
        crate::output_bounds::Allowance::film(16 * 1024 * 1024 * 1024)?,
    )
}
/// Explicit, metadata-only preparation for an older retained layout. Preview
/// exposes two bounded phases; application preserves coverage before aliases
/// are removed. Reuse the original close key when reconciling either phase.
pub fn prepare_close_layout(
    request: Close<'_>,
    apply: bool,
) -> Result<crate::closing::LayoutOutcome> {
    reel_workflow::check_result(request.packet, request.result)?;
    if request
        .packet
        .task
        .tool
        .as_ref()
        .is_none_or(|t| t.capability != ToolCapability::FilmRender)
        || request.result.worker_receipt.is_none()
    {
        return Err(error(ErrorKind::Unsupported, "film-task-close-capability"));
    }
    admission::Prepared::open(
        request.packet,
        request.source,
        request.profile,
        &request.receipt.attempt_id,
    )?
    .prepare_close_layout(
        request.receipt,
        request.result,
        request.acknowledgement,
        request.operation,
        apply,
    )
}

/// Explicit action; neither inspection nor deserialization runs this adapter.
#[derive(Clone, Copy)]
pub enum Action {
    Render,
    Recover,
}
pub struct Invocation<'a> {
    pub source: &'a NativeProfile,
    pub receipt: &'a admission::Receipt,
    pub runtime: &'a Tool,
    pub programs: Programs,
    pub workspace: &'a Path,
    pub action: Action,
}
fn persist(path: &Path, expected: &[u8], recover: bool) -> Result<File> {
    if !recover || !path.exists() {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "film-task-original-control-preserve",
                )
            })?;
        file.write_all(expected)
            .and_then(|_| file.sync_all())
            .map_err(io_error)?;
        drop(file);
    }
    let (actual, held) = crate::staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if actual != expected {
        return Err(error(ErrorKind::Conflict, "film-task-original-result"));
    }
    Ok(held)
}
/// Caller must own workspace/capacity admission. Native job claims must already
/// be live. Durable input claims remain after read/fence handles are released.
pub fn run(packet: &Packet, native: NativeInputs, invocation: Invocation<'_>) -> Result<Generated> {
    run_expected(packet, native, invocation, None)
}
/// Reconcile only original admitted response/retention work, then independently
/// check original media. Never render or invent a new attempt/workspace.
pub fn resume(
    packet: &Packet,
    profile: &admission::Profile,
    mut invocation: Invocation<'_>,
    target: Option<&crate::retention::Target>,
) -> Result<Generated> {
    invocation.action = Action::Recover;
    let prepared = admission::Prepared::open(
        packet,
        invocation.source,
        profile,
        &invocation.receipt.attempt_id,
    )?;
    let original = prepared.original_response(invocation.receipt)?;
    if let (Some(result), Some(target)) = (&original, target)
        && prepared.has_retention_work(invocation.receipt, result, target)?
    {
        // Pending original retention must settle before acquiring new claim
        // fences. Decode only exact, guarded original controls here, and drop
        // their object gates before the native retention transaction.
        let allowance = {
            let mut controls = NativeInputs::open(packet, invocation.source)?;
            crate::output_bounds::Allowance::film(
                bind(packet, &mut controls)?.request.limits.output_bytes,
            )?
        };
        crate::retention::retain_bounded(
            packet,
            invocation.source,
            profile,
            invocation.receipt,
            result,
            target,
            allowance,
        )?;
    }
    let claims = if let Some(result) = &original {
        let (key, raw, outputs) = crate::response::identity(profile, invocation.receipt, result)?;
        prepared.response_guard(
            invocation.receipt,
            &key,
            &tig_core::Object::from_bytes(&raw),
            &outputs,
        )?
    } else {
        prepared.reopen(invocation.receipt)?
    };
    let native = NativeInputs::from_claims(packet, invocation.source, claims)?;
    run_expected(packet, native, invocation, original.as_ref())
}
fn run_expected(
    packet: &Packet,
    mut native: NativeInputs,
    invocation: Invocation<'_>,
    original: Option<&ReturnedResult>,
) -> Result<Generated> {
    if native.snapshot.packet != *packet {
        return Err(error(ErrorKind::Conflict, "film-task-native-packet"));
    }
    native
        ._claims
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Denied, "film-task-live-claims"))?
        .check_receipt(invocation.receipt)?;
    native
        ._claims
        .as_ref()
        .unwrap()
        .check_source(packet, invocation.source)?;
    let bound = bind(packet, &mut native)?;
    // Control decoding finished; keep durable claims while the path provider reopens object gates.
    native.held.clear();
    let tool = packet.task.tool.as_ref().unwrap();
    let inputs = Inputs::prepare(packet, native, &tool.source_bindings, invocation.programs)?;
    inputs.validate_runtime(invocation.runtime)?;
    let _runtime = crate::hold_tool(invocation.runtime)?;
    let job = reference(&bound.request.job_path, &bound.job);
    let production = reference(&bound.request.production_path, &bound.production);
    let (parsed, plan) =
        scene_delivery::plan_with_control_files(&job, &production, &inputs, &inputs)
            .map_err(|_| error(ErrorKind::Integrity, "film-task-plan"))?;
    let expected = scene_delivery::output_contract(&parsed, &plan)
        .map_err(|_| error(ErrorKind::Integrity, "film-task-render-contract"))?;
    let actual: BTreeSet<_> = bound
        .request
        .output_paths
        .values()
        .map(String::as_str)
        .collect();
    let expected: BTreeSet<_> = expected
        .names
        .iter()
        .map(|n| format!("delivery/{n}"))
        .chain([
            "delivery/receipt.json".into(),
            "render-request.json".into(),
            "render-result.json".into(),
        ])
        .collect();
    if actual != expected.iter().map(String::as_str).collect() {
        return Err(error(
            ErrorKind::Conflict,
            "film-task-exact-producer-outputs",
        ));
    }
    let workspace = reel_assets::jobs::open_workspace_directory(invocation.workspace)
        .map_err(|_| error(ErrorKind::Denied, "film-task-workspace"))?;
    if matches!(invocation.action, Action::Render)
        && std::fs::read_dir(workspace.path())
            .map_err(io_error)?
            .next()
            .is_some()
    {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "film-task-empty-owned-workspace",
        ));
    }
    let intent = admission::encode(
        &serde_json::json!({"schema":"reel.workflow-film-invocation.v1", "task_key":packet.task_key,
        "attempt_id":invocation.receipt.attempt_id, "admission":invocation.receipt.operation_identity()?,
        "source":reel_workflow::canonical_digest(invocation.source)?,"runtime":invocation.runtime,"programs":inputs.programs(),
        "workspace":workspace.path(),"workspace_identity":workspace.identity()}),
    )?;
    let (intent_guard, intent_namespace) = inputs.freeze_invocation(
        invocation.receipt,
        &intent,
        matches!(invocation.action, Action::Recover),
    )?;
    let rendered = match invocation.action {
        Action::Render => film_process::render(
            &inputs,
            &job,
            &production,
            invocation.workspace,
            invocation.runtime,
            &bound.request.limits,
        )?,
        Action::Recover => film_process::recover(
            &inputs,
            &job,
            &production,
            invocation.workspace,
            invocation.runtime,
            &bound.request.limits,
        )?,
    };
    let mut controls = vec![intent_guard];
    let mut pins: BTreeMap<String, FileRef> = rendered
        .files
        .iter()
        .map(|(name, output)| (format!("delivery/{name}"), output.reference().clone()))
        .collect();
    for name in ["render-request.json", "render-result.json"] {
        let (raw, held) = crate::staging::read_held(
            &workspace.path().join(name),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        let pin = artifact(name.into(), &raw);
        pins.insert(name.into(), reference(name, &pin));
        controls.push(held);
    }
    let artifacts = bound
        .request
        .output_paths
        .iter()
        .map(|(name, path)| {
            let pin = &pins[path];
            Ok((
                name.clone(),
                Artifact {
                    logical_id: format!("workflow/film/{}/{name}", bound.task_key),
                    sha256: pin.sha256.clone(),
                    bytes: pin.bytes,
                    media_type: media(path)?.into(),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let worker = WorkerReceipt {
        schema: "reel.workflow-film-worker.v1".into(),
        task_key: packet.task_key.clone(),
        attempt_id: invocation.receipt.attempt_id.clone(),
        request: tool.request.clone(),
        publication: tool.publication.clone(),
        runtime: tool.executable.clone(),
        auxiliary_executables: tool.auxiliary_executables.clone(),
        output_paths: bound.request.output_paths.clone(),
        outputs: artifacts.clone(),
        human_approval: false,
        retained_owner_acknowledged: false,
    };
    let raw = admission::encode(&worker)?;
    let worker_artifact = artifact(
        format!("workflow/film/{}/worker-receipt", packet.task_key),
        &raw,
    );
    let result = ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: invocation.receipt.attempt_id.clone(),
        artifacts,
        provenance: Provenance {
            mode: Mode::Tool,
            provider: None,
            model: None,
            executable: Some(tool.executable.clone()),
        },
        worker_receipt: Some(worker_artifact),
    };
    reel_workflow::check_result(packet, &result)?;
    if original.is_some_and(|original| original != &result) {
        return Err(error(ErrorKind::Conflict, "film-task-original-response"));
    }
    let recover = matches!(invocation.action, Action::Recover);
    controls.push(persist(
        &workspace.path().join("worker-receipt.json"),
        &raw,
        recover,
    )?);
    controls.push(persist(
        &workspace.path().join("workflow-result.json"),
        &admission::encode(&result)?,
        recover,
    )?);
    // Release the live input fence before the response/retention phases acquire
    // that actor. Positive durable claims are not released by this drop.
    drop(inputs);
    Ok(Generated {
        allowance: crate::output_bounds::Allowance::film(bound.request.limits.output_bytes)?,
        packet: packet.clone(),
        source: reel_workflow::canonical_digest(invocation.source)?,
        receipt: invocation.receipt.clone(),
        result,
        returns: crate::response::Returns {
            schema: "reel.workflow-returns.v1".into(),
            root: workspace.path().into(),
            files: bound.request.output_paths,
        },
        _rendered: rendered,
        _controls: controls,
        _workspace: workspace,
        _intent_namespace: intent_namespace,
    })
}

#[cfg(test)]
#[path = "film_task_tests.rs"]
mod tests;
