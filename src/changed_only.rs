use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const GRAPH_SCHEMA: &str = "reel.changed-only-graph.v0.1";
const STATE_SCHEMA: &str = "reel.changed-only-state.v0.1";
const PLAN_SCHEMA: &str = "reel.changed-only-plan.v0.1";
const ACTION_SCHEMA: &str = "reel.changed-only-action.v0.1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildGraph {
    schema: String,
    graph_id: String,
    nodes: Vec<BuildNode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildNode {
    node_id: String,
    operation_kind: String,
    recipe: LocalFileEvidence,
    inputs: Vec<LocalFileEvidence>,
    dependencies: Vec<String>,
    expected_outputs: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFileEvidence {
    file_id: String,
    path: PathBuf,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PriorState {
    schema: String,
    graph_id: String,
    nodes: Vec<PriorNode>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PriorNode {
    node_id: String,
    action_key: String,
    outputs: Vec<LocalFileEvidence>,
}

#[derive(Debug, Serialize)]
struct ActionKeyMaterial<'a> {
    schema: &'static str,
    graph_id: &'a str,
    node_id: &'a str,
    operation_kind: &'a str,
    recipe: PortableFileEvidence,
    inputs: Vec<PortableFileEvidence>,
    dependencies: Vec<DependencyEvidence>,
    expected_outputs: Vec<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
struct PortableFileEvidence {
    file_id: String,
    sha256: String,
    bytes: u64,
}

#[derive(Debug, Serialize)]
struct DependencyEvidence {
    node_id: String,
    outputs: Vec<PortableFileEvidence>,
}

#[derive(Debug, Serialize)]
struct ChangedOnlyPlan {
    schema: &'static str,
    graph_id: String,
    graph_sha256: String,
    prior_state_sha256: String,
    summary: PlanSummary,
    nodes: Vec<PlannedNode>,
    authority: PlanAuthority,
}

#[derive(Debug, Serialize)]
struct PlanSummary {
    node_count: usize,
    exact_byte_reuse_count: usize,
    rebuild_count: usize,
    blocked_dependency_count: usize,
}

#[derive(Debug, Serialize)]
struct PlannedNode {
    node_id: String,
    operation_kind: String,
    status: PlanStatus,
    reason: PlanReason,
    action_key: Option<String>,
    recipe: PortableFileEvidence,
    inputs: Vec<PortableFileEvidence>,
    dependencies: Vec<String>,
    expected_outputs: Vec<String>,
    verified_outputs: Vec<PortableFileEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlanStatus {
    ExactByteReuse,
    Rebuild,
    BlockedDependency,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlanReason {
    ActionAndOutputsMatch,
    MissingPriorState,
    ActionKeyChanged,
    OutputUnavailable,
    OutputMismatch,
    DependencyNotReusable,
}

#[derive(Debug, Serialize)]
struct PlanAuthority {
    executes_builds: bool,
    mutates_cache: bool,
    selects_creative_output: bool,
    grants_approval: bool,
    authorizes_publication: bool,
    authorizes_release: bool,
}

enum CurrentFileState {
    Match,
    Unavailable,
    Mismatch,
}

pub fn write_changed_only_plan(
    graph_path: &Path,
    prior_state_path: &Path,
    output_path: &Path,
) -> Result<()> {
    require_json_output(output_path)?;

    let graph_bytes = fs::read(graph_path)
        .with_context(|| format!("failed to read build graph {}", graph_path.display()))?;
    let state_bytes = fs::read(prior_state_path).with_context(|| {
        format!(
            "failed to read changed-only prior state {}",
            prior_state_path.display()
        )
    })?;
    let graph: BuildGraph =
        serde_json::from_slice(&graph_bytes).context("invalid changed-only build graph JSON")?;
    let state: PriorState =
        serde_json::from_slice(&state_bytes).context("invalid changed-only prior state JSON")?;

    let plan = create_plan(
        graph,
        state,
        sha256_bytes(&graph_bytes),
        sha256_bytes(&state_bytes),
    )?;
    let bytes =
        serde_json::to_vec_pretty(&plan).context("failed to serialize changed-only plan")?;
    atomic_write_new(output_path, &bytes)
}

fn create_plan(
    graph: BuildGraph,
    state: PriorState,
    graph_sha256: String,
    prior_state_sha256: String,
) -> Result<ChangedOnlyPlan> {
    if graph.schema != GRAPH_SCHEMA {
        bail!(
            "unsupported changed-only graph schema {:?}; expected {GRAPH_SCHEMA:?}",
            graph.schema
        );
    }
    if state.schema != STATE_SCHEMA {
        bail!(
            "unsupported changed-only state schema {:?}; expected {STATE_SCHEMA:?}",
            state.schema
        );
    }
    validate_token("graph_id", &graph.graph_id)?;
    if state.graph_id != graph.graph_id {
        bail!(
            "prior state graph_id {:?} does not match graph {:?}",
            state.graph_id,
            graph.graph_id
        );
    }
    if graph.nodes.is_empty() {
        bail!("changed-only graph must contain at least one node");
    }

    let mut nodes = BTreeMap::new();
    for node in graph.nodes {
        validate_node(&node)?;
        let node_id = node.node_id.clone();
        if nodes.insert(node_id.clone(), node).is_some() {
            bail!("duplicate changed-only node_id {node_id:?}");
        }
    }

    let order = topological_order(&nodes)?;
    let prior_nodes = validate_prior_state(state.nodes, &graph.graph_id)?;
    let mut planned_by_id: BTreeMap<String, PlannedNode> = BTreeMap::new();
    let mut reusable_outputs: BTreeMap<String, Vec<PortableFileEvidence>> = BTreeMap::new();
    let mut planned_order = Vec::with_capacity(order.len());

    for node_id in order {
        planned_order.push(node_id.clone());
        let node = nodes
            .get(&node_id)
            .ok_or_else(|| anyhow!("internal error: missing node {node_id:?}"))?;
        verify_declared_file(&node.recipe, "recipe", &node.node_id)?;
        for input in &node.inputs {
            verify_declared_file(input, "input", &node.node_id)?;
        }

        let recipe = portable(&node.recipe);
        let mut inputs: Vec<_> = node.inputs.iter().map(portable).collect();
        inputs.sort_by(|a, b| a.file_id.cmp(&b.file_id));
        let mut dependencies = node.dependencies.clone();
        dependencies.sort();
        let mut expected_outputs = node.expected_outputs.clone();
        expected_outputs.sort();

        let blocked = dependencies.iter().any(|dependency| {
            planned_by_id
                .get(dependency)
                .is_none_or(|planned| planned.status != PlanStatus::ExactByteReuse)
        });
        if blocked {
            planned_by_id.insert(
                node_id.clone(),
                PlannedNode {
                    node_id,
                    operation_kind: node.operation_kind.clone(),
                    status: PlanStatus::BlockedDependency,
                    reason: PlanReason::DependencyNotReusable,
                    action_key: None,
                    recipe,
                    inputs,
                    dependencies,
                    expected_outputs,
                    verified_outputs: Vec::new(),
                },
            );
            continue;
        }

        let dependency_evidence = dependencies
            .iter()
            .map(|dependency| {
                let outputs = reusable_outputs.get(dependency).cloned().ok_or_else(|| {
                    anyhow!("internal error: missing reusable dependency outputs")
                })?;
                Ok(DependencyEvidence {
                    node_id: dependency.clone(),
                    outputs,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let action_key = action_key(
            &graph.graph_id,
            node,
            recipe.clone(),
            inputs.clone(),
            dependency_evidence,
            &expected_outputs,
        )?;

        let (status, reason, verified_outputs) = match prior_nodes.get(&node_id) {
            None => (
                PlanStatus::Rebuild,
                PlanReason::MissingPriorState,
                Vec::new(),
            ),
            Some(prior) if prior.action_key != action_key => (
                PlanStatus::Rebuild,
                PlanReason::ActionKeyChanged,
                Vec::new(),
            ),
            Some(prior) => match verify_prior_outputs(prior, &expected_outputs) {
                (CurrentFileState::Match, outputs) => (
                    PlanStatus::ExactByteReuse,
                    PlanReason::ActionAndOutputsMatch,
                    outputs,
                ),
                (CurrentFileState::Unavailable, _) => (
                    PlanStatus::Rebuild,
                    PlanReason::OutputUnavailable,
                    Vec::new(),
                ),
                (CurrentFileState::Mismatch, _) => {
                    (PlanStatus::Rebuild, PlanReason::OutputMismatch, Vec::new())
                }
            },
        };

        if status == PlanStatus::ExactByteReuse {
            reusable_outputs.insert(node_id.clone(), verified_outputs.clone());
        }
        planned_by_id.insert(
            node_id.clone(),
            PlannedNode {
                node_id,
                operation_kind: node.operation_kind.clone(),
                status,
                reason,
                action_key: Some(action_key),
                recipe,
                inputs,
                dependencies,
                expected_outputs,
                verified_outputs,
            },
        );
    }

    let nodes = planned_order
        .into_iter()
        .map(|node_id| {
            planned_by_id
                .remove(&node_id)
                .ok_or_else(|| anyhow!("internal error: planned node {node_id:?} is missing"))
        })
        .collect::<Result<Vec<_>>>()?;
    let summary = PlanSummary {
        node_count: nodes.len(),
        exact_byte_reuse_count: nodes
            .iter()
            .filter(|node| node.status == PlanStatus::ExactByteReuse)
            .count(),
        rebuild_count: nodes
            .iter()
            .filter(|node| node.status == PlanStatus::Rebuild)
            .count(),
        blocked_dependency_count: nodes
            .iter()
            .filter(|node| node.status == PlanStatus::BlockedDependency)
            .count(),
    };
    Ok(ChangedOnlyPlan {
        schema: PLAN_SCHEMA,
        graph_id: graph.graph_id,
        graph_sha256,
        prior_state_sha256,
        summary,
        nodes,
        authority: PlanAuthority {
            executes_builds: false,
            mutates_cache: false,
            selects_creative_output: false,
            grants_approval: false,
            authorizes_publication: false,
            authorizes_release: false,
        },
    })
}

fn validate_node(node: &BuildNode) -> Result<()> {
    validate_token("node_id", &node.node_id)?;
    validate_token("operation_kind", &node.operation_kind)?;
    if node.expected_outputs.is_empty() {
        bail!(
            "node {:?} must declare at least one expected output",
            node.node_id
        );
    }
    validate_file_evidence(&node.recipe, "recipe", &node.node_id)?;

    let mut file_ids = BTreeSet::new();
    if !file_ids.insert(node.recipe.file_id.clone()) {
        bail!("node {:?} has duplicate file identifiers", node.node_id);
    }
    for input in &node.inputs {
        validate_file_evidence(input, "input", &node.node_id)?;
        if !file_ids.insert(input.file_id.clone()) {
            bail!(
                "node {:?} has duplicate file_id {:?}",
                node.node_id,
                input.file_id
            );
        }
    }
    require_unique_tokens("dependency", &node.dependencies, &node.node_id)?;
    require_unique_tokens("expected output", &node.expected_outputs, &node.node_id)
}

fn validate_prior_state(
    nodes: Vec<PriorNode>,
    graph_id: &str,
) -> Result<BTreeMap<String, PriorNode>> {
    let mut by_id = BTreeMap::new();
    for node in nodes {
        validate_token("prior node_id", &node.node_id)?;
        validate_sha256("prior action_key", &node.action_key)?;
        if node.outputs.is_empty() {
            bail!(
                "prior node {:?} in graph {:?} has no outputs",
                node.node_id,
                graph_id
            );
        }
        let mut output_ids = BTreeSet::new();
        for output in &node.outputs {
            validate_file_evidence(output, "prior output", &node.node_id)?;
            if !output_ids.insert(output.file_id.clone()) {
                bail!(
                    "prior node {:?} has duplicate output file_id {:?}",
                    node.node_id,
                    output.file_id
                );
            }
        }
        let node_id = node.node_id.clone();
        if by_id.insert(node_id.clone(), node).is_some() {
            bail!("duplicate prior node_id {node_id:?}");
        }
    }
    Ok(by_id)
}

fn topological_order(nodes: &BTreeMap<String, BuildNode>) -> Result<Vec<String>> {
    let mut indegree: BTreeMap<String, usize> = nodes.keys().map(|id| (id.clone(), 0)).collect();
    let mut dependents: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for node in nodes.values() {
        for dependency in &node.dependencies {
            if dependency == &node.node_id {
                bail!("node {:?} cannot depend on itself", node.node_id);
            }
            if !nodes.contains_key(dependency) {
                bail!(
                    "node {:?} depends on unknown node {:?}",
                    node.node_id,
                    dependency
                );
            }
            *indegree
                .get_mut(&node.node_id)
                .ok_or_else(|| anyhow!("internal error: missing indegree"))? += 1;
            dependents
                .entry(dependency.clone())
                .or_default()
                .push(node.node_id.clone());
        }
    }

    let mut ready: BTreeSet<String> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| id.clone())
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.clone());
        if let Some(children) = dependents.get(&id) {
            for child in children {
                let degree = indegree
                    .get_mut(child)
                    .ok_or_else(|| anyhow!("internal error: missing child indegree"))?;
                *degree = degree
                    .checked_sub(1)
                    .ok_or_else(|| anyhow!("internal error: invalid child indegree"))?;
                if *degree == 0 {
                    ready.insert(child.clone());
                }
            }
        }
    }
    if order.len() != nodes.len() {
        let cyclic: Vec<_> = indegree
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(id, _)| id)
            .collect();
        bail!(
            "changed-only graph contains a dependency cycle involving: {}",
            cyclic.join(", ")
        );
    }
    Ok(order)
}

fn action_key(
    graph_id: &str,
    node: &BuildNode,
    recipe: PortableFileEvidence,
    inputs: Vec<PortableFileEvidence>,
    dependencies: Vec<DependencyEvidence>,
    expected_outputs: &[String],
) -> Result<String> {
    let material = ActionKeyMaterial {
        schema: ACTION_SCHEMA,
        graph_id,
        node_id: &node.node_id,
        operation_kind: &node.operation_kind,
        recipe,
        inputs,
        dependencies,
        expected_outputs: expected_outputs.iter().map(String::as_str).collect(),
    };
    let bytes =
        serde_json::to_vec(&material).context("failed to serialize changed-only action key")?;
    Ok(sha256_bytes(&bytes))
}

fn verify_prior_outputs(
    prior: &PriorNode,
    expected_outputs: &[String],
) -> (CurrentFileState, Vec<PortableFileEvidence>) {
    let expected: BTreeSet<_> = expected_outputs.iter().map(String::as_str).collect();
    let actual: BTreeSet<_> = prior
        .outputs
        .iter()
        .map(|output| output.file_id.as_str())
        .collect();
    if actual != expected {
        return (CurrentFileState::Mismatch, Vec::new());
    }

    let mut outputs = Vec::with_capacity(prior.outputs.len());
    let mut aggregate = CurrentFileState::Match;
    for output in &prior.outputs {
        match current_file_state(output) {
            CurrentFileState::Match => outputs.push(portable(output)),
            CurrentFileState::Unavailable if matches!(aggregate, CurrentFileState::Match) => {
                aggregate = CurrentFileState::Unavailable;
            }
            CurrentFileState::Unavailable => {}
            CurrentFileState::Mismatch => aggregate = CurrentFileState::Mismatch,
        }
    }
    outputs.sort_by(|a, b| a.file_id.cmp(&b.file_id));
    if !matches!(aggregate, CurrentFileState::Match) {
        outputs.clear();
    }
    (aggregate, outputs)
}

fn verify_declared_file(file: &LocalFileEvidence, kind: &str, node_id: &str) -> Result<()> {
    match current_file_state(file) {
        CurrentFileState::Match => Ok(()),
        CurrentFileState::Unavailable => bail!(
            "{kind} {:?} for node {:?} is unavailable at {}",
            file.file_id,
            node_id,
            file.path.display()
        ),
        CurrentFileState::Mismatch => bail!(
            "{kind} {:?} for node {:?} does not match its declared current bytes",
            file.file_id,
            node_id
        ),
    }
}

fn current_file_state(file: &LocalFileEvidence) -> CurrentFileState {
    let metadata = match fs::metadata(&file.path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) | Err(_) => return CurrentFileState::Unavailable,
    };
    if metadata.len() != file.bytes {
        return CurrentFileState::Mismatch;
    }
    match sha256_path(&file.path) {
        Ok(actual) if actual == file.sha256 => CurrentFileState::Match,
        Ok(_) => CurrentFileState::Mismatch,
        Err(_) => CurrentFileState::Unavailable,
    }
}

fn validate_file_evidence(file: &LocalFileEvidence, kind: &str, node_id: &str) -> Result<()> {
    validate_token(&format!("{kind} file_id"), &file.file_id)?;
    validate_sha256(&format!("{kind} sha256"), &file.sha256)?;
    if file.path.as_os_str().is_empty() {
        bail!(
            "{kind} {:?} for node {:?} has an empty path",
            file.file_id,
            node_id
        );
    }
    Ok(())
}

fn require_unique_tokens(kind: &str, values: &[String], node_id: &str) -> Result<()> {
    let mut unique = BTreeSet::new();
    for value in values {
        validate_token(kind, value)?;
        if !unique.insert(value) {
            bail!("node {node_id:?} has duplicate {kind} {value:?}");
        }
    }
    Ok(())
}

fn validate_token(label: &str, value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        bail!(
            "{label} must be a portable 1..=128 byte token using ASCII letters, digits, '.', ':', '_' or '-'"
        );
    }
    Ok(())
}

fn validate_sha256(label: &str, value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!("{label} must be a lowercase 64-character SHA-256 digest");
    }
    Ok(())
}

fn portable(file: &LocalFileEvidence) -> PortableFileEvidence {
    PortableFileEvidence {
        file_id: file.file_id.clone(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
    }
}

fn sha256_path(path: &Path) -> Result<String> {
    let file = File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("failed to read {}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn require_json_output(path: &Path) -> Result<()> {
    let is_json = path
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
    if !is_json {
        bail!("changed-only plan output must use a .json extension");
    }
    Ok(())
}

fn atomic_write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create output directory {}", parent.display()))?;
    }
    let temp_parent = parent.unwrap_or_else(|| Path::new("."));
    let mut temp = NamedTempFile::new_in(temp_parent).with_context(|| {
        format!(
            "failed to create temporary output in {}",
            temp_parent.display()
        )
    })?;
    temp.write_all(bytes)
        .context("failed to write temporary changed-only plan")?;
    temp.flush()
        .context("failed to flush temporary changed-only plan")?;
    temp.as_file()
        .sync_all()
        .context("failed to sync temporary changed-only plan")?;

    temp.persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| format!("refusing to overwrite existing output {}", path.display()))?;
    Ok(())
}
