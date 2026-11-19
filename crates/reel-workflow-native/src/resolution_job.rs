//! Owned resolution production over a declared parent payload closure. Original
//! retained contexts/tools must remain available; no latest-context fallback.
use crate::{
    NativeInputs, SnapshotInputs, error,
    resolution::{self, RetainedReview},
    upstream::ParentControl,
};
use reel_workflow::{
    Artifact, ArtifactSet, ErrorKind, Packet, Plan, Result, ToolCapability,
    publication::InputReader,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::Object;
#[cfg(test)]
#[path = "resolution_recovery_tests.rs"]
mod recovery_tests;
pub const REQUEST_ID: &str = "control/resolution-request";
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub baseline: RetainedReview,
    pub plan: Plan,
    pub repair: reel_workflow::review::Repair,
    pub revision: reel_workflow::repair::Revision,
    pub rebuilds: crate::graph::Bindings,
    pub productions: BTreeMap<String, ParentControl>,
    pub corrected: BTreeMap<String, RetainedReview>,
    pub decision: reel_workflow::resolution::Decision,
}
fn observed(binding: &crate::graph::Binding) -> Result<ParentControl> {
    let (raw, _guard) = binding.profile.read()?;
    let profile: crate::handoff::Profile = reel_workflow::decode(&raw)?;
    let job = crate::handoff::observe(&binding.journal, &profile)?;
    if job.state != reel_workflow::handoff::State::Retained {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "resolution-job-parent-not-retained",
        ));
    }
    let acknowledgement: crate::retention::Acknowledgement =
        crate::read_control(&profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            job.ticket.packet.task_key, profile.binding.admission.attempt_id
        )))?;
    if job.acknowledgement.as_ref() != Some(&acknowledgement.artifact()?) {
        return Err(error(ErrorKind::Integrity, "resolution-job-parent-ack"));
    }
    Ok(ParentControl {
        packet: job.ticket.packet,
        result: job
            .result
            .ok_or_else(|| error(ErrorKind::Missing, "resolution-job-parent-result"))?,
        profile,
        acknowledgement,
    })
}
impl Request {
    pub fn capture(
        baseline: RetainedReview,
        plan: Plan,
        repair: reel_workflow::review::Repair,
        revision: reel_workflow::repair::Revision,
        rebuilds: crate::graph::Bindings,
        corrected: BTreeMap<String, RetainedReview>,
        decision: reel_workflow::resolution::Decision,
    ) -> Result<Self> {
        let productions = rebuilds
            .tasks
            .iter()
            .map(|(id, b)| Ok((id.clone(), observed(b)?)))
            .collect::<Result<_>>()?;
        let result = Self {
            schema: "reel.workflow-resolution-job.v1".into(),
            baseline,
            plan,
            repair,
            revision,
            rebuilds,
            productions,
            corrected,
            decision,
        };
        result.resources()?;
        reel_workflow::canonical_digest(&result)?;
        Ok(result)
    }
    fn check(&self) -> Result<()> {
        if self.schema != "reel.workflow-resolution-job.v1"
            || self.corrected.is_empty()
            || self.corrected.len() > 128
            || self.rebuilds.tasks.is_empty()
            || self.productions.len() > 128
            || self.rebuilds.tasks.keys().ne(self.productions.keys())
        {
            return Err(error(ErrorKind::Invalid, "resolution-job-context"));
        }
        for (id, binding) in &self.rebuilds.tasks {
            let actual = observed(binding)?;
            if reel_workflow::canonical_digest(&actual)?
                != reel_workflow::canonical_digest(&self.productions[id])?
            {
                return Err(error(ErrorKind::Stale, "resolution-job-frozen-production"));
            }
        }
        Ok(())
    }
    /// Claims every declared native payload (including unused record streams)
    /// and every result/worker-receipt object needed by the original proofs.
    /// Alias IDs describe custody only; original record/leaf identity is checked
    /// again by each retained proof during production/replay.
    pub fn resources(&self) -> Result<Vec<Artifact>> {
        self.check()?;
        let mut objects = BTreeSet::new();
        let mut pool = None;
        let sources = std::iter::once((
            &self.baseline.packet,
            &self.baseline.source,
            &self.baseline.result,
        ))
        .chain(
            self.corrected
                .values()
                .map(|r| (&r.packet, &r.source, &r.result)),
        )
        .chain(
            self.productions
                .values()
                .map(|r| (&r.packet, &r.profile.source, &r.result)),
        );
        for (packet, source, result) in sources {
            let identity = (
                source.source.store.clone(),
                source
                    .source
                    .pool
                    .repository
                    .canonicalize()
                    .map_err(crate::io_error)?,
            );
            if pool.as_ref().is_some_and(|p| p != &identity) {
                return Err(error(
                    ErrorKind::Unsupported,
                    "resolution-job-one-pool-required",
                ));
            }
            pool = Some(identity);
            let snapshot = SnapshotInputs::open(packet, &source.snapshot)?;
            objects.extend(snapshot.declared_objects.iter().cloned());
            for a in crate::response::artifacts(result) {
                objects.insert(Object {
                    sha256: tig_core::Digest::parse(&a.sha256)
                        .map_err(|_| error(ErrorKind::Invalid, "resolution-job-resource-digest"))?,
                    bytes: a.bytes,
                });
            }
        }
        if objects.len() > 126
            || objects
                .iter()
                .try_fold(0u64, |n, o| n.checked_add(o.bytes))
                .is_none_or(|n| n > 512 * 1024 * 1024)
        {
            return Err(error(ErrorKind::Budget, "resolution-job-resource-closure"));
        }
        Ok(objects
            .into_iter()
            .map(|o| Artifact {
                logical_id: format!("resolution/resource/{}-{}", o.sha256.as_str(), o.bytes),
                sha256: o.sha256.as_str().into(),
                bytes: o.bytes,
                media_type: "application/octet-stream".into(),
            })
            .collect())
    }
}
pub struct Generated {
    pub outputs: ArtifactSet,
    pub files: BTreeMap<String, Vec<u8>>,
    pub checked: resolution::Proposal,
}
pub(crate) fn generate(packet: &Packet, native: &mut NativeInputs) -> Result<Generated> {
    reel_workflow::check_packet(packet)?;
    let tool = packet
        .task
        .tool
        .as_ref()
        .ok_or_else(|| error(ErrorKind::Missing, "resolution-job-tool"))?;
    if tool.capability != ToolCapability::RepairResolution
        || tool.request.logical_id != REQUEST_ID
        || tool.part.is_some()
        || !tool.auxiliary_executables.is_empty()
        || packet.task.outputs.len() != 1
        || packet.task.outputs.get("resolution.json").is_none_or(|o| {
            !o.required
                || o.media_type != "application/json"
                || o.max_bytes > reel_workflow::MAX_CONTROL_BYTES as u64
        })
    {
        return Err(error(ErrorKind::Invalid, "resolution-job-tool-contract"));
    }
    if native._claims.is_none() && native._permit.is_none() {
        return Err(error(
            ErrorKind::Denied,
            "resolution-job-live-custody-required",
        ));
    }
    let raw = native.read(&tool.request, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let request: Request = reel_workflow::decode(&raw)?;
    let plan_raw = native.read(&tool.publication, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    let plan: Plan = reel_workflow::decode(&plan_raw)?;
    if reel_workflow::canonical_digest(&plan)? != reel_workflow::canonical_digest(&request.plan)?
        || packet.project != plan.project
        || packet.task.language != request.baseline.packet.task.language
    {
        return Err(error(ErrorKind::Stale, "resolution-job-plan-context"));
    }
    let resources = request.resources()?;
    if native.source.store != request.baseline.source.source.store
        || native
            .source
            .pool
            .repository
            .canonicalize()
            .map_err(crate::io_error)?
            != request
                .baseline
                .source
                .source
                .pool
                .repository
                .canonicalize()
                .map_err(crate::io_error)?
    {
        return Err(error(ErrorKind::Conflict, "resolution-job-child-pool"));
    }
    let expected: BTreeMap<_, _> = [&tool.request, &tool.publication]
        .into_iter()
        .map(|a| (&a.logical_id, a))
        .collect();
    if expected.len() != 2
        || packet.task.inputs.len() != expected.len()
        || packet
            .task
            .inputs
            .iter()
            .any(|a| expected.get(&a.logical_id) != Some(&a))
    {
        return Err(error(
            ErrorKind::Conflict,
            "resolution-job-declared-closure",
        ));
    }
    let objects: BTreeSet<_> = resources
        .iter()
        .chain([&tool.request, &tool.publication])
        .map(|a| {
            Ok(Object {
                sha256: tig_core::Digest::parse(&a.sha256)
                    .map_err(|_| error(ErrorKind::Invalid, "resolution-job-resource-digest"))?,
                bytes: a.bytes,
            })
        })
        .collect::<Result<_>>()?;
    if native.snapshot.declared_objects != objects || !tool.source_bindings.is_empty() {
        return Err(error(
            ErrorKind::Conflict,
            "resolution-job-resource-bindings",
        ));
    }
    // Captured control bytes are owned and hash-checked. Keep durable admission
    // fences but release payload gates before independently reopening parents;
    // a control object may also occur in a parent's declared input set.
    native.held.clear();
    native.captured.clear();
    // The SDK admits every declared stream, even though only two control inputs
    // are selected. Verify their bytes sequentially without retaining duplicate
    // gates across independent parent proofs.
    use sha2::{Digest, Sha256};
    use std::io::Read;
    for asset in &resources {
        let object = Object {
            sha256: tig_core::Digest::parse(&asset.sha256)
                .map_err(|_| error(ErrorKind::Invalid, "resolution-job-resource-digest"))?,
            bytes: asset.bytes,
        };
        let mut held = tig_git::annex::workflow::reader::open(&native.source, &object)
            .map_err(crate::io_error)?;
        let mut hash = Sha256::new();
        let mut bytes = 0u64;
        let mut buffer = [0u8; 65536];
        loop {
            let n = held.read(&mut buffer).map_err(crate::io_error)?;
            if n == 0 {
                break;
            }
            bytes = bytes
                .checked_add(n as u64)
                .filter(|b| *b <= asset.bytes)
                .ok_or_else(|| error(ErrorKind::Integrity, "resolution-job-resource-bytes"))?;
            hash.update(&buffer[..n]);
        }
        let actual: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if bytes != asset.bytes || actual != asset.sha256 {
            return Err(error(
                ErrorKind::Integrity,
                "resolution-job-resource-content",
            ));
        }
    }
    let checked = resolution::propose(
        &request.baseline,
        &plan,
        &request.repair,
        &request.revision,
        &request.rebuilds,
        &request.corrected,
        &request.decision,
    )?;
    let output = serde_json::to_vec(&checked)
        .map_err(|_| error(ErrorKind::Invalid, "resolution-job-output-json"))?;
    let outputs = ArtifactSet::from([(
        "resolution.json".into(),
        Artifact {
            logical_id: format!("workflow/resolution/{}/proposal", packet.task_key),
            sha256: reel_workflow::digest(&output),
            bytes: output.len() as u64,
            media_type: "application/json".into(),
        },
    )]);
    reel_publication::check_outputs(&packet.task.outputs, &outputs)?;
    let receipt=serde_json::to_vec(&serde_json::json!({"schema":"reel.workflow-resolution-receipt.v1","task_id":packet.task.id,"task_key":packet.task_key,"request":tool.request,"outputs":outputs,"approved":false,"retained_owner_acknowledged":false})).map_err(|_|error(ErrorKind::Invalid,"resolution-job-receipt-json"))?;
    if output.len() > reel_workflow::MAX_CONTROL_BYTES
        || receipt.len() > reel_workflow::MAX_CONTROL_BYTES
    {
        return Err(error(ErrorKind::Budget, "resolution-job-output-bytes"));
    }
    Ok(Generated {
        outputs,
        files: BTreeMap::from([
            ("resolution.json".into(), output),
            ("receipt.json".into(), receipt),
        ]),
        checked,
    })
}
