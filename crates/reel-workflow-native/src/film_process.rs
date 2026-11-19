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
    time::{Duration, Instant},
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    streaming: Option<reel_workflow::film_render::Streaming>,
}
impl Request {
    fn validate(&self) -> Result<()> {
        match (self.schema.as_str(), &self.streaming) {
            ("reel.workflow-film-process.v1", None) => self.limits.validate()?,
            ("reel.workflow-film-process.v2", Some(s)) => {
                s.validate()?;
                self.limits.validate_streaming()?;
                // Aliases can name the same file. The parent checks the full
                // logical input set; the child independently bounds unique pins.
                let pins: BTreeSet<_> = self
                    .inputs
                    .values()
                    .map(|f| (&f.path, &f.sha256, f.bytes))
                    .collect();
                let total = pins.iter().try_fold(0u64, |n, (_, _, bytes)| {
                    if *bytes > s.input_object_bytes {
                        return None;
                    }
                    n.checked_add(*bytes)
                });
                if total.is_none_or(|n| n > s.input_bytes)
                    || self
                        .outputs
                        .values()
                        .try_fold(0u64, |n, f| n.checked_add(f.bytes))
                        .is_none_or(|n| n > self.limits.output_bytes)
                {
                    return Err(error(
                        ErrorKind::Budget,
                        "film-process-streaming-file-bounds",
                    ));
                }
            }
            _ => return Err(error(ErrorKind::Invalid, "film-process-version-envelope")),
        }
        if !self.workspace.is_absolute() || self.inputs.is_empty() || self.inputs.len() > 256 {
            return Err(error(ErrorKind::Invalid, "film-process-request"));
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    schema: String,
    request_sha256: String,
    error: Option<reel_workflow::Error>,
}
struct Files<'a> {
    request: &'a Request,
    files: &'a BTreeMap<String, FileRef>,
    maximum_bytes: u64,
}
impl<'a> Files<'a> {
    fn inputs(request: &'a Request) -> Self {
        Self {
            request,
            files: &request.inputs,
            maximum_bytes: request
                .streaming
                .as_ref()
                .map(|s| s.input_object_bytes)
                .unwrap_or(512 * 1024 * 1024),
        }
    }
    fn outputs(request: &'a Request) -> Self {
        Self {
            request,
            files: &request.outputs,
            maximum_bytes: if request.streaming.is_some() {
                request.limits.output_bytes
            } else {
                512 * 1024 * 1024
            },
        }
    }
}
fn hash_file(
    path: &Path,
    bytes: u64,
    maximum: u64,
    deadline: Option<Duration>,
) -> anyhow::Result<String> {
    anyhow::ensure!(bytes <= maximum, "process input byte bound");
    let started = Instant::now();
    anyhow::ensure!(
        deadline.is_none_or(|d| started.elapsed() < d),
        "process input verification deadline"
    );
    let mut file = File::open(path)?.take(bytes + 1);
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut length = 0;
    loop {
        anyhow::ensure!(
            deadline.is_none_or(|d| started.elapsed() < d),
            "process input verification deadline"
        );
        let count = match file.read(&mut buffer) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if count == 0 {
            break;
        }
        length += count as u64;
        hash.update(&buffer[..count]);
    }
    anyhow::ensure!(
        deadline.is_none_or(|d| started.elapsed() < d),
        "process input verification deadline"
    );
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
            .files
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("undeclared process input"))?;
        anyhow::ensure!(
            physical.sha256 == reference.sha256 && physical.bytes == reference.bytes,
            "process input pin"
        );
        anyhow::ensure!(
            physical.path.is_absolute()
                && fs::metadata(&physical.path)?.len() == physical.bytes
                && hash_file(
                    &physical.path,
                    physical.bytes,
                    self.maximum_bytes,
                    self.request
                        .streaming
                        .as_ref()
                        .map(|s| Duration::from_millis(s.verify_millis))
                )? == physical.sha256,
            "process input bytes"
        );
        Ok(physical.path.clone())
    }
    fn media_program(&self, program: MediaProgram) -> anyhow::Result<PathBuf> {
        Ok(match program {
            MediaProgram::Ffmpeg => &self.request.programs.ffmpeg.path,
            MediaProgram::Ffprobe => &self.request.programs.ffprobe.path,
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
fn reference_matches(file: &FileRef, path: &str, asset: &reel_workflow::Artifact) -> bool {
    file.path == Path::new(path) && file.sha256 == asset.sha256 && file.bytes == asset.bytes
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
    if let Some(expected) = inputs.streaming_limits() {
        if limits != expected {
            return Err(error(
                ErrorKind::Conflict,
                "film-process-original-streaming-limits",
            ));
        }
        limits.validate_streaming()?;
    } else {
        limits.validate()?;
    }
    if let Some(bound) = inputs.streaming_intent()
        && (!reference_matches(job, &bound.request().job_path, bound.job())
            || !reference_matches(
                production,
                &bound.request().production_path,
                bound.production(),
            ))
    {
        return Err(error(
            ErrorKind::Conflict,
            "film-process-original-streaming-controls",
        ));
    }
    inputs
        .validate_reference_name(job)
        .and_then(|_| inputs.validate_reference_name(production))
        .map_err(|_| error(ErrorKind::Conflict, "film-process-control-binding"))?;
    let request = Request {
        schema: if inputs.streaming_bounds().is_some() {
            "reel.workflow-film-process.v2"
        } else {
            "reel.workflow-film-process.v1"
        }
        .into(),
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
        streaming: inputs.streaming_bounds().cloned(),
    };
    request.validate()?;
    Ok(request)
}
/// Exact response and guards remain live until the caller finishes importing.
pub struct Rendered {
    pub receipt: scene_delivery::Receipt,
    pub files: BTreeMap<String, Vec<u8>>,
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
/// File-backed collection for the owned worker; no complete media buffers.
pub struct Captured {
    pub receipt: scene_delivery::Receipt,
    pub files: BTreeMap<String, FileRef>,
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
fn collect(workspace: &Path, limits: &Limits) -> Result<Rendered> {
    let captured = collect_files(workspace, limits)?;
    let mut files = BTreeMap::new();
    for (name, pin) in &captured.files {
        let (raw, _held) = crate::staging::read_held(&pin.path, pin.bytes)?;
        files.insert(name.clone(), raw);
    }
    Ok(Rendered {
        receipt: captured.receipt,
        files,
        _files: captured._files,
        _directories: captured._directories,
    })
}
fn collect_files(workspace: &Path, limits: &Limits) -> Result<Captured> {
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
    let receipt_bytes = raw.len() as u64;
    let mut files = BTreeMap::from([(
        "receipt.json".into(),
        FileRef {
            path: output.join("receipt.json"),
            sha256: reel_workflow::digest(&raw),
            bytes: receipt_bytes,
        },
    )]);
    let mut guards = vec![held];
    let mut total = receipt_bytes;
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
        let (object, held) = crate::staging::observe_held(&path, pin.bytes)?;
        if object.bytes != pin.bytes || object.sha256.as_str() != pin.sha256 {
            return Err(error(ErrorKind::Integrity, "film-process-output-pin"));
        }
        files.insert(
            name.clone(),
            FileRef {
                path,
                sha256: pin.sha256.clone(),
                bytes: pin.bytes,
            },
        );
        guards.push(held);
    }
    if total > limits.output_bytes {
        return Err(error(ErrorKind::Budget, "film-process-output-bytes"));
    }
    Ok(Captured {
        receipt,
        files,
        _files: guards,
        _directories: directories,
    })
}
fn launch(request: &Request, name: &str) -> Result<()> {
    request.validate()?;
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
/// File-backed producer port over live exact inputs. Caller must own and admit
/// the empty workspace/capacity. This grants no publication or task completion.
pub fn render_files(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
) -> Result<Captured> {
    let expected = inputs.streaming_intent().map(|bound| {
        bound
            .request()
            .output_paths
            .values()
            .filter_map(|p| p.strip_prefix("delivery/"))
            .filter(|p| *p != "receipt.json")
            .map(str::to_owned)
            .collect()
    });
    render_declared_files(
        inputs, job, production, workspace, runtime, limits, expected,
    )
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
    if inputs.streaming_bounds().is_some() {
        return Err(error(ErrorKind::Unsupported, "film-streaming-buffered-api"));
    }
    // Explicit buffered compatibility API. The owned worker uses the file API.
    let captured = render_declared_files(
        inputs,
        job,
        production,
        workspace,
        runtime,
        limits,
        expected_outputs,
    )?;
    let mut files = BTreeMap::new();
    for (name, pin) in &captured.files {
        files.insert(
            name.clone(),
            crate::staging::read_held(&pin.path, pin.bytes)?.0,
        );
    }
    Ok(Rendered {
        receipt: captured.receipt,
        files,
        _files: captured._files,
        _directories: captured._directories,
    })
}
pub(crate) fn render_declared_files(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
    expected_outputs: Option<BTreeSet<String>>,
) -> Result<Captured> {
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
    collect_files(directory.path(), limits)
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
    if inputs.streaming_bounds().is_some() {
        return Err(error(ErrorKind::Unsupported, "film-streaming-buffered-api"));
    }
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
/// Adopt only a completed original render/response under identical frozen
/// locators, tools and bounds. Missing/failed outcomes and partial staging stop.
pub(crate) fn adopt_declared(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
    expected_outputs: BTreeSet<String>,
) -> Result<Captured> {
    let directory = directory(workspace)?;
    let mut expected = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Render,
    )?;
    expected.expected_outputs = Some(expected_outputs);
    adopt_request(directory.path(), &expected)
}
fn adopt_request(workspace: &Path, expected: &Request) -> Result<Captured> {
    expected.validate()?;
    let raw = serde_json::to_vec(&expected)
        .map_err(|_| error(ErrorKind::Invalid, "film-process-request-json"))?;
    let (actual, _request) = crate::staging::read_held(
        &workspace.join("render-request.json"),
        reel_workflow::MAX_CONTROL_BYTES as u64,
    )
    .map_err(|_| {
        error(
            ErrorKind::NeedsRecovery,
            "movie-original-render-request-required",
        )
    })?;
    let (outcome, _outcome) =
        crate::staging::read_held(&workspace.join("render-result.json"), 65536).map_err(|_| {
            error(
                ErrorKind::NeedsRecovery,
                "movie-original-render-outcome-required",
            )
        })?;
    let outcome: Outcome = reel_workflow::decode(&outcome)?;
    if raw != actual
        || outcome.schema != "reel.workflow-film-process-outcome.v1"
        || outcome.request_sha256 != reel_workflow::digest(&actual)
        || outcome.error.is_some()
    {
        return Err(error(
            ErrorKind::Conflict,
            "movie-original-completed-render",
        ));
    }
    collect_files(workspace, &expected.limits)
}
/// Adopt an exact completed file-backed render. Missing/failed evidence refuses;
/// this never launches the renderer or substitutes a fresh operation identity.
pub fn recover_files(
    inputs: &Inputs,
    job: &FileRef,
    production: &FileRef,
    workspace: &Path,
    runtime: &Tool,
    limits: &Limits,
) -> Result<Captured> {
    let directory = directory(workspace)?;
    let mut expected = request(
        inputs,
        job,
        production,
        directory.path(),
        limits,
        runtime,
        Action::Render,
    )?;
    expected.expected_outputs = inputs.streaming_intent().map(|bound| {
        bound
            .request()
            .output_paths
            .values()
            .filter_map(|p| p.strip_prefix("delivery/"))
            .filter(|p| *p != "receipt.json")
            .map(str::to_owned)
            .collect()
    });
    adopt_request(directory.path(), &expected)
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
    let expected: BTreeSet<_> = bound
        .request()
        .output_paths
        .values()
        .filter_map(|p| p.strip_prefix("delivery/"))
        .filter(|p| *p != "receipt.json")
        .map(str::to_owned)
        .collect();
    let expected_schema = if bound.request().streaming.is_some() {
        "reel.workflow-film-process.v2"
    } else {
        "reel.workflow-film-process.v1"
    };
    if evidence.schema != expected_schema
        || evidence.streaming != bound.request().streaming
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
    request.validate()?;
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
    let inputs = Files::inputs(request);
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
            let outputs = Files::outputs(request);
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
    let rendered = collect_files(&request.workspace, &request.limits)?;
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
    fn protocol(workspace: &Path, streamed: bool) -> Request {
        let file = FileRef {
            path: workspace.join("synthetic.bin"),
            sha256: reel_workflow::digest(b"{}"),
            bytes: 2,
        };
        let tool = Tool {
            path: workspace.join("not-invoked.exe"),
            sha256: reel_workflow::digest(b"tool"),
            bytes: 4,
        };
        let mut bounds = limits();
        if streamed {
            bounds.timeout_seconds = 18000;
            bounds.workspace_bytes = 32 * 1024 * 1024 * 1024;
            bounds.output_bytes = 10 * 1024 * 1024 * 1024;
        }
        Request {
            schema: if streamed {
                "reel.workflow-film-process.v2"
            } else {
                "reel.workflow-film-process.v1"
            }
            .into(),
            action: Action::Render,
            job: file.clone(),
            production: file.clone(),
            inputs: BTreeMap::from([("job.json".into(), file)]),
            programs: Programs {
                ffmpeg: tool.clone(),
                ffprobe: tool.clone(),
            },
            runtime: tool,
            workspace: workspace.into(),
            limits: bounds,
            expected_outputs: None,
            outputs: BTreeMap::new(),
            streaming: streamed.then(|| reel_workflow::film_render::Streaming {
                schema: "reel.workflow-film-streaming.v1".into(),
                input_bytes: 8 * 1024 * 1024 * 1024,
                input_object_bytes: 4 * 1024 * 1024 * 1024,
                verify_millis: 300000,
            }),
        }
    }
    #[test]
    fn process_versions_require_exact_envelopes_and_bound_unique_input_and_output_pins() {
        let root = tempfile::tempdir().unwrap();
        let legacy = protocol(root.path(), false);
        legacy.validate().unwrap();
        assert!(
            serde_json::to_value(&legacy)
                .unwrap()
                .get("streaming")
                .is_none()
        );
        let mut r = protocol(root.path(), true);
        r.validate().unwrap();
        r.inputs
            .insert("alias.json".into(), r.inputs["job.json"].clone());
        r.streaming.as_mut().unwrap().input_bytes = 2;
        r.streaming.as_mut().unwrap().input_object_bytes = 2;
        r.validate().unwrap();
        r.inputs.get_mut("alias.json").unwrap().path = root.path().join("other.bin");
        assert_eq!(r.validate().unwrap_err().kind, ErrorKind::Budget);
        r = protocol(root.path(), true);
        r.outputs.insert(
            "master.mkv".into(),
            FileRef {
                path: root.path().join("master"),
                sha256: reel_workflow::digest(b"unverified output"),
                bytes: r.limits.output_bytes + 1,
            },
        );
        assert_eq!(r.validate().unwrap_err().kind, ErrorKind::Budget);
        r = protocol(root.path(), true);
        r.schema = "reel.workflow-film-process.v1".into();
        assert_eq!(r.validate().unwrap_err().kind, ErrorKind::Invalid);
        r = protocol(root.path(), true);
        r.streaming = None;
        assert_eq!(r.validate().unwrap_err().kind, ErrorKind::Invalid);
    }
    #[test]
    fn streamed_output_hashing_uses_output_allowance_and_checks_verification_deadline() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("media.bin");
        fs::write(&path, b"exact media").unwrap();
        let mut r = protocol(root.path(), true);
        r.streaming.as_mut().unwrap().input_object_bytes = 2;
        let pin = FileRef {
            path: path.clone(),
            sha256: reel_workflow::digest(b"exact media"),
            bytes: 11,
        };
        r.inputs = BTreeMap::from([("media.bin".into(), pin.clone())]);
        r.outputs = r.inputs.clone();
        let reference = FileRef {
            path: "media.bin".into(),
            ..pin
        };
        assert!(Files::inputs(&r).verified_path(&reference).is_err());
        assert_eq!(Files::outputs(&r).verified_path(&reference).unwrap(), path);
        assert!(
            hash_file(&root.path().join("missing"), 1, 1, Some(Duration::ZERO))
                .unwrap_err()
                .to_string()
                .contains("deadline")
        );
    }
    #[test]
    fn completed_render_adoption_refuses_changed_streaming_parameters_without_launch_or_replacement()
     {
        let root = tempfile::tempdir().unwrap();
        let mut r = protocol(root.path(), true);
        let raw = serde_json::to_vec(&r).unwrap();
        let outcome = serde_json::to_vec(&Outcome {
            schema: "reel.workflow-film-process-outcome.v1".into(),
            request_sha256: reel_workflow::digest(&raw),
            error: None,
        })
        .unwrap();
        fs::write(root.path().join("render-request.json"), &raw).unwrap();
        fs::write(root.path().join("render-result.json"), &outcome).unwrap();
        r.streaming.as_mut().unwrap().verify_millis -= 1;
        assert_eq!(
            adopt_request(root.path(), &r).err().unwrap().kind,
            ErrorKind::Conflict
        );
        r = protocol(root.path(), true);
        r.limits.timeout_seconds -= 1;
        assert_eq!(
            adopt_request(root.path(), &r).err().unwrap().kind,
            ErrorKind::Conflict
        );
        assert_eq!(
            fs::read(root.path().join("render-request.json")).unwrap(),
            raw
        );
        assert_eq!(
            fs::read(root.path().join("render-result.json")).unwrap(),
            outcome
        );
        assert!(!root.path().join("delivery").exists());
    }

    #[test]
    #[ignore = "Existing F-first capacity coordinator; preserves a 3.6 GB synthetic file and 4 GiB reservation; no native admission/render/publication"]
    fn qualified_large_stream_hash_preserves_v1_request_bytes_and_uses_explicit_v2_bounds() {
        let evidence = PathBuf::from(
            std::env::var_os("REEL_LARGE_HASH_EVIDENCE").expect("Set evidence directory"),
        );
        let admission_path = PathBuf::from(
            std::env::var_os("REEL_LARGE_HASH_ADMISSION")
                .expect("Set existing admission profile for capacity coordinator"),
        );
        let old_request = PathBuf::from(
            std::env::var_os("REEL_FILM_V1_REQUEST").expect("Set exact retained v1 render request"),
        );
        let (old_raw, _old) =
            crate::staging::read_held(&old_request, reel_workflow::MAX_CONTROL_BYTES as u64)
                .unwrap();
        let decoded: Request = reel_workflow::decode(&old_raw).unwrap();
        assert_eq!(decoded.schema, "reel.workflow-film-process.v1");
        decoded.validate().unwrap();
        assert_eq!(serde_json::to_vec(&decoded).unwrap(), old_raw);
        let admission: crate::admission::Profile = crate::read_control(&admission_path).unwrap();
        let mut temp = tempfile::tempdir().unwrap();
        temp.disable_cleanup(true);
        let root = temp.path();
        let bytes = 3_600_000_001u64;
        let marker = serde_json::json!({"root":root,"bytes":bytes,"reservation_bytes":4u64*1024*1024*1024,"old_request":old_request,"native_admission":false,"rendered":false});
        let save = |path: &Path, value: &serde_json::Value| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
            file.sync_all().unwrap();
        };
        save(&root.join("original-operation.json"), &marker);
        save(&evidence.join("original-operation-roots.json"), &marker);
        let config = reel_assets::Config {
            cache_roots: vec![root.into()],
            coordination_root: Some(admission.scope.coordination_root),
            ..Default::default()
        };
        let reservation =
            reel_assets::admission::reserve_on(root, &config, root, 4 * 1024 * 1024 * 1024)
                .unwrap();
        save(
            &root.join("capacity-ticket.json"),
            &serde_json::json!({"id":reservation.id,"volume":reservation.volume_id,"bytes":4u64*1024*1024*1024,"parent":root,"coordination":config.coordination_root}),
        );
        reservation.retain_for_recovery();
        let _directory = directory(root).unwrap();
        let path = root.join("synthetic-master.bin");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        file.set_len(bytes).unwrap();
        file.sync_all().unwrap();
        drop(file);
        let _held = crate::staging::open_held(&path, bytes).unwrap();
        let mut expected = Sha256::new();
        let zeros = [0u8; 65536];
        let mut remaining = bytes;
        while remaining > 0 {
            let n = remaining.min(zeros.len() as u64) as usize;
            expected.update(&zeros[..n]);
            remaining -= n as u64;
        }
        let expected: String = expected
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert!(hash_file(&path, bytes, 512 * 1024 * 1024, None).is_err());
        let actual = hash_file(
            &path,
            bytes,
            4 * 1024 * 1024 * 1024,
            Some(Duration::from_secs(300)),
        )
        .unwrap();
        assert_eq!(actual, expected);
        assert_eq!(fs::metadata(&path).unwrap().len(), bytes);
        let result = serde_json::json!({"root":root,"bytes":bytes,"sha256":actual,"hash_buffer_bytes":65536,"legacy_limit_refused":true,"v1_request_bytes_preserved":true,"capacity_preserved":true,"scratch_preserved":true,"native_admission":false,"rendered":false,"published":false,"consumer_adopted":false});
        save(&root.join("qualification-result.json"), &result);
        save(&evidence.join("qualification-result.json"), &result);
    }
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
            hash_file(&root.path().join("piece"), 5, 512 * 1024 * 1024, None).unwrap(),
            reel_workflow::digest(b"exact")
        );
        assert!(hash_file(&root.path().join("piece"), 4, 512 * 1024 * 1024, None).is_err());
    }
}
