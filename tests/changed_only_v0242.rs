use reel::changed_only::{
    advance_changed_only_state, write_changed_only_plan, write_changed_only_result_receipt,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

fn write_file(root: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn evidence(file_id: &str, path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    json!({
        "file_id": file_id,
        "path": path,
        "sha256": hash(&bytes),
        "bytes": bytes.len(),
    })
}

fn write_json(root: &Path, name: &str, value: &Value) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn create_plan(root: &Path, name: &str, graph_path: &Path, state_path: &Path) -> (PathBuf, Value) {
    let path = root.join(format!("{name}-plan.json"));
    write_changed_only_plan(graph_path, state_path, &path).unwrap();
    let value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    (path, value)
}

fn node_action_key(plan: &Value, node_id: &str) -> String {
    plan["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == node_id)
        .unwrap()["action_key"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn single_node_fixture(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let recipe = write_file(root, "recipe.py", b"owner recipe");
    let input = write_file(root, "input.yaml", b"owner input");
    let output = write_file(root, "output.json", b"owner output");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "immutable-result-proof",
        "nodes": [{
            "node_id": "build-manifest",
            "operation_kind": "manifest-build",
            "recipe": evidence("recipe", &recipe),
            "inputs": [evidence("source", &input)],
            "dependencies": [],
            "expected_outputs": ["manifest"]
        }]
    });
    let state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "immutable-result-proof",
        "nodes": []
    });
    (
        write_json(root, "graph.json", &graph),
        write_json(root, "state.json", &state),
        output,
        input,
    )
}

fn result_input(root: &Path, action_key: &str, output: &Path) -> (PathBuf, Value) {
    let value = json!({
        "schema": "reel.changed-only-result-input.v0.1",
        "graph_id": "immutable-result-proof",
        "node_id": "build-manifest",
        "action_key": action_key,
        "owner_result_id_sha256": "1".repeat(64),
        "outcome": "completed",
        "outputs": [{
            "file_id": "manifest",
            "path": output
        }]
    });
    (write_json(root, "result.json", &value), value)
}

#[test]
fn records_a_path_free_receipt_advances_state_and_enables_exact_reuse() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (graph_path, state_path, output, _) = single_node_fixture(root);
    let (plan_path, initial_plan) = create_plan(root, "initial", &graph_path, &state_path);
    let action_key = node_action_key(&initial_plan, "build-manifest");
    let (result_path, _) = result_input(root, &action_key, &output);

    let receipt_path = root.join("receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();
    let receipt_bytes = fs::read(&receipt_path).unwrap();
    let receipt: Value = serde_json::from_slice(&receipt_bytes).unwrap();
    assert_eq!(receipt["schema"], "reel.changed-only-result-receipt.v0.1");
    assert_eq!(receipt["node_id"], "build-manifest");
    assert_eq!(receipt["action_key"], action_key);
    assert_eq!(receipt["outputs"][0]["sha256"], hash(b"owner output"));
    assert_eq!(receipt["current_output_bytes_verified"], true);
    assert_eq!(receipt["owner_attested_external_execution"], true);
    assert_eq!(receipt["executed_by_reel"], false);
    assert_eq!(receipt["state_mutated"], false);
    assert_eq!(receipt["selects_creative_output"], false);
    assert_eq!(receipt["grants_approval"], false);
    let serialized = String::from_utf8(receipt_bytes.clone()).unwrap();
    assert!(!serialized.contains(&root.display().to_string()));
    assert!(!serialized.contains("owner output"));

    let advanced_state_path = root.join("advanced-state.json");
    advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
        &advanced_state_path,
    )
    .unwrap();
    let advanced_state: Value =
        serde_json::from_slice(&fs::read(&advanced_state_path).unwrap()).unwrap();
    assert_eq!(advanced_state["schema"], "reel.changed-only-state.v0.2");
    assert_eq!(
        advanced_state["nodes"][0]["receipt_sha256"],
        hash(&receipt_bytes)
    );
    assert_eq!(advanced_state["nodes"][0]["action_key"], action_key);

    let (_, reuse_plan) = create_plan(root, "reuse", &graph_path, &advanced_state_path);
    assert_eq!(reuse_plan["nodes"][0]["status"], "exact-byte-reuse");
    assert_eq!(reuse_plan["nodes"][0]["reason"], "action-and-outputs-match");
}

#[test]
fn rejects_output_tampering_before_state_advance() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (graph_path, state_path, output, _) = single_node_fixture(root);
    let (plan_path, plan) = create_plan(root, "initial", &graph_path, &state_path);
    let (result_path, _) = result_input(root, &node_action_key(&plan, "build-manifest"), &output);
    let receipt_path = root.join("receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();

    fs::write(&output, b"owner outpux").unwrap();
    let advanced = root.join("tampered-state.json");
    let error = advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
        &advanced,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not exactly match current graph")
    );
    assert!(!advanced.exists());
}

