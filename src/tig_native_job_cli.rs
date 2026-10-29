//! Explicit frozen native jobs through the existing bounded child runner.
use crate::{
    media_runtime::{Bindings, BoundPrograms},
    tig_inputs::ArchiveProfile,
    tig_job_runner,
    tig_scene::{self, Request},
    tig_scene_production::{
        self,
        jobs::{Coordination, Identity, JobPlan},
    },
};
use anyhow::{Result, bail};
use reel_assets::{Config, jobs};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tig_core::{Digest, LogicalPath, Object, identity, tree::TreeLimits};
use tig_store::{
    Key,
    metadata::{Checkpoint, MetadataReader, MetadataStore},
    source_claims::{self, Selection, SourceJournal},
};

const PROFILE_SCHEMA: &str = "reel.tig-native-job-profile.experimental-1";
const TICKET_SCHEMA: &str = "reel.tig-native-worker.experimental-1";
mod capture;
mod comparison;
mod metadata_wait;
pub use comparison::compare_retained;
mod handoff;
mod planning;
mod preparation;
mod publication;
pub use publication::publish_file;
mod records;
mod retained;
mod setup;
pub use capture::capture_context;
use preparation::ContextMode;
pub use setup::{check_stores, setup_stores};
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Plan,
    Render,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Worker {
    #[serde(default, skip_serializing_if = "ContextMode::is_declared")]
    context_mode: ContextMode,
    metadata_root: PathBuf,
    metadata_minimum: Checkpoint,
    source: Value,
    context: Selection,
    request: Request,
    runtime: Bindings,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    outputs: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    output_budget: Option<planning::Budget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    record_outputs: Vec<records::Spec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    git_handoff: Option<handoff::Handoff>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ticket {
    schema: String,
    mode: Mode,
    admission: Worker,
    recipe: Digest,
    producer: Object,
    #[serde(default)]
    output_roots: Vec<PathBuf>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    #[serde(default)]
    preparation_cache: Option<PathBuf>,
    schema: String,
    worker: Worker,
    cache: Config,
    journal_root: PathBuf,
    journal_minimum: source_claims::Checkpoint,
    source_id: String,
    owner: String,
    expected_revision: u64,
    start_operation: String,
    retain_operation: String,
    budget_bytes: u64,
    scratch_bytes: u64,
    timeout_seconds: u64,
}
fn read<T: DeserializeOwned>(path: &Path, max: usize, expected: Option<&Object>) -> Result<T> {
    Ok(serde_json::from_value(identity::parse_json(&read_bytes(
        path, max, expected,
    )?)?)?)
}
fn read_bytes(path: &Path, max: usize, expected: Option<&Object>) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > max {
        bail!("native job intake exceeds budget");
    }
    if expected.is_some_and(|pin| *pin != Object::from_bytes(&bytes)) {
        bail!("native worker admission bytes differ");
    }
    Ok(bytes)
}
fn pin(programs: &BoundPrograms) -> Result<Object> {
    Ok(Object {
        sha256: Digest::parse(&programs.identity().sha256)?,
        bytes: programs.identity().bytes,
    })
}
fn restore(
    worker: &Worker,
    output: Option<&Path>,
) -> Result<(
    crate::tig_inputs::Source,
    tig_git::source::native::context::ProductionContext,
)> {
    let profile: ArchiveProfile = serde_json::from_value(worker.source.clone())?;
    if !worker.metadata_root.is_absolute() {
        bail!("existing absolute metadata root required");
    }
    let mut reader = MetadataReader::open(&worker.metadata_root, &worker.metadata_minimum)?;
    if worker.context_mode == ContextMode::Compiled {
        return tig_scene::restore_render_context(
            profile,
            &worker.request,
            &worker.context,
            output
                .ok_or_else(|| anyhow::anyhow!("compiled restore requires admitted preparation"))?,
            &mut reader,
            TreeLimits::default(),
        );
    }
    tig_scene::restore_context(
        profile,
        &worker.request,
        &worker.context,
        &mut reader,
        TreeLimits::default(),
    )
}
fn job_plan<'a>(
    mode: ContextMode,
    request: &'a Request,
    source: &mut crate::tig_inputs::Source,
    context: &'a tig_git::source::native::context::ProductionContext,
    programs: &'a BoundPrograms,
    output: Option<&Path>,
) -> Result<JobPlan<'a>> {
    match mode {
        ContextMode::Declared => JobPlan::from_context(source, request, context, programs),
        ContextMode::Compiled => JobPlan::from_render_context(
            source,
            request,
            context,
            programs,
            output.ok_or_else(|| {
                anyhow::anyhow!("compiled planning requires admitted preparation")
            })?,
        ),
    }
}
fn output_names(worker: &Worker) -> Result<BTreeSet<String>> {
    if let Some(budget) = &worker.output_budget {
        if !worker.outputs.is_empty() {
            bail!("native output map and budget policy are mutually exclusive");
        }
        budget.validate()?;
        return Ok(BTreeSet::new());
    }
    if worker.outputs.is_empty() || worker.outputs.len() > 511 {
        bail!("native output set exceeds budget");
    }
    for (name, bytes) in &worker.outputs {
        LogicalPath::parse(name)?;
        if *bytes == 0 || name == "worker.json" {
            bail!("invalid native output budget or reserved name");
        }
    }
    for name in [
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
        "receipt.json",
    ] {
        if !worker.outputs.contains_key(name) {
            bail!("native output set omits required output");
        }
    }
    Ok(worker.outputs.keys().cloned().collect())
}
/// No initialization or discovery: roots and trusted minimum checkpoints exist.
pub fn run(path: &Path) -> Result<Value> {
    execute(path, false)
}
/// A retained bounded planning job, not a read-only/dry-run command.
pub fn plan_outputs(path: &Path) -> Result<Value> {
    execute(path, true)
}
/// Read declared profile and named filesystem geometry only. No cache census,
/// reservation, workspace creation, source/metadata opens or runtime execution.
pub fn preparation_budget(path: &Path) -> Result<Value> {
    let profile: Profile = read(path, 128 * 1024, None)?;
    if profile.schema != PROFILE_SCHEMA {
        bail!("unsupported native job profile");
    }
    if profile.cache.cache_roots.is_empty()
        || profile
            .cache
            .cache_roots
            .iter()
            .any(|p| !p.is_absolute() || !p.join("objects/sha256").is_dir())
    {
        bail!("existing active CAS roots required");
    }
    preparation::report(&profile)
}
fn execute(path: &Path, only_plan: bool) -> Result<Value> {
    let mut profile: Profile = read(path, 128 * 1024, None)?;
    if profile.schema != PROFILE_SCHEMA {
        bail!("unsupported native job profile");
    }
    output_names(&profile.worker)?;
    let automatic = profile.worker.output_budget.is_some();
    if only_plan && !automatic {
        bail!("plan-outputs requires an output budget policy");
    }
    let coordination = Coordination {
        root: profile
            .cache
            .coordination_root
            .clone()
            .ok_or_else(|| anyhow::anyhow!("existing shared coordination root required"))?,
    };
    if !coordination.root.is_absolute() || !coordination.root.is_dir() {
        bail!("existing shared coordination root required");
    }
    if profile.cache.cache_roots.is_empty()
        || profile
            .cache
            .cache_roots
            .iter()
            .any(|p| !p.is_absolute() || !p.join("objects/sha256").is_dir())
    {
        bail!("existing active CAS roots required");
    }
    let identity = Identity {
        source: Key::parse(&profile.source_id)?,
        owner: Key::parse(&profile.owner)?,
    };
    let start = Key::parse(&profile.start_operation)?;
    let retain = Key::parse(&profile.retain_operation)?;
    if !profile.journal_root.is_absolute() {
        bail!("existing absolute journal root required");
    }
    let mut journal = SourceJournal::open(&profile.journal_root, &profile.journal_minimum)?;
    if let Some(state) = journal.source(&identity.source)? {
        if state.owner != identity.owner
            || state.revision != profile.expected_revision
            || state.pending.is_some()
        {
            bail!("native job owner/revision/pending state differs");
        }
    } else if profile.expected_revision != 0 {
        bail!("native job source missing");
    }
    let allowance = preparation::worker_allowance(&profile)?;
    if profile.scratch_bytes < allowance {
        bail!("compiled worker scratch below preparation allowance");
    }
    if profile.worker.context_mode == ContextMode::Compiled
        && profile
            .scratch_bytes
            .checked_add(1024 * 1024)
            .is_none_or(|n| n > profile.budget_bytes)
    {
        bail!("compiled workspace budget below scratch and metadata allowance");
    }
    let programs = BoundPrograms::bind(&profile.worker.runtime)?;
    let prepared = preparation::Preparation::open(&profile)?;
    let restored = prepared.as_ref().map(|p| p.path().join("source"));
    let planned = prepared.as_ref().map(|p| p.path().join("recipe"));
    let (mut source, context) = restore(&profile.worker, restored.as_deref())?;
    let templates = records::prepare(&profile.worker, &context, &profile.cache.cache_roots)?;
    let plan = job_plan(
        profile.worker.context_mode,
        &profile.worker.request,
        &mut source,
        &context,
        &programs,
        planned.as_deref(),
    )?;
    if let Some(prepared) = prepared {
        prepared.finish()?;
    }
    let mut spec = Phase {
        mode: if automatic { Mode::Plan } else { Mode::Render },
        expected_revision: profile.expected_revision,
        start,
        retain,
        minimum: profile.worker.metadata_minimum.clone(),
    };
    if automatic {
        spec.start = phase_operation(&identity, &spec.start, "plan")?;
        spec.retain = phase_operation(&identity, &spec.retain, "plan")?;
    }
    let first = execute_phase(
        &profile,
        &plan,
        &programs,
        &mut journal,
        &coordination,
        &identity,
        &spec,
    )?;
    if !automatic {
        let exported = records::readback(&templates, &profile, &plan, &first, &mut journal)?;
        let mut result = summary(&first.value, &journal);
        if let Some(exported) = exported {
            result["record_candidates"] = exported;
        }
        return Ok(result);
    }
    let (report, minimum) = planning::readback(&profile, &plan, &programs, &first, &mut journal)?;
    if only_plan {
        return Ok(
            json!({"schema":"reel.tig-native-output-plan-result.experimental-1","result":first.value,"planning":report,"journal_checkpoint":journal.checkpoint(),"production_claim":null,"complete_claim_coverage":false}),
        );
    }
    profile.worker.outputs = report.outputs.clone();
    profile.worker.output_budget = None;
    spec = Phase {
        mode: Mode::Render,
        expected_revision: first.revision,
        start: phase_operation(&identity, &Key::parse(&profile.start_operation)?, "render")?,
        retain: phase_operation(&identity, &Key::parse(&profile.retain_operation)?, "render")?,
        minimum,
    };
    let rendered = execute_phase(
        &profile,
        &plan,
        &programs,
        &mut journal,
        &coordination,
        &identity,
        &spec,
    )?;
    let mut result = summary(&rendered.value, &journal);
    if let Some(exported) = records::readback(&templates, &profile, &plan, &rendered, &mut journal)?
    {
        result["record_candidates"] = exported;
    }
    result["planning"] = serde_json::to_value(report)?;
    result["planning_result"] = first.value;
    Ok(result)
}
fn summary(result: &Value, journal: &SourceJournal) -> Value {
    json!({"schema":"reel.tig-native-job-result.experimental-1","result":result,"journal_checkpoint":journal.checkpoint(),"production_claim":null,"complete_claim_coverage":false})
}
fn phase_operation(identity: &Identity, operation: &Key, label: &str) -> Result<Key> {
    Ok(Key::parse(Digest::blob(&identity::canonical_json(&json!({"schema":"reel.native-output-planning-operation.experimental-1","source":identity.source.as_str(),"operation":operation.as_str(),"phase":label}))?).as_str())?)
}
struct Phase {
    mode: Mode,
    expected_revision: u64,
    start: Key,
    retain: Key,
    minimum: Checkpoint,
}
struct PhaseResult {
    value: Value,
    revision: u64,
    resources: Selection,
    minimum: Checkpoint,
}
fn execute_phase(
    profile: &Profile,
    plan: &JobPlan<'_>,
    programs: &BoundPrograms,
    journal: &mut SourceJournal,
    coordination: &Coordination,
    identity: &Identity,
    spec: &Phase,
) -> Result<PhaseResult> {
    let ticket = serde_json::to_string(
        &json!({"schema":TICKET_SCHEMA,"mode":spec.mode,"admission":profile.worker,"recipe":plan.recipe().fingerprint(),"producer":pin(programs)?,"output_roots":if profile.worker.git_handoff.is_some() {profile.cache.cache_roots.clone()} else {vec![]}}),
    )?;
    if ticket.len() > 65_536 {
        bail!("native worker admission exceeds budget");
    }
    let ticket_pin = Object::from_bytes(ticket.as_bytes());
    let scratch_budget =
        if spec.mode == Mode::Plan && profile.worker.context_mode == ContextMode::Declared {
            planning::scratch_budget(profile.scratch_bytes)
        } else {
            profile.scratch_bytes
        };
    let workspace_budget =
        if spec.mode == Mode::Plan && profile.worker.context_mode == ContextMode::Declared {
            planning::workspace_budget(
                profile.scratch_bytes,
                ticket.len() as u64,
                profile.budget_bytes,
            )?
        } else {
            profile.budget_bytes
        };
    let mut outputs: BTreeMap<_, _> = if spec.mode == Mode::Plan {
        BTreeMap::from([(
            "planning.json".into(),
            json!({"path":"planning.json","max_bytes":65_536}),
        )])
    } else {
        profile
            .worker
            .outputs
            .iter()
            .map(|(name, max)| {
                (
                    format!("native/{name}"),
                    json!({"path":format!("native/{name}"),"max_bytes":max}),
                )
            })
            .chain(std::iter::once((
                "worker.json".into(),
                json!({"path":"worker.json","max_bytes":65_536}),
            )))
            .collect()
    };
    if spec.mode == Mode::Render {
        records::check_names(&profile.worker, &output_names(&profile.worker)?)?;
        for (name, max) in records::artifacts(&profile.worker)? {
            outputs.insert(name.clone(), json!({"path":name,"max_bytes":max}));
        }
    }
    let request = serde_json::to_vec(&json!({
        "output_count_limit":512,"budget_bytes":workspace_budget,"scratch_bytes":scratch_budget,
        "inputs":{},"tools":{},"installed_tools":{"worker":{"path":std::env::current_exe()?.canonicalize()?,"sha256":profile.worker.runtime.host.sha256,"bytes":profile.worker.runtime.host.bytes}},
        "files":{"worker-input.json":ticket},"prepare_output_parents":false,
        "steps":[{"tool":"worker","args":["worker","worker-input.json","--sha256",ticket_pin.sha256,"--bytes",ticket_pin.bytes.to_string()],"timeout_seconds":profile.timeout_seconds}],"outputs":outputs
    }))?;
    jobs::check_request(&request)
        .map_err(|e| anyhow::anyhow!("native job request refused: {e}"))?;
    // Validation above precedes all metadata and source-journal transitions.
    let mut store = MetadataStore::open(&profile.worker.metadata_root, &spec.minimum)?;
    let frozen = plan.freeze(&mut store, TreeLimits::default())?;
    let minimum = store.finish()?;
    drop(store);
    let mut reader = MetadataReader::open(&profile.worker.metadata_root, &minimum)?;
    let ready = plan.readback(frozen.selection(), &mut reader, TreeLimits::default())?;
    let resources = plan.readback(frozen.selection(), &mut reader, TreeLimits::default())?;
    let protected = ready.protect(
        journal,
        coordination,
        identity.clone(),
        spec.expected_revision,
        &spec.start,
    )?;
    let revision = protected.revision();
    drop(protected);
    let mut manifest = tempfile::Builder::new()
        .prefix("tig-native-request-")
        .tempfile_in(&coordination.root)?;
    manifest.write_all(&request)?;
    manifest.as_file().sync_all()?;
    let result = tig_job_runner::run(
        &coordination.root,
        manifest.path(),
        resources.resources(),
        journal,
        &tig_job_runner::Profile {
            config: &profile.cache,
            coordination,
            identity: identity.clone(),
            expected_revision: revision,
            operation: spec.retain.clone(),
            metadata_root: &profile.worker.metadata_root,
            metadata_minimum: &minimum,
            tree_limits: TreeLimits::default(),
        },
    )?;
    let completed = revision
        .checked_add(2)
        .ok_or_else(|| anyhow::anyhow!("native phase revision overflow"))?;
    let state = journal
        .source(&identity.source)?
        .ok_or_else(|| anyhow::anyhow!("native phase source missing"))?;
    if state.revision != completed || state.owner != identity.owner || state.pending.is_some() {
        bail!("native phase handoff changed");
    }
    Ok(PhaseResult {
        value: result,
        revision: completed,
        resources: frozen.selection().clone(),
        minimum,
    })
}
/// Internal trusted child. Coordinator supplies containment and durable claims.
/// Calling this directly does not perform admission or establish retention.
pub fn worker(path: &Path, expected: &Object) -> Result<Value> {
    let ticket: Ticket = read(path, 65_536, Some(expected))?;
    if ticket.schema != TICKET_SCHEMA {
        bail!("unsupported native worker admission");
    }
    let expected = output_names(&ticket.admission)?;
    if ticket.mode == Mode::Render && ticket.admission.output_budget.is_some() {
        bail!("render requires derived output contract");
    }
    let programs = BoundPrograms::bind(&ticket.admission.runtime)?;
    if pin(&programs)? != ticket.producer {
        bail!("native worker runtime differs");
    }
    let (mut source, context) = restore(&ticket.admission, Some(Path::new("source-context")))?;
    let record_templates = records::prepare(&ticket.admission, &context, &ticket.output_roots)?;
    let plan = job_plan(
        ticket.admission.context_mode,
        &ticket.admission.request,
        &mut source,
        &context,
        &programs,
        Some(Path::new("recipe-preparation")),
    )?;
    if plan.recipe().fingerprint() != &ticket.recipe {
        bail!("native worker recipe differs");
    }
    if ticket.mode == Mode::Plan {
        return planning::produce(&ticket, &mut source, &context, &programs);
    }
    records::check_names(&ticket.admission, &expected)?;
    let render = match ticket.admission.context_mode {
        ContextMode::Declared => tig_scene_production::render_with_programs_and_outputs,
        ContextMode::Compiled => tig_scene_production::render_projected_with_programs_and_outputs,
    };
    let candidate = render(
        &mut source,
        &ticket.admission.request,
        &context,
        &programs,
        Path::new("compiled"),
        Path::new("native"),
        Some(&expected),
    )?;
    let actual: BTreeSet<_> = candidate
        .receipt()
        .outputs
        .keys()
        .cloned()
        .chain(std::iter::once("receipt.json".into()))
        .collect();
    if actual != expected {
        bail!("native worker output set differs from admission");
    }
    for (name, max) in &ticket.admission.outputs {
        if fs::metadata(Path::new("native").join(name))?.len() > *max {
            bail!("native worker output exceeds declared size");
        }
    }
    let exported = records::emit(&record_templates, &candidate)?;
    let result = json!({"schema":"reel.tig-native-worker-result.experimental-1","context":ticket.admission.context,"recipe":candidate.recipe().fingerprint(),"producer":ticket.producer,"receipt":candidate.receipt_object(),"record_candidates":exported,"production_claim":null});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open("worker.json")?;
    file.write_all(&serde_json::to_vec(&result)?)?;
    file.sync_all()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changed_worker_ticket_refuses_before_runtime_or_metadata_lookup() {
        let root = tempfile::tempdir().unwrap();
        let ticket = root.path().join("ticket.json");
        fs::write(&ticket, b"{}").unwrap();
        let expected = Object::from_bytes(b"original private coordinator ticket");
        let error = worker(&ticket, &expected).unwrap_err().to_string();
        assert!(error.contains("admission bytes differ"), "{error}");
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
    }
}
