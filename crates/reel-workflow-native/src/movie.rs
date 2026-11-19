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
#[cfg(test)]
#[path = "movie_streaming_tests.rs"]
mod streaming_tests;

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
    native: NativeInputs,
    programs: &BTreeMap<String, Tool>,
    runtime: &Tool,
    workspace: &Path,
) -> Result<Generated> {
    produce(packet, native, programs, runtime, workspace, false)
}
pub(crate) fn generate_or_adopt(
    packet: &Packet,
    native: NativeInputs,
    programs: &BTreeMap<String, Tool>,
    runtime: &Tool,
    workspace: &Path,
) -> Result<Generated> {
    let _namespace = reel_assets::jobs::open_workspace_directory(workspace)
        .map_err(|_| error(ErrorKind::Denied, "movie-owned-packet-namespace"))?;
    let existing = std::fs::read_dir(workspace)
        .map_err(crate::io_error)?
        .next()
        .is_some();
    produce(packet, native, programs, runtime, workspace, existing)
}
fn produce(
    packet: &Packet,
    mut native: NativeInputs,
    programs: &BTreeMap<String, Tool>,
    runtime: &Tool,
    workspace: &Path,
    adopt: bool,
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
    if adopt {
        for entry in std::fs::read_dir(workspace).map_err(crate::io_error)? {
            let name = entry.map_err(crate::io_error)?.file_name();
            if ![
                "delivery",
                "render-request.json",
                "render-result.json",
                "receipt.json",
            ]
            .iter()
            .any(|allowed| name == *allowed)
            {
                return Err(error(
                    ErrorKind::NeedsRecovery,
                    "movie-original-packet-has-foreign-entry",
                ));
            }
        }
    }
    let rendered = if adopt {
        film_process::adopt_declared(
            &inputs,
            &reference(&bound.request().job_path, bound.job()),
            &reference(&bound.request().production_path, bound.production()),
            workspace,
            runtime,
            &bound.request().limits,
            expected_outputs,
        )?
    } else {
        film_process::render_declared(
            &inputs,
            &reference(&bound.request().job_path, bound.job()),
            &reference(&bound.request().production_path, bound.production()),
            workspace,
            runtime,
            &bound.request().limits,
            Some(expected_outputs),
        )?
    };
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
    if adopt && workspace.join("receipt.json").exists() {
        let (old, held) = crate::staging::read_held(
            &workspace.join("receipt.json"),
            reel_workflow::MAX_CONTROL_BYTES as u64,
        )?;
        if old != receipt_raw {
            return Err(error(ErrorKind::Integrity, "movie-original-worker-receipt"));
        }
        controls.push(held);
    }
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
impl Checked {
    pub(crate) fn into_return_guards(
        self,
    ) -> (Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>) {
        (self._files, self._directories)
    }
}
fn validate_candidate(
    packet: &Packet,
    bound: &film_render::Bound,
    result: &ReturnedResult,
    pins: &BTreeMap<String, film_render::OutputContent>,
    buffers: &BTreeMap<String, Vec<u8>>,
    wrapper: &[u8],
) -> Result<()> {
    let expected = film_render::capture_output_pins(packet, bound, pins)?;
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
        bound,
        &buffers["render-request.json"],
        &buffers["render-result.json"],
    )
}

/// Only execution controls and explicitly requested review metadata need byte
/// buffers. Media stays behind held file handles and is independently checked.
fn checked_output_file(
    packet: &Packet,
    result: &ReturnedResult,
    name: &str,
    physical: &Path,
    capture: bool,
) -> Result<(film_render::OutputContent, Option<Vec<u8>>, File)> {
    let asset = &result.artifacts[name];
    let (bytes, held) = if capture {
        let (raw, held) =
            crate::staging::read_held(physical, reel_workflow::MAX_CONTROL_BYTES as u64)?;
        reel_workflow::check_output(packet, result, name, &raw)?;
        (Some(raw), held)
    } else {
        let mut held = crate::staging::open_held(physical, asset.bytes)?;
        reel_workflow::check_output_reader(packet, result, name, &mut held, asset.bytes)?;
        (None, held)
    };
    Ok((
        film_render::OutputContent {
            sha256: asset.sha256.clone(),
            bytes: asset.bytes,
        },
        bytes,
        held,
    ))
}

