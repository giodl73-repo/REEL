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
pub struct ProductionEvidence {
    pub task_id: String,
    pub task_key: String,
    pub result_sha256: String,
    pub acknowledgement_sha256: String,
    pub copy_sha256: String,
}
#[derive(Serialize)]
pub struct Proposal {
    pub schema: String,
    pub proposal: resolution::Proposal,
    pub baseline: Evidence,
    pub corrected: BTreeMap<String, Evidence>,
    pub rebuild_task_keys: BTreeMap<String, String>,
    pub rebuild_bindings_sha256: String,
    pub productions: BTreeMap<String, ProductionEvidence>,
}
fn check_production_copy(
    expected: &reel_workflow::review::Copy,
    actual: &reel_workflow::review::Copy,
) -> Result<()> {
    if expected != actual {
        return Err(error(ErrorKind::Stale, "resolution-production-review-copy"));
    }
    Ok(())
}
fn production_copy(
    plan: &reel_workflow::Plan,
    bindings: &crate::graph::Bindings,
    id: &str,
    task_key: &str,
    report: &reel_workflow::review::CheckedReport,
) -> Result<ProductionEvidence> {
    let binding = bindings
        .tasks
        .get(id)
        .ok_or_else(|| error(ErrorKind::Missing, "resolution-production-binding"))?;
    let (raw, _profile) = binding.profile.read()?;
    let profile: crate::handoff::Profile = reel_workflow::decode(&raw)?;
    let job = crate::handoff::observe(&binding.journal, &profile)?;
    if job.state != reel_workflow::handoff::State::Retained
        || job.ticket.packet.task_key != task_key
        || !plan.tasks.iter().any(|t| t == &job.ticket.packet.task)
        || job.ticket.packet.source_commit != plan.source_commit
        || job.ticket.packet.project != plan.project
    {
        return Err(error(ErrorKind::Stale, "resolution-production-observation"));
    }
    let result = job
        .result
        .ok_or_else(|| error(ErrorKind::Missing, "resolution-production-result"))?;
    let ack: retention::Acknowledgement =
        crate::read_control(&profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            task_key, profile.binding.admission.attempt_id
        )))?;
    if job.acknowledgement.as_ref() != Some(&ack.artifact()?) {
        return Err(error(
            ErrorKind::Integrity,
            "resolution-production-acknowledgement",
        ));
    }
    let generated = retention::review_copy(
        &job.ticket.packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &result,
        &ack,
    )?;
    check_production_copy(generated.copy(), report.copy())?;
    Ok(ProductionEvidence {
        task_id: id.into(),
        task_key: task_key.into(),
        result_sha256: reel_workflow::canonical_digest(&result)?,
        acknowledgement_sha256: reel_workflow::canonical_digest(&ack)?,
        copy_sha256: reel_workflow::canonical_digest(generated.copy())?,
    })
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
    let mut productions = BTreeMap::new();
    for (language, review) in after {
        let (report, pin) = review.checked()?;
        let context = decision
            .contexts
            .get(language)
            .ok_or_else(|| error(ErrorKind::Missing, "resolution-context-languages"))?;
        let task_key = rebuild_task_keys
            .get(&context.production_task)
            .ok_or_else(|| error(ErrorKind::Missing, "resolution-context-production-task"))?;
        productions.insert(
            language.clone(),
            production_copy(
                &candidate.plan,
                rebuilds,
                &context.production_task,
                task_key,
                &report,
            )?,
        );
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
        productions,
    };
    reel_workflow::canonical_digest(&result)?;
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn producer_copy_requires_exact_media_and_context_even_at_the_same_commit() {
        use reel_workflow::{
            Artifact,
            review::{Copy, Format, Unit},
        };
        let artifact = Artifact {
            logical_id: "review".into(),
            sha256: "a".repeat(64),
            bytes: 12,
            media_type: "application/json".into(),
        };
        let copy = Copy {
            schema: "reel.workflow-review-copy.v1".into(),
            project: "synthetic".into(),
            source_commit: "a".repeat(40),
            language: "es".into(),
            publication: artifact.clone(),
            review: artifact,
            format: Format::Comic {
                page_order: vec!["page".into()],
            },
            units: BTreeMap::from([(
                "page".into(),
                Unit::Comic {
                    page_id: "page".into(),
                    panel_id: None,
                    balloon_id: None,
                },
            )]),
        };
        check_production_copy(&copy, &copy).unwrap();
        let mut other = copy.clone();
        other.review.sha256 = "b".repeat(64);
        assert_eq!(
            check_production_copy(&copy, &other).unwrap_err().code,
            "resolution-production-review-copy"
        );
        let mut other = copy.clone();
        other.publication.logical_id = "other-publication".into();
        assert!(check_production_copy(&copy, &other).is_err());
        let mut other = copy.clone();
        other.units.insert(
            "extra".into(),
            Unit::Comic {
                page_id: "page".into(),
                panel_id: Some("panel".into()),
                balloon_id: None,
            },
        );
        assert!(check_production_copy(&copy, &other).is_err());
    }
}
