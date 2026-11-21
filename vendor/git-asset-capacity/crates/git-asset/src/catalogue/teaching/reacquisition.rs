//! Recorded fresh acquisition and historical old retry; no provider execution.
use super::*;
use git_asset_store::{
    AckLevel, Key,
    final_retirement::{bulk, provider::reacquire},
    ledger,
    retention::RetentionGrant,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    genesis: Digest,
    operation: Key,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    request: git_asset_store::Request,
    ack: AckLevel,
    allocation: Reference,
    settlement: Reference,
    checkpoint: ledger::Checkpoint,
    was_replayed: bool,
    charged_footprint: u64,
    basis: String,
    streamed_bytes: u64,
}
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct State {
    source: git_asset_store::source_claims::Checkpoint,
    ledgers: Vec<ledger::Checkpoint>,
    witness: ledger::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Retry {
    plan: Digest,
    phase: String,
    intent_acknowledged: bool,
    selected_roots: usize,
    selected_bindings: usize,
    unique_objects: usize,
    planned_copy_bytes: u64,
    new_removed_bytes: u64,
    historical_removed_bytes: u64,
    holds: Vec<Value>,
    removal_authority: bool,
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
struct Observation {
    schema: String,
    lesson: String,
    source_commit: String,
    source_checkpoint: git_asset_store::source_claims::Checkpoint,
    providers: Vec<Digest>,
    old_bulk_plan: Value,
    plan: Value,
    retention: Value,
    publication: Publication,
    target_role: String,
    old_retry: Retry,
    before_replay: State,
    after_replay: State,
    observations: BTreeMap<String, Row>,
    acquisition_time: u64,
    retry_time: u64,
    synthetic_owner_consent: bool,
    simulated_clock: bool,
    retention_verified_at_capture: bool,
    independent_hardware_backup_proven: bool,
    authorizes_storage: bool,
}
pub(super) struct Context<'a> {
    pub entry: &'a Entry,
    pub previous: &'a Value,
    pub expected: &'a [Digest],
    pub roles: &'a [Role],
    pub identities: &'a BTreeMap<String, Object>,
    pub commits: &'a BTreeSet<String>,
}
fn canonical(value: &Value) -> Result<Vec<u8>> {
    identity::canonical_json(value).map_err(|_| "Invalid acquisition metadata".into())
}
pub(super) fn decode(value: Value, ctx: Context<'_>) -> Result<Value> {
    let f: Observation =
        serde_json::from_value(value).map_err(|_| "Unsupported acquisition observation")?;
    let plan = reacquire::Plan::from_json(&canonical(&f.plan)?)
        .map_err(|_| "Invalid reacquisition plan")?;
    let old = bulk::Plan::from_json(&canonical(&f.old_bulk_plan)?)
        .map_err(|_| "Invalid prior bulk plan")?;
    let grant = RetentionGrant::from_json(&canonical(&f.retention)?)
        .map_err(|_| "Invalid acquisition grant")?;
    let id = plan.id().map_err(|_| "Invalid acquisition identity")?;
    let old_id = old.id().map_err(|_| "Invalid prior bulk identity")?;
    let parent = git_asset_store::final_retirement::provider::Plan::from_json(&canonical(
        &f.plan["parent"],
    )?)
    .map_err(|_| "Invalid parent provider plan")?;
    let parent_id = parent.id().map_err(|_| "Invalid parent identity")?;
    let target = ctx
        .expected
        .iter()
        .position(|store| store == grant.store())
        .ok_or("Unknown acquisition provider")?;
    let request: git_asset_store::Request =
        serde_json::from_value(f.plan["request"]["publication"].clone())
            .map_err(|_| "Invalid acquisition publication")?;
    let capacity: Digest = serde_json::from_value(f.plan["request"]["capacity"].clone())
        .map_err(|_| "Invalid acquisition capacity")?;
    let birth = f.plan["birth"]["index"]
        .as_u64()
        .ok_or("Missing new birth")?;
    let parent_birth = f.plan["parent"]["birth"]["index"]
        .as_u64()
        .ok_or("Missing parent birth")?;
    let p = &f.publication;
    let r = &f.old_retry;
    let actions = old.actions().collect::<Vec<_>>();
    let copy_bytes = actions.iter().try_fold(0u64, |n, (_, a)| {
        n.checked_add(a.request().publication.object.bytes)
            .ok_or("Acquisition copy byte overflow")
    })?;
    if f.schema != "tig.teaching-reacquisition-observation.design-1"
        || f.lesson != ctx.entry.lesson
        || f.providers != ctx.expected
        || f.target_role != ctx.roles[target].id
        || !f.synthetic_owner_consent
        || !f.simulated_clock
        || !f.retention_verified_at_capture
        || f.independent_hardware_backup_proven
        || f.authorizes_storage
        || f.source_commit != ctx.previous["source_commit"]
        || f.source_commit.len() != 40
        || ctx.previous["final_loss"]["phase"] != "completed"
        || ctx.previous["plan"] != json!(old_id)
        || ctx.previous["source_sequence"] != json!(f.source_checkpoint.sequence())
        || !old
            .enrollment()
            .sources
            .iter()
            .any(|s| &s.namespace == f.source_checkpoint.identity())
        || f.before_replay != f.after_replay
        || f.before_replay.source != f.source_checkpoint
        || f.before_replay.ledgers.len() != ctx.expected.len()
        || f.before_replay.witness.identity() != &old.enrollment().capacity
        || f.before_replay.witness.sequence() == 0
        || f.before_replay.ledgers[target].identity() != &capacity
        || f.before_replay.ledgers[target].sequence() < p.checkpoint.sequence()
        || f.before_replay
            .ledgers
            .iter()
            .map(|c| c.identity())
            .collect::<BTreeSet<_>>()
            .len()
            != ctx.expected.len()
        || p.request != request
        || p.ack != AckLevel::ProcessRestart
        || p.request.required != p.ack
        || p.was_replayed
        || p.streamed_bytes != request.object.bytes
        || p.charged_footprint < request.object.bytes
        || p.charged_footprint
            > plan
                .required_peak()
                .map_err(|_| "Invalid acquisition peak")?
        || p.basis != "conservative-allocated-file-data-footprint"
        || p.allocation.genesis != capacity
        || p.settlement.genesis != capacity
        || p.checkpoint.identity() != &capacity
        || p.checkpoint.sequence() == 0
        || p.allocation.operation == p.settlement.operation
        || grant.request().publication != request
        || grant.request().minimum_ack != p.ack
        || grant.request().owner != request.owner
        || f.retention["record"]["job"]["genesis"] != json!(capacity)
        || f.retention["record"]["allocation"]["genesis"] != json!(capacity)
        || parent.request().capacity != capacity
        || parent.request().publication.object != request.object
        || !actions
            .iter()
            .any(|(store, a)| *store == grant.store() && a.id().is_ok_and(|id| id == parent_id))
        || f.plan["birth"]["store"] != json!(grant.store())
        || f.plan["birth"]["object"] != json!(request.object)
        || f.plan["birth"]["reacquire"] != json!(request)
        || birth != parent_birth.checked_add(1).ok_or("Birth overflow")?
        || f.plan["payload"]["request"] != json!(request)
        || f.plan["payload"]["store"] != json!(grant.store())
        || r.plan != old_id
        || r.phase != "completed"
        || !r.intent_acknowledged
        || r.new_removed_bytes != 0
        || r.historical_removed_bytes != copy_bytes
        || r.planned_copy_bytes != copy_bytes
        || r.selected_roots != old.request().selections.len()
        || r.selected_bindings != old.bindings().len()
        || r.unique_objects != old.subjects().len()
        || !r.holds.is_empty()
        || r.removal_authority
        || f.acquisition_time == 0
        || f.acquisition_time
            < f.plan["request"]["window"]["not_before"]
                .as_u64()
                .ok_or("Missing acquisition window")?
        || f.acquisition_time
            >= f.plan["request"]["window"]["expires"]
                .as_u64()
                .ok_or("Missing acquisition expiry")?
        || f.retry_time < old.request().window.expires
        || f.retry_time <= f.acquisition_time
        || f.observations.len() != ctx.identities.len()
        || f.observations.len() > 32
    {
        return Err("Inconsistent acquisition observation".into());
    }
    let mut rows = Vec::new();
    let mut changed = 0;
    for (name, o) in f.observations {
        let previous = ctx.previous["objects"]
            .as_array()
            .and_then(|rows| rows.iter().find(|r| r["name"] == name))
            .ok_or("Missing preceding acquisition object")?;
        if !token(&name)
            || ctx.identities.get(&name) != Some(&o.object)
            || !o.unknown_copies.is_empty()
            || previous["object"] != json!(o.object)
            || previous["observed"] != json!(o.before_copies)
        {
            return Err("Acquisition before state mismatch".into());
        }
        for set in [&o.before_copies, &o.verified_copies] {
            if set.windows(2).any(|p| p[0] >= p[1])
                || set.iter().any(|id| !ctx.roles.iter().any(|r| &r.id == id))
            {
                return Err("Invalid acquisition roles".into());
            }
        }
        let acquired = o.object == request.object;
        if acquired {
            if !o.before_copies.is_empty() || o.verified_copies != vec![f.target_role.clone()] {
                return Err("Acquired copy scope mismatch".into());
            }
            changed += 1;
        } else if o.before_copies != o.verified_copies {
            return Err("Unplanned acquisition copy change".into());
        }
        rows.push(json!({"name":name,"object":o.object,"before":o.before_copies,"desired":null,"observed":o.verified_copies,"unknown":[],"acquisition_selection":if acquired{"new_birth"}else{"unchanged"}}));
    }
    if changed != 1 {
        return Err("Incomplete acquisition object scope".into());
    }
    Ok(
        json!({"id":ctx.entry.lesson,"title":ctx.entry.title,"event":ctx.entry.event,"report":ctx.entry.report,"plan":id,
        "source_commit":f.source_commit,"source_available":ctx.commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),
        "objects":rows,"stream_bytes":p.streamed_bytes,"basis_current":true,"converged":true,"storage_converged":false,"remaining_page":false,"holds":[],
        "effects":[{"kind":"reacquired","role":f.target_role,"object":request.object,"logical_bytes":request.object.bytes}],
        "reacquisition":{"phase":"retained","object":request.object,"role":f.target_role,"parent_birth":parent_birth,"new_birth":birth,
            "added_logical_copy_bytes":request.object.bytes,"ack":p.ack,"charged_footprint":p.charged_footprint,
            "old_plan":old_id,"old_retry_new_removed_bytes":r.new_removed_bytes,"old_retry_historical_removed_bytes":r.historical_removed_bytes,
            "synthetic_owner_consent":true,"simulated_clock":true,"retention":"verified_at_recording"}}),
    )
}
