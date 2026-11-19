//! Checked technical resolution proposals. Explicit review declarations are not
//! proof of viewing, tracker effects, selection, or creative/release approval.
use crate::*;
use review::{CheckedReport, Format, Modality, Unit};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub report_sha256: String,
    pub production_task: String,
    /// Every unit of the corrected edition, including added neighboring units.
    pub unit_modalities: BTreeMap<String, Vec<Modality>>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evaluation {
    pub finding_key: String,
    pub corrected_unit: String,
    pub rationale: String,
    pub evidence: Vec<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub schema: String,
    pub baseline_report_sha256: String,
    pub candidate_plan_sha256: String,
    pub contexts: BTreeMap<String, Context>,
    /// Explicit original-language correspondence; timing changes may change keys.
    pub unit_map: BTreeMap<String, String>,
    pub evaluations: Vec<Evaluation>,
}
#[derive(Debug, Serialize)]
pub struct Proposal {
    pub schema: String,
    pub decision_sha256: String,
    pub baseline_report_sha256: String,
    pub repair_sha256: String,
    pub candidate_plan_sha256: String,
    pub corrected_reports: BTreeMap<String, String>,
    pub finding_keys: Vec<String>,
    pub remaining_findings: BTreeMap<String, Vec<String>>,
    pub remote_issues_closed: bool,
    pub selected: bool,
    pub approved: bool,
}
fn same_identity(a: &Unit, b: &Unit) -> bool {
    match (a, b) {
        (
            Unit::Comic {
                page_id: a,
                panel_id: b,
                balloon_id: c,
            },
            Unit::Comic {
                page_id: x,
                panel_id: y,
                balloon_id: z,
            },
        ) => (a, b, c) == (x, y, z),
        (Unit::Shot { shot_id: a, .. }, Unit::Shot { shot_id: b, .. }) => a == b,
        (
            Unit::Film {
                cel_id: a,
                shot_id: b,
                cue_id: c,
                ..
            },
            Unit::Film {
                cel_id: x,
                shot_id: y,
                cue_id: z,
                ..
            },
        ) => (a, b, c) == (x, y, z),
        _ => false,
    }
}
fn modality_allowed(format: &Format, m: Modality) -> bool {
    match format {
        Format::Comic { .. } => !matches!(m, Modality::Audio | Modality::Audiovisual),
        Format::Film { .. } => m != Modality::Layout,
    }
}
/// Recompute the repaired candidate, bind all affected language editions and
/// require explicit full-context review declarations plus per-finding rationale.
/// No absence-of-findings inference or remote effect follows from this function.
pub fn propose(
    plan: &Plan,
    before: &CheckedReport,
    repair: &review::Repair,
    revision: &repair::Revision,
    after: &BTreeMap<String, CheckedReport>,
    decision: &Decision,
) -> Result<Proposal> {
    if after.is_empty() || after.len() > 128 {
        return fail(ErrorKind::Budget, "resolution-context-count");
    }
    let candidate = repair::materialize(plan, before, repair, revision)?;
    let decision_sha256 = canonical_digest(decision)?;
    if decision.schema != "reel.workflow-resolution-decision.v1"
        || decision.baseline_report_sha256 != before.report_sha256()
        || decision.candidate_plan_sha256 != candidate.plan_sha256
    {
        return fail(ErrorKind::Stale, "resolution-baseline-or-candidate");
    }
    let required: BTreeSet<_> = candidate
        .impact
        .contextual_languages
        .iter()
        .cloned()
        .chain(std::iter::once(before.copy().language.clone()))
        .collect();
    if after.keys().cloned().collect::<BTreeSet<_>>() != required
        || decision.contexts.keys().cloned().collect::<BTreeSet<_>>() != required
    {
        return fail(ErrorKind::Missing, "resolution-context-languages");
    }
    for (language, report) in after {
        let copy = report.copy();
        let context = &decision.contexts[language];
        if copy.project != plan.project
            || &copy.language != language
            || copy.source_commit != candidate.plan.source_commit_for(&context.production_task)?
            || context.report_sha256 != report.report_sha256()
        {
            return fail(ErrorKind::Stale, "resolution-corrected-edition");
        }
        if !candidate.impact.rebuild.contains(&context.production_task)
            || candidate
                .plan
                .tasks
                .iter()
                .find(|t| t.id == context.production_task)
                .is_none_or(|t| t.language.as_ref() != Some(language))
        {
            return fail(ErrorKind::Missing, "resolution-context-production-task");
        }
        if !matches!(
            (&before.copy().format, &copy.format),
            (Format::Comic { .. }, Format::Comic { .. })
                | (Format::Film { .. }, Format::Film { .. })
        ) {
            return fail(ErrorKind::Conflict, "resolution-publication-format");
        }
        if context.unit_modalities.keys().ne(copy.units.keys()) {
            return fail(ErrorKind::Missing, "resolution-full-context-required");
        }
        match &copy.format {
            Format::Comic{page_order}=> {
                if page_order.iter().any(|p|!copy.units.values().any(|u|matches!(u,Unit::Comic{page_id,panel_id:None,balloon_id:None} if page_id==p))) {
                    return fail(ErrorKind::Missing,"resolution-whole-page-context");
                }
            }
            Format::Film{frames}=> {
                let mut ranges:Vec<_>=copy.units.values().filter_map(|u|match u {Unit::Shot{start_frame,end_frame,..}|Unit::Film{start_frame,end_frame,..}=>Some((*start_frame,*end_frame)),_=>None}).collect();
                ranges.sort();let mut covered=0;
                for (start,end) in ranges {if start>covered {return fail(ErrorKind::Missing,"resolution-whole-film-context");} covered=covered.max(end);}
                if covered!=*frames {return fail(ErrorKind::Missing,"resolution-whole-film-context");}
            }
        }
        for modes in context.unit_modalities.values() {
            if modes.is_empty()
                || modes.len() > 5
                || modes
                    .iter()
                    .enumerate()
                    .any(|(i, m)| !modality_allowed(&copy.format, *m) || modes[..i].contains(m))
            {
                return fail(ErrorKind::Invalid, "resolution-context-modalities");
            }
        }
    }
    let corrected = &after[&before.copy().language];
    if decision.unit_map.keys().ne(before.copy().units.keys())
        || decision.unit_map.values().collect::<BTreeSet<_>>().len() != decision.unit_map.len()
    {
        return fail(ErrorKind::Conflict, "resolution-original-unit-coverage");
    }
    for (old, new) in &decision.unit_map {
        if corrected
            .copy()
            .units
            .get(new)
            .is_none_or(|u| !same_identity(&before.copy().units[old], u))
        {
            return fail(ErrorKind::Conflict, "resolution-unit-correspondence");
        }
    }
    if let (Format::Comic { page_order: old }, Format::Comic { page_order: new }) =
        (&before.copy().format, &corrected.copy().format)
    {
        let kept: Vec<_> = new.iter().filter(|p| old.contains(p)).collect();
        if kept != old.iter().collect::<Vec<_>>() {
            return fail(ErrorKind::Conflict, "resolution-context-page-order");
        }
    }
    let required_keys: BTreeSet<_> = repair.finding_keys.iter().cloned().collect();
    if decision.evaluations.len() != required_keys.len()
        || decision
            .evaluations
            .iter()
            .map(|e| e.finding_key.clone())
            .collect::<BTreeSet<_>>()
            != required_keys
    {
        return fail(ErrorKind::Missing, "resolution-explicit-evaluations");
    }
    for evaluation in &decision.evaluations {
        let finding = before.findings()[&evaluation.finding_key].finding();
        if decision.unit_map[&finding.unit] != evaluation.corrected_unit
            || evaluation.rationale.trim().is_empty()
            || evaluation.rationale.len() > 4096
            || evaluation
                .rationale
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return fail(ErrorKind::Invalid, "resolution-finding-evaluation");
        }
        let modes =
            &decision.contexts[&before.copy().language].unit_modalities[&evaluation.corrected_unit];
        if !modes.contains(&finding.modality) {
            return fail(ErrorKind::Missing, "resolution-finding-modality");
        }
        if corrected.findings().values().any(|f| {
            f.finding().code == finding.code
                && same_identity(
                    &corrected.copy().units[&f.finding().unit],
                    &corrected.copy().units[&evaluation.corrected_unit],
                )
        }) {
            return fail(ErrorKind::Conflict, "resolution-finding-still-reported");
        }
        if evaluation.evidence.is_empty()
            || evaluation.evidence.len() > 16
            || !evaluation.evidence.contains(&corrected.copy().review)
            || evaluation
                .evidence
                .iter()
                .any(|a| a != &corrected.copy().review && a != &corrected.copy().publication)
            || evaluation
                .evidence
                .iter()
                .enumerate()
                .any(|(i, a)| evaluation.evidence[..i].contains(a))
        {
            return fail(ErrorKind::Denied, "resolution-corrected-review-evidence");
        }
    }
    let proposal = Proposal {
        schema: "reel.workflow-resolution-proposal.v1".into(),
        decision_sha256,
        baseline_report_sha256: before.report_sha256().into(),
        repair_sha256: candidate.impact.repair_sha256,
        candidate_plan_sha256: candidate.plan_sha256,
        corrected_reports: after
            .iter()
            .map(|(l, r)| (l.clone(), r.report_sha256().into()))
            .collect(),
        finding_keys: required_keys.into_iter().collect(),
        remaining_findings: after
            .iter()
            .map(|(l, r)| (l.clone(), r.findings().keys().cloned().collect()))
            .collect(),
        remote_issues_closed: false,
        selected: false,
        approved: false,
    };
    canonical_digest(&proposal)?;
    Ok(proposal)
}