pub(crate) struct Proof {
    _inputs: Inputs,
    pub(crate) files: Option<BTreeMap<String, Vec<u8>>>,
    bound: film_render::Bound,
}
impl Proof {
    pub(crate) fn review_copy(
        &self,
        packet: &Packet,
        result: &ReturnedResult,
    ) -> Result<reel_workflow::review::Export> {
        let files = self
            .files
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Unsupported, "movie-review-capture-required"))?;
        let raw = files
            .get("delivery/review-ids.json")
            .ok_or_else(|| error(ErrorKind::Unsupported, "movie-review-ids-required"))?;
        let metadata: serde_json::Value = reel_workflow::decode(raw)?;
        if metadata["schema"] != "reel.scene-review-ids.v1"
            || metadata["job_sha256"] != self.bound.job().sha256
        {
            return Err(error(ErrorKind::Integrity, "movie-review-map-job"));
        }
        let frames = metadata["frame_count"]
            .as_u64()
            .ok_or_else(|| error(ErrorKind::Integrity, "movie-review-map-frames"))?;
        let labels: Vec<reel::scene_review_ids::Label> =
            serde_json::from_value(metadata["labels"].clone())
                .map_err(|_| error(ErrorKind::Integrity, "movie-review-map-labels"))?;
        let mut units = BTreeMap::new();
        for label in labels {
            let unit = if let Some(cel_id) = label.cel_id {
                reel_workflow::review::Unit::Film {
                    cel_id,
                    shot_id: Some(label.shot_id),
                    cue_id: None,
                    start_frame: label.start_frame,
                    end_frame: label.end_frame,
                }
            } else {
                reel_workflow::review::Unit::Shot {
                    shot_id: label.shot_id,
                    start_frame: label.start_frame,
                    end_frame: label.end_frame,
                }
            };
            if units.insert(label.attachment_id, unit).is_some() {
                return Err(error(
                    ErrorKind::Conflict,
                    "movie-review-attachment-duplicate",
                ));
            }
        }
        let review_name = self
            .bound
            .request()
            .output_paths
            .iter()
            .find(|(_, path)| path.as_str() == "delivery/review.mp4")
            .map(|(name, _)| name)
            .ok_or_else(|| error(ErrorKind::Missing, "movie-review-output"))?;
        let copy = reel_workflow::review::Copy {
            schema: "reel.workflow-review-copy.v1".into(),
            project: packet.project.clone(),
            source_commit: packet.source_commit.clone(),
            language: packet
                .task
                .language
                .clone()
                .ok_or_else(|| error(ErrorKind::Missing, "movie-review-language"))?,
            publication: self.bound.job().clone(),
            review: result.artifacts[review_name].clone(),
            format: reel_workflow::review::Format::Film { frames },
            units,
        };
        reel_workflow::review::check_copy(&copy)?;
        Ok(reel_workflow::review::Export {
            copy,
            manifest: None,
        })
    }
}
pub(crate) fn native_proof(
    packet: &Packet,
    source: &crate::NativeProfile,
    result: &ReturnedResult,
    capture: bool,
    permit: crate::admission::InputPermit,
) -> Result<Proof> {
    reel_workflow::check_result(packet, result)?;
    let config = source
        .movie_verifier
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Unsupported, "movie-verifier-profile-required"))?;
    let mut native = NativeInputs::from_permit(packet, source, permit)?;
    let (admission, attempt) = native.movie_authority()?;
    if attempt != result.attempt_id {
        return Err(error(ErrorKind::Conflict, "movie-original-result-attempt"));
    }
    let admission = admission.clone();
    let bound = film_render::bind(packet, &mut native)?;
    crate::worker::validate_programs(packet, &config.auxiliary_programs)?;
    let mut inputs = Inputs::prepare(
        packet,
        native,
        bound.aliases(),
        Programs {
            ffmpeg: config.auxiliary_programs["ffmpeg"].clone(),
            ffprobe: config.auxiliary_programs["ffprobe"].clone(),
        },
    )?;
    let worker = result
        .worker_receipt
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "movie-worker-receipt-required"))?;
    let artifacts = result
        .artifacts
        .iter()
        .map(|(name, a)| (bound.request().output_paths[name].clone(), a.clone()))
        .chain(std::iter::once(("receipt.json".into(), worker.clone())))
        .collect();
    let paths = inputs.returned_paths(&artifacts)?;
    let mut buffers = BTreeMap::new();
    let mut pins = BTreeMap::new();
    let mut outputs = BTreeMap::new();
    let mut wrapper = Vec::new();
    for (name, pin) in &paths {
        if name == "receipt.json" {
            let (raw, _guard) =
                crate::staging::read_held(&pin.path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
            if raw.len() as u64 != pin.bytes || reel_workflow::digest(&raw) != pin.sha256 {
                return Err(error(ErrorKind::Integrity, "movie-native-candidate-pin"));
            }
            wrapper = raw;
        } else {
            let output_name = bound
                .request()
                .output_paths
                .iter()
                .find(|(_, p)| *p == name)
                .map(|(n, _)| n)
                .ok_or_else(|| error(ErrorKind::Conflict, "movie-native-output-path"))?;
            let needs_bytes = matches!(name.as_str(), "render-request.json" | "render-result.json")
                || (capture && name == "delivery/review-ids.json");
            let (content, raw, _guard) =
                checked_output_file(packet, result, output_name, &pin.path, needs_bytes)?;
            pins.insert(name.clone(), content);
            if let Some(raw) = raw {
                buffers.insert(name.clone(), raw);
            }
        }
        if let Some(virtual_name) = name.strip_prefix("delivery/") {
            outputs.insert(virtual_name.to_owned(), pin.clone());
        }
    }
    validate_candidate(packet, &bound, result, &pins, &buffers, &wrapper)?;
    let required = bound
        .request()
        .limits
        .workspace_bytes
        .checked_add(10 * reel_workflow::MAX_CONTROL_BYTES as u64)
        .ok_or_else(|| error(ErrorKind::Budget, "movie-verification-reservation"))?;
    if required > config.staging.maximum_bytes {
        return Err(error(ErrorKind::Budget, "movie-verification-reservation"));
    }
    let context = serde_json::json!({"schema":"reel.workflow-native-movie-check.v1","packet":packet,"result":result,
        "inputs":inputs.process_files()?,"outputs":outputs,"verifier":config});
    crate::staging::movie_verification::run(
        &config.staging,
        source,
        &admission,
        &context,
        |workspace| {
            film_process::verify(
                &inputs,
                &reference(&bound.request().job_path, bound.job()),
                &reference(&bound.request().production_path, bound.production()),
                &outputs,
                workspace,
                &config.runtime,
                &bound.request().limits,
            )
        },
    )?;
    if capture {
        buffers.insert("receipt.json".into(), wrapper);
    }
    Ok(Proof {
        _inputs: inputs,
        files: capture.then_some(buffers),
        bound,
    })
}

