//! Exact scene-control derivation check over a caller's native input authority.
use crate::{InputReader, error};
use reel::scene_delivery::{Job, repair};
use reel_workflow::{Artifact, ErrorKind, Packet, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Derivation {
    pub schema: String,
    pub original_job: Artifact,
    pub replacement: repair::StillReplacement,
}
#[derive(Serialize)]
pub struct Verified {
    pub schema: &'static str,
    pub original_job_sha256: String,
    pub derived_job_sha256: String,
    pub geometry_review_required: bool,
    pub creative_approval: bool,
}
fn read_job(reader: &mut impl InputReader, artifact: &Artifact) -> Result<Job> {
    if artifact.media_type != "application/json"
        || artifact.bytes > reel_workflow::MAX_CONTROL_BYTES as u64
    {
        return Err(error(ErrorKind::Budget, "film-control-job-envelope"));
    }
    let raw = reader.read(artifact, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if raw.len() as u64 != artifact.bytes || reel_workflow::digest(&raw) != artifact.sha256 {
        return Err(error(ErrorKind::Integrity, "film-control-job-bytes"));
    }
    reel_workflow::decode(&raw)
}
pub fn verify(
    packet: &Packet,
    reader: &mut impl InputReader,
    spec: &Derivation,
) -> Result<Verified> {
    reel_workflow::check_packet(packet)?;
    let tool = packet
        .task
        .tool
        .as_ref()
        .filter(|t| t.capability == reel_workflow::ToolCapability::FilmRender && t.part.is_none())
        .ok_or_else(|| error(ErrorKind::Unsupported, "film-control-producer"))?;
    if spec.schema != "reel.workflow-film-control-derivation.v1"
        || !packet.task.inputs.contains(&spec.original_job)
        || spec.original_job.logical_id == tool.publication.logical_id
    {
        return Err(error(ErrorKind::Conflict, "film-control-original-job"));
    }
    for reference in [&spec.replacement.expected, &spec.replacement.replacement] {
        let path = reference
            .path
            .to_str()
            .ok_or_else(|| error(ErrorKind::Invalid, "film-control-picture-path"))?;
        let artifact = tool
            .source_bindings
            .get(path)
            .and_then(|id| packet.task.inputs.iter().find(|a| &a.logical_id == id))
            .ok_or_else(|| error(ErrorKind::Missing, "film-control-picture-binding"))?;
        if artifact.sha256 != reference.sha256 || artifact.bytes != reference.bytes {
            return Err(error(ErrorKind::Conflict, "film-control-picture-subject"));
        }
    }
    let original = read_job(reader, &spec.original_job)?;
    let candidate = read_job(reader, &tool.publication)?;
    let expected = repair::derive(&original, &spec.replacement)
        .map_err(|_| error(ErrorKind::Conflict, "film-control-still-derivation"))?;
    if reel_workflow::canonical_digest(&candidate)?
        != reel_workflow::canonical_digest(&expected.job)?
    {
        return Err(error(
            ErrorKind::Conflict,
            "film-control-unrequested-change",
        ));
    }
    Ok(Verified {
        schema: "reel.workflow-film-control-verified.v1",
        original_job_sha256: spec.original_job.sha256.clone(),
        derived_job_sha256: tool.publication.sha256.clone(),
        geometry_review_required: expected.geometry_review_required,
        creative_approval: false,
    })
}
