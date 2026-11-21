//! Final bulk observations are never decoded as placement receipts.
use super::*;
use git_asset_store::final_retirement::bulk;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    object: Object,
    before_copies: Vec<String>,
    verified_copies: Vec<String>,
    unknown_copies: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Final {
    schema: String,
    lesson: String,
    source_commit: String,
    source_checkpoint: git_asset_store::source_claims::Checkpoint,
    plan: Value,
    receipt: Receipt,
    providers: Vec<Digest>,
    observations: BTreeMap<String, Row>,
    synthetic_owner_consent: bool,
    simulated_clock: bool,
    independent_hardware_backup_proven: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    plan: Digest,
    phase: String,
    intent_acknowledged: bool,
    selected_roots: usize,
    selected_bindings: usize,
    unique_objects: usize,
    selected_unique_bytes: u64,
    eligible_unique_bytes: u64,
    planned_copy_bytes: u64,
    new_logical_copy_bytes_removed: u64,
    historical_deleted_copy_bytes: u64,
    file_allocation_before_new_removals: u64,
    required_witness_metadata_peak: u64,
    excluded: Vec<bulk::Subject>,
    source_status: Vec<Option<SourceStatus>>,
    action_status: Vec<Option<ActionStatus>>,
    effects: Vec<Effect>,
    holds: Vec<Value>,
    removal_authority: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionStatus {
    schema: String,
    plan: Digest,
    phase: String,
    observed: String,
    grants_released: bool,
    charged_metadata: Option<u64>,
    removal_authority: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceStatus {
    schema: String,
    plan: Digest,
    historical_retirement: Option<git_asset_store::source_claims::Receipt>,
    historical_survivor_withdrawal: Option<git_asset_store::source_claims::Receipt>,
    survivor_protection_current: bool,
    original_predecessor_present: bool,
    settlement_current: bool,
    authorizes_payload_deletion: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Effect {
    action: usize,
    receipt: ActionReceipt,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActionReceipt {
    schema: String,
    plan: Digest,
    outcome: Option<String>,
    grants_released: bool,
    historical: bool,
    observed: String,
    file_allocation_before: u64,
    charged_metadata: u64,
    logical_bytes_removed: u64,
}
fn sum(mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0u64, |a, b| {
        a.checked_add(b).ok_or("Final byte overflow".into())
    })
}

pub(super) fn decode(
    value: Value,
    entry: &Entry,
    expected: &[Digest],
    roles: &[Role],
    commits: &BTreeSet<String>,
    identities: &BTreeMap<String, Object>,
    previous: &Value,
) -> Result<Value> {
    let f: Final = serde_json::from_value(value).map_err(|_| "Unsupported final observation")?;
    let plan = bulk::Plan::from_json(
        &identity::canonical_json(&f.plan).map_err(|_| "Invalid final plan")?,
    )
    .map_err(|_| "Invalid final plan")?;
    let id = plan.id().map_err(|_| "Invalid final plan")?;
    let r = &f.receipt;
    let actions = plan.actions().collect::<Vec<_>>();
    let subjects = plan.subjects();
    let excluded = subjects
        .iter()
        .filter(|s| s.hold.is_some())
        .cloned()
        .collect::<Vec<_>>();
    if f.schema != "tig.teaching-final-observation.design-1"
        || f.lesson != entry.lesson
        || f.providers != expected
        || !f.synthetic_owner_consent
        || !f.simulated_clock
        || f.independent_hardware_backup_proven
        || !plan
            .enrollment()
            .sources
            .iter()
            .any(|s| &s.namespace == f.source_checkpoint.identity())
        || f.source_commit.len() != 40
        || !f.source_commit.bytes().all(|b| b.is_ascii_hexdigit())
        || r.schema != "tig.bulk-final-status.design-1"
        || r.plan != id
        || !matches!(r.phase.as_str(), "completed" | "cancelled")
        || !r.intent_acknowledged
        || !r.holds.is_empty()
        || r.removal_authority
        || r.selected_roots != plan.request().selections.len()
        || r.selected_bindings != plan.bindings().len()
        || r.unique_objects != subjects.len()
        || r.selected_unique_bytes != sum(subjects.iter().map(|s| s.object.bytes))?
        || r.eligible_unique_bytes
            != sum(subjects
                .iter()
                .filter(|s| s.hold.is_none())
                .map(|s| s.object.bytes))?
        || r.planned_copy_bytes
            != sum(actions
                .iter()
                .map(|(_, p)| p.request().publication.object.bytes))?
        || r.excluded != excluded
        || r.action_status.len() != actions.len()
        || r.effects.len() > actions.len()
        || f.observations.is_empty()
        || f.observations.len() > 32
        || r.required_witness_metadata_peak == 0
    {
        return Err("Inconsistent final observation".into());
    }
    let settlements = f.plan["settlements"]
        .as_array()
        .ok_or("Missing final settlements")?;
    if r.source_status.len() != settlements.len() {
        return Err("Incomplete final source status".into());
    }
    for (status, settlement) in r.source_status.iter().zip(settlements) {
        let status = status.as_ref().ok_or("Unknown final source status")?;
        let sp = git_asset_store::source_claims::subset::Plan::from_json(
            &identity::canonical_json(&settlement["plan"]).map_err(|_| "Invalid subset plan")?,
        )
        .map_err(|_| "Invalid subset plan")?;
        let survivor = sp
            .survivor_source()
            .map_err(|_| "Invalid final survivor")?
            .is_some();
        if status.schema != "tig.subset-status.design-1"
            || status.plan != sp.id().map_err(|_| "Invalid subset plan")?
            || !status.settlement_current
            || status.original_predecessor_present
            || status.authorizes_payload_deletion
            || status.historical_retirement.is_none()
            || status.survivor_protection_current != survivor
            || status.historical_survivor_withdrawal.is_some() != survivor
        {
            return Err("Unsettled final source".into());
        }
    }
    let mut deleted = 0u64;
    for ((store, p), status) in actions.iter().zip(&r.action_status) {
        let s = status.as_ref().ok_or("Unknown final action status")?;
        if !expected.contains(store)
            || s.schema != "tig.final-retirement-status.design-1"
            || s.plan != p.id().map_err(|_| "Invalid action")?
            || s.removal_authority
            || s.charged_metadata.is_none()
            || !matches!(s.phase.as_str(), "deleted" | "cancelled")
            || !matches!(s.observed.as_str(), "absent" | "present_unverified")
            || (s.phase == "deleted" && !s.grants_released)
        {
            return Err("Invalid final action status".into());
        }
        if s.phase == "deleted" {
            deleted = deleted
                .checked_add(p.request().publication.object.bytes)
                .ok_or("Final bytes overflow")?;
        }
    }
    if deleted != r.historical_deleted_copy_bytes
        || (r.phase == "completed"
            && r.action_status
                .iter()
                .any(|s| s.as_ref().is_none_or(|s| s.phase != "deleted")))
    {
        return Err("False final completion".into());
    }
    let mut effects = Vec::new();
    let mut seen = BTreeSet::new();
    let mut new_bytes = 0u64;
    let mut allocation = 0u64;
    for e in &r.effects {
        let (store, p) = actions.get(e.action).ok_or("Unknown final effect action")?;
        let a = &e.receipt;
        if !seen.insert(e.action)
            || a.schema != "tig.final-retirement-receipt.design-1"
            || a.plan != p.id().map_err(|_| "Invalid final effect plan")?
            || (a.outcome.as_deref() == Some("deleted") && !a.grants_released)
            || !matches!(a.observed.as_str(), "absent" | "present_unverified")
            || !matches!(a.outcome.as_deref(), Some("deleted" | "cancelled"))
            || a.charged_metadata == 0
        {
            return Err("Invalid final effect".into());
        }
        let object = &p.request().publication.object;
        let removed = !a.historical && a.outcome.as_deref() == Some("deleted");
        if a.logical_bytes_removed
            != if a.outcome.as_deref() == Some("deleted") {
                object.bytes
            } else {
                0
            }
        {
            return Err("Invalid final effect bytes".into());
        }
        if removed {
            new_bytes = new_bytes
                .checked_add(object.bytes)
                .ok_or("Final bytes overflow")?;
            allocation = allocation
                .checked_add(a.file_allocation_before)
                .ok_or("Final allocation overflow")?;
        }
        let column = expected
            .iter()
            .position(|id| id == *store)
            .ok_or("Unknown final provider")?;
        effects.push(json!({"kind":if removed {"final_deleted"}else{"final_historical_or_cancelled"},"role":roles[column].id,"object":object,"logical_bytes":a.logical_bytes_removed}));
    }
    if new_bytes != r.new_logical_copy_bytes_removed
        || allocation != r.file_allocation_before_new_removals
    {
        return Err("Final effect totals differ".into());
    }
    let mut rows = Vec::new();
    for (name, o) in f.observations {
        let preceding = previous["objects"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["name"] == name))
            .ok_or("Missing preceding final object observation")?;
        if preceding["object"] != json!(o.object) || preceding["observed"] != json!(o.before_copies)
        {
            return Err("Final before state differs from preceding lesson".into());
        }
        if !token(&name) || identities.get(&name) != Some(&o.object) || !o.unknown_copies.is_empty()
        {
            return Err("Unknown final object coverage".into());
        }
        for set in [&o.before_copies, &o.verified_copies] {
            if set.windows(2).any(|p| p[0] >= p[1])
                || set.iter().any(|id| !roles.iter().any(|r| &r.id == id))
            {
                return Err("Invalid final roles".into());
            }
        }
        for ((store, p), status) in actions.iter().zip(&r.action_status) {
            if p.request().publication.object == o.object {
                let column = expected
                    .iter()
                    .position(|id| id == *store)
                    .ok_or("Unknown final provider")?;
                let role = &roles[column].id;
                let s = status.as_ref().ok_or("Unknown final status")?;
                if !o.before_copies.contains(role)
                    || o.verified_copies.contains(role) != (s.observed == "present_unverified")
                {
                    return Err("Final copies conflict with status".into());
                }
            }
        }
        for role in roles {
            if o.before_copies.contains(&role.id) != o.verified_copies.contains(&role.id) {
                let column = roles
                    .iter()
                    .position(|r| r.id == role.id)
                    .ok_or("Missing final role")?;
                if !actions.iter().any(|(store, p)| {
                    *store == &expected[column] && p.request().publication.object == o.object
                }) {
                    return Err("Unplanned final copy change".into());
                }
            }
        }
        let subject = subjects
            .iter()
            .find(|s| s.object == o.object)
            .ok_or("Unknown final subject")?;
        rows.push(json!({"name":name,"object":o.object,"before":o.before_copies,"desired":null,"observed":o.verified_copies,"unknown":[],
            "final_selection":if subject.hold.is_none(){"eligible"}else{"protected"}}));
    }
    if subjects
        .iter()
        .any(|s| !rows.iter().any(|r| r["object"] == json!(s.object)))
    {
        return Err("Incomplete final scope".into());
    }
    Ok(
        json!({"id":entry.lesson,"title":entry.title,"event":entry.event,"report":entry.report,"plan":id,
        "source_commit":f.source_commit,"source_available":commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),
        "objects":rows,"stream_bytes":0,"basis_current":true,"converged":r.phase=="completed","storage_converged":false,"remaining_page":false,"holds":[],"effects":effects,
        "final_loss":{"phase":r.phase,"new_removed_bytes":new_bytes,"historical_removed_bytes":deleted,"file_allocation_before_new_removals":allocation,"synthetic_owner_consent":true,"simulated_clock":true}}),
    )
}