#[test]
fn rejects_a_hand_forged_receipt_during_state_advance() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (graph_path, state_path, output, _) = single_node_fixture(root);
    let (plan_path, plan) = create_plan(root, "initial", &graph_path, &state_path);
    let (result_path, mut result) =
        result_input(root, &node_action_key(&plan, "build-manifest"), &output);
    let receipt_path = root.join("receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();

    let forged_key = "4".repeat(64);
    result["action_key"] = json!(forged_key);
    let forged_result_path = write_json(root, "forged-result.json", &result);
    let mut receipt: Value = serde_json::from_slice(&fs::read(&receipt_path).unwrap()).unwrap();
    receipt["action_key"] = json!(forged_key);
    let forged_receipt_path = write_json(root, "forged-receipt.json", &receipt);

    let error = advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &forged_result_path,
        &forged_receipt_path,
        &root.join("forged-state.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("does not match planned node"));
}

#[test]
fn rejects_stale_or_forged_plans_and_non_rebuild_nodes() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (graph_path, state_path, output, input) = single_node_fixture(root);
    let (plan_path, plan) = create_plan(root, "initial", &graph_path, &state_path);
    let action_key = node_action_key(&plan, "build-manifest");
    let (result_path, _) = result_input(root, &action_key, &output);

    let mut forged: Value = serde_json::from_slice(&fs::read(&plan_path).unwrap()).unwrap();
    forged["nodes"][0]["action_key"] = json!("2".repeat(64));
    let forged_path = write_json(root, "forged-plan.json", &forged);
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &forged_path,
        &result_path,
        &root.join("forged-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("does not exactly match"));

    fs::write(&input, b"changed after planning").unwrap();
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &root.join("stale-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("declared current bytes"));

    fs::write(&input, b"owner input").unwrap();
    let receipt_path = root.join("receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();
    let advanced_state = root.join("advanced.json");
    advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
        &advanced_state,
    )
    .unwrap();
    let (reuse_plan, _) = {
        let (path, value) = create_plan(root, "reuse", &graph_path, &advanced_state);
        (path, value)
    };
    let error = write_changed_only_result_receipt(
        &graph_path,
        &advanced_state,
        &reuse_plan,
        &result_path,
        &root.join("reuse-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("only rebuild nodes"));
}

#[test]
fn rejects_incomplete_aliased_outputs_and_wrong_state_lineage() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "recipe.py", b"recipe");
    let output = write_file(root, "output.bin", b"one physical output");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "two-output-proof",
        "nodes": [{
            "node_id": "package",
            "operation_kind": "package",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": [],
            "expected_outputs": ["manifest", "media"]
        }]
    });
    let state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "two-output-proof",
        "nodes": []
    });
    let graph_path = write_json(root, "graph.json", &graph);
    let state_path = write_json(root, "state.json", &state);
    let (plan_path, plan) = create_plan(root, "initial", &graph_path, &state_path);
    let action_key = node_action_key(&plan, "package");

    let incomplete = json!({
        "schema": "reel.changed-only-result-input.v0.1",
        "graph_id": "two-output-proof",
        "node_id": "package",
        "action_key": action_key,
        "owner_result_id_sha256": "3".repeat(64),
        "outcome": "completed",
        "outputs": [{"file_id": "manifest", "path": output}]
    });
    let incomplete_path = write_json(root, "incomplete.json", &incomplete);
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &incomplete_path,
        &root.join("incomplete-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("do not exactly match"));

    let mut aliased = incomplete;
    aliased["outputs"] = json!([
        {"file_id": "manifest", "path": output},
        {"file_id": "media", "path": output}
    ]);
    let aliased_path = write_json(root, "aliased.json", &aliased);
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &aliased_path,
        &root.join("aliased-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("same physical file"));

    let hardlink = root.join("output-hardlink.bin");
    fs::hard_link(&output, &hardlink).unwrap();
    aliased["outputs"][1]["path"] = json!(hardlink);
    let hardlinked_path = write_json(root, "hardlinked.json", &aliased);
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &hardlinked_path,
        &root.join("hardlinked-receipt.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("same physical file"));

    let aliased_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "two-output-proof",
        "nodes": [{
            "node_id": "package",
            "action_key": action_key,
            "outputs": [
                evidence("manifest", &output),
                evidence("media", &hardlink)
            ]
        }]
    });
    let aliased_state_path = write_json(root, "aliased-state.json", &aliased_state);
    let (_, aliased_plan) = create_plan(root, "aliased-prior", &graph_path, &aliased_state_path);
    assert_eq!(aliased_plan["nodes"][0]["status"], "rebuild");
    assert_eq!(aliased_plan["nodes"][0]["reason"], "output-mismatch");

    let second_output = write_file(root, "media.bin", b"media");
    aliased["outputs"][1]["path"] = json!(second_output);
    let valid_path = write_json(root, "valid.json", &aliased);
    let receipt_path = root.join("receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &valid_path,
        &receipt_path,
    )
    .unwrap();
    let reformatted_state = root.join("reformatted-state.json");
    fs::write(&reformatted_state, serde_json::to_vec(&state).unwrap()).unwrap();
    let error = advance_changed_only_state(
        &graph_path,
        &reformatted_state,
        &plan_path,
        &valid_path,
        &receipt_path,
        &root.join("wrong-lineage-state.json"),
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("does not exactly match a plan regenerated")
    );
}

