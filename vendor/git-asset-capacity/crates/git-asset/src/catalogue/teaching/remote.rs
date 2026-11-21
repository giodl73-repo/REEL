//! Pure bounded consistency checks for saved LFS fixture observations.
use super::*;
use git_asset_core::tree::{MemoryNodes, Tree, TreeLimits};
use git_asset_store::{ledger, source_claims::Checkpoint};
type Copies = BTreeMap<String, BTreeMap<String, Object>>;
#[derive(Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Local {
    source: Checkpoint,
    ledgers: Vec<ledger::Checkpoint>,
    copies: Copies,
    seed_attempts: u64,
    git_head: String,
}
#[derive(Clone, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    store: Digest,
    capacity: Digest,
    checkpoint: ledger::Checkpoint,
    objects: Vec<Object>,
    registered: Vec<Digest>,
    requests: u64,
    put_count: u64,
    put_payload_bytes: u64,
    get_payload_bytes: u64,
    corrupt_response_injected: bool,
    retained_custody: bool,
    removal_authority: bool,
}
impl Endpoint {
    fn content_eq(&self, b: &Self) -> bool {
        self.store == b.store
            && self.capacity == b.capacity
            && self.checkpoint == b.checkpoint
            && self.objects == b.objects
            && self.registered == b.registered
            && self.put_count == b.put_count
            && self.put_payload_bytes == b.put_payload_bytes
    }
    fn valid(&self) -> bool {
        !self.retained_custody
            && !self.removal_authority
            && self.capacity == *self.checkpoint.identity()
            && self.checkpoint.sequence() > 0
            && self.objects.len() <= 2
            && self.registered.len() <= 2
            && self.requests <= 64
            && self.put_count <= 2
            && self.put_payload_bytes <= 54
            && self.get_payload_bytes <= 4096
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cli {
    apply: Vec<Value>,
    status: Option<Vec<Status>>,
    unauthorized_plan: Option<Failure>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Failure {
    schema: String,
    ok: bool,
    code: String,
}
impl Failure {
    fn is(&self, code: &str) -> bool {
        self.schema == "tig.lfs-upload-response.design-1" && !self.ok && self.code == code
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Uploaded {
    schema: String,
    plan: Digest,
    object: Object,
    payload_io_bytes: u64,
    retained_custody: bool,
    removal_authority: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Status {
    schema: String,
    plan: Digest,
    historical_completion: bool,
    intent_recorded: bool,
    accounting_settled: bool,
    verified_present: bool,
    requires_recovery: bool,
    retained_custody: bool,
    removal_authority: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Peak {
    schema: String,
    plan: Digest,
    local_witness_peak_bytes: u64,
    remote_quota_reserved: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Remote {
    schema: String,
    lesson: String,
    phase: String,
    source_commit: String,
    source_checkpoint: Checkpoint,
    providers: Vec<Digest>,
    authority: Object,
    witness_store: Digest,
    native_selection: Object,
    source_bindings: BTreeMap<git_asset_core::LogicalPath, Object>,
    selected: Vec<Object>,
    plans: Vec<Value>,
    peaks: Vec<Peak>,
    fixture_input_staging_bytes: u64,
    local_before: Local,
    local_after: Local,
    local_plan: placement::Plan,
    local_status: Receipt,
    previous_report: Object,
    endpoint_before: Endpoint,
    endpoint_after: Endpoint,
    witness_before: ledger::Checkpoint,
    witness_after: ledger::Checkpoint,
    cli: Cli,
    seed_disabled: bool,
    retained_custody: bool,
    removal_authority: bool,
    synthetic_endpoint: bool,
    independent_hardware_backup_proven: bool,
}
pub(super) struct Token {
    step: usize,
    proof: Digest,
    report: Object,
    endpoint: Endpoint,
    witness: ledger::Checkpoint,
    pub finished: bool,
}
pub(super) struct Context<'a> {
    pub entry: &'a Entry,
    pub previous: &'a Value,
    pub binding: &'a (Digest, Vec<Digest>),
    pub roles: &'a [Role],
    pub identities: &'a BTreeMap<String, Object>,
    pub source: &'a Checkpoint,
    pub commits: &'a BTreeSet<String>,
}
fn canonical(v: &Value) -> Result<Vec<u8>> {
    identity::canonical_json(v).map_err(|_| "Invalid remote metadata".into())
}
pub(super) fn decode(
    value: Value,
    ctx: Context<'_>,
    token: Option<&Token>,
) -> Result<(Value, Token)> {
    let projection = json!({"source":value["source_checkpoint"],"commit":value["source_commit"],"providers":value["providers"],"authority":value["authority"],"witness_store":value["witness_store"],"native_selection":value["native_selection"],"source_bindings":value["source_bindings"],"selected":value["selected"],"plans":value["plans"],"peaks":value["peaks"],"local_before":value["local_before"],"local_plan":value["local_plan"],"staging":value["fixture_input_staging_bytes"]});
    let proof = Digest::blob(&canonical(&projection)?);
    let f: Remote =
        serde_json::from_value(value).map_err(|_| "Unsupported remote teaching metadata")?;
    let ids = [
        "remote-upload-disabled",
        "remote-payload-published",
        "remote-readback-held",
        "remote-publication-rechecked",
    ];
    let phases = [
        "upload_disabled",
        "payload_published",
        "readback_held",
        "publication_rechecked",
    ];
    let step = ids
        .iter()
        .position(|id| *id == f.lesson)
        .ok_or("Unknown remote lesson")?;
    let before = &f.endpoint_before;
    let after = &f.endpoint_after;
    let local_id = f
        .local_plan
        .id()
        .map_err(|_| "Invalid local remote-lesson plan")?;
    if f.schema != "tig.teaching-remote-publication-observation.design-1"
        || f.lesson != ctx.entry.lesson
        || f.phase != phases[step]
        || ctx.previous["report"] != json!(f.previous_report)
        || ctx.previous["source_commit"] != f.source_commit
        || GitId::parse(&f.source_commit).is_err()
        || f.source_checkpoint != *ctx.source
        || f.local_before.source != *ctx.source
        || f.local_before.git_head != f.source_commit
        || f.local_before != f.local_after
        || !f.seed_disabled
        || f.retained_custody
        || f.removal_authority
        || !f.synthetic_endpoint
        || f.independent_hardware_backup_proven
        || f.fixture_input_staging_bytes != 54
        || f.providers != ctx.binding.1
        || f.local_plan.providers != ctx.binding.1
        || f.local_plan.enrollment != ctx.binding.0
        || ctx.previous["plan"] != json!(local_id)
        || ctx.roles.len() != 3
        || ctx.identities.len() != 3
        || f.plans.len() != 2
        || f.peaks.len() != 2
        || f.selected.len() != 2
        || f.cli.apply.len() != 2
        || !before.valid()
        || !after.valid()
        || f.local_before.ledgers.len() != 3
        || f.local_before
            .ledgers
            .iter()
            .map(|c| c.identity())
            .collect::<BTreeSet<_>>()
            .len()
            != 3
        || ctx.binding.1.contains(&before.store)
        || f.witness_before.identity() != f.witness_after.identity()
        || f.witness_before.sequence() == 0
        || before.store != after.store
        || before.capacity != after.capacity
        || f.local_before.ledgers.iter().any(|c| {
            c.identity() == &before.capacity || c.identity() == f.witness_before.identity()
        })
        || before.capacity == *f.witness_before.identity()
    {
        return Err("Inconsistent remote teaching context".into());
    }
    if step == 0 {
        if token.is_some()
            || ctx.previous["id"] != "pull-return-v2"
            || before != after
            || before.requests != 0
            || before.put_count != 0
            || before.put_payload_bytes != 0
            || before.get_payload_bytes != 0
            || !before.objects.is_empty()
            || !before.registered.is_empty()
            || f.witness_before != f.witness_after
        {
            return Err("Invalid disabled remote entry".into());
        }
    } else {
        let old = token.ok_or("Missing preceding remote phase")?;
        let mut expected = old.endpoint.clone();
        expected.corrupt_response_injected = step == 2;
        if old.finished
            || old.step + 1 != step
            || old.proof != proof
            || old.report != f.previous_report
            || ctx.previous["id"] != ids[step - 1]
            || expected != *before
            || old.witness != f.witness_before
        {
            return Err("Remote phase continuity mismatch".into());
        }
    }
    let selected = ctx.previous["objects"]
        .as_array()
        .ok_or("Missing preceding local rows")?
        .iter()
        .filter(|r| {
            r["desired"]
                .as_array()
                .is_some_and(|a| a.contains(&json!("working")) && a.contains(&json!("backup")))
        })
        .map(|r| {
            serde_json::from_value::<Object>(r["object"].clone())
                .map_err(|_| "Invalid selected remote object".into())
        })
        .collect::<Result<Vec<_>>>()?;
    if selected.len() != 2
        || f.selected.iter().any(|o| !selected.contains(o))
        || f.selected[0].sha256 >= f.selected[1].sha256
        || f.selected
            .iter()
            .try_fold(0u64, |n, o| n.checked_add(o.bytes))
            != Some(54)
        || f.source_bindings.is_empty()
        || f.source_bindings.len() > 16
        || f.source_bindings.values().any(|o| !f.selected.contains(o))
        || f.selected
            .iter()
            .any(|o| !f.source_bindings.values().any(|v| v == o))
    {
        return Err("Remote selected scope mismatch".into());
    }
    let mut nodes = MemoryNodes::default();
    let (native, _) = Tree::build(&f.source_bindings, &mut nodes, TreeLimits::default())
        .map_err(|_| "Invalid native remote bindings")?;
    let entries = f
        .selected
        .iter()
        .map(|o| {
            Ok((
                git_asset_core::LogicalPath::parse(o.sha256.as_str())
                    .map_err(|_| "Invalid remote OID")?,
                o.clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let (index, _) = Tree::build(&entries, &mut nodes, TreeLimits::default())
        .map_err(|_| "Invalid remote OID authority")?;
    if native.root() != &f.native_selection || index.root() != &f.authority {
        return Err("Remote authority projection mismatch".into());
    }
    let expected_context = Digest::blob(&canonical(
        &json!({"source":f.source_checkpoint,"authority":f.authority}),
    )?);
    let mut plans = Vec::new();
    let mut operations = BTreeSet::new();
    for (raw, object) in f.plans.iter().zip(&f.selected) {
        let p = crate::upload::Plan::from_json(&canonical(raw)?)
            .map_err(|_| "Invalid SDK upload plan")?;
        if &p.object != object
            || p.authority != f.authority
            || p.capacity != *f.witness_before.identity()
            || p.source_generation != 1
            || p.source != expected_context
            || p.context != expected_context
            || p.owner.as_str() != "sample"
            || p.witness.instance != f.witness_store
            || p.witness.generation == 0
            || p.witness.instance == before.store
            || ctx.binding.1.contains(&p.witness.instance)
            || !operations.insert(p.operation.as_str().to_owned())
        {
            return Err("Upload plan context mismatch".into());
        }
        plans.push(p);
    }
    if plans[0].source != plans[1].source
        || plans[0].context != plans[1].context
        || plans[0].target != plans[1].target
        || plans[0].witness != plans[1].witness
        || plans[0].owner != plans[1].owner
        || plans[0].budget != plans[1].budget
    {
        return Err("Mixed remote upload plans".into());
    }
    for (p, peak) in plans.iter().zip(&f.peaks) {
        if peak.schema != "tig.lfs-upload-peak.design-1"
            || peak.plan != p.id().map_err(|_| "Invalid upload id")?
            || peak.local_witness_peak_bytes == 0
            || peak.local_witness_peak_bytes > 64 * 1024 * 1024
            || peak.remote_quota_reserved
        {
            return Err("Invalid upload peak evidence".into());
        }
    }
    let mut rows = Vec::new();
    if f.local_before.copies.len() != 3
        || f.local_plan.rows.len() != 3
        || f.local_plan.next.is_some()
        || f.local_plan.after.is_some()
    {
        return Err("Invalid local remote scope".into());
    }
    for (name, object) in ctx.identities {
        let previous = ctx.previous["objects"]
            .as_array()
            .and_then(|r| r.iter().find(|r| r["name"] == *name))
            .ok_or("Missing local remote object")?;
        let actual = f
            .local_before
            .copies
            .iter()
            .filter(|(_, m)| m.get(name) == Some(object))
            .map(|(r, _)| r.clone())
            .collect::<Vec<_>>();
        if previous["object"] != json!(object)
            || previous["observed"] != json!(actual)
            || f.local_before.copies.iter().any(|(r, m)| {
                !ctx.roles.iter().any(|p| &p.id == r)
                    || m.iter().any(|(n, o)| ctx.identities.get(n) != Some(o))
            })
        {
            return Err("Remote changed local physical copies".into());
        }
        let plan = f
            .local_plan
            .rows
            .iter()
            .find(|r| &r.object == object)
            .ok_or("Missing local remote plan row")?;
        if plan.required.len() != 3 {
            return Err("Invalid local remote requirements".into());
        }
        let desired = plan
            .required
            .iter()
            .zip(ctx.roles)
            .filter(|(v, _)| **v)
            .map(|(_, r)| r.id.clone())
            .collect::<Vec<_>>();
        if previous["desired"] != json!(desired) {
            return Err("Remote changed local desired state".into());
        }
        rows.push(json!({"name":name,"object":object,"before":actual,"observed":actual,"desired":desired,"unknown":[]}));
    }
    restoration::receipt(
        &f.local_status,
        &f.local_plan,
        &ctx.binding.1,
        &f.local_after.copies,
        ctx.roles,
    )?;
    for raw in &f.local_status.observations {
        let observed: RecordedObserved =
            serde_json::from_value(raw.clone()).map_err(|_| "Invalid remote local observation")?;
        let col = ctx
            .binding
            .1
            .iter()
            .position(|id| id == &observed.provider)
            .ok_or("Unknown local provider")?;
        let satisfied = if !observed.required {
            observed.state == "not_started" || observed.state == "authorized_absence"
        } else if ctx.roles[col].id == "backup" {
            observed.state == "retained"
        } else {
            matches!(
                observed.state.as_str(),
                Some("published" | "retained" | "released")
            )
        };
        if !satisfied {
            return Err("Unsatisfied local copy in converged report".into());
        }
    }
    if !f.local_status.page_converged
        || f.local_status.storage_converged != Some(true)
        || !f.local_status.holds.is_empty()
        || !f.local_status.effects.is_empty()
        || f.local_status.stream_bytes != 0
    {
        return Err("Remote activity masquerades as local placement".into());
    }
    let deltas = [(0, 0, 0), (16, 108, 54), (4, 54, 0), (8, 108, 0)];
    let (req, get, put) = deltas[step];
    if after.requests.checked_sub(before.requests) != Some(req)
        || after
            .get_payload_bytes
            .checked_sub(before.get_payload_bytes)
            != Some(get)
        || after
            .put_payload_bytes
            .checked_sub(before.put_payload_bytes)
            != Some(put)
        || before.corrupt_response_injected != (step == 2)
        || after.corrupt_response_injected != (step == 2)
    {
        return Err("Remote IO counter mismatch".into());
    }
    let mut io = None;
    if step == 0 || step == 2 {
        let code = if step == 0 {
            "upload_disabled"
        } else {
            "integrity"
        };
        for v in f.cli.apply {
            let failure: Failure = serde_json::from_value(v).map_err(|_| "Invalid CLI refusal")?;
            if !failure.is(code) {
                return Err("Wrong upload refusal".into());
            }
        }
        if f.cli.status.is_some()
            || !before.content_eq(after)
            || f.witness_before != f.witness_after
        {
            return Err("Failed upload mutated custody".into());
        }
        if step == 0 {
            if !f
                .cli
                .unauthorized_plan
                .as_ref()
                .is_some_and(|f| f.is("authority"))
            {
                return Err("Missing unauthorized OID refusal".into());
            }
        } else if f.cli.unauthorized_plan.is_some() {
            return Err("Mixed upload refusal evidence".into());
        }
    } else {
        let statuses = f.cli.status.ok_or("Missing independent remote status")?;
        if statuses.len() != 2 || f.cli.unauthorized_plan.is_some() {
            return Err("Invalid remote completion scope".into());
        }
        let mut total = 0u64;
        for ((raw, status), p) in f.cli.apply.into_iter().zip(statuses).zip(&plans) {
            let r: Uploaded = serde_json::from_value(raw).map_err(|_| "Invalid upload receipt")?;
            let id = p.id().map_err(|_| "Invalid upload id")?;
            if r.schema != "tig.lfs-upload-receipt.design-1"
                || r.plan != id
                || r.object != p.object
                || r.payload_io_bytes < p.object.bytes
                || r.payload_io_bytes > p.budget.transfer_bytes
                || r.retained_custody
                || r.removal_authority
                || status.schema != "tig.lfs-upload-status.design-1"
                || status.plan != id
                || !status.historical_completion
                || !status.intent_recorded
                || !status.accounting_settled
                || !status.verified_present
                || status.requires_recovery
                || status.retained_custody
                || status.removal_authority
            {
                return Err("Inconsistent upload completion/status".into());
            }
            total = total
                .checked_add(r.payload_io_bytes)
                .ok_or("Remote IO overflow")?;
        }
        io = Some(total);
        if step == 1 {
            if before.put_count != 0
                || after.put_count != 2
                || after.objects != f.selected
                || after.registered
                    != f.selected
                        .iter()
                        .map(|o| o.sha256.clone())
                        .collect::<Vec<_>>()
                || after.checkpoint.sequence() <= before.checkpoint.sequence()
                || f.witness_after.sequence() <= f.witness_before.sequence()
            {
                return Err("Incomplete remote publication".into());
            }
        } else if !before.content_eq(after) || f.witness_before != f.witness_after {
            return Err("Recheck recreated remote publication".into());
        }
    }
    if step > 1
        && (after.objects != f.selected || after.put_count != 2 || after.put_payload_bytes != 54)
    {
        return Err("Remote retained physical set mismatch".into());
    }
    let remote = json!({"phase":f.phase,"receiver":after.store,"objects":after.objects,"readback_state":if step==0{"not_started"}else if step==2{"held"}else{"verified"},"held_reason":if step==2{Some("integrity")}else{None},"fixture_input_staging_bytes":54,"put_payload_bytes_this_step":put,"put_payload_bytes_total":after.put_payload_bytes,"put_count_total":after.put_count,"readback_payload_bytes_this_step":get,"http_requests_this_step":req,"client_body_io_bytes":io,"retained_custody":false,"removal_authority":false,"synthetic_endpoint":true});
    let lesson = json!({"id":ctx.entry.lesson,"title":ctx.entry.title,"event":ctx.entry.event,"report":ctx.entry.report,"plan":local_id,"source_commit":f.source_commit,"source_available":ctx.commits.contains(&f.source_commit),"source_sequence":f.source_checkpoint.sequence(),"objects":rows,"stream_bytes":0,"basis_current":true,"converged":true,"storage_converged":true,"remaining_page":false,"holds":[],"effects":[],"remote_publication":remote});
    Ok((
        lesson,
        Token {
            step,
            proof,
            report: ctx.entry.report.clone(),
            endpoint: f.endpoint_after,
            witness: f.witness_after,
            finished: step == 3,
        },
    ))
}
