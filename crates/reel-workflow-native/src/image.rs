//! Explicit PNG handoff envelopes. Packing neither registers candidates nor
//! supplies custody, a provider/model identity, selection or approval.
use crate::{error, io_error, response::Returns};
use reel_workflow::{
    Artifact, ErrorKind, Mode, Operation, Packet, Provenance, Result, ReturnedResult,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
};

pub struct Packed {
    pub result: ReturnedResult,
    _files: Vec<File>,
    _directories: Vec<reel_assets::jobs::WorkspaceDirectory>,
}
/// Holds ordinary file/directory guards while the caller writes the envelope.
/// Original input protection is the enclosing admitted caller's responsibility.
pub fn pack_files(packet: &Packet, attempt: &str, returns: &Returns) -> Result<Packed> {
    reel_workflow::check_packet(packet)?;
    if packet.task.operation != Operation::ImageCandidate
        || returns.schema != "reel.workflow-returns.v1"
        || !returns.root.is_absolute()
        || returns
            .root
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
        || returns.files.is_empty()
        || returns.files.len() > reel_publication::MAX_OUTPUTS
    {
        return Err(error(ErrorKind::Invalid, "image-handoff-contract"));
    }
    if packet
        .task
        .outputs
        .values()
        .any(|c| c.required && c.media_type != "image/png")
    {
        return Err(error(
            ErrorKind::Unsupported,
            "image-handoff-required-companion",
        ));
    }
    let mut directories = vec![
        reel_assets::jobs::open_workspace_directory(&returns.root)
            .map_err(|_| error(ErrorKind::Denied, "image-return-directory"))?,
    ];
    let root = returns.root.canonicalize().map_err(io_error)?;
    let mut files = Vec::new();
    let mut artifacts = BTreeMap::new();
    let mut paths = BTreeSet::new();
    let mut total = 0u64;
    for (name, path) in &returns.files {
        let contract = packet
            .task
            .outputs
            .get(name)
            .ok_or_else(|| error(ErrorKind::Invalid, "image-undeclared-output"))?;
        if contract.media_type != "image/png"
            || !crate::staging::portable(path)
            || !paths.insert(path.to_ascii_lowercase())
        {
            return Err(error(ErrorKind::Invalid, "image-return-type-or-path"));
        }
        let path = root.join(path);
        directories.push(
            reel_assets::jobs::open_workspace_directory(path.parent().unwrap())
                .map_err(|_| error(ErrorKind::Denied, "image-return-ancestors"))?,
        );
        let (raw, held) = crate::staging::read_held(
            &path,
            contract
                .max_bytes
                .min(reel_workflow_http::raster::MAX_IMAGE),
        )?;
        total = total
            .checked_add(raw.len() as u64)
            .filter(|n| *n <= 128 * 1024 * 1024)
            .ok_or_else(|| error(ErrorKind::Budget, "image-return-total-bytes"))?;
        reel_workflow_http::raster::inspect_png(&raw)?;
        artifacts.insert(
            name.clone(),
            Artifact {
                logical_id: format!("workflow/handoff/{}/{attempt}/{name}", packet.task_key),
                sha256: reel_workflow::digest(&raw),
                bytes: raw.len() as u64,
                media_type: contract.media_type.clone(),
            },
        );
        files.push(held);
    }
    let result = ReturnedResult {
        schema: reel_workflow::RESULT_SCHEMA.into(),
        task_id: packet.task.id.clone(),
        task_key: packet.task_key.clone(),
        attempt_id: attempt.into(),
        artifacts,
        provenance: Provenance {
            mode: Mode::Handoff,
            provider: None,
            model: None,
            executable: None,
        },
        worker_receipt: None,
    };
    reel_workflow::check_result(packet, &result)?;
    Ok(Packed {
        result,
        _files: files,
        _directories: directories,
    })
}
