//! Contained existing-renderer execution over live guarded native inputs.
//! The caller owns workspace/capacity admission and the surrounding job lifecycle.
use crate::{
    Tool, error,
    film_inputs::{Inputs, Programs},
    io_error,
};
use reel::scene_delivery::{self, FileRef, InputFiles, MediaProgram};
pub use reel_workflow::film_render::Limits;
use reel_workflow::{ErrorKind, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Action {
    Render,
    Check,
    Verify,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    action: Action,
    job: FileRef,
    production: FileRef,
    inputs: BTreeMap<String, FileRef>,
    programs: Programs,
    runtime: Tool,
    workspace: PathBuf,
    limits: Limits,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expected_outputs: Option<BTreeSet<String>>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outputs: BTreeMap<String, FileRef>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    schema: String,
    request_sha256: String,
    error: Option<reel_workflow::Error>,
}
struct Files<'a>(&'a Request, &'a BTreeMap<String, FileRef>);
fn hash_file(path: &Path, bytes: u64) -> anyhow::Result<String> {
    anyhow::ensure!(bytes <= 512 * 1024 * 1024, "process input byte bound");
    let mut file = File::open(path)?.take(bytes + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut length = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&buffer[..count]);
    }
    anyhow::ensure!(length == bytes, "process input length");
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}
fn directory(path: &Path) -> Result<reel_assets::jobs::WorkspaceDirectory> {
    reel_assets::jobs::open_workspace_directory(path)
        .map_err(|_| error(ErrorKind::Denied, "film-process-directory-namespace"))
}
impl InputFiles for Files<'_> {
    fn verified_path(&self, reference: &FileRef) -> anyhow::Result<PathBuf> {
        let name = reference
            .path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("reference encoding"))?;
        let physical = self
            .1
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("undeclared process input"))?;
        anyhow::ensure!(
            physical.sha256 == reference.sha256 && physical.bytes == reference.bytes,
            "process input pin"
        );
        anyhow::ensure!(
            physical.path.is_absolute()
                && fs::metadata(&physical.path)?.len() == physical.bytes
                && hash_file(&physical.path, physical.bytes)? == physical.sha256,
            "process input bytes"
        );
        Ok(physical.path.clone())
    }
    fn media_program(&self, program: MediaProgram) -> anyhow::Result<PathBuf> {
        Ok(match program {
            MediaProgram::Ffmpeg => &self.0.programs.ffmpeg.path,
            MediaProgram::Ffprobe => &self.0.programs.ffprobe.path,
        }
        .clone())
    }
}

