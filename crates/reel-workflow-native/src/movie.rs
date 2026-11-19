//! Task-bound production port. Caller supplies an admitted owned workspace.
//! No job acceptance, candidate registration, retention or completion occurs.
use crate::{
    NativeInputs, Tool, error,
    film_inputs::{Inputs, Programs},
    film_process::{self, Rendered},
};
use reel::scene_delivery::FileRef;
use reel_workflow::{
    Artifact, ArtifactSet, ErrorKind, Packet, Result, ReturnedResult, film_render,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    path::Path,
};

pub struct Generated {
    pub files: BTreeMap<String, Vec<u8>>,
    pub outputs: ArtifactSet,
    pub paths: BTreeMap<String, String>,
    pub receipt: film_render::Receipt,
    _inputs: Inputs,
    _rendered: Rendered,
    _controls: Vec<File>,
}
fn reference(path: &str, artifact: &Artifact) -> FileRef {
    FileRef {
        path: path.into(),
        sha256: artifact.sha256.clone(),
        bytes: artifact.bytes,
    }
}
/// Produce a complete candidate packet for one exact FilmRender task. The
/// guarded process port checks media. Automatic dispatch remains gated until
/// independent returned/native verification and retained recovery are connected.
pub fn generate(
    packet: &Packet,
    mut native: NativeInputs,
    programs: &BTreeMap<String, Tool>,
    runtime: &Tool,
    workspace: &Path,
) -> Result<Generated> {
    let bound = film_render::bind(packet, &mut native)?;
    crate::worker::validate_programs(packet, programs)?;
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "movie-tool"))?;
    if runtime.sha256 != tool.executable.sha256 || runtime.bytes != tool.executable.bytes {
        return Err(error(ErrorKind::Conflict, "movie-native-runtime-pin"));
    }
    if bound
        .request()
        .output_paths
        .values()
        .any(|path| !crate::staging::portable(path))
    {
        return Err(error(ErrorKind::Invalid, "movie-portable-output-path"));
    }
    let inputs = Inputs::prepare(
        packet,
        native,
        bound.aliases(),
        Programs {
            ffmpeg: programs["ffmpeg"].clone(),
            ffprobe: programs["ffprobe"].clone(),
        },
    )?;
    let expected_outputs = bound
        .request()
        .output_paths
        .values()
        .filter_map(|path| path.strip_prefix("delivery/"))
        .filter(|name| *name != "receipt.json")
        .map(str::to_owned)
        .collect();
    let rendered = film_process::render_declared(
        &inputs,
        &reference(&bound.request().job_path, bound.job()),
        &reference(&bound.request().production_path, bound.production()),
        workspace,
        runtime,
        &bound.request().limits,
        Some(expected_outputs),
    )?;
    let mut files: BTreeMap<_, _> = rendered
        .files
        .iter()
        .map(|(name, raw)| (format!("delivery/{name}"), raw.clone()))
        .collect();
    let mut controls = Vec::new();
    for name in ["render-request.json", "render-result.json"] {
        let (raw, held) = crate::staging::read_held(
            &workspace.join(name),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        files.insert(name.into(), raw);
        controls.push(held);
    }
    let receipt = film_render::capture_outputs(packet, &bound, &files)?;
    let receipt_raw = serde_json::to_vec(&receipt)
        .map_err(|_| error(ErrorKind::Invalid, "movie-receipt-json"))?;
    files
        .values()
        .try_fold(receipt_raw.len() as u64, |n, raw| {
            n.checked_add(raw.len() as u64)
        })
        .filter(|n| *n <= 128 * 1024 * 1024)
        .ok_or_else(|| error(ErrorKind::Budget, "movie-result-and-receipt-bytes"))?;
    let worker_receipt = Artifact {
        logical_id: format!("workflow/film/{}/worker-receipt", packet.task_key),
        sha256: reel_workflow::digest(&receipt_raw),
        bytes: receipt_raw.len() as u64,
        media_type: "application/json".into(),
    };
    files.insert("receipt.json".into(), receipt_raw);
    let identities = files
        .iter()
        .map(|(name, raw)| (name.clone(), tig_core::Object::from_bytes(raw)))
        .collect();
    crate::output_layout::check(
        &receipt.outputs,
        &worker_receipt,
        &bound.request().output_paths,
        &identities,
    )?;
    Ok(Generated {
        outputs: receipt.outputs.clone(),
        paths: bound.request().output_paths.clone(),
        files,
        receipt,
        _inputs: inputs,
        _rendered: rendered,
        _controls: controls,
    })
}

