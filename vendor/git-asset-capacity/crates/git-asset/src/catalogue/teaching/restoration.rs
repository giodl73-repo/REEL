//! Joined recorded acquisition/restoration, isolated from older lesson contracts.
use super::*;
use git_asset_store::{
    AckLevel, Key,
    final_retirement::{bulk, provider::reacquire},
    ledger,
    retention::RetentionGrant,
    source_claims::Checkpoint,
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Acquisition {
    plan: Value,
    retention: Value,
    publication: Publication,
}
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct State {
    source: Checkpoint,
    ledgers: Vec<ledger::Checkpoint>,
    witness: ledger::Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Replay {
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
type Copies = BTreeMap<String, BTreeMap<String, Object>>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    schema: String,
    lesson: String,
    phase: String,
    source_commit: String,
    source_checkpoint: Checkpoint,
    providers: Vec<Digest>,
    old_bulk_plan: Value,
    final_parent_report: Object,
    missing_report: Object,
    previous_report: Object,
    acquisition: Value,
    acquisition_time: u64,
    seed_disabled: bool,
    seed_attempts_before: u64,
    seed_attempts_after: u64,
    history_final_guard: bool,
    synthetic_owner_consent: bool,
    simulated_clock: bool,
    retention_verified_at_capture: bool,
    independent_hardware_backup_proven: bool,
    authorizes_storage: bool,
    before: Copies,
    after: Copies,
    placement_plan: placement::Plan,
    placement_apply: Option<Receipt>,
    placement_status: Receipt,
    replay: Option<Replay>,
    before_replay: Option<State>,
    after_replay: Option<State>,
    retry_time: Option<u64>,
}
pub(super) struct Token {
    proof: Digest,
    acquisition_time: u64,
    report: Object,
    pub finished: bool,
}
pub(super) struct Context<'a> {
    pub entry: &'a Entry,
    pub lessons: &'a [Value],
    pub binding: &'a (Digest, Vec<Digest>),
    pub roles: &'a [Role],
    pub identities: &'a BTreeMap<String, Object>,
    pub commits: &'a BTreeSet<String>,
    pub source: &'a Checkpoint,
    pub seed_attempts: u64,
}
fn canonical(v: &Value) -> Result<Vec<u8>> {
    identity::canonical_json(v).map_err(|_| "Invalid joined metadata".into())
}
fn valid_hold(s: &str) -> bool {
    matches!(
        s,
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
}
pub(super) fn receipt(
    r: &Receipt,
    p: &placement::Plan,
    expected: &[Digest],
    copies: &Copies,
    roles: &[Role],
) -> Result<()> {
    if r.schema != "tig.placement-report.design-1"
        || r.plan != p.id().map_err(|_| "Invalid joined placement")?
        || r.observed_basis != p.basis
        || !r.basis_current
        || r.next.is_some()
        || r.effects.len() > 128
        || r.holds.len() > 64
        || r.holds.iter().any(|h| !valid_hold(h))
        || r.observations.len() != p.rows.len() * roles.len()
    {
        return Err("Inconsistent joined placement receipt".into());
    }
    let mut seen = BTreeSet::new();
    for value in &r.observations {
        let o: RecordedObserved =
            serde_json::from_value(value.clone()).map_err(|_| "Invalid joined observed state")?;
        let col = expected
            .iter()
            .position(|p| p == &o.provider)
            .ok_or("Unknown joined provider")?;
        let row = p
            .rows
            .iter()
            .find(|r| r.object == o.object)
            .ok_or("Unknown joined object")?;
        if row.required.len() != roles.len()
            || row.required[col] != o.required
            || !seen.insert((o.object.sha256.clone(), col))
        {
            return Err("Inconsistent joined observed scope".into());
        }
        if let Some(s) = o.state.as_str() {
            if !matches!(
                s,
                "not_started"
                    | "untracked_present"
                    | "partial"
                    | "published"
                    | "retained"
                    | "released"
                    | "retirement_pending"
                    | "authorized_absence"
                    | "cancelled"
            ) {
                return Err("Unknown joined state".into());
            }
            let present = copies[&roles[col].id].values().any(|v| v == &o.object);
            if (matches!(
                s,
                "published" | "retained" | "released" | "untracked_present"
            ) && !present)
                || (matches!(s, "not_started" | "authorized_absence") && present)
            {
                return Err("Joined state contradicts physical observation".into());
            }
        } else if !o.state.as_object().is_some_and(|m| {
            m.len() == 1
                && m.get("held")
                    .and_then(Value::as_str)
                    .is_some_and(|h| r.holds.iter().any(|s| s == h))
        }) {
            return Err("Invalid joined held state".into());
        }
    }
    Ok(())
}
pub(super) fn decode(
    value: Value,
    ctx: Context<'_>,
    token: Option<&Token>,
) -> Result<(Value, Token)> {
    let f: Observation =
        serde_json::from_value(value).map_err(|_| "Unsupported joined restoration")?;
    let previous = ctx
        .lessons
        .last()
        .ok_or("Missing preceding joined lesson")?;
    let final_parent = ctx
        .lessons
        .iter()
        .find(|v| v["id"] == "final-loss-completed")
        .ok_or("Missing joined final parent")?;
    let missing = ctx
        .lessons
        .iter()
        .find(|v| v["id"] == "git-return-after-final-loss")
        .ok_or("Missing joined source return")?;
    let acquired = f.phase == "backup_acquired";
    let restored = f.phase == "working_restored";
    let proof = Digest::blob(&canonical(&f.acquisition)?);
    let a: Acquisition =
        serde_json::from_value(f.acquisition.clone()).map_err(|_| "Invalid joined acquisition")?;
    let plan = reacquire::Plan::from_json(&canonical(&a.plan)?)
        .map_err(|_| "Invalid joined acquisition plan")?;
    let grant = RetentionGrant::from_json(&canonical(&a.retention)?)
        .map_err(|_| "Invalid joined retained grant")?;
    let old = bulk::Plan::from_json(&canonical(&f.old_bulk_plan)?)
        .map_err(|_| "Invalid joined old bulk plan")?;
    let parent = git_asset_store::final_retirement::provider::Plan::from_json(&canonical(
        &a.plan["parent"],
    )?)
    .map_err(|_| "Invalid joined old provider plan")?;
    let parent_id = parent.id().map_err(|_| "Invalid joined parent identity")?;
    let old_id = old.id().map_err(|_| "Invalid joined bulk identity")?;
    let req: git_asset_store::Request =
        serde_json::from_value(a.plan["request"]["publication"].clone())
            .map_err(|_| "Missing joined request")?;
    let capacity: Digest = serde_json::from_value(a.plan["request"]["capacity"].clone())
        .map_err(|_| "Missing joined capacity")?;
    let target = ctx
        .binding
        .1
        .iter()
        .position(|p| p == grant.store())
        .ok_or("Unknown joined backup provider")?;
    let birth = a.plan["birth"]["index"]
        .as_u64()
        .ok_or("Missing joined birth")?;
    let parent_birth = a.plan["parent"]["birth"]["index"]
        .as_u64()
        .ok_or("Missing joined old birth")?;
    let pubn = &a.publication;
    let id = f
        .placement_plan
        .id()
        .map_err(|_| "Invalid joined placement plan")?;
    if f.schema != "tig.teaching-return-restore-observation.design-1"
        || f.lesson != ctx.entry.lesson
        || (!acquired && !restored)
        || (acquired
            && (f.lesson != "returned-backup-acquired"
                || previous["id"] != "git-return-after-final-loss"
                || token.is_some()))
        || (restored
            && (f.lesson != "returned-working-restored"
                || previous["id"] != "returned-backup-acquired"
                || token.is_none_or(|t| {
                    t.finished
                        || t.proof != proof
                        || t.report != f.previous_report
                        || t.acquisition_time != f.acquisition_time
                })))
        || previous["report"] != json!(f.previous_report)
        || final_parent["report"] != json!(f.final_parent_report)
        || missing["report"] != json!(f.missing_report)
        || final_parent["final_loss"]["phase"] != "completed"
        || final_parent["plan"] != json!(old_id)
        || missing["converged"] != false
        || &f.source_checkpoint != ctx.source
        || f.source_commit != missing["source_commit"]
        || f.source_commit != previous["source_commit"]
        || GitId::parse(&f.source_commit).is_err()
        || missing["source_sequence"] != json!(f.source_checkpoint.sequence())
        || !old
            .enrollment()
            .sources
            .iter()
            .any(|s| &s.namespace == f.source_checkpoint.identity())
        || f.providers != ctx.binding.1
        || f.placement_plan.providers != ctx.binding.1
        || f.placement_plan.enrollment != ctx.binding.0
        || missing["plan"] != json!(id)
        || f.placement_plan.rows.len() != ctx.identities.len()
        || f.placement_plan.next.is_some()
        || f.placement_plan.after.is_some()
        || f.placement_plan
            .rows
            .iter()
            .map(|r| &r.object.sha256)
            .collect::<BTreeSet<_>>()
            .len()
            != ctx.identities.len()
        || !f.seed_disabled
        || f.seed_attempts_before != ctx.seed_attempts
        || f.seed_attempts_before != f.seed_attempts_after
        || !f.history_final_guard
        || !f.synthetic_owner_consent
        || !f.simulated_clock
        || !f.retention_verified_at_capture
        || f.authorizes_storage
        || f.independent_hardware_backup_proven
        || ctx.roles[target].id != "backup"
        || ctx.identities.get("original") != Some(&req.object)
        || parent.request().publication.object != req.object
        || !old
            .actions()
            .any(|(store, p)| store == grant.store() && p.id().is_ok_and(|p| p == parent_id))
        || birth != parent_birth.checked_add(1).ok_or("Joined birth overflow")?
        || a.plan["birth"]["store"] != json!(grant.store())
        || a.plan["birth"]["object"] != json!(req.object)
        || a.plan["birth"]["reacquire"] != json!(req)
        || a.plan["payload"]["request"] != json!(req)
        || a.plan["payload"]["store"] != json!(grant.store())
        || pubn.request != req
        || pubn.ack != AckLevel::ProcessRestart
        || req.required != pubn.ack
        || pubn.was_replayed
        || pubn.streamed_bytes != req.object.bytes
        || pubn.charged_footprint < req.object.bytes
        || pubn.charged_footprint > plan.required_peak().map_err(|_| "Invalid joined peak")?
        || pubn.basis != "conservative-allocated-file-data-footprint"
        || pubn.allocation.genesis != capacity
        || pubn.settlement.genesis != capacity
        || pubn.checkpoint.identity() != &capacity
        || pubn.checkpoint.sequence() == 0
        || pubn.allocation.operation == pubn.settlement.operation
        || grant.request().publication != req
        || grant.request().owner != req.owner
        || grant.request().minimum_ack != pubn.ack
        || parent.request().capacity != capacity
        || a.retention["record"]["job"]["genesis"] != json!(capacity)
        || a.retention["record"]["allocation"]["genesis"] != json!(capacity)
        || f.acquisition_time == 0
        || f.acquisition_time
            < a.plan["request"]["window"]["not_before"]
                .as_u64()
                .ok_or("Missing joined window")?
        || f.acquisition_time
            >= a.plan["request"]["window"]["expires"]
                .as_u64()
                .ok_or("Missing joined expiry")?
    {
        return Err("Inconsistent joined restoration evidence".into());
    }
    for snapshot in [&f.before, &f.after] {
        if snapshot.len() != ctx.roles.len()
            || snapshot
                .keys()
                .any(|id| !ctx.roles.iter().any(|r| &r.id == id))
            || snapshot.values().any(|m| {
                m.iter()
                    .any(|(name, o)| ctx.identities.get(name) != Some(o))
            })
        {
            return Err("Invalid joined physical scope".into());
        }
    }
    let mut rows = Vec::new();
    for (name, object) in ctx.identities {
        let copies = |s: &Copies| {
            s.iter()
                .filter(|(_, m)| m.get(name) == Some(object))
                .map(|(r, _)| r.clone())
                .collect::<Vec<_>>()
        };
        let before = copies(&f.before);
        let after = copies(&f.after);
        let previous = previous["objects"]
            .as_array()
            .and_then(|v| v.iter().find(|v| v["name"] == *name))
            .ok_or("Missing prior joined object")?;
        if previous["object"] != json!(object) || previous["observed"] != json!(before) {
            return Err("Joined copy history mismatch".into());
        }
        if object == &req.object {
            let want_before = if acquired {
                vec![]
            } else {
                vec!["backup".to_owned()]
            };
            let want_after = if acquired {
                vec!["backup".to_owned()]
            } else {
                vec!["backup".to_owned(), "working".to_owned()]
            };
            if before != want_before || after != want_after {
                return Err("Joined original copy mismatch".into());
            }
        } else if before != after {
            return Err("Joined restoration changed unrelated copies".into());
        }
        let row = f
            .placement_plan
            .rows
            .iter()
            .find(|r| &r.object == object)
            .ok_or("Missing joined plan object")?;
        if row.required.len() != ctx.roles.len() {
            return Err("Invalid joined desired scope".into());
        }
        let desired = row
            .required
            .iter()
            .zip(ctx.roles)
            .filter(|(v, _)| **v)
            .map(|(_, r)| r.id.clone())
            .collect::<Vec<_>>();
        if object == &req.object
            && (desired.iter().any(|r| r == "history")
                || !["working", "backup"]
                    .iter()
                    .all(|r| desired.iter().any(|v| v == r)))
        {
            return Err("Joined desired source mismatch".into());
        }
        rows.push(json!({"name":name,"object":object,"before":before,"desired":desired,"observed":after,"unknown":[]}));
    }
    receipt(
        &f.placement_status,
        &f.placement_plan,
        &ctx.binding.1,
        &f.after,
        ctx.roles,
    )?;
    if f.placement_status.page_converged != restored
        || f.placement_status.storage_converged != Some(restored)
        || !f.placement_status.effects.is_empty()
        || f.placement_status.stream_bytes != 0
    {
        return Err("Invalid joined readonly status".into());
    }
    let mut effects = Vec::new();
    let mut working_stream = 0;
    let mut replay_summary = Value::Null;
    if acquired {
        if f.placement_apply.is_some()
            || f.replay.is_some()
            || f.before_replay.is_some()
            || f.after_replay.is_some()
            || f.retry_time.is_some()
        {
            return Err("Acquisition masquerades as completed restoration".into());
        }
        effects.push(json!({"kind":"reacquired","role":"backup","object":req.object,"logical_bytes":req.object.bytes}));
    } else {
        let applied = f
            .placement_apply
            .as_ref()
            .ok_or("Missing actual joined apply")?;
        receipt(
            applied,
            &f.placement_plan,
            &ctx.binding.1,
            &f.after,
            ctx.roles,
        )?;
        if !applied.page_converged
            || applied.storage_converged != Some(true)
            || !applied.holds.is_empty()
            || !f.placement_status.holds.is_empty()
            || applied.stream_bytes != req.object.bytes
        {
            return Err("Incomplete actual joined apply".into());
        }
        working_stream = applied.stream_bytes;
        let mut streamed = 0u64;
        for effect in &applied.effects {
            let col = ctx
                .binding
                .1
                .iter()
                .position(|p| p == &effect.provider)
                .ok_or("Unknown joined effect provider")?;
            if effect.object != req.object
                || ctx.roles[col].id != "working"
                || !matches!(
                    effect.kind.as_str(),
                    "published" | "reacquired" | "retained" | "handoff"
                )
            {
                return Err("Unplanned joined effect".into());
            }
            if matches!(effect.kind.as_str(), "published" | "reacquired") {
                if effect.logical_bytes != req.object.bytes {
                    return Err("Invalid joined transfer length".into());
                }
                streamed = streamed
                    .checked_add(effect.logical_bytes)
                    .ok_or("Joined stream overflow")?;
            }
            effects.push(json!({"kind":effect.kind,"role":ctx.roles[col].id,"object":effect.object,"logical_bytes":effect.logical_bytes,"file_allocation_before":effect.file_allocation_before}));
        }
        if streamed != working_stream {
            return Err("Joined stream counter mismatch".into());
        }
        let replay = f.replay.as_ref().ok_or("Missing joined old retry")?;
        let before = f.before_replay.as_ref().ok_or("Missing pre-retry state")?;
        let after = f.after_replay.as_ref().ok_or("Missing post-retry state")?;
        for (store, checkpoint) in ctx.binding.1.iter().zip(&before.ledgers) {
            let capacities = old
                .request()
                .copies
                .iter()
                .filter(|copy| &copy.store == store)
                .map(|copy| &copy.capacity)
                .collect::<BTreeSet<_>>();
            if capacities.len() != 1 || !capacities.contains(checkpoint.identity()) {
                return Err("Joined replay ledger/provider mismatch".into());
            }
        }
        let bytes = old.actions().try_fold(0u64, |n, (_, p)| {
            n.checked_add(p.request().publication.object.bytes)
                .ok_or("Joined old size overflow")
        })?;
        if before != after
            || before.source != f.source_checkpoint
            || before.ledgers.len() != ctx.roles.len()
            || before
                .ledgers
                .iter()
                .map(|c| c.identity())
                .collect::<BTreeSet<_>>()
                .len()
                != ctx.roles.len()
            || before.ledgers[target].identity() != &capacity
            || before.ledgers[target].sequence() < pubn.checkpoint.sequence()
            || before.witness.identity() != &old.enrollment().capacity
            || before.witness.sequence() == 0
            || f.retry_time
                .is_none_or(|t| t < old.request().window.expires || t <= f.acquisition_time)
            || replay.plan != old_id
            || replay.phase != "completed"
            || !replay.intent_acknowledged
            || replay.selected_roots != old.request().selections.len()
            || replay.selected_bindings != old.bindings().len()
            || replay.unique_objects != old.subjects().len()
            || replay.planned_copy_bytes != bytes
            || replay.new_removed_bytes != 0
            || replay.historical_removed_bytes != bytes
            || !replay.holds.is_empty()
            || replay.removal_authority
        {
            return Err("Joined old retry changed current protection".into());
        }
        replay_summary = json!({"new_removed_bytes":replay.new_removed_bytes,"historical_removed_bytes":replay.historical_removed_bytes});
    }
    let value = json!({"id":ctx.entry.lesson,"title":ctx.entry.title,"event":ctx.entry.event,"report":ctx.entry.report,"plan":id,"source_commit":f.source_commit,"source_available":ctx.commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),"objects":rows,"basis_current":true,"converged":restored,"storage_converged":restored,"stream_bytes":if acquired{pubn.streamed_bytes}else{working_stream},"holds":f.placement_status.holds,"remaining_page":false,"effects":effects,
        "restoration":{"phase":f.phase,"object":req.object,"parent_birth":parent_birth,"new_birth":birth,"ack":pubn.ack,"acquired_input_bytes":pubn.streamed_bytes,"working_stream_bytes":working_stream,"charged_footprint":pubn.charged_footprint,"retention":"verified_at_recording","old_retry":replay_summary,"synthetic_owner_consent":true,"simulated_clock":true}});
    Ok((
        value,
        Token {
            proof,
            acquisition_time: f.acquisition_time,
            report: ctx.entry.report.clone(),
            finished: restored,
        },
    ))
}