fn inventory(
    root: &Path,
    path: &Path,
    entries: &mut usize,
    total: &mut u64,
    depth: usize,
) -> Result<()> {
    if depth > 16 {
        return Err(error(ErrorKind::Budget, "film-process-depth"));
    }
    for entry in fs::read_dir(path).map_err(io_error)? {
        let path = entry.map_err(io_error)?.path();
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io_error(e)),
        };
        *entries += 1;
        if *entries > 2048 {
            return Err(error(ErrorKind::Budget, "film-process-entries"));
        }
        #[cfg(windows)]
        let linked = {
            use std::os::windows::fs::MetadataExt;
            metadata.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let linked = metadata.file_type().is_symlink();
        if linked || !path.starts_with(root) {
            return Err(error(ErrorKind::Denied, "film-process-workspace-link"));
        }
        if metadata.is_dir() {
            match inventory(root, &path, entries, total, depth + 1) {
                Err(e) if e.kind == ErrorKind::Missing => (),
                other => other?,
            }
        } else if metadata.is_file() {
            *total = total
                .checked_add(metadata.len())
                .ok_or_else(|| error(ErrorKind::Budget, "film-process-workspace-overflow"))?;
        } else {
            return Err(error(ErrorKind::Denied, "film-process-workspace-file"));
        }
    }
    Ok(())
}
fn workspace_budget(workspace: &Path, maximum: u64) -> Result<()> {
    let mut total = 0;
    inventory(workspace, workspace, &mut 0, &mut total, 0)?;
    if total > maximum {
        return Err(error(ErrorKind::Budget, "film-process-workspace-bytes"));
    }
    Ok(())
}
fn request(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    limits: &Limits,
    runtime: &Tool,
    action: Action,
) -> Result<Request> {
    limits.validate()?;
    inputs
        .validate_reference_name(job)
        .and_then(|_| inputs.validate_reference_name(production))
        .map_err(|_| error(ErrorKind::Conflict, "film-process-control-binding"))?;
    Ok(Request {
        schema: "reel.workflow-film-process.v1".into(),
        action,
        job: job.clone(),
        production: production.clone(),
        inputs: inputs.process_files()?,
        programs: inputs.programs(),
        runtime: runtime.clone(),
        workspace: workspace.into(),
        limits: limits.clone(),
        expected_outputs: None,
        outputs: BTreeMap::new(),
    })
}
/// Exact response and guards remain live until the caller finishes importing.
pub struct Rendered {
    pub receipt: scene_delivery::Receipt,
    pub files: BTreeMap<String, Vec<u8>>,
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
fn collect(workspace: &Path, limits: &Limits) -> Result<Rendered> {
    let output = workspace.join("delivery");
    let mut directories = vec![directory(&output)?];
    let (raw, held) = crate::staging::read_held(&output.join("receipt.json"), 4 * 1024 * 1024)?;
    let receipt: scene_delivery::Receipt = serde_json::from_slice(&raw)
        .map_err(|_| error(ErrorKind::Integrity, "film-process-receipt"))?;
    let mut actual = BTreeSet::new();
    crate::comic::inventory(&output, &output, 0, &mut actual, &mut directories, &mut 0)?;
    let expected = receipt
        .outputs
        .keys()
        .cloned()
        .chain(std::iter::once("receipt.json".into()))
        .collect();
    if actual != expected {
        return Err(error(ErrorKind::Integrity, "film-process-output-inventory"));
    }
    let mut files = BTreeMap::from([("receipt.json".into(), raw)]);
    let mut guards = vec![held];
    let mut total = files["receipt.json"].len() as u64;
    if receipt.outputs.len() + 1 > crate::staging::MAX_FILES {
        return Err(error(ErrorKind::Budget, "film-process-output-count"));
    }
    for (name, pin) in &receipt.outputs {
        if name == "receipt.json" || !crate::staging::portable(name) || pin.path != Path::new(name)
        {
            return Err(error(ErrorKind::Invalid, "film-process-output-name"));
        }
        total = total
            .checked_add(pin.bytes)
            .filter(|n| *n <= limits.output_bytes)
            .ok_or_else(|| error(ErrorKind::Budget, "film-process-output-bytes"))?;
        let path = output.join(name);
        directories.push(directory(path.parent().unwrap())?);
        let (raw, held) = crate::staging::read_held(&path, pin.bytes)?;
        if raw.len() as u64 != pin.bytes || reel_workflow::digest(&raw) != pin.sha256 {
            return Err(error(ErrorKind::Integrity, "film-process-output-pin"));
        }
        files.insert(name.clone(), raw);
        guards.push(held);
    }
    if total > limits.output_bytes {
        return Err(error(ErrorKind::Budget, "film-process-output-bytes"));
    }
    Ok(Rendered {
        receipt,
        files,
        _files: guards,
        _directories: directories,
    })
}
fn launch(request: &Request, name: &str) -> Result<()> {
    let runtime = &request.runtime;
    let _runtime = crate::hold_tool(runtime)?;
    let _namespace = directory(
        runtime
            .path
            .parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "film-process-runtime-parent"))?,
    )?;
    let raw = serde_json::to_vec(request)
        .map_err(|_| error(ErrorKind::Invalid, "film-process-request-json"))?;
    if raw.len() > reel_workflow::MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "film-process-request-bytes"));
    }
    let path = request.workspace.join(name);
    // Render/effect launches never repeat. A complete read-only Verify may rerun
    // the exact original request; it cannot replace its prior outcome bytes.
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => file
            .write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "film-process-request-write-preserve",
                )
            })?,
        Err(e)
            if e.kind() == std::io::ErrorKind::AlreadyExists
                && request.action == Action::Verify =>
        {
            if crate::staging::read_held(&path, reel_workflow::MAX_CONTROL_BYTES as u64)?.0 != raw {
                return Err(error(
                    ErrorKind::Conflict,
                    "film-verification-original-request",
                ));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "film-process-original-launch-exists",
            ));
        }
        Err(e) => return Err(io_error(e)),
    }
    let (_, _request_guard) =
        crate::staging::read_held(&path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let args = vec![
        "film-process-child".into(),
        path.to_string_lossy().into_owned(),
        reel_workflow::digest(&raw),
        "--apply".into(),
    ];
    let execution = reel_assets::jobs::run_quiet(
        &runtime.path,
        &args,
        &request.workspace,
        Duration::from_secs(request.limits.timeout_seconds),
        reel_assets::jobs::QuietLimits {
            memory_bytes: usize::try_from(request.limits.memory_bytes)
                .map_err(|_| error(ErrorKind::Budget, "film-process-memory-platform"))?,
            processes: request.limits.processes,
        },
        || workspace_budget(&request.workspace, request.limits.workspace_bytes).map_err(Into::into),
    );
    let outcome_name = match request.action {
        Action::Render => "render-result.json",
        Action::Check => "check-result.json",
        Action::Verify => "verify-result.json",
    };
    let (outcome, _outcome_guard) =
        crate::staging::read_held(&request.workspace.join(outcome_name), 65536).map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "film-process-child-outcome-preserve",
            )
        })?;
    let outcome: Outcome = reel_workflow::decode(&outcome)?;
    if outcome.schema != "reel.workflow-film-process-outcome.v1"
        || outcome.request_sha256 != reel_workflow::digest(&raw)
    {
        return Err(error(
            ErrorKind::Integrity,
            "film-process-child-outcome-binding",
        ));
    }
    match (execution, outcome.error) {
        (Ok(()), None) => (),
        (Err(_), Some(error)) => return Err(error),
        _ => {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "film-process-child-preserve",
            ));
        }
    }
    workspace_budget(&request.workspace, request.limits.workspace_bytes)
}
/// Render in an explicitly admitted, empty, caller-owned workspace. This port
/// does not reserve capacity, accept a job, publish bytes or complete a task.
/// The live input provider holds exact native objects/programs through child exit.
pub fn render(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
) -> Result<Rendered> {
    render_declared(inputs, job, production, workspace, runtime, limits, None)
}
pub(crate) fn render_declared(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
    expected_outputs: Option<BTreeSet<String>>,
) -> Result<Rendered> {
    let directory = directory(workspace)?;
    if fs::read_dir(directory.path())
        .map_err(io_error)?
        .next()
        .is_some()
    {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "film-process-empty-owned-workspace-required",
        ));
    }
    let mut request = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Render,
    )?;
    request.expected_outputs = expected_outputs;
    launch(&request, "render-request.json")?;
    collect(directory.path(), limits)
}
/// Reconcile an existing rendered directory without rendering again. Requires
/// the original request bytes and uses a separate, explicit verification launch.
pub fn recover(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
) -> Result<Rendered> {
    let directory = directory(workspace)?;
    let original = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Render,
    )?;
    let expected = serde_json::to_vec(&original)
        .map_err(|_| error(ErrorKind::Invalid, "film-process-request-json"))?;
    let (actual, _request_guard) = crate::staging::read_held(
        &directory.path().join("render-request.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )?;
    if actual != expected {
        return Err(error(ErrorKind::Conflict, "film-process-original-request"));
    }
    let rendered = collect(directory.path(), limits)?;
    let mut check = original;
    check.action = Action::Check;
    launch(&check, "check-request.json")?;
    Ok(rendered)
}
/// Independent check over caller-held exact output files; no rendered movie is
/// produced and no original worker directory is required. The caller admits
/// this separate empty scratch workspace and retains every supplied file guard.
pub(crate) fn verify(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    outputs: &BTreeMap<String, FileRef>,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
) -> Result<()> {
    let directory = directory(workspace)?;
    let entries = fs::read_dir(directory.path())
        .map_err(io_error)?
        .map(|e| e.map(|e| e.file_name()).map_err(io_error))
        .collect::<Result<Vec<_>>>()?;
    if !entries.is_empty() {
        if entries.len() != 2
            || entries
                .iter()
                .any(|name| name != "verify-request.json" && name != "verify-result.json")
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "film-verification-original-workspace-exists",
            ));
        }
        let (raw, _held) =
            crate::staging::read_held(&directory.path().join("verify-result.json"), 65536)?;
        let outcome: Outcome = reel_workflow::decode(&raw)?;
        let (original, _request) = crate::staging::read_held(
            &directory.path().join("verify-request.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        if outcome.schema != "reel.workflow-film-process-outcome.v1"
            || outcome.error.is_some()
            || outcome.request_sha256 != reel_workflow::digest(&original)
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "film-verification-original-outcome-required",
            ));
        }
    }
    let mut request = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Verify,
    )?;
    request.expected_outputs = Some(
        outputs
            .keys()
            .filter(|name| name.as_str() != "receipt.json")
            .cloned()
            .collect(),
    );
    request.outputs = outputs.clone();
    launch(&request, "verify-request.json")
}
/// Producer evidence binds declared pins and parameters, never current machine
/// locators or authenticated execution. Historical evidence paths are not opened.
pub(crate) fn check_render_evidence(
    packet: &reel_workflow::Packet,
    bound: &reel_workflow::film_render::Bound,
    raw: &[u8],
    outcome_raw: &[u8],
) -> Result<()> {
    let evidence: Request = reel_workflow::decode(raw)?;
    let outcome: Outcome = reel_workflow::decode(outcome_raw)?;
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "movie-tool"))?;
    let reference_matches = |file: &FileRef, path: &str, asset: &reel_workflow::Artifact| {
        file.path == Path::new(path) && file.sha256 == asset.sha256 && file.bytes == asset.bytes
    };
    let expected: BTreeSet<_> = bound
        .request()
        .output_paths
        .values()
        .filter_map(|p| p.strip_prefix("delivery/"))
        .filter(|p| *p != "receipt.json")
        .map(str::to_owned)
        .collect();
    if evidence.schema != "reel.workflow-film-process.v1"
        || evidence.action != Action::Render
        || !reference_matches(&evidence.job, &bound.request().job_path, bound.job())
        || !reference_matches(
            &evidence.production,
            &bound.request().production_path,
            bound.production(),
        )
        || evidence.limits != bound.request().limits
        || evidence.expected_outputs.as_ref() != Some(&expected)
        || !evidence.outputs.is_empty()
        || !evidence.workspace.is_absolute()
        || evidence.runtime.sha256 != tool.executable.sha256
        || evidence.runtime.bytes != tool.executable.bytes
        || !evidence.runtime.path.is_absolute()
        || evidence.inputs.keys().ne(bound.aliases().keys())
        || outcome.schema != "reel.workflow-film-process-outcome.v1"
        || outcome.error.is_some()
        || outcome.request_sha256 != reel_workflow::digest(raw)
    {
        return Err(error(
            ErrorKind::Conflict,
            "movie-original-execution-evidence",
        ));
    }
    for (name, id) in bound.aliases() {
        let artifact = packet
            .task
            .inputs
            .iter()
            .find(|a| &a.logical_id == id)
            .ok_or_else(|| error(ErrorKind::Missing, "movie-evidence-input"))?;
        let file = &evidence.inputs[name];
        if !file.path.is_absolute()
            || file.sha256 != artifact.sha256
            || file.bytes != artifact.bytes
        {
            return Err(error(ErrorKind::Conflict, "movie-evidence-input-binding"));
        }
    }
    for (role, program) in [
        ("ffmpeg", &evidence.programs.ffmpeg),
        ("ffprobe", &evidence.programs.ffprobe),
    ] {
        let artifact = &tool.auxiliary_executables[role];
        if !program.path.is_absolute()
            || program.sha256 != artifact.sha256
            || program.bytes != artifact.bytes
        {
            return Err(error(ErrorKind::Conflict, "movie-evidence-media-program"));
        }
    }
    Ok(())
}
/// Internal trusted child port. The parent must hold source/program/output guards.
/// A pinned request is execution intent, not an admission or retained-owner proof.
pub fn child(path: &Path, pin: &str) -> Result<()> {
    let (raw, _guard) = crate::staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "film-process-request-pin"));
    }
    let request: Request = reel_workflow::decode(&raw)?;
    request.limits.validate()?;
    if request.schema != "reel.workflow-film-process.v1"
        || !request.workspace.is_absolute()
        || request.inputs.is_empty()
        || request.inputs.len() > 256
    {
        return Err(error(ErrorKind::Invalid, "film-process-request"));
    }
    let _directory = directory(&request.workspace)?;
    let request_name = match request.action {
        Action::Render => "render-request.json",
        Action::Check => "check-request.json",
        Action::Verify => "verify-request.json",
    };
    if path.canonicalize().map_err(io_error)? != _directory.path().join(request_name) {
        return Err(error(ErrorKind::Conflict, "film-process-request-location"));
    }
    let result = perform(&request);
    let outcome = Outcome {
        schema: "reel.workflow-film-process-outcome.v1".into(),
        request_sha256: pin.into(),
        error: result.as_ref().err().cloned(),
    };
    let outcome_name = match request.action {
        Action::Render => "render-result.json",
        Action::Check => "check-result.json",
        Action::Verify => "verify-result.json",
    };
    let raw = serde_json::to_vec(&outcome)
        .map_err(|_| error(ErrorKind::Invalid, "film-process-outcome-json"))?;
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(_directory.path().join(outcome_name))
    {
        Ok(mut file) => file
            .write_all(&raw)
            .and_then(|_| file.sync_all())
            .map_err(|_| {
                error(
                    ErrorKind::NeedsRecovery,
                    "film-process-outcome-write-preserve",
                )
            })?,
        Err(e)
            if e.kind() == std::io::ErrorKind::AlreadyExists
                && request.action == Action::Verify =>
        {
            if crate::staging::read_held(&_directory.path().join(outcome_name), 65536)?.0 != raw {
                return Err(error(
                    ErrorKind::Conflict,
                    "film-verification-original-outcome",
                ));
            }
        }
        Err(e) => return Err(io_error(e)),
    }
    result
}
fn perform(request: &Request) -> Result<()> {
    let _runtime = crate::handoff::runtime(&request.runtime)?;
    let _tools = [
        crate::hold_tool(&request.programs.ffmpeg)?,
        crate::hold_tool(&request.programs.ffprobe)?,
    ];
    let inputs = Files(request, &request.inputs);
    let (job, plan) = scene_delivery::plan_with_control_files(
        &request.job,
        &request.production,
        &inputs,
        &inputs,
    )
    .map_err(|_| error(ErrorKind::Integrity, "film-process-plan"))?;
    let contract = scene_delivery::output_contract(&job, &plan)
        .map_err(|_| error(ErrorKind::Integrity, "film-process-output-contract"))?;
    if request
        .expected_outputs
        .as_ref()
        .is_some_and(|names| names != &contract.names)
    {
        return Err(error(
            ErrorKind::Conflict,
            "film-process-declared-output-contract",
        ));
    }
    if plan.frame_count > request.limits.frames
        || plan.duration_samples > request.limits.samples
        || contract.raw_rgb24_video_bytes > request.limits.raw_rgb_bytes
        || contract.names.len() + 1 > crate::staging::MAX_FILES
    {
        return Err(error(ErrorKind::Budget, "film-process-scene-bounds"));
    }
    let output = request.workspace.join("delivery");
    match request.action {
        Action::Render => {
            scene_delivery::render_with_control_files_preserving(
                &request.job,
                &request.production,
                &inputs,
                &inputs,
                &output,
            )
            .map_err(|_| error(ErrorKind::NeedsRecovery, "film-process-render-preserve"))?;
        }
        Action::Check => {
            let (raw, _held) =
                crate::staging::read_held(&output.join("receipt.json"), 4 * 1024 * 1024)?;
            let receipt = FileRef {
                path: "receipt.json".into(),
                sha256: reel_workflow::digest(&raw),
                bytes: raw.len() as u64,
            };
            scene_delivery::check_with_all_files(
                &request.job,
                &request.production,
                &inputs,
                &output,
                &receipt,
                &inputs,
            )
            .map_err(|_| error(ErrorKind::Integrity, "film-process-check"))?;
        }
        Action::Verify => {
            if request.outputs.keys().cloned().collect::<BTreeSet<_>>()
                != contract
                    .names
                    .iter()
                    .cloned()
                    .chain(std::iter::once("receipt.json".into()))
                    .collect()
                || request
                    .outputs
                    .values()
                    .try_fold(0u64, |n, f| n.checked_add(f.bytes))
                    .is_none_or(|n| n > request.limits.output_bytes)
            {
                return Err(error(ErrorKind::Budget, "film-verification-output-closure"));
            }
            let outputs = Files(request, &request.outputs);
            let receipt = &request.outputs["receipt.json"];
            let receipt = FileRef {
                path: "receipt.json".into(),
                sha256: receipt.sha256.clone(),
                bytes: receipt.bytes,
            };
            scene_delivery::check_with_all_files(
                &request.job,
                &request.production,
                &inputs,
                &outputs,
                &receipt,
                &inputs,
            )
            .map_err(|_| error(ErrorKind::Integrity, "film-verification-media"))?;
            let master = &request.outputs["master.mkv"];
            let master = FileRef {
                path: "master.mkv".into(),
                sha256: master.sha256.clone(),
                bytes: master.bytes,
            };
            reel::scene_primary_reference::compare_master_to_primaries(
                &request.job,
                &request.production,
                &inputs,
                &inputs,
                &outputs,
                &master,
                &request.workspace,
                &reel::scene_primary_reference::Budget {
                    max_staged_bytes: request.limits.workspace_bytes,
                    timeout_seconds: request.limits.timeout_seconds,
                    max_picture_processes: (request.limits.processes - 2).max(2) as u16,
                },
            )
            .map_err(|_| error(ErrorKind::Integrity, "film-verification-primary-reference"))?;
            return Ok(());
        }
    }
    let rendered = collect(&request.workspace, &request.limits)?;
    if rendered
        .receipt
        .outputs
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>()
        != contract.names
    {
        return Err(error(
            ErrorKind::Integrity,
            "film-process-complete-output-set",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn limits() -> Limits {
        Limits {
            timeout_seconds: 120,
            memory_bytes: 1024 * 1024 * 1024,
            processes: 8,
            workspace_bytes: 128 * 1024 * 1024,
            output_bytes: 32 * 1024 * 1024,
            frames: 48,
            samples: 96000,
            raw_rgb_bytes: 64 * 1024 * 1024,
        }
    }
    #[test]
    fn refuses_unbounded_execution_and_inconsistent_scratch_limits() {
        limits().validate().unwrap();
        let mut value = limits();
        value.timeout_seconds = 0;
        assert_eq!(value.validate().unwrap_err().kind, ErrorKind::Budget);
        value = limits();
        value.memory_bytes = u64::MAX;
        assert!(value.validate().is_err());
        value = limits();
        value.processes = 1;
        assert!(value.validate().is_err());
        value = limits();
        value.workspace_bytes = value.output_bytes - 1;
        assert!(value.validate().is_err());
        value = limits();
        value.output_bytes = 128 * 1024 * 1024 + 1;
        assert!(value.validate().is_err());
    }
    #[test]
    fn scratch_monitor_and_stream_hash_refuse_growth() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("piece"), b"exact").unwrap();
        workspace_budget(root.path(), 5).unwrap();
        assert_eq!(
            workspace_budget(root.path(), 4).unwrap_err().kind,
            ErrorKind::Budget
        );
        assert_eq!(
            hash_file(&root.path().join("piece"), 5).unwrap(),
            reel_workflow::digest(b"exact")
        );
        assert!(hash_file(&root.path().join("piece"), 4).is_err());
    }
}
