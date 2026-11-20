//! Complete authored resource accounting for bounded publication recipes.
//! No native binding, admission, media conformance or approval is inferred.
use crate::{
    expansion::{Input, TaskTemplate},
    *,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Film,
    ComicIssue,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Consumer task snapshot, not an assertion of Git/annex origin or custody.
    pub context_commit: String,
    pub artifact: Artifact,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Edition {
    pub language: String,
    /// Authored editorial order; producer-specific controls must enforce it.
    pub components: Vec<String>,
    /// Other directly consumed parent tasks, excluded from editorial order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<String>,
    pub final_task: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub source_bytes: u64,
    pub input_bytes: u64,
    pub input_object_bytes: u64,
    pub inputs_per_task: usize,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Declaration {
    pub schema: String,
    pub format: Format,
    pub recipe: recipe::Recipe,
    /// Stable authored binding names; identical bytes do not merge origins.
    pub resources: BTreeMap<String, Resource>,
    pub editions: BTreeMap<String, Edition>,
    pub limits: Limits,
}
#[derive(Serialize)]
pub struct Stage {
    pub task_id: String,
    pub context_commit: String,
    pub fixed_bindings: Vec<String>,
    pub input_count: usize,
    pub input_ceiling_bytes: u64,
}
#[derive(Serialize)]
pub struct Report {
    pub schema: String,
    pub format: Format,
    pub declaration_sha256: String,
    pub recipe_sha256: String,
    pub resources: usize,
    pub logical_source_bytes: u64,
    pub distinct_objects: usize,
    pub distinct_object_bytes: u64,
    pub stages: Vec<Stage>,
    pub editions: BTreeMap<String, Edition>,
    pub native_bindings_verified: bool,
    pub production_admitted: bool,
    pub producer_order_verified: bool,
    pub producer_closure_verified: bool,
    pub task_completed: bool,
}
fn template_inputs(task: &TaskTemplate) -> Vec<&Input> {
    let mut all: Vec<_> = task.inputs.iter().collect();
    if let Some(tool) = &task.tool {
        all.extend([&tool.publication, &tool.request]);
        all.extend(tool.sources.values());
        if let Some(part) = &tool.part {
            all.push(&part.plan);
        }
    }
    all
}
fn key(commit: &str, asset: &Artifact) -> Result<String> {
    canonical_digest(&Resource {
        context_commit: commit.into(),
        artifact: asset.clone(),
    })
}
struct Node<'a> {
    commit: &'a str,
    language: Option<&'a str>,
    fixed: Vec<&'a Artifact>,
    inputs: Vec<&'a Artifact>,
    parents: Vec<&'a upstream::Reference>,
    dependencies: Vec<&'a str>,
    outputs: &'a OutputContracts,
    worker: bool,
    streaming: Option<&'a upstream::Streaming>,
}
fn nodes(recipe: &recipe::Recipe) -> Result<BTreeMap<&str, Node<'_>>> {
    let mut nodes = BTreeMap::new();
    for task in &recipe.base.tasks {
        let mut fixed: Vec<_> = task.inputs.iter().collect();
        fixed.extend(task.native_inputs.iter());
        fixed.extend(task.upstream_inputs.iter());
        if let Some(tool) = &task.tool {
            fixed.push(&tool.executable);
            fixed.extend(tool.auxiliary_executables.values());
        }
        nodes.insert(
            task.id.as_str(),
            Node {
                commit: recipe.base.source_commit_for(&task.id)?,
                language: task.language.as_deref(),
                fixed,
                inputs: task.inputs.iter().collect(),
                parents: vec![],
                dependencies: task.depends_on.iter().map(String::as_str).collect(),
                outputs: &task.outputs,
                worker: task.tool.is_some(),
                streaming: None,
            },
        );
    }
    for task in &recipe.steps {
        let mut fixed = Vec::new();
        let mut parents = Vec::new();
        for input in template_inputs(task) {
            match input {
                Input::Fixed { artifact } => fixed.push(artifact),
                Input::Parent { reference } => parents.push(reference),
            }
        }
        let inputs = fixed.clone();
        if let Some(tool) = &task.tool {
            fixed.push(&tool.executable);
            fixed.extend(tool.auxiliary_executables.values());
        }
        nodes.insert(
            task.id.as_str(),
            Node {
                commit: &recipe.base.source_commit,
                language: task.language.as_deref(),
                fixed,
                inputs,
                parents,
                dependencies: task.depends_on.iter().map(String::as_str).collect(),
                outputs: &task.outputs,
                worker: task.tool.is_some(),
                streaming: task.streaming.as_ref(),
            },
        );
    }
    Ok(nodes)
}
pub fn check(declaration: &Declaration) -> Result<Report> {
    let declaration_sha256 = canonical_digest(declaration)?;
    recipe::check(&declaration.recipe)?;
    let limits = &declaration.limits;
    if declaration.schema != "reel.workflow-composition.v1"
        || declaration.resources.is_empty()
        || declaration.resources.len() > 4096
        || declaration.editions.is_empty()
        || declaration.editions.len() > 32
    {
        return fail(ErrorKind::Invalid, "composition-contract");
    }
    if limits.source_bytes == 0
        || limits.source_bytes > 128 * 1024 * 1024 * 1024
        || limits.input_bytes == 0
        || limits.input_bytes > 64 * 1024 * 1024 * 1024
        || limits.input_object_bytes == 0
        || limits.input_object_bytes > limits.input_bytes
        || limits.input_object_bytes > 16 * 1024 * 1024 * 1024
        || !(1..=MAX_INPUTS).contains(&limits.inputs_per_task)
    {
        return fail(ErrorKind::Budget, "composition-limits");
    }
    let nodes = nodes(&declaration.recipe)?;
    let mut reached = BTreeSet::new();
    for (id, edition) in &declaration.editions {
        if !token(id)
            || !token(&edition.language)
            || edition.components.is_empty()
            || edition.components.len() > MAX_TASKS
            || edition.controls.len() > MAX_TASKS
            || edition.components.iter().collect::<BTreeSet<_>>().len() != edition.components.len()
        {
            return fail(ErrorKind::Invalid, "composition-edition");
        }
        let final_node = nodes.get(edition.final_task.as_str()).ok_or(Error {
            kind: ErrorKind::Missing,
            code: "composition-final-task".into(),
        })?;
        let expected: BTreeSet<_> = edition
            .components
            .iter()
            .chain(&edition.controls)
            .map(String::as_str)
            .collect();
        let actual: BTreeSet<_> = final_node
            .parents
            .iter()
            .map(|r| r.task_id.as_str())
            .collect();
        if expected != actual || expected.len() != edition.components.len() + edition.controls.len()
        {
            return fail(ErrorKind::Conflict, "composition-edition-parent-set");
        }
        if final_node.language != Some(edition.language.as_str()) {
            return fail(ErrorKind::Conflict, "composition-final-language");
        }
        let mut needed = BTreeSet::new();
        let mut pending = vec![edition.final_task.as_str()];
        while let Some(id) = pending.pop() {
            if needed.insert(id) {
                pending.extend(nodes[id].dependencies.iter().copied());
            }
        }
        for component in &edition.components {
            let node = nodes.get(component.as_str()).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "composition-component".into(),
            })?;
            if component == &edition.final_task
                || !needed.contains(component.as_str())
                || node
                    .language
                    .is_some_and(|language| language != edition.language)
            {
                return fail(ErrorKind::Conflict, "composition-component-scope");
            }
        }
        for control in &edition.controls {
            if nodes[control.as_str()]
                .language
                .is_some_and(|language| language != edition.language)
            {
                return fail(ErrorKind::Conflict, "composition-control-language");
            }
        }
        reached.extend(needed);
    }
    if declaration
        .recipe
        .steps
        .iter()
        .any(|step| !reached.contains(step.id.as_str()))
    {
        return fail(ErrorKind::Conflict, "composition-unconsumed-step");
    }
    let mut by_pin = BTreeMap::new();
    let mut objects = BTreeMap::new();
    let mut logical_source_bytes = 0u64;
    for (binding, resource) in &declaration.resources {
        if !resource.artifact.logical_id.split('/').all(token)
            || !token(binding)
            || !hex(&resource.context_commit, &[40, 64])
            || resource.context_commit.bytes().all(|b| b == b'0')
        {
            return fail(ErrorKind::Invalid, "composition-resource");
        }
        artifact(&resource.artifact)?;
        if by_pin
            .insert(key(&resource.context_commit, &resource.artifact)?, binding)
            .is_some()
        {
            return fail(ErrorKind::Conflict, "composition-duplicate-binding");
        }
        logical_source_bytes = logical_source_bytes
            .checked_add(resource.artifact.bytes)
            .filter(|n| *n <= limits.source_bytes)
            .ok_or(Error {
                kind: ErrorKind::Budget,
                code: "composition-source-bytes".into(),
            })?;
        if objects
            .insert(&resource.artifact.sha256, resource.artifact.bytes)
            .is_some_and(|old| old != resource.artifact.bytes)
        {
            return fail(ErrorKind::Conflict, "composition-object-length");
        }
    }
    let mut used = BTreeSet::new();
    let mut stages = Vec::new();
    for id in reached {
        let node = &nodes[id];
        let mut fixed_bindings = BTreeSet::new();
        for asset in &node.fixed {
            let binding = by_pin.get(&key(node.commit, asset)?).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "composition-resource-binding".into(),
            })?;
            fixed_bindings.insert((*binding).clone());
            used.insert((*binding).clone());
        }
        let mut fixed = BTreeMap::new();
        for asset in &node.inputs {
            if fixed
                .insert(asset.logical_id.as_str(), *asset)
                .is_some_and(|old| old != *asset)
            {
                return fail(ErrorKind::Conflict, "composition-input-alias");
            }
        }
        let mut ceilings: Vec<u64> = fixed.values().map(|a| a.bytes).collect();
        let mut references = BTreeSet::new();
        let mut upstream_bytes = 0u64;
        let (upstream_total, upstream_object) = node
            .streaming
            .map(|s| (s.input_bytes, s.input_object_bytes))
            .unwrap_or((64 * 1024 * 1024, 64 * 1024 * 1024));
        for reference in &node.parents {
            if references.insert(canonical_digest(reference)?) {
                let parent = &nodes[reference.task_id.as_str()];
                let ceiling = match &reference.artifact {
                    upstream::Selector::Output { name } => parent.outputs[name].max_bytes,
                    upstream::Selector::WorkerReceipt if parent.worker => MAX_CONTROL_BYTES as u64,
                    _ => return fail(ErrorKind::Invalid, "composition-parent-receipt"),
                };
                upstream_bytes = upstream_bytes
                    .checked_add(ceiling)
                    .filter(|n| *n <= upstream_total)
                    .ok_or(Error {
                        kind: ErrorKind::Budget,
                        code: "composition-upstream-budget".into(),
                    })?;
                if ceiling > upstream_object {
                    return fail(ErrorKind::Budget, "composition-upstream-budget");
                }
                ceilings.push(ceiling);
            }
        }
        let total = ceilings
            .iter()
            .try_fold(0u64, |sum, bytes| sum.checked_add(*bytes))
            .ok_or(Error {
                kind: ErrorKind::Budget,
                code: "composition-input-overflow".into(),
            })?;
        if ceilings.len() > limits.inputs_per_task
            || total > limits.input_bytes
            || ceilings.iter().any(|n| *n > limits.input_object_bytes)
        {
            return fail(ErrorKind::Budget, "composition-stage-inputs");
        }
        stages.push(Stage {
            task_id: id.into(),
            context_commit: node.commit.into(),
            fixed_bindings: fixed_bindings.into_iter().collect(),
            input_count: ceilings.len(),
            input_ceiling_bytes: total,
        });
    }
    if used.len() != declaration.resources.len() {
        return fail(ErrorKind::Conflict, "composition-unused-resource");
    }
    let distinct_object_bytes = objects
        .values()
        .try_fold(0u64, |sum, bytes| sum.checked_add(*bytes))
        .ok_or(Error {
            kind: ErrorKind::Budget,
            code: "composition-object-overflow".into(),
        })?;
    Ok(Report {
        schema: "reel.workflow-composition-report.v1".into(),
        format: declaration.format.clone(),
        declaration_sha256,
        recipe_sha256: canonical_digest(&declaration.recipe)?,
        resources: declaration.resources.len(),
        logical_source_bytes,
        distinct_objects: objects.len(),
        distinct_object_bytes,
        stages,
        editions: declaration.editions.clone(),
        native_bindings_verified: false,
        production_admitted: false,
        producer_order_verified: false,
        producer_closure_verified: false,
        task_completed: false,
    })
}
