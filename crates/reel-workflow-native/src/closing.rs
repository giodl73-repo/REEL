//! Explicit retained-owner handoff/slot close over the unchanged qualified SDK.
//! No scratch retirement, retention release, capacity growth or payload deletion.
use crate::{NativeProfile, Tool, admission, error, handoff, retention, staging};
use reel_workflow::{ErrorKind, Packet, Result, ReturnedResult};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    path::{Path, PathBuf},
};
use tig_core::Object;
use tig_store::{
    Key, metadata,
    source_claims::{self, protection::work::WorkRef},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Preview,
    Stage,
    Apply,
    Recover,
}
#[derive(Debug)]
pub(crate) struct Staged;
impl std::fmt::Display for Staged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("original close handoff staged")
    }
}
impl std::error::Error for Staged {}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub journal: PathBuf,
    pub packet: Packet,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub receipt: admission::Receipt,
    pub result: ReturnedResult,
    pub acknowledgement: retention::Acknowledgement,
    pub operation: Key,
    /// Original journal runtime stays immutable; this close is a separately
    /// pinned operation and never replaces/restarts an admitted producer.
    pub original_lifecycle_tool: reel_workflow::Artifact,
    pub runtime: Tool,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub schema: String,
    pub operation: Key,
    pub admission_identity: String,
    pub activation: Object,
    pub target: retention::Target,
    pub applied: bool,
    pub slot_idle: bool,
    pub original_clear_complete: bool,
    pub handoff: Option<WorkRef>,
    pub clear: Option<WorkRef>,
    pub index_checkpoint: source_claims::Checkpoint,
    pub metadata_checkpoint: metadata::Checkpoint,
}
pub fn read(path: &Path, sha256: &str) -> Result<(Request, File)> {
    let (raw, held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    if reel_workflow::digest(&raw) != sha256 {
        return Err(error(ErrorKind::Integrity, "close-request-pin"));
    }
    Ok((reel_workflow::decode(&raw)?, held))
}
/// Preview checks only; effects require an explicit action and the SAME pinned
/// request/runtime/operation on recovery. A timeout never means delete/retry new.
pub fn run(request: &Request, action: Action) -> Result<Outcome> {
    if request.schema != "reel.workflow-native-close.v1" {
        return Err(error(ErrorKind::Invalid, "close-request-schema"));
    }
    let _runtime = handoff::runtime(&request.runtime)?;
    let journal = reel_workflow::handoff::Journal::open(&request.journal)?;
    let binding = handoff::Binding {
        schema: "reel.workflow-native-handoff-binding.v1".into(),
        admission: request.receipt.clone(),
        target: request.acknowledgement.target.clone(),
    };
    if journal.state() != reel_workflow::handoff::State::Retained
        || journal.ticket().packet != request.packet
        || journal.ticket().attempt_id != request.receipt.attempt_id
        || journal.ticket().native_binding != binding.artifact()?
        || journal.ticket().lifecycle_tool != request.original_lifecycle_tool
        || journal.result() != Some(&request.result)
        || journal.acknowledgement() != Some(&request.acknowledgement.artifact()?)
    {
        return Err(error(
            ErrorKind::Conflict,
            "close-original-retained-journal",
        ));
    }
    reel_workflow::check_result(&request.packet, &request.result)?;
    if request.result.attempt_id != request.receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "close-original-attempt"));
    }
    // Actual retained bytes must reopen, not merely a supplied acknowledgement.
    // Drop non-reentrant read/fence guards before the SDK's ordered close locks;
    // durable retained claims remain and the SDK rechecks coverage before clear.
    if action != Action::Recover {
        drop(retention::verify(
            &request.packet,
            &request.source,
            &request.admission,
            &request.receipt,
            &request.result,
            &request.acknowledgement,
        )?);
    }
    admission::Prepared::open(
        &request.packet,
        &request.source,
        &request.admission,
        &request.receipt.attempt_id,
    )?
    .close_retained(
        &request.receipt,
        &request.result,
        &request.acknowledgement,
        &request.acknowledgement.target,
        &request.operation,
        action,
        &request.source,
    )
}