#[test]
fn never_overwrites_receipts_or_advanced_state_and_exposes_both_commands() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let (graph_path, state_path, output, _) = single_node_fixture(root);
    let (plan_path, plan) = create_plan(root, "initial", &graph_path, &state_path);
    let (result_path, _) = result_input(root, &node_action_key(&plan, "build-manifest"), &output);

    let receipt_path = root.join("receipt.json");
    fs::write(&receipt_path, b"owner receipt").unwrap();
    let error = write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(fs::read(&receipt_path).unwrap(), b"owner receipt");

    fs::remove_file(&receipt_path).unwrap();
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();
    let advanced_path = root.join("advanced.json");
    fs::write(&advanced_path, b"owner state").unwrap();
    let error = advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
        &advanced_path,
    )
    .unwrap_err();
    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(fs::read(&advanced_path).unwrap(), b"owner state");

    for command in ["changed-only-result-receipt", "changed-only-state-advance"] {
        let output = Command::new(env!("CARGO_BIN_EXE_reel"))
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{command} help failed");
        assert!(
            String::from_utf8(output.stdout)
                .unwrap()
                .contains("--output-path")
        );
    }
}

#[test]
fn resolves_relative_graph_state_and_result_paths_from_their_contract_files() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "relative-recipe.py", b"recipe");
    let input = write_file(root, "relative-input.yaml", b"input");
    let output = write_file(root, "relative-output.json", b"output");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "relative-path-proof",
        "nodes": [{
            "node_id": "node",
            "operation_kind": "build",
            "recipe": {
                "file_id": "recipe",
                "path": "relative-recipe.py",
                "sha256": hash(&fs::read(&recipe).unwrap()),
                "bytes": fs::metadata(&recipe).unwrap().len()
            },
            "inputs": [{
                "file_id": "input",
                "path": "relative-input.yaml",
                "sha256": hash(&fs::read(&input).unwrap()),
                "bytes": fs::metadata(&input).unwrap().len()
            }],
            "dependencies": [],
            "expected_outputs": ["output"]
        }]
    });
    let empty_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "relative-path-proof",
        "nodes": []
    });
    let graph_path = write_json(root, "relative-graph.json", &graph);
    let state_path = write_json(root, "relative-empty-state.json", &empty_state);
    let (plan_path, plan) = create_plan(root, "relative", &graph_path, &state_path);
    let action_key = node_action_key(&plan, "node");

    let legacy_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "relative-path-proof",
        "nodes": [{
            "node_id": "node",
            "action_key": action_key,
            "outputs": [{
                "file_id": "output",
                "path": "relative-output.json",
                "sha256": hash(&fs::read(&output).unwrap()),
                "bytes": fs::metadata(&output).unwrap().len()
            }]
        }]
    });
    let legacy_state_path = write_json(root, "relative-legacy-state.json", &legacy_state);
    let (_, legacy_reuse) = create_plan(root, "legacy-reuse", &graph_path, &legacy_state_path);
    assert_eq!(legacy_reuse["nodes"][0]["status"], "exact-byte-reuse");

    let result = json!({
        "schema": "reel.changed-only-result-input.v0.1",
        "graph_id": "relative-path-proof",
        "node_id": "node",
        "action_key": action_key,
        "owner_result_id_sha256": "5".repeat(64),
        "outcome": "completed",
        "outputs": [{"file_id": "output", "path": "relative-output.json"}]
    });
    let result_path = write_json(root, "relative-result.json", &result);
    let receipt_path = root.join("relative-receipt.json");
    write_changed_only_result_receipt(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
    )
    .unwrap();
    let advanced_path = root.join("relative-advanced-state.json");
    advance_changed_only_state(
        &graph_path,
        &state_path,
        &plan_path,
        &result_path,
        &receipt_path,
        &advanced_path,
    )
    .unwrap();
    let advanced: Value = serde_json::from_slice(&fs::read(&advanced_path).unwrap()).unwrap();
    let persisted_path =
        PathBuf::from(advanced["nodes"][0]["outputs"][0]["path"].as_str().unwrap());
    assert!(persisted_path.is_absolute());
    assert_eq!(persisted_path, fs::canonicalize(output).unwrap());
}