/// Owns current input authority and exact returned-file guards after independent
/// media/primary comparison. This supplies no registration or retained proof.
pub struct Checked {
    _inputs: Inputs,
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
/// Verify external/scattered candidate files against exact task inputs. The
/// separate verifier runtime is explicitly pinned; it need not be the historic
/// producer executable. Caller admits an empty owned verification workspace.
pub fn check_returned(
    packet: &Packet,
    mut native: NativeInputs,
    result: &ReturnedResult,
    returns: &crate::response::Returns,
    programs: &BTreeMap<String, Tool>,
    verifier: &Tool,
    workspace: &Path,
) -> Result<Checked> {
    reel_workflow::check_result(packet, result)?;
    let (admission, attempt) = native.movie_authority()?;
    if result.attempt_id != attempt {
        return Err(error(ErrorKind::Conflict, "movie-original-result-attempt"));
    }
    let protected = [
        admission.index_root.clone(),
        admission.metadata_root.clone(),
        admission.scope.coordination_root.clone(),
        native.source.pool.repository.clone(),
        native.repository.clone(),
    ];
    if returns.schema != "reel.workflow-returns.v1"
        || !returns.root.is_absolute()
        || !workspace.is_absolute()
        || returns.files.keys().ne(result.artifacts.keys())
        || returns.files.values().any(|p| !crate::staging::portable(p))
    {
        return Err(error(ErrorKind::Invalid, "movie-return-file-set"));
    }
    let mut directories = vec![
        reel_assets::jobs::open_workspace_directory(&returns.root)
            .map_err(|_| error(ErrorKind::Denied, "movie-return-root"))?,
        reel_assets::jobs::open_workspace_directory(workspace)
            .map_err(|_| error(ErrorKind::Denied, "movie-verification-root"))?,
    ];
    let root = directories[0].path().to_path_buf();
    let workspace = directories[1].path().to_path_buf();
    if root.starts_with(&workspace) || workspace.starts_with(&root) {
        return Err(error(
            ErrorKind::Denied,
            "movie-return-verification-overlap",
        ));
    }
    for path in protected {
        let path = path.canonicalize().map_err(crate::io_error)?;
        if root.starts_with(&path)
            || path.starts_with(&root)
            || workspace.starts_with(&path)
            || path.starts_with(&workspace)
        {
            return Err(error(ErrorKind::Denied, "movie-managed-workspace-overlap"));
        }
    }
    let bound = film_render::bind(packet, &mut native)?;
    crate::worker::validate_programs(packet, programs)?;
    let worker = result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "movie-worker-receipt-required"))?;
    result
        .artifacts
        .values()
        .try_fold(worker.bytes, |n, a| n.checked_add(a.bytes))
        .filter(|n| *n <= 128 * 1024 * 1024)
        .ok_or_else(|| error(ErrorKind::Budget, "movie-return-byte-bound"))?;
    let mut actual = BTreeSet::new();
    crate::comic::inventory(&root, &root, 0, &mut actual, &mut directories, &mut 0)?;
    let expected: BTreeSet<_> = returns
        .files
        .values()
        .cloned()
        .chain(std::iter::once("receipt.json".into()))
        .collect();
    if actual != expected {
        return Err(error(ErrorKind::Integrity, "movie-return-inventory"));
    }
    let (wrapper, held) = crate::staging::read_held(&root.join("receipt.json"), worker.bytes)?;
    if wrapper.len() as u64 != worker.bytes || reel_workflow::digest(&wrapper) != worker.sha256 {
        return Err(error(
            ErrorKind::Integrity,
            "movie-return-worker-receipt-pin",
        ));
    }
    let mut guards = vec![held];
    let mut buffers = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut identities = BTreeMap::new();
    identities.insert(
        "receipt.json".into(),
        tig_core::Object::from_bytes(&wrapper),
    );
    for (name, relative) in &returns.files {
        let artifact = &result.artifacts[name];
        let physical = root.join(relative);
        directories.push(
            reel_assets::jobs::open_workspace_directory(physical.parent().unwrap())
                .map_err(|_| error(ErrorKind::Denied, "movie-return-file-namespace"))?,
        );
        let (raw, held) = crate::staging::read_held(&physical, artifact.bytes)?;
        reel_workflow::check_output(packet, result, name, &raw)?;
        identities.insert(relative.clone(), tig_core::Object::from_bytes(&raw));
        guards.push(held);
        let nominal = &bound.request().output_paths[name];
        if let Some(name) = nominal.strip_prefix("delivery/") {
            outputs.insert(
                name.to_owned(),
                FileRef {
                    path: physical,
                    sha256: artifact.sha256.clone(),
                    bytes: artifact.bytes,
                },
            );
        }
        buffers.insert(nominal.clone(), raw);
    }
    crate::output_layout::check(&result.artifacts, worker, &returns.files, &identities)?;
    let expected = film_render::capture_outputs(packet, &bound, &buffers)?;
    if serde_json::to_vec(&expected)
        .map_err(|_| error(ErrorKind::Invalid, "movie-return-receipt-json"))?
        != wrapper
        || expected.outputs != result.artifacts
    {
        return Err(error(
            ErrorKind::Conflict,
            "movie-return-receipt-correspondence",
        ));
    }
    film_process::check_render_evidence(
        packet,
        &bound,
        &buffers["render-request.json"],
        &buffers["render-result.json"],
    )?;
    let inputs = Inputs::prepare(
        packet,
        native,
        bound.aliases(),
        Programs {
            ffmpeg: programs["ffmpeg"].clone(),
            ffprobe: programs["ffprobe"].clone(),
        },
    )?;
    film_process::verify(
        &inputs,
        &reference(&bound.request().job_path, bound.job()),
        &reference(&bound.request().production_path, bound.production()),
        &outputs,
        &workspace,
        verifier,
        &bound.request().limits,
    )?;
    Ok(Checked {
        _inputs: inputs,
        _files: guards,
        _directories: directories,
    })
}
