//! Task-bound production port. Caller supplies an admitted owned workspace.
//! No job acceptance, candidate registration, retention or completion occurs.
use crate::{
    NativeInputs, Tool, error,
    film_inputs::{Inputs, Programs},
    film_process::{self, Rendered},
};
use reel::scene_delivery::FileRef;
use reel_workflow::{Artifact, ArtifactSet, ErrorKind, Packet, Result, film_render};
use std::{collections::BTreeMap, fs::File, path::Path};

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
