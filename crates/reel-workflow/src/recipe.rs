//! Bounded recipe frontier. Scheduling proposals grant no execution or custody.
use crate::{
    expansion::{self, Input, TaskTemplate, Template},
    graph::{Job, Observer, Status},
    *,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema: String,
    /// Immutable starting plan; later materialization only appends recipe steps.
    pub base: Plan,
    pub steps: Vec<TaskTemplate>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Existing,
    Ready,
    Deferred,
    Blocked,
    Refused,
}
#[derive(Serialize)]
pub struct Row {
    pub task_id: String,
    pub state: State,
    pub existing_status: Option<Status>,
    pub waiting_on: Vec<String>,
    pub error: Option<Error>,
}
#[derive(Serialize)]
pub struct Proposal {
    pub template: Template,
    pub packet: Packet,
    pub upstream: Option<upstream::Bindings>,
}
#[derive(Serialize)]
pub struct Schedule {
    pub schema: String,
    pub recipe_sha256: String,
    pub plan_sha256: String,
    pub rows: Vec<Row>,
    pub proposals: Vec<Proposal>,
    pub existing: Vec<graph::Row>,
}

fn inputs(task: &TaskTemplate) -> Vec<&Input> {
    let mut inputs: Vec<_> = task.inputs.iter().collect();
    if let Some(tool) = &task.tool {
        inputs.extend([&tool.publication, &tool.request]);
        inputs.extend(tool.sources.values());
        if let Some(part) = &tool.part {
            inputs.push(&part.plan);
        }
    }
    inputs
}
pub fn check(recipe: &Recipe) -> Result<()> {
    canonical_digest(recipe)?;
    check_plan(&recipe.base)?;
    if !matches!(
        recipe.schema.as_str(),
        "reel.workflow-recipe.v1" | "reel.workflow-recipe.v2"
    ) {
        return fail(ErrorKind::Invalid, "recipe-schema");
    }
    if recipe.steps.is_empty() || recipe.steps.len() + recipe.base.tasks.len() > MAX_TASKS {
        return fail(ErrorKind::Budget, "recipe-step-count");
    }
    let mut ids: BTreeSet<_> = recipe.base.tasks.iter().map(|t| t.id.as_str()).collect();
    for step in &recipe.steps {
        if let Some(streaming) = &step.streaming {
            if recipe.schema != "reel.workflow-recipe.v2" {
                return fail(ErrorKind::Invalid, "recipe-streaming-version");
            }
            streaming.validate()?;
            if !inputs(step)
                .iter()
                .any(|i| matches!(i, Input::Parent { .. }))
            {
                return fail(ErrorKind::Invalid, "recipe-streaming-without-parent-inputs");
            }
        }
        if !token(&step.id) || !ids.insert(&step.id) {
            return fail(ErrorKind::Conflict, "recipe-step-identity");
        }
        if step.inputs.len() > MAX_INPUTS
            || step.depends_on.len() > MAX_TASKS
            || step.prompt.len() > MAX_PROMPT_BYTES
            || step.language.as_ref().is_some_and(|l| !token(l))
        {
            return fail(ErrorKind::Invalid, "recipe-step-contract");
        }
        reel_publication::check_contracts(&step.outputs)?;
        match (&step.operation, &step.tool) {
            (Operation::DeterministicTool, Some(tool)) => {
                if !step.prompt.is_empty()
                    || !token(&tool.edition_id)
                    || tool.sources.len() > MAX_INPUTS
                    || tool
                        .sources
                        .keys()
                        .any(|path| !publication::portable_source(path))
                    || tool.part.as_ref().is_some_and(|p| {
                        tool.capability != ToolCapability::ComicRender || !token(&p.part_id)
                    })
                {
                    return fail(ErrorKind::Invalid, "recipe-tool-contract");
                }
                artifact(&tool.executable)?;
            }
            (Operation::DeterministicTool, None) | (_, Some(_)) => {
                return fail(ErrorKind::Invalid, "recipe-operation-tool");
            }
            (_, None) if step.prompt.trim().is_empty() => {
                return fail(ErrorKind::Invalid, "recipe-step-prompt");
            }
            _ => (),
        }
    }
    for step in &recipe.steps {
        let deps: BTreeSet<_> = step.depends_on.iter().map(String::as_str).collect();
        if deps.len() != step.depends_on.len()
            || deps.iter().any(|id| *id == step.id || !ids.contains(id))
        {
            return fail(ErrorKind::Invalid, "recipe-dependency-set");
        }
        for input in inputs(step) {
            match input {
                Input::Fixed { artifact: a } => artifact(a)?,
                Input::Parent { reference } => {
                    if !deps.contains(reference.task_id.as_str()) {
                        return fail(ErrorKind::Invalid, "recipe-undeclared-parent");
                    }
                    let (outputs, worker) = if let Some(parent) =
                        recipe.base.tasks.iter().find(|t| t.id == reference.task_id)
                    {
                        (&parent.outputs, parent.tool.is_some())
                    } else {
                        let parent = recipe
                            .steps
                            .iter()
                            .find(|t| t.id == reference.task_id)
                            .ok_or_else(|| Error {
                                kind: ErrorKind::Missing,
                                code: "recipe-parent".into(),
                            })?;
                        (&parent.outputs, parent.tool.is_some())
                    };
                    if match &reference.artifact {
                        upstream::Selector::Output { name } => !outputs.contains_key(name),
                        upstream::Selector::WorkerReceipt => !worker,
                    } {
                        return fail(ErrorKind::Invalid, "recipe-parent-selector");
                    }
                }
            }
        }
    }
    let mut resolved: BTreeSet<_> = recipe.base.tasks.iter().map(|t| &t.id).collect();
    loop {
        let before = resolved.len();
        for step in &recipe.steps {
            if step.depends_on.iter().all(|id| resolved.contains(id)) {
                resolved.insert(&step.id);
            }
        }
        if resolved.len() == ids.len() {
            break;
        }
        if resolved.len() == before {
            return fail(ErrorKind::Conflict, "recipe-dependency-cycle");
        }
    }
    Ok(())
}

