//! Historical retained review proof -> private resolution proposal. No tracker
//! effect or native task completion; owned checked reports grant no live custody.
use crate::{NativeProfile, admission, error, retention};
use reel_workflow::{ErrorKind, Packet, Result, ReturnedResult, resolution};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedReview {
    pub packet: Packet,
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub receipt: admission::Receipt,
    pub result: ReturnedResult,
    pub acknowledgement: retention::Acknowledgement,
}
#[derive(Serialize)]
pub struct Evidence {
    pub task_key: String,
    pub attempt_id: String,
    pub result_sha256: String,
    pub acknowledgement_sha256: String,
}
#[derive(Serialize)]
pub struct Proposal {
    pub schema: String,
    pub proposal: resolution::Proposal,
    pub baseline: Evidence,
    pub corrected: BTreeMap<String, Evidence>,
    pub rebuild_task_keys: BTreeMap<String, String>,
    pub rebuild_bindings_sha256: String,
}
impl RetainedReview {
    fn checked(&self) -> Result<(reel_workflow::review::CheckedReport, Evidence)> {
        let guard = retention::verify(
            &self.packet,
            &self.source,
            &self.admission,
            &self.receipt,
            &self.result,
            &self.acknowledgement,
        )?;
        let report = guard
            .review_report()
            .ok_or_else(|| {
                error(
                    ErrorKind::Unsupported,
                    "resolution-retained-review-required",
                )
            })?
            .clone();
        let evidence = Evidence {
            task_key: self.packet.task_key.clone(),
            attempt_id: self.receipt.attempt_id.clone(),
            result_sha256: reel_workflow::canonical_digest(&self.result)?,
            acknowledgement_sha256: reel_workflow::canonical_digest(&self.acknowledgement)?,
        };
        Ok((report, evidence))
    }
}
/// Keep proposal writes outside declared native roots and rebuild controls. Keep returned
/// profile-file guards through the caller's exclusive private output write.
pub fn check_destination(
    parent: &std::path::Path,
    before: &RetainedReview,
    after: &BTreeMap<String, RetainedReview>,
    rebuilds: &crate::graph::Bindings,
) -> Result<Vec<std::fs::File>> {
    let canonical = parent.canonicalize().map_err(crate::io_error)?;
    for review in std::iter::once(before).chain(after.values()) {
        crate::staging::check_parent(parent, &review.source, &review.admission)?;
        if canonical.starts_with(
            review
                .source
                .snapshot
                .repository
                .canonicalize()
                .map_err(crate::io_error)?,
        ) {
            return Err(error(
                ErrorKind::Denied,
                "resolution-output-source-repository",
            ));
        }
    }
    let mut held = Vec::new();
    for binding in rebuilds.tasks.values() {
        let (raw, file) = binding.profile.read()?;
        let profile: crate::handoff::Profile = reel_workflow::decode(&raw)?;
        crate::staging::check_parent(parent, &profile.source, &profile.admission)?;
        for root in [
            &binding.journal,
            &profile.acknowledgements,
            &profile.source.snapshot.repository,
        ] {
            if canonical.starts_with(root.canonicalize().map_err(crate::io_error)?) {
                return Err(error(ErrorKind::Denied, "resolution-output-job-control"));
            }
        }
        held.push(file);
    }
    Ok(held)
}
pub fn propose(
    before: &RetainedReview,
    plan: &reel_workflow::Plan,
    repair: &reel_workflow::review::Repair,
    revision: &reel_workflow::repair::Revision,
    rebuilds: &crate::graph::Bindings,
    after: &BTreeMap<String, RetainedReview>,
    decision: &resolution::Decision,
) -> Result<Proposal> {
    if after.is_empty() || after.len() > 128 {
        return Err(error(ErrorKind::Budget, "resolution-retained-report-count"));
    }
    // Each complete proof is dropped before opening another; two reviews may
    // share retained actors or native object gates. The result is a proposal,
    // not an atomic selection/closure transaction or reusable live authority.
    let (baseline, baseline_evidence) = before.checked()?;
    let candidate = reel_workflow::repair::materialize(plan, &baseline, repair, revision)?;
    let frontier = crate::graph::inspect(&candidate.plan, rebuilds)?;
    let mut rebuild_task_keys = BTreeMap::new();
    for id in &candidate.impact.rebuild {
        let row = frontier
            .rows
            .iter()
            .find(|r| &r.task_id == id)
            .ok_or_else(|| error(ErrorKind::Missing, "resolution-rebuild-observation"))?;
        if row.status != reel_workflow::graph::Status::RetainedObserved {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "resolution-rebuild-not-retained",
            ));
        }
        rebuild_task_keys.insert(
            id.clone(),
            row.task_key
                .clone()
                .ok_or_else(|| error(ErrorKind::Integrity, "resolution-rebuild-task-key"))?,
        );
    }
    let mut reports = BTreeMap::new();
    let mut evidence = BTreeMap::new();
    for (language, review) in after {
        let (report, pin) = review.checked()?;
        reports.insert(language.clone(), report);
        evidence.insert(language.clone(), pin);
    }
    let proposal = resolution::propose(plan, &baseline, repair, revision, &reports, decision)?;
    let result = Proposal {
        schema: "reel.workflow-retained-resolution-proposal.v1".into(),
        proposal,
        baseline: baseline_evidence,
        corrected: evidence,
        rebuild_task_keys,
        rebuild_bindings_sha256: reel_workflow::canonical_digest(rebuilds)?,
    };
    reel_workflow::canonical_digest(&result)?;
    Ok(result)
}
