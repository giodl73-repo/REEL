use anyhow::{Context, Result, anyhow, bail};
use same_file::Handle;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

const GRAPH_SCHEMA: &str = "reel.changed-only-graph.v0.1";
const STATE_SCHEMA_V01: &str = "reel.changed-only-state.v0.1";
const STATE_SCHEMA_V02: &str = "reel.changed-only-state.v0.2";
const PLAN_SCHEMA: &str = "reel.changed-only-plan.v0.1";
const ACTION_SCHEMA: &str = "reel.changed-only-action.v0.1";
const RESULT_INPUT_SCHEMA: &str = "reel.changed-only-result-input.v0.1";
const RESULT_RECEIPT_SCHEMA: &str = "reel.changed-only-result-receipt.v0.1";

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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct LocalFileEvidence {
    file_id: String,
    path: PathBuf,
    sha256: String,
    bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PriorState {
    schema: String,
    graph_id: String,
    nodes: Vec<PriorNode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PriorNode {
    node_id: String,
    action_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    receipt_sha256: Option<String>,
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ChangedOnlyPlan {
    schema: &'static str,
    graph_id: String,
    graph_sha256: String,
    prior_state_sha256: String,
    summary: PlanSummary,
    nodes: Vec<PlannedNode>,
    authority: PlanAuthority,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanSummary {
    node_count: usize,
    exact_byte_reuse_count: usize,
    rebuild_count: usize,
    blocked_dependency_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlanStatus {
    ExactByteReuse,
    Rebuild,
    BlockedDependency,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
enum PlanReason {
    ActionAndOutputsMatch,
    MissingPriorState,
    ActionKeyChanged,
    OutputUnavailable,
    OutputMismatch,
    DependencyNotReusable,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PlanAuthority {
    executes_builds: bool,
    mutates_cache: bool,
    selects_creative_output: bool,
    grants_approval: bool,
    authorizes_publication: bool,
    authorizes_release: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultInput {
    schema: String,
    graph_id: String,
    node_id: String,
    action_key: String,
    owner_result_id_sha256: String,
    outcome: ResultOutcome,
    outputs: Vec<OutputBinding>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
enum ResultOutcome {
    Completed,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputBinding {
    file_id: String,
    path: PathBuf,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ResultReceipt {
    schema: String,
    graph_id: String,
    graph_sha256: String,
    prior_state_sha256: String,
    plan_sha256: String,
    node_id: String,
    operation_kind: String,
    action_key: String,
    owner_result_id_sha256: String,
    outcome: ResultOutcome,
    outputs: Vec<PortableFileEvidence>,
    plan_regenerated_from_current_evidence: bool,
    current_output_bytes_verified: bool,
    owner_attested_external_execution: bool,
    executed_by_reel: bool,
    state_mutated: bool,
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
    let mut graph: BuildGraph =
        serde_json::from_slice(&graph_bytes).context("invalid changed-only build graph JSON")?;
    resolve_graph_paths(&mut graph, graph_path)?;
    let mut state: PriorState =
        serde_json::from_slice(&state_bytes).context("invalid changed-only prior state JSON")?;
    resolve_state_paths(&mut state, prior_state_path)?;

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
    validate_state_schema(&state.schema)?;
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
    let prior_nodes = validate_prior_state(state.nodes, &graph.graph_id, &state.schema)?;
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

pub fn write_changed_only_result_receipt(
    graph_path: &Path,
    prior_state_path: &Path,
    plan_path: &Path,
    result_input_path: &Path,
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
    let supplied_plan_bytes = fs::read(plan_path)
        .with_context(|| format!("failed to read changed-only plan {}", plan_path.display()))?;
    let result_bytes = fs::read(result_input_path).with_context(|| {
        format!(
            "failed to read changed-only result input {}",
            result_input_path.display()
        )
    })?;

    let receipt = create_result_receipt(
        graph_path,
        &graph_bytes,
        prior_state_path,
        &state_bytes,
        &supplied_plan_bytes,
        result_input_path,
        &result_bytes,
    )?;
    let bytes = serde_json::to_vec_pretty(&receipt)
        .context("failed to serialize changed-only result receipt")?;
    atomic_write_new(output_path, &bytes)
}

fn create_result_receipt(
    graph_path: &Path,
    graph_bytes: &[u8],
    prior_state_path: &Path,
    state_bytes: &[u8],
    supplied_plan_bytes: &[u8],
    result_input_path: &Path,
    result_bytes: &[u8],
) -> Result<ResultReceipt> {
    let mut graph: BuildGraph =
        serde_json::from_slice(graph_bytes).context("invalid changed-only build graph JSON")?;
    resolve_graph_paths(&mut graph, graph_path)?;
    let mut state: PriorState =
        serde_json::from_slice(state_bytes).context("invalid changed-only prior state JSON")?;
    resolve_state_paths(&mut state, prior_state_path)?;
    let regenerated = create_plan(
        graph,
        state,
        sha256_bytes(graph_bytes),
        sha256_bytes(state_bytes),
    )?;
    let regenerated_bytes = serde_json::to_vec_pretty(&regenerated)
        .context("failed to regenerate changed-only plan")?;
    if supplied_plan_bytes != regenerated_bytes {
        bail!(
            "supplied changed-only plan does not exactly match a plan regenerated from current graph and prior-state evidence"
        );
    }

    let mut result: ResultInput =
        serde_json::from_slice(result_bytes).context("invalid changed-only result input JSON")?;
    resolve_result_paths(&mut result, result_input_path)?;
    validate_result_input(&result)?;
    if result.graph_id != regenerated.graph_id {
        bail!(
            "result graph_id {:?} does not match plan graph {:?}",
            result.graph_id,
            regenerated.graph_id
        );
    }
    let planned = regenerated
        .nodes
        .iter()
        .find(|node| node.node_id == result.node_id)
        .ok_or_else(|| anyhow!("result cites unknown planned node {:?}", result.node_id))?;
    if planned.status != PlanStatus::Rebuild {
        bail!(
            "node {:?} has status {:?}; only rebuild nodes can record a new result",
            result.node_id,
            planned.status
        );
    }
    let planned_action_key = planned
        .action_key
        .as_ref()
        .ok_or_else(|| anyhow!("rebuild node {:?} has no action key", result.node_id))?;
    if result.action_key != *planned_action_key {
        bail!(
            "result action_key does not match planned node {:?}",
            result.node_id
        );
    }

    let outputs = measure_result_outputs(&result, &planned.expected_outputs)?;
    let receipt = ResultReceipt {
        schema: RESULT_RECEIPT_SCHEMA.to_string(),
        graph_id: regenerated.graph_id,
        graph_sha256: regenerated.graph_sha256,
        prior_state_sha256: regenerated.prior_state_sha256,
        plan_sha256: sha256_bytes(supplied_plan_bytes),
        node_id: result.node_id,
        operation_kind: planned.operation_kind.clone(),
        action_key: result.action_key,
        owner_result_id_sha256: result.owner_result_id_sha256,
        outcome: result.outcome,
        outputs: outputs.iter().map(portable).collect(),
        plan_regenerated_from_current_evidence: true,
        current_output_bytes_verified: true,
        owner_attested_external_execution: true,
        executed_by_reel: false,
        state_mutated: false,
        selects_creative_output: false,
        grants_approval: false,
        authorizes_publication: false,
        authorizes_release: false,
    };
    validate_result_receipt(&receipt)?;
    Ok(receipt)
}

pub fn advance_changed_only_state(
    graph_path: &Path,
    prior_state_path: &Path,
    plan_path: &Path,
    result_input_path: &Path,
    receipt_path: &Path,
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
    let plan_bytes = fs::read(plan_path)
        .with_context(|| format!("failed to read changed-only plan {}", plan_path.display()))?;
    let result_bytes = fs::read(result_input_path).with_context(|| {
        format!(
            "failed to read changed-only result input {}",
            result_input_path.display()
        )
    })?;
    let receipt_bytes = fs::read(receipt_path).with_context(|| {
        format!(
            "failed to read changed-only result receipt {}",
            receipt_path.display()
        )
    })?;

    let expected_receipt = create_result_receipt(
        graph_path,
        &graph_bytes,
        prior_state_path,
        &state_bytes,
        &plan_bytes,
        result_input_path,
        &result_bytes,
    )?;
    let expected_receipt_bytes = serde_json::to_vec_pretty(&expected_receipt)
        .context("failed to regenerate changed-only result receipt")?;
    if receipt_bytes != expected_receipt_bytes {
        bail!(
            "supplied result receipt does not exactly match current graph, prior-state, plan, result, and output evidence"
        );
    }

    let mut state: PriorState =
        serde_json::from_slice(&state_bytes).context("invalid changed-only prior state JSON")?;
    resolve_state_paths(&mut state, prior_state_path)?;
    validate_state_schema(&state.schema)?;
    validate_token("prior state graph_id", &state.graph_id)?;
    validate_prior_state(state.nodes.clone(), &state.graph_id, &state.schema)?;
    let mut result: ResultInput =
        serde_json::from_slice(&result_bytes).context("invalid changed-only result input JSON")?;
    resolve_result_paths(&mut result, result_input_path)?;
    validate_result_input(&result)?;
    let receipt = expected_receipt;

    if sha256_bytes(&state_bytes) != receipt.prior_state_sha256 {
        bail!("result receipt does not advance the exact supplied prior-state bytes");
    }
    if state.graph_id != receipt.graph_id || result.graph_id != receipt.graph_id {
        bail!("result receipt, result input, and prior state graph_id values do not match");
    }
    if result.node_id != receipt.node_id
        || result.action_key != receipt.action_key
        || result.owner_result_id_sha256 != receipt.owner_result_id_sha256
        || result.outcome != receipt.outcome
    {
        bail!("result input does not match the immutable result receipt");
    }

    let expected_outputs: Vec<_> = receipt
        .outputs
        .iter()
        .map(|output| output.file_id.clone())
        .collect();
    let local_outputs = measure_result_outputs(&result, &expected_outputs)?;
    for (actual, expected) in local_outputs.iter().zip(&receipt.outputs) {
        if actual.file_id != expected.file_id
            || actual.sha256 != expected.sha256
            || actual.bytes != expected.bytes
        {
            bail!(
                "current output {:?} does not match the immutable result receipt",
                actual.file_id
            );
        }
    }

    state.schema = STATE_SCHEMA_V02.to_string();
    state.nodes.retain(|node| node.node_id != receipt.node_id);
    state.nodes.push(PriorNode {
        node_id: receipt.node_id,
        action_key: receipt.action_key,
        receipt_sha256: Some(sha256_bytes(&receipt_bytes)),
        outputs: local_outputs,
    });
    state.nodes.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    validate_prior_state(state.nodes.clone(), &state.graph_id, &state.schema)?;
    let bytes = serde_json::to_vec_pretty(&state)
        .context("failed to serialize advanced changed-only state")?;
    atomic_write_new(output_path, &bytes)
}

fn resolve_graph_paths(graph: &mut BuildGraph, graph_path: &Path) -> Result<()> {
    let base = contract_base(graph_path)?;
    for node in &mut graph.nodes {
        rebase_path(&mut node.recipe.path, &base);
        for input in &mut node.inputs {
            rebase_path(&mut input.path, &base);
        }
    }
    Ok(())
}

fn resolve_state_paths(state: &mut PriorState, state_path: &Path) -> Result<()> {
    let base = contract_base(state_path)?;
    for node in &mut state.nodes {
        for output in &mut node.outputs {
            rebase_path(&mut output.path, &base);
        }
    }
    Ok(())
}

fn resolve_result_paths(result: &mut ResultInput, result_path: &Path) -> Result<()> {
    let base = contract_base(result_path)?;
    for output in &mut result.outputs {
        rebase_path(&mut output.path, &base);
    }
    Ok(())
}

fn contract_base(contract_path: &Path) -> Result<PathBuf> {
    let parent = contract_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve contract directory for {}",
            contract_path.display()
        )
    })
}

fn rebase_path(path: &mut PathBuf, base: &Path) {
    if path.is_relative() {
        *path = base.join(&*path);
    }
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
    schema: &str,
) -> Result<BTreeMap<String, PriorNode>> {
    let mut by_id = BTreeMap::new();
    for node in nodes {
        validate_token("prior node_id", &node.node_id)?;
        validate_sha256("prior action_key", &node.action_key)?;
        match (&node.receipt_sha256, schema) {
            (Some(_), STATE_SCHEMA_V01) => {
                bail!("changed-only state v0.1 must not contain receipt bindings")
            }
            (Some(receipt_sha256), STATE_SCHEMA_V02) => {
                validate_sha256("prior receipt_sha256", receipt_sha256)?;
            }
            (None, STATE_SCHEMA_V01 | STATE_SCHEMA_V02) => {}
            (_, _) => unreachable!("state schema is validated before prior nodes"),
        }
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

fn validate_state_schema(schema: &str) -> Result<()> {
    if !matches!(schema, STATE_SCHEMA_V01 | STATE_SCHEMA_V02) {
        bail!(
            "unsupported changed-only state schema {schema:?}; expected {STATE_SCHEMA_V01:?} or {STATE_SCHEMA_V02:?}"
        );
    }
    Ok(())
}

fn validate_result_input(result: &ResultInput) -> Result<()> {
    if result.schema != RESULT_INPUT_SCHEMA {
        bail!(
            "unsupported changed-only result input schema {:?}; expected {RESULT_INPUT_SCHEMA:?}",
            result.schema
        );
    }
    validate_token("result graph_id", &result.graph_id)?;
    validate_token("result node_id", &result.node_id)?;
    validate_sha256("result action_key", &result.action_key)?;
    validate_sha256(
        "result owner_result_id_sha256",
        &result.owner_result_id_sha256,
    )?;
    if result.outputs.is_empty() {
        bail!("changed-only result must declare at least one output");
    }
    let mut file_ids = BTreeSet::new();
    for output in &result.outputs {
        validate_token("result output file_id", &output.file_id)?;
        if output.path.as_os_str().is_empty() {
            bail!("result output {:?} has an empty path", output.file_id);
        }
        if !file_ids.insert(output.file_id.as_str()) {
            bail!("duplicate result output file_id {:?}", output.file_id);
        }
    }
    Ok(())
}

fn validate_result_receipt(receipt: &ResultReceipt) -> Result<()> {
    if receipt.schema != RESULT_RECEIPT_SCHEMA {
        bail!(
            "unsupported changed-only result receipt schema {:?}; expected {RESULT_RECEIPT_SCHEMA:?}",
            receipt.schema
        );
    }
    validate_token("receipt graph_id", &receipt.graph_id)?;
    validate_sha256("receipt graph_sha256", &receipt.graph_sha256)?;
    validate_sha256("receipt prior_state_sha256", &receipt.prior_state_sha256)?;
    validate_sha256("receipt plan_sha256", &receipt.plan_sha256)?;
    validate_token("receipt node_id", &receipt.node_id)?;
    validate_token("receipt operation_kind", &receipt.operation_kind)?;
    validate_sha256("receipt action_key", &receipt.action_key)?;
    validate_sha256(
        "receipt owner_result_id_sha256",
        &receipt.owner_result_id_sha256,
    )?;
    if receipt.outputs.is_empty() {
        bail!("changed-only result receipt must contain at least one output");
    }
    let mut previous_id: Option<&str> = None;
    for output in &receipt.outputs {
        validate_portable_file_evidence(output, "receipt output")?;
        if previous_id.is_some_and(|previous| previous >= output.file_id.as_str()) {
            bail!("changed-only result receipt outputs must have unique sorted file_id values");
        }
        previous_id = Some(&output.file_id);
    }
    if !receipt.plan_regenerated_from_current_evidence
        || !receipt.current_output_bytes_verified
        || !receipt.owner_attested_external_execution
        || receipt.executed_by_reel
        || receipt.state_mutated
        || receipt.selects_creative_output
        || receipt.grants_approval
        || receipt.authorizes_publication
        || receipt.authorizes_release
    {
        bail!("changed-only result receipt has inconsistent verification or authority boundaries");
    }
    Ok(())
}

fn measure_result_outputs(
    result: &ResultInput,
    expected_outputs: &[String],
) -> Result<Vec<LocalFileEvidence>> {
    let expected: BTreeSet<_> = expected_outputs.iter().map(String::as_str).collect();
    let actual: BTreeSet<_> = result
        .outputs
        .iter()
        .map(|output| output.file_id.as_str())
        .collect();
    if actual != expected {
        bail!(
            "result output identities do not exactly match the planned expected outputs for node {:?}",
            result.node_id
        );
    }

    let mut physical_files = HashSet::new();
    let mut outputs = Vec::with_capacity(result.outputs.len());
    for output in &result.outputs {
        let canonical = fs::canonicalize(&output.path).with_context(|| {
            format!(
                "failed to resolve result output {:?} at {}",
                output.file_id,
                output.path.display()
            )
        })?;
        let metadata = fs::metadata(&canonical).with_context(|| {
            format!(
                "failed to inspect result output {:?} at {}",
                output.file_id,
                output.path.display()
            )
        })?;
        if !metadata.is_file() {
            bail!("result output {:?} is not a regular file", output.file_id);
        }
        let identity = physical_file_identity(&canonical)?;
        if !physical_files.insert(identity) {
            bail!(
                "multiple result output identities resolve to the same physical file {}",
                canonical.display()
            );
        }
        outputs.push(LocalFileEvidence {
            file_id: output.file_id.clone(),
            path: canonical.clone(),
            sha256: sha256_path(&canonical)?,
            bytes: metadata.len(),
        });
    }
    outputs.sort_by(|a, b| a.file_id.cmp(&b.file_id));
    Ok(outputs)
}

fn physical_file_identity(path: &Path) -> Result<Handle> {
    Handle::from_path(path)
        .with_context(|| format!("failed to identify physical file {}", path.display()))
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

    let mut physical_files = HashSet::new();
    for output in &prior.outputs {
        if let Ok(identity) = physical_file_identity(&output.path) {
            if !physical_files.insert(identity) {
                return (CurrentFileState::Mismatch, Vec::new());
            }
        }
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

fn validate_portable_file_evidence(file: &PortableFileEvidence, kind: &str) -> Result<()> {
    validate_token(&format!("{kind} file_id"), &file.file_id)?;
    validate_sha256(&format!("{kind} sha256"), &file.sha256)
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
        bail!("changed-only output must use a .json extension");
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
        .context("failed to write temporary changed-only output")?;
    temp.flush()
        .context("failed to flush temporary changed-only output")?;
    temp.as_file()
        .sync_all()
        .context("failed to sync temporary changed-only output")?;

    temp.persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| format!("refusing to overwrite existing output {}", path.display()))?;
    Ok(())
}
