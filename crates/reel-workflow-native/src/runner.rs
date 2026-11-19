//! Bounded, resumable recipe admission. Provider/worker execution stays explicit.
use crate::{
    NativeProfile, admission,
    advancement::{self, persist, raw},
    dispatch, error, expansion, graph, handoff, io_error, staging,
};
use reel_workflow::{
    ErrorKind, Plan, Result,
    recipe::{Recipe, Schedule},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub source: NativeProfile,
    pub admission: admission::Profile,
    pub attempt_id: String,
    pub handoff: advancement::Config,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<dispatch::Config>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: String,
    pub steps: BTreeMap<String, Step>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema: String,
    sequence: usize,
    previous_sha256: Option<String>,
    pending_sha256: Option<String>,
    plan: Plan,
    bindings: graph::Bindings,
}
#[derive(Serialize)]
pub struct Summary {
    pub schema: String,
    pub advanced: Vec<String>,
    pub dispatched: Vec<dispatch::Summary>,
    pub checkpoint: usize,
    pub schedule: Schedule,
}
fn read(path: &Path) -> Result<Vec<u8>> {
    let (bytes, _held) = staging::read_held(path, reel_workflow::MAX_CONTROL_BYTES as u64)?;
    Ok(bytes)
}
fn state_path(root: &Path, sequence: usize) -> std::path::PathBuf {
    root.join(format!("state-{sequence:03}.json"))
}
fn pending_path(root: &Path, sequence: usize) -> std::path::PathBuf {
    root.join(format!("pending-{sequence:03}.json"))
}
fn capsule(root: &Path, sequence: usize) -> std::path::PathBuf {
    root.join(format!("task-{sequence:03}"))
}
fn same(a: &impl Serialize, b: &impl Serialize) -> Result<bool> {
    Ok(reel_workflow::canonical_digest(a)? == reel_workflow::canonical_digest(b)?)
}