struct Cached<'a, O> {
    delegate: &'a mut O,
    jobs: BTreeMap<String, Option<Job>>,
    verified: BTreeSet<String>,
    bytes: usize,
}
impl<O: Observer> Observer for Cached<'_, O> {
    fn observe(&mut self, id: &str) -> Result<Option<Job>> {
        if let Some(job) = self.jobs.get(id) {
            return Ok(job.clone());
        }
        let job = self.delegate.observe(id)?;
        if let Some(job) = &job {
            let raw = serde_json::to_vec(&(&job.ticket, &job.result)).map_err(|_| Error {
                kind: ErrorKind::Invalid,
                code: "recipe-observation-json".into(),
            })?;
            self.bytes = self.bytes.checked_add(raw.len()).ok_or_else(|| Error {
                kind: ErrorKind::Budget,
                code: "recipe-observation-budget".into(),
            })?;
            if self.bytes > 32 * MAX_CONTROL_BYTES {
                return fail(ErrorKind::Budget, "recipe-observation-budget");
            }
        }
        self.jobs.insert(id.into(), job.clone());
        Ok(job)
    }
    fn verify(&mut self, job: &Job) -> Result<()> {
        let id = &job.ticket.packet.task.id;
        if self.jobs.get(id).and_then(Option::as_ref) != Some(job) {
            return fail(ErrorKind::Stale, "recipe-observation-changed");
        }
        if !self.verified.contains(id) {
            self.delegate.verify(job)?;
            self.verified.insert(id.clone());
        }
        Ok(())
    }
}

fn ancestors(plan: &Plan, base_count: usize, step: &TaskTemplate) -> Result<Plan> {
    let mut needed: BTreeSet<_> = plan.tasks[..base_count]
        .iter()
        .map(|t| t.id.clone())
        .collect();
    let mut pending = step.depends_on.clone();
    while let Some(id) = pending.pop() {
        if needed.insert(id.clone()) {
            let task = plan
                .tasks
                .iter()
                .find(|t| t.id == id)
                .ok_or_else(|| Error {
                    kind: ErrorKind::Missing,
                    code: "recipe-materialized-parent".into(),
                })?;
            pending.extend(task.depends_on.iter().cloned());
        }
    }
    let mut ancestor = plan.clone();
    ancestor.tasks.retain(|t| needed.contains(&t.id));
    Ok(ancestor)
}

