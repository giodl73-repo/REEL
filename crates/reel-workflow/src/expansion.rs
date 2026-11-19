//! Late-bound planning from verified observations. No admission or dispatch.
use crate::{
    graph::{Job, Observer, Status},
    *,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Input {
    Fixed { artifact: Artifact },
    Parent { reference: upstream::Reference },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    pub capability: ToolCapability,
    pub executable: Artifact,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub auxiliary_executables: BTreeMap<String, Artifact>,
    pub publication: Input,
    pub edition_id: String,
    pub request: Input,
    pub sources: BTreeMap<String, Input>,
    pub part: Option<Part>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Part {
    pub part_id: String,
    pub plan: Input,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TaskTemplate {
    pub id: String,
    pub operation: Operation,
    pub language: Option<String>,
    pub prompt: String,
    pub depends_on: Vec<String>,
    pub inputs: Vec<Input>,
    pub outputs: OutputContracts,
    pub tool: Option<Tool>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Template {
    pub schema: String,
    /// Compare against the complete existing plan, not a mutable "latest" view.
    pub base_plan_sha256: String,
    pub task: TaskTemplate,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Expansion {
    pub schema: String,
    pub base_plan_sha256: String,
    pub plan: Plan,
    pub packet: Packet,
    pub upstream: Option<upstream::Bindings>,
}

struct Observations<'a, O> {
    delegate: &'a mut O,
    needed: BTreeSet<String>,
    jobs: BTreeMap<String, Job>,
    bytes: usize,
}
impl<O: Observer> Observer for Observations<'_, O> {
    fn observe(&mut self, task: &str) -> Result<Option<Job>> {
        self.delegate.observe(task)
    }
    fn verify(&mut self, job: &Job) -> Result<()> {
        self.delegate.verify(job)?;
        if job.state == handoff::State::Retained && self.needed.contains(&job.ticket.packet.task.id)
        {
            let size = serde_json::to_vec(&(&job.ticket.packet, &job.result))
                .map_err(|_| Error {
                    kind: ErrorKind::Invalid,
                    code: "expansion-parent-json".into(),
                })?
                .len();
            self.bytes = self.bytes.checked_add(size).ok_or_else(|| Error {
                kind: ErrorKind::Budget,
                code: "expansion-parent-evidence".into(),
            })?;
            if self.bytes > 32 * MAX_CONTROL_BYTES {
                return fail(ErrorKind::Budget, "expansion-parent-evidence");
            }
            self.jobs
                .insert(job.ticket.packet.task.id.clone(), job.clone());
        }
        Ok(())
    }
}

/// Observe and verify original parent jobs before using their exact selected
/// results. Sequential observations are not atomic custody or completion proof
/// for a future job. Native source freezing/admission must recheck the parents.
pub fn prepare(
    plan: &Plan,
    template: &Template,
    observer: &mut impl Observer,
) -> Result<Expansion> {
    check_plan(plan)?;
    canonical_digest(template)?;
    let base = canonical_digest(plan)?;
    if template.schema != "reel.workflow-expansion-template.v1" || template.base_plan_sha256 != base
    {
        return fail(ErrorKind::Stale, "expansion-base-plan");
    }
    let t = &template.task;
    if !token(&t.id) || plan.tasks.iter().any(|task| task.id == t.id) {
        return fail(ErrorKind::Conflict, "expansion-new-task-identity");
    }
    if plan.tasks.len() >= MAX_TASKS
        || t.inputs.len() > MAX_INPUTS
        || t.depends_on.len() > MAX_TASKS
        || t.tool
            .as_ref()
            .is_some_and(|tool| tool.sources.len() > MAX_INPUTS)
    {
        return fail(ErrorKind::Budget, "expansion-task-bound");
    }
    let needed: BTreeSet<_> = t.depends_on.iter().cloned().collect();
    if needed.len() != t.depends_on.len()
        || needed
            .iter()
            .any(|id| !plan.tasks.iter().any(|task| &task.id == id))
    {
        return fail(ErrorKind::Invalid, "expansion-parent-set");
    }
    let mut observations = Observations {
        delegate: observer,
        needed,
        jobs: BTreeMap::new(),
        bytes: 0,
    };
    let frontier = graph::inspect(plan, &mut observations)?;
    for id in &t.depends_on {
        let row = frontier
            .rows
            .iter()
            .find(|row| &row.task_id == id)
            .ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "expansion-parent-row".into(),
            })?;
        if let Some(error) = &row.error {
            return Err(error.clone());
        }
        if row.status != Status::RetainedObserved {
            return fail(
                if row.status == Status::Cancelled {
                    ErrorKind::Denied
                } else {
                    ErrorKind::Busy
                },
                "expansion-parent-not-retained",
            );
        }
    }
    let dependencies = observations
        .jobs
        .iter()
        .map(|(id, job)| {
            let result = job.result.as_ref().ok_or_else(|| Error {
                kind: ErrorKind::Missing,
                code: "expansion-parent-result".into(),
            })?;
            Ok((
                id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(result)?,
                    artifacts: result.artifacts.clone(),
                },
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut inputs = BTreeMap::<String, Artifact>::new();
    let mut origins = BTreeMap::<String, Option<upstream::Reference>>::new();
    let mut upstream_inputs = BTreeMap::new();
    let mut resolve = |input: &Input| -> Result<Artifact> {
        let (a, origin) = match input {
            Input::Fixed { artifact: a } => (a.clone(), None),
            Input::Parent { reference } => {
                let result = observations
                    .jobs
                    .get(&reference.task_id)
                    .and_then(|j| j.result.as_ref())
                    .ok_or_else(|| Error {
                        kind: ErrorKind::Missing,
                        code: "expansion-declared-parent".into(),
                    })?;
                let a = match &reference.artifact {
                    upstream::Selector::Output { name } => result.artifacts.get(name),
                    upstream::Selector::WorkerReceipt => result.worker_receipt.as_ref(),
                }
                .ok_or_else(|| Error {
                    kind: ErrorKind::Missing,
                    code: "expansion-parent-selector".into(),
                })?;
                (a.clone(), Some(reference.clone()))
            }
        };
        artifact(&a)?;
        if inputs.get(&a.logical_id).is_some_and(|old| old != &a)
            || origins.get(&a.logical_id).is_some_and(|old| old != &origin)
        {
            return fail(ErrorKind::Conflict, "expansion-logical-origin-alias");
        }
        if let Some(reference) = &origin {
            upstream_inputs.insert(a.logical_id.clone(), reference.clone());
        }
        origins.insert(a.logical_id.clone(), origin);
        inputs.insert(a.logical_id.clone(), a.clone());
        if inputs.len() > MAX_INPUTS {
            return fail(ErrorKind::Budget, "expansion-input-count");
        }
        Ok(a)
    };
    for input in &t.inputs {
        resolve(input)?;
    }
    let tool = if let Some(tool) = &t.tool {
        let publication = resolve(&tool.publication)?;
        let request = resolve(&tool.request)?;
        let mut source_bindings = BTreeMap::new();
        for (path, input) in &tool.sources {
            source_bindings.insert(path.clone(), resolve(input)?.logical_id);
        }
        let part = tool
            .part
            .as_ref()
            .map(|part| -> Result<ComicPart> {
                Ok(ComicPart {
                    schema: "reel.workflow-comic-part.v1".into(),
                    part_id: part.part_id.clone(),
                    plan: resolve(&part.plan)?,
                })
            })
            .transpose()?;
        Some(ToolContract {
            auxiliary_executables: tool.auxiliary_executables.clone(),
            schema: "reel.workflow-tool.v1".into(),
            capability: tool.capability,
            executable: tool.executable.clone(),
            publication,
            edition_id: tool.edition_id.clone(),
            request,
            source_bindings,
            part,
        })
    } else {
        None
    };
    let upstream = (!upstream_inputs.is_empty()).then_some(upstream::Bindings {
        schema: if plan.schema == SCOPED_PLAN_SCHEMA {
            "reel.workflow-upstream-inputs.v2"
        } else {
            "reel.workflow-upstream-inputs.v1"
        }
        .into(),
        inputs: upstream_inputs,
    });
    let upstream_pin = upstream
        .as_ref()
        .map(|bindings| -> Result<Artifact> {
            let raw = bindings.canonical_bytes()?;
            Ok(Artifact {
                logical_id: format!("workflow/expansion/{}/upstream", t.id),
                sha256: digest(&raw),
                bytes: raw.len() as u64,
                media_type: "application/json".into(),
            })
        })
        .transpose()?;
    let task = Task {
        id: t.id.clone(),
        native_inputs: None,
        upstream_inputs: upstream_pin,
        operation: t.operation,
        tool,
        language: t.language.clone(),
        prompt: t.prompt.clone(),
        inputs: inputs.into_values().collect(),
        depends_on: t.depends_on.clone(),
        outputs: t.outputs.clone(),
    };
    let mut expanded = plan.clone();
    expanded.tasks.push(task);
    if expanded.schema == SCOPED_PLAN_SCHEMA {
        expanded
            .task_snapshots
            .insert(t.id.clone(), plan.source_commit.clone());
    }
    let packet = make_packet(&expanded, &t.id, dependencies)?;
    let expansion = Expansion {
        schema: "reel.workflow-expansion.v1".into(),
        base_plan_sha256: base,
        plan: expanded,
        packet,
        upstream,
    };
    canonical_digest(&expansion)?;
    Ok(expansion)
}