/// One call admits at most `limit` tasks. Re-running observes the current native
/// frontier; a Waiting/Cancelled/uncertain task is never a fresh attempt.
pub fn advance(
    recipe: &Recipe,
    base: &Plan,
    bindings: &graph::Bindings,
    config: &Config,
    root: &Path,
    limit: usize,
) -> Result<Summary> {
    reel_workflow::recipe::check(recipe)?;
    if !(1..=16).contains(&limit)
        || config.schema != "reel.workflow-recipe-runner.v1"
        || config.steps.len() != recipe.steps.len()
        || !same(base, &recipe.base)?
        || recipe
            .steps
            .iter()
            .any(|s| !config.steps.contains_key(&s.id))
    {
        return Err(error(ErrorKind::Invalid, "runner-configuration"));
    }
    // Validate every declared output namespace before the first admission.
    if !root.is_absolute()
        || root
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(error(ErrorKind::Invalid, "runner-output-path"));
    }
    let parent = reel_assets::jobs::open_workspace_directory(
        root.parent()
            .ok_or_else(|| error(ErrorKind::Invalid, "runner-output-parent"))?,
    )
    .map_err(|_| error(ErrorKind::Denied, "runner-output-parent"))?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| staging::portable(n))
        .ok_or_else(|| error(ErrorKind::Invalid, "runner-output-name"))?;
    let root = parent.path().join(name);
    let mut operations = std::collections::BTreeSet::new();
    for (id, step) in &config.steps {
        if let Some(execution) = &step.execution {
            let authored = recipe
                .steps
                .iter()
                .find(|s| &s.id == id)
                .ok_or_else(|| error(ErrorKind::Invalid, "runner-configuration"))?;
            execution.check_operation(
                authored.operation,
                authored.tool.as_ref().map(|t| t.capability),
            )?;
        }
        if step.source.snapshot.upstream.is_some()
            || step.source.snapshot.schema != "reel.workflow-native-snapshot.v1"
            || step.source.snapshot.commit != base.source_commit
            || step.attempt_id.is_empty()
            || step.attempt_id.len() > 128
            || !staging::portable(&step.attempt_id)
            || step.attempt_id.contains('/')
            || !operations.insert((
                step.admission.index_identity.clone(),
                step.admission.scope.actor.as_str().to_owned(),
                step.admission.scope.operation.as_str().to_owned(),
            ))
        {
            return Err(error(ErrorKind::Invalid, "runner-step-identity"));
        }
        drop(handoff::runtime(&step.handoff.runtime)?);
        staging::check_parent(parent.path(), &step.source, &step.admission)?;
        if parent.path().starts_with(
            step.source
                .snapshot
                .repository
                .canonicalize()
                .map_err(io_error)?,
        ) {
            return Err(error(ErrorKind::Denied, "runner-output-inside-source"));
        }
    }
    for binding in bindings.tasks.values() {
        let (bytes, _held) = binding.profile.read()?;
        let profile: handoff::Profile = reel_workflow::decode(&bytes)?;
        staging::check_parent(parent.path(), &profile.source, &profile.admission)?;
        for control in [
            &binding.journal,
            &profile.acknowledgements,
            &profile.source.snapshot.repository,
        ] {
            if parent
                .path()
                .starts_with(control.canonicalize().map_err(io_error)?)
            {
                return Err(error(ErrorKind::Denied, "runner-output-overlaps-controls"));
            }
        }
    }
    let first = config
        .steps
        .values()
        .next()
        .ok_or_else(|| error(ErrorKind::Invalid, "runner-empty-configuration"))?;
    let _writer = reel_assets::admission::lock(
        &first
            .admission
            .scope
            .coordination_root
            .join("workflow-recipe-runners")
            .join(reel_workflow::digest(root.to_string_lossy().as_bytes())),
    )
    .map_err(|_| error(ErrorKind::Busy, "runner-writer-busy"))?;
    let created = match fs::create_dir(&root) {
        Ok(()) => true,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(e) => return Err(io_error(e)),
    };
    let owned = reel_assets::jobs::open_workspace_directory(&root)
        .map_err(|_| error(ErrorKind::Denied, "runner-output-namespace"))?;
    let root = owned.path();
    let intent_path = root.join("intent.json");
    if !created && !intent_path.exists() {
        return Err(error(
            ErrorKind::NeedsRecovery,
            "runner-original-intent-missing",
        ));
    }
    persist(
        &intent_path,
        &raw(
            &serde_json::json!({"schema":"reel.workflow-recipe-runner-intent.v1","recipe":reel_workflow::canonical_digest(recipe)?,"base":reel_workflow::canonical_digest(base)?,"bindings":reel_workflow::canonical_digest(bindings)?,"config":reel_workflow::canonical_digest(config)?,"directory_identity":owned.identity()}),
        )?,
    )?;
    let initial = State {
        schema: "reel.workflow-recipe-state.v1".into(),
        sequence: 0,
        previous_sha256: None,
        pending_sha256: None,
        plan: base.clone(),
        bindings: reel_workflow::decode(&raw(bindings)?)?,
    };
    persist(&state_path(root, 0), &raw(&initial)?)?;
    let mut state = initial;
    let mut state_raw = read(&state_path(root, 0))?;
    let mut total = state_raw.len();
    // Bounded scan rejects gaps and surplus state identities; no directory walk
    // reaches a payload store or arbitrary returned package.
    let mut count = 0usize;
    let mut state_count = 0usize;
    for entry in fs::read_dir(root).map_err(io_error)? {
        let name = entry.map_err(io_error)?.file_name();
        count += 1;
        if count > 512 {
            return Err(error(ErrorKind::Budget, "runner-control-entry-count"));
        }
        if name.to_str().is_some_and(|n| n.starts_with("state-")) {
            state_count += 1;
        }
    }
    if state_count > 129 {
        return Err(error(ErrorKind::Budget, "runner-state-count"));
    }
    for sequence in 1..state_count {
        let bytes = read(&state_path(root, sequence))?;
        total += bytes.len();
        if total > 32 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "runner-history-bytes"));
        }
        let next: State = reel_workflow::decode(&bytes)?;
        let pending = read(&pending_path(root, sequence))?;
        let frozen: expansion::Frozen = reel_workflow::decode(&pending)?;
        frozen.check(&state.plan)?;
        if next.schema != state.schema
            || next.sequence != sequence
            || next.previous_sha256.as_ref() != Some(&reel_workflow::digest(&state_raw))
            || next.pending_sha256.as_ref() != Some(&reel_workflow::digest(&pending))
            || !same(&frozen.bindings, &state.bindings)?
            || !same(&next.plan, &frozen.expansion.plan)?
            || !same(
                &next.bindings,
                &reel_workflow::decode::<graph::Bindings>(&read(
                    &capsule(root, sequence).join("bindings.json"),
                )?)?,
            )?
        {
            return Err(error(ErrorKind::Integrity, "runner-state-chain"));
        }
        state = next;
        state_raw = bytes;
    }
    let mut dispatched = Vec::new();
    let mut resumed = 0usize;
    for sequence in 1..=state.sequence {
        if resumed >= limit {
            break;
        }
        let frozen: expansion::Frozen =
            reel_workflow::decode(&read(&pending_path(root, sequence))?)?;
        let id = &frozen.expansion.packet.task.id;
        let Some(execution) = config.steps.get(id).and_then(|s| s.execution.as_ref()) else {
            continue;
        };
        let folder = capsule(root, sequence);
        let prior = reel_workflow::handoff::Journal::open(&folder.join("journal"))?.state();
        if matches!(
            prior,
            reel_workflow::handoff::State::Retained | reel_workflow::handoff::State::Cancelled
        ) || (prior == reel_workflow::handoff::State::ImportPending
            && execution.worker_staging.is_none()
            && execution.provider.is_none())
        {
            continue;
        }
        let plan: Plan = reel_workflow::decode(&read(&folder.join("plan.json"))?)?;
        let bindings: graph::Bindings =
            reel_workflow::decode(&read(&folder.join("bindings.json"))?)?;
        let result = dispatch::run(&plan, &bindings, id, execution, &folder.join("execution"))?;
        if !result.already_ready
            || execution.worker_staging.is_some()
            || execution.provider.is_some()
        {
            resumed += 1;
        }
        dispatched.push(result);
    }
    let mut advanced = Vec::new();
    for _ in 0..limit {
        let sequence = state.sequence + 1;
        if sequence > 128 {
            break;
        }
        let path = pending_path(root, sequence);
        let frozen: expansion::Frozen = if path.exists() {
            reel_workflow::decode(&read(&path)?)?
        } else {
            let schedule = graph::schedule_recipe(&state.plan, &state.bindings, recipe, 1)?;
            let Some(proposal) = schedule.proposals.into_iter().next() else {
                break;
            };
            let step = &config.steps[&proposal.packet.task.id];
            let frozen = expansion::prepare(
                &state.plan,
                &state.bindings,
                &proposal.template,
                reel_workflow::decode(&raw(&step.source)?)?,
                step.admission.clone(),
                &step.attempt_id,
            )?;
            frozen.export(&path)?;
            frozen
        };
        frozen.check(&state.plan)?;
        if !same(&state.bindings, &frozen.bindings)? {
            return Err(error(ErrorKind::Conflict, "runner-pending-base-bindings"));
        }
        let step = config
            .steps
            .get(&frozen.expansion.packet.task.id)
            .ok_or_else(|| error(ErrorKind::Conflict, "runner-pending-task"))?;
        if !same(&frozen.admission, &step.admission)? || frozen.attempt_id != step.attempt_id {
            return Err(error(ErrorKind::Conflict, "runner-pending-operation"));
        }
        let mut source: NativeProfile = reel_workflow::decode(&raw(&frozen.source)?)?;
        source.snapshot.schema = step.source.snapshot.schema.clone();
        source.snapshot.upstream = None;
        if !same(&source, &step.source)? {
            return Err(error(ErrorKind::Conflict, "runner-pending-source"));
        }
        let authored = recipe
            .steps
            .iter()
            .find(|s| s.id == frozen.expansion.packet.task.id)
            .ok_or_else(|| error(ErrorKind::Conflict, "runner-pending-template"))?;
        let template = reel_workflow::expansion::Template {
            schema: "reel.workflow-expansion-template.v1".into(),
            base_plan_sha256: reel_workflow::canonical_digest(&state.plan)?,
            task: authored.clone(),
        };
        let (expected, _parents) =
            graph::expansion_parents(&state.plan, &state.bindings, &template)?;
        let mut actual = frozen.expansion.packet.task.clone();
        actual.native_inputs = None;
        if actual != expected.packet.task
            || frozen.expansion.packet.dependencies != expected.packet.dependencies
        {
            return Err(error(ErrorKind::Conflict, "runner-pending-template"));
        }
        let folder = capsule(root, sequence);
        if total + reel_workflow::MAX_CONTROL_BYTES > 32 * 1024 * 1024 {
            return Err(error(ErrorKind::Budget, "runner-history-next-checkpoint"));
        }
        advancement::advance(&state.plan, &frozen, &step.handoff, &folder)?;
        let next = State {
            schema: state.schema.clone(),
            sequence,
            previous_sha256: Some(reel_workflow::digest(&state_raw)),
            pending_sha256: Some(reel_workflow::digest(&read(&path)?)),
            plan: reel_workflow::decode(&read(&folder.join("plan.json"))?)?,
            bindings: reel_workflow::decode(&read(&folder.join("bindings.json"))?)?,
        };
        let bytes = raw(&next)?;
        persist(&state_path(root, sequence), &bytes)?;
        if let Some(execution) = &step.execution {
            dispatched.push(dispatch::run(
                &next.plan,
                &next.bindings,
                &frozen.expansion.packet.task.id,
                execution,
                &folder.join("execution"),
            )?);
        }
        advanced.push(frozen.expansion.packet.task.id);
        total += bytes.len();
        state = next;
        state_raw = bytes;
    }
    let schedule = graph::schedule_recipe(&state.plan, &state.bindings, recipe, 1)?;
    Ok(Summary {
        schema: "reel.workflow-recipe-runner-summary.v1".into(),
        advanced,
        dispatched,
        checkpoint: state.sequence,
        schedule,
    })
}