/// One bounded observation session, cached only for this planning call. No
/// first-admission decision may rely on these cached custody observations.
pub fn inspect(
    recipe: &Recipe,
    plan: &Plan,
    limit: usize,
    observer: &mut impl Observer,
) -> Result<Schedule> {
    check(recipe)?;
    check_plan(plan)?;
    if !(1..=16).contains(&limit) {
        return fail(ErrorKind::Budget, "recipe-proposal-limit");
    }
    if plan.schema != recipe.base.schema
        || plan.project != recipe.base.project
        || plan.source_commit != recipe.base.source_commit
        || plan.tasks.len() < recipe.base.tasks.len()
        || plan.tasks[..recipe.base.tasks.len()] != recipe.base.tasks
        || recipe.base.tasks.iter().any(|t| {
            plan.source_commit_for(&t.id).ok() != recipe.base.source_commit_for(&t.id).ok()
        })
        || plan.tasks[recipe.base.tasks.len()..]
            .iter()
            .any(|t| plan.source_commit_for(&t.id).ok() != Some(recipe.base.source_commit.as_str()))
        || plan.tasks[recipe.base.tasks.len()..]
            .iter()
            .any(|t| !recipe.steps.iter().any(|s| s.id == t.id))
    {
        return fail(ErrorKind::Stale, "recipe-append-only-plan");
    }
    let mut cached = Cached {
        delegate: observer,
        jobs: BTreeMap::new(),
        verified: BTreeSet::new(),
        bytes: 0,
    };
    let frontier = graph::inspect(plan, &mut cached)?;
    let mut schedule = Schedule {
        schema: "reel.workflow-recipe-frontier.v1".into(),
        recipe_sha256: canonical_digest(recipe)?,
        plan_sha256: canonical_digest(plan)?,
        rows: Vec::new(),
        proposals: Vec::new(),
        existing: frontier.rows,
    };
    let mut processed: BTreeSet<_> = recipe.base.tasks.iter().map(|t| t.id.clone()).collect();
    let mut usable: BTreeSet<_> = schedule
        .existing
        .iter()
        .filter(|r| {
            r.status == Status::RetainedObserved
                && recipe.base.tasks.iter().any(|t| t.id == r.task_id)
        })
        .map(|r| r.task_id.clone())
        .collect();
    let mut pending: Vec<_> = recipe.steps.iter().collect();
    while !pending.is_empty() {
        let index = pending
            .iter()
            .position(|s| s.depends_on.iter().all(|id| processed.contains(id)))
            .ok_or_else(|| Error {
                kind: ErrorKind::Conflict,
                code: "recipe-dependency-cycle".into(),
            })?;
        let step = pending.remove(index);
        let existing = plan.tasks.iter().find(|t| t.id == step.id);
        let waiting_on: Vec<_> = step
            .depends_on
            .iter()
            .filter(|id| !usable.contains(*id))
            .cloned()
            .collect();
        let observed = schedule.existing.iter().find(|r| r.task_id == step.id);
        let mut row = Row {
            task_id: step.id.clone(),
            state: if existing.is_some() && waiting_on.is_empty() {
                State::Existing
            } else {
                State::Blocked
            },
            existing_status: observed.map(|r| r.status),
            waiting_on,
            error: observed.and_then(|r| r.error.clone()),
        };
        if row.error.is_some() {
            row.state = State::Refused;
        }
        if row.waiting_on.is_empty() && row.error.is_none() {
            let base = if existing.is_some() {
                ancestors(plan, recipe.base.tasks.len(), step)?
            } else {
                plan.clone()
            };
            let template = Template {
                schema: step.expansion_schema().into(),
                base_plan_sha256: canonical_digest(&base)?,
                task: step.clone(),
            };
            match expansion::prepare(&base, &template, &mut cached) {
                Ok(expanded) => {
                    if let Some(actual) = existing {
                        let mut actual = actual.clone();
                        actual.native_inputs = None;
                        if actual != expanded.packet.task {
                            row.state = State::Refused;
                            row.error = Some(Error {
                                kind: ErrorKind::Stale,
                                code: "recipe-original-step-template".into(),
                            });
                        } else if row.existing_status == Some(Status::RetainedObserved) {
                            usable.insert(step.id.clone());
                        }
                    } else {
                        row.state = State::Ready;
                        schedule.proposals.push(Proposal {
                            template,
                            packet: expanded.packet,
                            upstream: expanded.upstream,
                        });
                    }
                }
                Err(error) => {
                    row.state = State::Refused;
                    row.error = Some(error);
                }
            }
        }
        processed.insert(step.id.clone());
        schedule.rows.push(row);
    }
    let order: BTreeMap<_, _> = recipe
        .steps
        .iter()
        .enumerate()
        .map(|(i, s)| (s.id.as_str(), i))
        .collect();
    schedule.rows.sort_by_key(|r| order[r.task_id.as_str()]);
    schedule
        .proposals
        .sort_by_key(|p| order[p.packet.task.id.as_str()]);
    for proposal in schedule.proposals.iter().skip(limit) {
        if let Some(row) = schedule
            .rows
            .iter_mut()
            .find(|r| r.task_id == proposal.packet.task.id)
        {
            row.state = State::Deferred;
        }
    }
    schedule.proposals.truncate(limit);
    canonical_digest(&schedule)?;
    Ok(schedule)
}
