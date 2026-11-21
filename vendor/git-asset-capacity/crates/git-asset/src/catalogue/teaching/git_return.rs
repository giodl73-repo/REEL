//! Recorded known absence after a real Git/source return. No provider IO.
use super::*;
use git_asset_store::source_claims::{Checkpoint, Kind, Source};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    first_commit: String,
    prior_commit: String,
    returned_commit: String,
    record_path: String,
    first_record: Object,
    returned_record: Object,
    source_before: Checkpoint,
    source_selected: Checkpoint,
    selected_source: Source,
    returned_snapshot: Digest,
    seed_disabled: bool,
    seed_attempts_before: u64,
    seed_attempts_after: u64,
    lost_object: Object,
    before: BTreeMap<String, BTreeMap<String, Object>>,
    after: BTreeMap<String, BTreeMap<String, Object>>,
    authorizes_storage: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReturnReport {
    schema: String,
    lesson: String,
    source_commit: String,
    source_checkpoint: Checkpoint,
    plan: placement::Plan,
    receipt: Receipt,
    status: Receipt,
    observations: BTreeMap<String, Observation>,
    declared_fixture_domains: usize,
    physical_domain_coverage: String,
    independent_hardware_backup_proven: bool,
    git_return: Evidence,
}
pub(super) fn decode(
    value: Value,
    entry: &Entry,
    previous: &Value,
    binding: &(Digest, Vec<Digest>),
    roles: &[Role],
    identities: &BTreeMap<String, Object>,
    commits: &BTreeSet<String>,
) -> Result<Value> {
    let f: ReturnReport =
        serde_json::from_value(value).map_err(|_| "Unsupported Git return observation")?;
    let (enrollment, expected) = binding;
    let id = f.plan.id().map_err(|_| "Invalid returned placement plan")?;
    let e = &f.git_return;
    let r = &f.receipt;
    let s = &f.status;
    let streamed = r
        .effects
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "published" | "reacquired"))
        .try_fold(0u64, |n, e| {
            n.checked_add(e.logical_bytes)
                .ok_or("Return stream overflow")
        })?;
    if f.schema != "tig.teaching-git-return-observation.design-1"
        || f.lesson != entry.lesson
        || r.stream_bytes != streamed
        || r.stream_bytes > f.plan.budget.max_stream_bytes
        || previous["final_loss"]["phase"] != "completed"
        || previous["source_commit"] != e.prior_commit
        || previous["source_sequence"] != json!(e.source_before.sequence())
        || f.source_commit != e.returned_commit
        || f.source_commit == e.prior_commit
        || f.source_commit == e.first_commit
        || [&e.first_commit, &e.prior_commit, &e.returned_commit]
            .iter()
            .any(|c| GitId::parse(c).is_err())
        || e.first_record != e.returned_record
        || e.first_record.bytes == 0
        || e.first_record.bytes > 65536
        || e.record_path != "records/series.asset"
        || !e.seed_disabled
        || e.authorizes_storage
        || e.seed_attempts_after < e.seed_attempts_before
        || e.source_before.identity() != e.source_selected.identity()
        || e.source_selected.sequence() <= e.source_before.sequence()
        || f.source_checkpoint != e.source_selected
        || e.selected_source.kind != Some(Kind::Branch)
        || e.selected_source.revision == 0
        || e.selected_source.pending.is_some()
        || e.selected_source.current.as_ref().is_none_or(|c| {
            c.revision != e.selected_source.revision || c.selection.context != e.returned_snapshot
        })
        || &f.plan.enrollment != enrollment
        || f.plan.providers.as_slice() != expected.as_slice()
        || f.plan.next.is_some()
        || f.plan.after.is_some()
        || f.declared_fixture_domains != roles.len()
        || f.independent_hardware_backup_proven
        || f.physical_domain_coverage.is_empty()
        || f.physical_domain_coverage.len() > 256
        || f.observations.len() != identities.len()
        || f.plan.rows.len() != identities.len()
        || f.plan
            .rows
            .iter()
            .map(|r| &r.object.sha256)
            .collect::<BTreeSet<_>>()
            .len()
            != f.plan.rows.len()
        || [&e.before, &e.after].iter().any(|snapshot| {
            snapshot.len() != roles.len()
                || snapshot.keys().any(|id| !roles.iter().any(|r| &r.id == id))
                || snapshot.values().any(|rows| {
                    rows.iter()
                        .any(|(name, object)| identities.get(name) != Some(object))
                })
        })
    {
        return Err("Inconsistent Git return evidence".into());
    }
    for (report, historical) in [(r, false), (s, true)] {
        if report.schema != "tig.placement-report.design-1"
            || report.plan != id
            || report.observed_basis != f.plan.basis
            || !report.basis_current
            || report.page_converged
            || report.storage_converged != Some(false)
            || report.next.is_some()
            || !report.holds.iter().any(|h| h == "final_retirement")
            || report.holds.iter().any(|h| {
                !matches!(
                    h.as_str(),
                    "invalid"
                        | "wrong_enrollment"
                        | "stale"
                        | "budget"
                        | "disabled"
                        | "busy"
                        | "unavailable"
                        | "integrity"
                        | "recovery"
                        | "protected"
                        | "retained"
                        | "missing_source"
                        | "reacquisition_required"
                        | "final_retirement"
                )
            })
            || report.holds.len() > 64
            || report.effects.len() > 128
            || report.observations.len() != f.plan.rows.len() * roles.len()
            || (historical && (report.stream_bytes != 0 || !report.effects.is_empty()))
        {
            return Err("Inconsistent held return report".into());
        }
        let mut seen = BTreeSet::new();
        for raw in &report.observations {
            let o: RecordedObserved =
                serde_json::from_value(raw.clone()).map_err(|_| "Invalid return state")?;
            let col = expected
                .iter()
                .position(|p| p == &o.provider)
                .ok_or("Unknown return provider")?;
            let row = f
                .plan
                .rows
                .iter()
                .find(|r| r.object == o.object)
                .ok_or("Unknown return object")?;
            if row.required.len() != roles.len()
                || row.required[col] != o.required
                || !seen.insert((o.object.sha256.clone(), col))
                || !(o.state.as_str().is_some_and(|v| {
                    matches!(
                        v,
                        "not_started"
                            | "untracked_present"
                            | "partial"
                            | "published"
                            | "retained"
                            | "released"
                            | "retirement_pending"
                            | "authorized_absence"
                            | "cancelled"
                    )
                }) || o.state.as_object().is_some_and(|v| {
                    v.len() == 1
                        && v.get("held")
                            .and_then(Value::as_str)
                            .is_some_and(|h| report.holds.iter().any(|v| v == h))
                }))
            {
                return Err("Inconsistent return state".into());
            }
        }
    }
    let mut rows = Vec::new();
    let mut lost = 0;
    for (name, o) in &f.observations {
        if identities.get(name) != Some(&o.object)
            || o.unknown_copies.as_ref().is_none_or(|v| !v.is_empty())
            || o.verified_copies.windows(2).any(|p| p[0] >= p[1])
        {
            return Err("Invalid return copy identity".into());
        }
        let copies = |snapshot: &BTreeMap<String, BTreeMap<String, Object>>| {
            roles
                .iter()
                .filter(|r| snapshot[&r.id].get(name) == Some(&o.object))
                .map(|r| r.id.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        };
        let before = copies(&e.before);
        let after = copies(&e.after);
        let prior = previous["objects"]
            .as_array()
            .and_then(|r| r.iter().find(|r| r["name"] == *name))
            .ok_or("Missing pre-return object")?;
        if prior["object"] != json!(o.object)
            || prior["observed"] != json!(before)
            || after != o.verified_copies
        {
            return Err("Return physical history mismatch".into());
        }
        let plan = f
            .plan
            .rows
            .iter()
            .find(|r| r.object == o.object)
            .ok_or("Missing return requirement")?;
        if plan.required.len() != roles.len() {
            return Err("Invalid return requirements".into());
        }
        let desired = roles
            .iter()
            .zip(&plan.required)
            .filter(|(_, yes)| **yes)
            .map(|(r, _)| r.id.clone())
            .collect::<Vec<_>>();
        if o.object == e.lost_object {
            lost += 1;
            if !before.is_empty()
                || !after.is_empty()
                || !["working", "backup"]
                    .iter()
                    .all(|r| desired.iter().any(|v| v == r))
            {
                return Err("Lost payload falsely restored".into());
            }
        } else if before.iter().any(|r| r == "backup") && !after.iter().any(|r| r == "backup") {
            return Err("Return lost retained sibling".into());
        }
        rows.push(json!({"name":name,"object":o.object,"before":before,"desired":desired,"observed":after,"unknown":[]}));
    }
    if lost != 1 {
        return Err("Missing lost return object".into());
    }
    let mut effects = Vec::new();
    for effect in &r.effects {
        let role = expected
            .iter()
            .position(|p| p == &effect.provider)
            .ok_or("Unknown return effect provider")?;
        if !identities.values().any(|o| o == &effect.object)
            || effect.object == e.lost_object
            || (matches!(effect.kind.as_str(), "published" | "reacquired")
                && effect.logical_bytes != effect.object.bytes)
            || !matches!(
                effect.kind.as_str(),
                "published"
                    | "retained"
                    | "handoff"
                    | "retired"
                    | "reacquired"
                    | "cancelled"
                    | "recovered"
            )
        {
            return Err("Unrecognized return effect".into());
        }
        effects.push(json!({"object":effect.object,"role":roles[role].id,"kind":effect.kind,"logical_bytes":effect.logical_bytes,"file_allocation_before":effect.file_allocation_before}));
    }
    Ok(
        json!({"id":entry.lesson,"title":entry.title,"event":entry.event,"report":entry.report,"plan":id,"source_commit":f.source_commit,"source_available":commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),"objects":rows,"stream_bytes":r.stream_bytes,"basis_current":true,"converged":false,"storage_converged":false,"remaining_page":false,"holds":r.holds,"effects":effects}),
    )
}