pub(crate) fn check_returned_owned(
    packet: &Packet,
    source: &crate::NativeProfile,
    mut native: NativeInputs,
    result: &ReturnedResult,
    root: &Path,
    paths: &BTreeMap<String, String>,
) -> Result<(Vec<File>, Vec<reel_assets::jobs::WorkspaceDirectory>)> {
    let config = source
        .movie_verifier
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Unsupported, "movie-verifier-profile-required"))?;
    let (admission, _) = native.movie_authority()?;
    let admission = admission.clone();
    let bound = film_render::bind(packet, &mut native)?;
    let required = bound
        .request()
        .limits
        .workspace_bytes
        .checked_add(10 * reel_workflow::MAX_CONTROL_BYTES as u64)
        .ok_or_else(|| error(ErrorKind::Budget, "movie-verification-reservation"))?;
    if required > config.staging.maximum_bytes {
        return Err(error(ErrorKind::Budget, "movie-verification-reservation"));
    }
    let parent = config
        .staging
        .parent
        .canonicalize()
        .map_err(crate::io_error)?;
    if parent.starts_with(root) || root.starts_with(&parent) {
        return Err(error(
            ErrorKind::Denied,
            "movie-return-verification-parent-overlap",
        ));
    }
    let context = serde_json::json!({"schema":"reel.workflow-return-movie-check.v1","packet":packet,"result":result,"source":source,
        "root":root,"paths":paths,"verifier":config});
    let returns = crate::response::Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: root.into(),
        files: paths.clone(),
    };
    crate::staging::movie_verification::run(
        &config.staging,
        source,
        &admission,
        &context,
        |workspace| {
            check_returned(
                packet,
                native,
                result,
                &returns,
                &config.auxiliary_programs,
                &config.runtime,
                workspace,
            )
            .map(Checked::into_return_guards)
        },
    )
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
    let mut pins = BTreeMap::new();
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
        let nominal = &bound.request().output_paths[name];
        let (content, raw, held) = checked_output_file(
            packet,
            result,
            name,
            &physical,
            matches!(
                nominal.as_str(),
                "render-request.json" | "render-result.json"
            ),
        )?;
        identities.insert(
            relative.clone(),
            tig_core::Object {
                sha256: tig_core::Digest::parse(&content.sha256)
                    .map_err(|_| error(ErrorKind::Invalid, "movie-return-pin"))?,
                bytes: content.bytes,
            },
        );
        pins.insert(nominal.clone(), content);
        guards.push(held);
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
        if let Some(raw) = raw {
            buffers.insert(nominal.clone(), raw);
        }
    }
    crate::output_layout::check(&result.artifacts, worker, &returns.files, &identities)?;
    validate_candidate(packet, &bound, result, &pins, &buffers, &wrapper)?;
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
