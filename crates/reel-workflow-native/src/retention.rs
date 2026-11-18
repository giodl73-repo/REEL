//! Workflow-specific retained actor registration, separate from selection and
//! scratch retirement. Serialized acknowledgements never grant a live guard.
use crate::{
    NativeProfile,
    admission::{self, Prepared, Receipt},
    error, hold_tool, io_error, response,
};
use reel_workflow::{Artifact, ErrorKind, Packet, Result, ReturnedResult, check_result};
use serde::{Deserialize, Serialize};
use std::fs::File;
use tig_core::Object;
use tig_git::annex::workflow::reader;
use tig_store::{
    Key, metadata,
    source_claims::{self, protection::work::WorkRef},
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub schema: String,
    pub actor: Key,
    pub owner: Key,
}

/// May also describe staged, incomplete work. Only `verify` supplies live proof.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Acknowledgement {
    pub schema: String,
    pub candidate: response::Candidate,
    pub target: Target,
    pub evidence: Object,
    pub work: WorkRef,
    pub index_checkpoint: source_claims::Checkpoint,
    pub metadata_checkpoint: metadata::Checkpoint,
}
impl Acknowledgement {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        admission::encode(self)
    }
    pub fn artifact(&self) -> Result<Artifact> {
        let raw = self.canonical_bytes()?;
        Ok(Artifact {
            logical_id: format!(
                "workflow/retained/{}/{}",
                self.candidate.task_key, self.candidate.attempt_id
            ),
            sha256: reel_workflow::digest(&raw),
            bytes: raw.len() as u64,
            media_type: "application/json".into(),
        })
    }
}

/// Holds actual retained actor fences, exact snapshot/Git/annex pins and reopened
/// native outputs. Cannot be constructed by deserializing an acknowledgement.
pub struct RetainedGuard {
    pub(crate) _fences: Vec<reel_assets::admission::MetadataLock>,
    pub(crate) _prepared: Prepared,
    _annex: File,
    _outputs: Vec<reader::VerifiedRead>,
}

fn check(packet: &Packet, receipt: &Receipt, result: &ReturnedResult) -> Result<()> {
    check_result(packet, result)?;
    if result.attempt_id != receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "retention-original-attempt"));
    }
    if result.provenance.mode == reel_workflow::Mode::Tool {
        return Err(error(
            ErrorKind::Unsupported,
            "retention-tool-receipt-adapter-required",
        ));
    }
    Ok(())
}
fn outputs(source: &NativeProfile, result: &ReturnedResult) -> Result<Vec<reader::VerifiedRead>> {
    if result.artifacts.len() > 32
        || result
            .artifacts
            .values()
            .try_fold(0u64, |n, a| n.checked_add(a.bytes))
            .is_none_or(|n| n > 128 * 1024 * 1024)
    {
        return Err(error(ErrorKind::Budget, "retention-output-bounds"));
    }
    result
        .artifacts
        .values()
        .map(|a| {
            reader::open(
                &source.source,
                &Object {
                    sha256: tig_core::Digest::parse(&a.sha256)
                        .map_err(|_| error(ErrorKind::Invalid, "retention-output-digest"))?,
                    bytes: a.bytes,
                },
            )
            .map_err(io_error)
        })
        .collect()
}

/// Stage exact retained work only. No output availability, retained claims,
/// completion or scratch retirement is inferred from this descriptor.
pub fn stage(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    target: &Target,
) -> Result<Acknowledgement> {
    check(packet, receipt, result)?;
    let prepared = Prepared::open(packet, source, profile, &receipt.attempt_id)?;
    if let Some(ack) = prepared.completed_retention(receipt, result, target)? {
        return Ok(ack);
    }
    let (guard, candidate) = prepared.retention_guard(receipt, result, target)?;
    guard.retain_response(receipt, &candidate, target, false)
}

/// Reopen already-published exact outputs, acknowledge positive retained claims,
/// then read them back. Does not release the job, retire scratch or select assets.
pub fn retain(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    target: &Target,
) -> Result<Acknowledgement> {
    check(packet, receipt, result)?;
    let _annex = hold_tool(&source.annex)?;
    let prepared = Prepared::open(packet, source, profile, &receipt.attempt_id)?;
    if let Some(ack) = prepared.completed_retention(receipt, result, target)? {
        let _verified = verify(packet, source, profile, receipt, result, &ack)?;
        return Ok(ack);
    }
    let (guard, candidate) = prepared.retention_guard(receipt, result, target)?;
    let _outputs = outputs(source, result)?;
    let acknowledgement = guard.retain_response(receipt, &candidate, target, true)?;
    guard.verify_retention(receipt, &acknowledgement)?;
    Ok(acknowledgement)
}

/// Verify exact original native work, retained-owner coverage and actual output
/// bytes. Historical verification does not require the source actor to remain
/// in its former generation, and never retargets selection or republishes bytes.
pub fn verify(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    acknowledgement: &Acknowledgement,
) -> Result<RetainedGuard> {
    check(packet, receipt, result)?;
    let _annex = hold_tool(&source.annex)?;
    let prepared = Prepared::open(packet, source, profile, &receipt.attempt_id)?;
    let fences = prepared.verify_retained(receipt, result, acknowledgement)?;
    let outputs = outputs(source, result)?;
    Ok(RetainedGuard {
        _fences: fences,
        _prepared: prepared,
        _annex,
        _outputs: outputs,
    })
}
