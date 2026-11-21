//! Recorded paged control outcomes; no execution or provider roots are exposed.
use super::*;
use git_asset_core::tree::{NodeStore, Tree, TreeError, TreeLimits, TreeResult};
use git_asset_store::final_retirement::paged::{self as selection, control};
use std::cell::RefCell;

struct Nodes<'a>(RefCell<(&'a mut ReadCache, &'a mut u64)>);
impl NodeStore for Nodes<'_> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let mut ports = self.0.borrow_mut();
        let (reader, left) = &mut *ports;
        raw_document(reader, object, left)
            .map(Some)
            .map_err(|_| TreeError::InvalidNode)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::InvalidNode)
    }
}
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
struct Bounds {
    earliest: u64,
    latest: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: String,
    lesson: String,
    source_commit: String,
    source_checkpoint: git_asset_store::source_claims::Checkpoint,
    enrollment: control::Enrollment,
    decision: Value,
    receipt: Receipt,
    providers: Vec<Digest>,
    observations: BTreeMap<String, Row>,
    synthetic_claim_consent: bool,
    synthetic_final_consent: bool,
    simulated_clock: bool,
    execution_time: Bounds,
    independent_hardware_backup_proven: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    decision: Digest,
    phase: String,
    completed_pages: Option<usize>,
    cancelled_pages: Option<usize>,
    historical_retired_pages: Option<usize>,
    total_pages: usize,
    historical_deleted_actions: Option<usize>,
    pending_cancellations: Option<usize>,
    total_actions: usize,
    excluded_objects: Option<usize>,
    metadata_available: bool,
    cleanup_unknown: bool,
    age_inheritance_unknown: bool,
    unreported_holds: usize,
    holds: Vec<Value>,
    effects: Vec<Effect>,
    authorizes_removal: bool,
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
pub(super) struct Context<'a> {
    pub previous: &'a Value,
    pub entry: &'a Entry,
    pub expected: &'a [Digest],
    pub roles: &'a [Role],
    pub commits: &'a BTreeSet<String>,
    pub identities: &'a BTreeMap<String, Object>,
    pub reader: &'a mut ReadCache,
    pub left: &'a mut u64,
}
pub(super) fn decode(value: Value, ctx: Context<'_>) -> Result<Value> {
    let f: Observation =
        serde_json::from_value(value).map_err(|_| "Unsupported paged final observation")?;
    for (key, maximum) in [
        ("pages", 16),
        ("bindings", 1024),
        ("objects", 32),
        ("action_count", 32),
    ] {
        if f.decision[key].as_u64().is_none_or(|n| n > maximum) {
            return Err("Paged teaching scope exceeds budget".into());
        }
    }
    let decision = control::Decision::from_json(
        &identity::canonical_json(&f.decision).map_err(|_| "Invalid paged decision")?,
    )
    .map_err(|_| "Invalid paged decision")?;
    let mut nodes = Nodes(RefCell::new((ctx.reader, ctx.left)));
    let inventory = control::inspect(
        &f.enrollment,
        &decision,
        &mut nodes,
        &selection::Limits::default(),
    )
    .map_err(|_| "Unavailable or inconsistent paged scope")?;
    let subjects = inventory.subjects().collect::<Vec<_>>();
    let actions = inventory.actions().collect::<Vec<_>>();
    let r = &f.receipt;
    let pages = f.decision["pages"].as_u64().ok_or("Missing page count")? as usize;
    if f.schema != "tig.teaching-paged-final-observation.design-1"
        || f.lesson != ctx.entry.lesson
        || f.providers != ctx.expected
        || !f.synthetic_claim_consent
        || !f.synthetic_final_consent
        || !f.simulated_clock
        || f.independent_hardware_backup_proven
        || f.source_commit.len() != 40
        || !f.source_commit.bytes().all(|b| b.is_ascii_hexdigit())
        || !f
            .enrollment
            .sources
            .iter()
            .any(|s| &s.namespace == f.source_checkpoint.identity())
        || r.schema != "tig.paged-control-status.design-1"
        || &r.decision != inventory.decision()
        || r.phase != "completed"
        || r.completed_pages != Some(pages)
        || r.cancelled_pages != Some(0)
        || r.historical_retired_pages != Some(pages)
        || r.total_pages != pages
        || r.total_actions != actions.len()
        || r.historical_deleted_actions != Some(actions.len())
        || r.pending_cancellations != Some(0)
        || r.excluded_objects != Some(subjects.iter().filter(|s| s.hold.is_some()).count())
        || !r.metadata_available
        || r.cleanup_unknown
        || r.age_inheritance_unknown
        || r.unreported_holds != 0
        || !r.holds.is_empty()
        || r.authorizes_removal
        || r.effects.len() != actions.len()
        || f.observations.len() != ctx.identities.len()
        || f.observations.len() > 32
    {
        return Err("Inconsistent paged final observation".into());
    }
    let required = f
        .enrollment
        .age
        .as_ref()
        .ok_or("Missing retained-age policy")?
        .required_age_millis;
    if f.execution_time.earliest == 0
        || f.execution_time.latest < f.execution_time.earliest
        || f.execution_time.earliest < decision.request().window.not_before
        || f.execution_time.latest >= decision.request().window.expires
    {
        return Err("Invalid simulated execution bounds".into());
    }
    let age_root: Object = serde_json::from_value(f.decision["age"].clone())
        .map_err(|_| "Missing retained-age proof")?;
    let (proofs, _) = Tree::from_root(age_root)
        .entries(
            &mut nodes,
            TreeLimits {
                max_entries: 16,
                max_reads: 256,
                max_read_bytes: 262144,
                ..TreeLimits::default()
            },
        )
        .map_err(|_| "Unavailable age proof")?;
    if proofs.is_empty() {
        return Err("Empty age proof".into());
    }
    for proof in proofs.values() {
        let raw = nodes
            .get(proof)
            .map_err(|_| "Unavailable age proof")?
            .ok_or("Missing age proof")?;
        let value = parse_json(&raw).map_err(|_| "Invalid age proof")?;
        let latest = value["observed"]["latest"]
            .as_u64()
            .ok_or("Missing age bounds")?;
        if latest
            .checked_add(required)
            .is_none_or(|deadline| deadline > f.execution_time.earliest)
        {
            return Err("Recorded execution precedes retained-age deadline".into());
        }
    }
    let (mut new_bytes, mut deleted, mut allocation) = (0u64, 0u64, 0u64);
    let mut seen = BTreeSet::new();
    let mut effects = Vec::new();
    for effect in &r.effects {
        let (store, plan) = actions.get(effect.action).ok_or("Unknown paged action")?;
        let a = &effect.receipt;
        let object = &plan.request().publication.object;
        let column = ctx
            .expected
            .iter()
            .position(|id| id == *store)
            .ok_or("Unknown paged provider")?;
        if !seen.insert(effect.action)
            || a.schema != "tig.final-retirement-receipt.design-1"
            || a.plan != plan.id().map_err(|_| "Invalid paged action")?
            || a.outcome.as_deref() != Some("deleted")
            || !a.grants_released
            || a.observed != "absent"
            || a.charged_metadata == 0
            || a.logical_bytes_removed != object.bytes
        {
            return Err("Invalid paged final effect".into());
        }
        deleted = deleted
            .checked_add(object.bytes)
            .ok_or("Final bytes overflow")?;
        if !a.historical {
            new_bytes = new_bytes
                .checked_add(object.bytes)
                .ok_or("Final bytes overflow")?;
            allocation = allocation
                .checked_add(a.file_allocation_before)
                .ok_or("Final allocation overflow")?;
        }
        effects.push(json!({"kind":if a.historical{"final_historical_or_cancelled"}else{"final_deleted"},"role":ctx.roles[column].id,"object":object,"logical_bytes":a.logical_bytes_removed}));
    }
    let mut rows = Vec::new();
    for (name, o) in &f.observations {
        let previous = ctx.previous["objects"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["name"] == *name))
            .ok_or("Missing preceding object observation")?;
        if previous["object"] != json!(o.object) || previous["observed"] != json!(o.before_copies) {
            return Err("Paged before state differs from preceding lesson".into());
        }
        if !token(name)
            || ctx.identities.get(name) != Some(&o.object)
            || !o.unknown_copies.is_empty()
        {
            return Err("Unknown paged object coverage".into());
        }
        for set in [&o.before_copies, &o.verified_copies] {
            if set.windows(2).any(|p| p[0] >= p[1])
                || set.iter().any(|id| !ctx.roles.iter().any(|r| &r.id == id))
            {
                return Err("Invalid paged roles".into());
            }
        }
        for (column, role) in ctx.roles.iter().enumerate() {
            let targeted = actions.iter().any(|(store, p)| {
                *store == &ctx.expected[column] && p.request().publication.object == o.object
            });
            if targeted {
                if !o.before_copies.contains(&role.id) || o.verified_copies.contains(&role.id) {
                    return Err("Paged copies conflict with deletion".into());
                }
            } else if o.before_copies.contains(&role.id) != o.verified_copies.contains(&role.id) {
                return Err("Unplanned paged copy change".into());
            }
        }
        let selection = match subjects.iter().find(|s| s.object == o.object) {
            Some(s) if s.hold.is_none() => "eligible",
            Some(_) => "protected",
            None => "unselected",
        };
        rows.push(json!({"name":name,"object":o.object,"before":o.before_copies,"desired":null,"observed":o.verified_copies,"unknown":[],"final_selection":selection}));
    }
    if subjects
        .iter()
        .any(|s| !f.observations.values().any(|o| o.object == s.object))
    {
        return Err("Incomplete paged subjects".into());
    }
    Ok(
        json!({"id":ctx.entry.lesson,"title":ctx.entry.title,"event":ctx.entry.event,"report":ctx.entry.report,"plan":inventory.decision(),
        "source_commit":f.source_commit,"source_available":ctx.commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),
        "objects":rows,"stream_bytes":0,"basis_current":true,"converged":true,"storage_converged":false,"remaining_page":false,"holds":[],"effects":effects,
        "final_loss":{"phase":"completed","new_removed_bytes":new_bytes,"historical_removed_bytes":deleted,"file_allocation_before_new_removals":allocation,"required_age_millis":required,"synthetic_owner_consent":true,"simulated_clock":true}}),
    )
}
