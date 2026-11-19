//! Workflow-specific retained actor registration, separate from selection and
//! scratch retirement. Serialized acknowledgements never grant a live guard.
use crate::in_phase as phase;
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
    pub(crate) _fences: admission::Fences,
    pub(crate) _prepared: Prepared,
    _annex: File,
    _outputs: Vec<reader::VerifiedRead>,
    _comic: Option<crate::producer::Proof>,
}
pub(crate) struct ReadPin {
    _git: File,
    _annex: File,
}
/// Verify every parent's complete result before any selected output read gates
/// are held. Shared retained actors use one session-owned fence, not reentrancy.
pub(crate) fn pin_for_read(
    parent: &crate::upstream::Parent<'_>,
    session: &mut admission::RetainedFences,
) -> Result<ReadPin> {
    check(parent.packet, parent.receipt, parent.result)?;
    let annex = hold_tool(&parent.source.annex)?;
    let prepared = Prepared::open(
        parent.packet,
        parent.source,
        parent.admission,
        &parent.receipt.attempt_id,
    )?;
    prepared.verify_retained_in(
        parent.receipt,
        parent.result,
        parent.acknowledgement,
        session,
    )?;
    drop(outputs(parent.source, parent.result)?);
    drop(crate::producer::check_native(
        parent.packet,
        parent.source,
        parent.result,
        prepared.input_permit(session.locks.clone()),
    )?);
    Ok(ReadPin {
        _git: prepared.into_git_guard(),
        _annex: annex,
    })
}
impl RetainedGuard {
    pub(crate) fn comic_files(&self) -> Result<&std::collections::BTreeMap<String, Vec<u8>>> {
        self._comic
            .as_ref()
            .and_then(|proof| proof.files())
            .ok_or_else(|| error(ErrorKind::Unsupported, "retention-comic-capture-required"))
    }
}

fn check(packet: &Packet, receipt: &Receipt, result: &ReturnedResult) -> Result<()> {
    check_result(packet, result)?;
    if result.attempt_id != receipt.attempt_id {
        return Err(error(ErrorKind::Conflict, "retention-original-attempt"));
    }
    if result.provenance.mode == reel_workflow::Mode::Tool && result.worker_receipt.is_none() {
        return Err(error(ErrorKind::Missing, "comic-worker-receipt-required"));
    }
    if result.artifacts.len() > reel_publication::MAX_OUTPUTS
        || response::artifacts(result)
            .try_fold(0u64, |n, a| n.checked_add(a.bytes))
            .is_none_or(|n| n > 128 * 1024 * 1024)
    {
        return Err(error(ErrorKind::Budget, "retention-output-bounds"));
    }
    Ok(())
}
fn outputs(source: &NativeProfile, result: &ReturnedResult) -> Result<Vec<reader::VerifiedRead>> {
    // Comic proof owns its source/output gates and performs full replay.
    if result.provenance.mode == reel_workflow::Mode::Tool {
        return Ok(Vec::new());
    }
    let mut seen = std::collections::BTreeSet::new();
    response::artifacts(result)
        .filter(|a| seen.insert((a.sha256.clone(), a.bytes)))
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
    if let Some(ack) = prepared
        .completed_retention(receipt, result, target)
        .map_err(|e| phase(e, "retention-completed-probe"))?
    {
        return Ok(ack);
    }
    let (guard, candidate) = prepared
        .retention_guard(receipt, result, target)
        .map_err(|e| phase(e, "retention-guard"))?;
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
    if let Some(ack) = prepared
        .completed_retention(receipt, result, target)
        .map_err(|e| phase(e, "retention-completed-probe"))?
    {
        let _verified = verify(packet, source, profile, receipt, result, &ack)?;
        return Ok(ack);
    }
    let (guard, candidate) = prepared
        .retention_guard(receipt, result, target)
        .map_err(|e| phase(e, "retention-guard"))?;
    // Native read guards hold object effect gates. Do not keep those gates
    // while admitting a NEW index update. Original positive job claims and the
    // ordered source/retained fences protect this transition instead.
    drop(outputs(source, result).map_err(|e| phase(e, "retention-output-preflight"))?);
    drop(
        crate::producer::check_native(packet, source, result, guard.input_permit())
            .map_err(|e| phase(e, "retention-comic-preflight"))?,
    );
    let acknowledgement = guard
        .retain_response(receipt, &candidate, target, true)
        .map_err(|e| phase(e, "retention-apply"))?;
    guard.verify_retention(receipt, &acknowledgement)?;
    let _outputs = outputs(source, result).map_err(|e| phase(e, "retention-output-readback"))?;
    let _comic = crate::producer::check_native(packet, source, result, guard.input_permit())
        .map_err(|e| phase(e, "retention-comic-readback"))?;
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
    verify_inner(
        packet,
        source,
        profile,
        receipt,
        result,
        acknowledgement,
        false,
    )
}
pub(crate) fn capture_comic(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    acknowledgement: &Acknowledgement,
) -> Result<RetainedGuard> {
    if result.provenance.mode != reel_workflow::Mode::Tool {
        return Err(error(
            ErrorKind::Unsupported,
            "retention-comic-result-required",
        ));
    }
    verify_inner(
        packet,
        source,
        profile,
        receipt,
        result,
        acknowledgement,
        true,
    )
}
#[allow(clippy::too_many_arguments)]
fn verify_inner(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &Receipt,
    result: &ReturnedResult,
    acknowledgement: &Acknowledgement,
    capture: bool,
) -> Result<RetainedGuard> {
    check(packet, receipt, result)?;
    let _annex = hold_tool(&source.annex)?;
    let prepared = Prepared::open(packet, source, profile, &receipt.attempt_id)?;
    let fences = prepared.verify_retained(receipt, result, acknowledgement)?;
    let outputs = outputs(source, result)?;
    let comic = if capture {
        crate::producer::capture_native(
            packet,
            source,
            result,
            prepared.input_permit(fences.clone()),
        )?
    } else {
        crate::producer::check_native(
            packet,
            source,
            result,
            prepared.input_permit(fences.clone()),
        )?
    };
    Ok(RetainedGuard {
        _fences: fences,
        _prepared: prepared,
        _annex,
        _outputs: outputs,
        _comic: comic,
    })
}
