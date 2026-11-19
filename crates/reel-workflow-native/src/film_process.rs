//! Contained existing-renderer execution over live guarded native inputs.
//! The caller owns workspace/capacity admission and the surrounding job lifecycle.
use crate::{
    Tool, error,
    film_inputs::{Inputs, Programs},
    io_error,
};
use reel::scene_delivery::{self, FileRef, InputFiles, MediaProgram};
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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub timeout_seconds: u64,
    pub memory_bytes: u64,
    pub processes: u32,
    pub workspace_bytes: u64,
    pub output_bytes: u64,
    pub frames: u64,
    pub samples: u64,
    pub raw_rgb_bytes: u64,
}
impl Limits {
    fn validate(&self) -> Result<()> {
        if !(1..=3600).contains(&self.timeout_seconds)
            || !(64 * 1024 * 1024..=16 * 1024 * 1024 * 1024).contains(&self.memory_bytes)
            || !(2..=32).contains(&self.processes)
            || self.output_bytes == 0
            || self.output_bytes > 128 * 1024 * 1024
            || self.workspace_bytes < self.output_bytes
            || self.workspace_bytes > 512 * 1024 * 1024
            || self.frames == 0
            || self.frames > 1_000_000
            || self.samples == 0
            || self.samples > 384_000 * 3600
            || self.raw_rgb_bytes == 0
            || self.raw_rgb_bytes > 16 * 1024 * 1024 * 1024
        {
            return Err(error(ErrorKind::Budget, "film-process-limits"));
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Action {
    Render,
    Check,
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
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    schema: String,
    request_sha256: String,
    error: Option<reel_workflow::Error>,
}
struct Files<'a>(&'a Request);
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
            .0
            .inputs
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
    // An existing launch is never reissued. Its exact request and scratch survive.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                error(
                    ErrorKind::NeedsRecovery,
                    "film-process-original-launch-exists",
                )
            } else {
                io_error(e)
            }
        })?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "film-process-request-write-preserve",
            )
        })?;
    drop(file);
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
    let request = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Render,
    )?;
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
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(_directory.path().join(outcome_name))
        .map_err(io_error)?;
    let raw = serde_json::to_vec(&outcome)
        .map_err(|_| error(ErrorKind::Invalid, "film-process-outcome-json"))?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "film-process-outcome-write-preserve",
            )
        })?;
    result
}
fn perform(request: &Request) -> Result<()> {
    let _runtime = crate::handoff::runtime(&request.runtime)?;
    let _tools = [
        crate::hold_tool(&request.programs.ffmpeg)?,
        crate::hold_tool(&request.programs.ffprobe)?,
    ];
    let inputs = Files(request);
    let (job, plan) = scene_delivery::plan_with_control_files(
        &request.job,
        &request.production,
        &inputs,
        &inputs,
    )
    .map_err(|_| error(ErrorKind::Integrity, "film-process-plan"))?;
    let contract = scene_delivery::output_contract(&job, &plan)
        .map_err(|_| error(ErrorKind::Integrity, "film-process-output-contract"))?;
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
