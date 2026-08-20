use reel::changed_only::write_changed_only_plan;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::os::windows::fs::OpenOptionsExt;

fn write_file(root: &Path, name: &str, contents: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, contents).unwrap();
    path
}

fn evidence(file_id: &str, path: &Path) -> Value {
    let bytes = fs::read(path).unwrap();
    let sha256: String = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    json!({
        "file_id": file_id,
        "path": path,
        "sha256": sha256,
        "bytes": bytes.len(),
    })
}

fn write_json(root: &Path, name: &str, value: &Value) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    path
}

fn run_plan(root: &Path, name: &str, graph: &Value, state: &Value) -> (PathBuf, Value) {
    let graph_path = write_json(root, &format!("{name}-graph.json"), graph);
    let state_path = write_json(root, &format!("{name}-state.json"), state);
    let output_path = root.join(format!("{name}-plan.json"));
    write_changed_only_plan(&graph_path, &state_path, &output_path).unwrap();
    let value = serde_json::from_slice(&fs::read(&output_path).unwrap()).unwrap();
    (output_path, value)
}

fn action_key(plan: &Value, node_id: &str) -> String {
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

fn status<'a>(plan: &'a Value, node_id: &str) -> (&'a str, &'a str) {
    let node = plan["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == node_id)
        .unwrap();
    (
        node["status"].as_str().unwrap(),
        node["reason"].as_str().unwrap(),
    )
}

#[test]
fn plans_exact_reuse_changed_roots_and_only_their_downstream_dependents() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe_a = write_file(root, "recipe-a.py", b"recipe a");
    let input_a = write_file(root, "input-a.txt", b"source a v1");
    let output_a = write_file(root, "output-a.json", b"output a v1");
    let recipe_b = write_file(root, "recipe-b.py", b"recipe b");
    let input_b = write_file(root, "input-b.txt", b"source b");
    let output_b = write_file(root, "output-b.json", b"output b");
    let recipe_c = write_file(root, "recipe-c.py", b"recipe c");
    let output_c = write_file(root, "output-c.json", b"output c");

    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "bertica-s1e01-proof",
        "nodes": [
            {
                "node_id": "source-register",
                "operation_kind": "contract-build",
                "recipe": evidence("register-script", &recipe_a),
                "inputs": [evidence("source-contract", &input_a)],
                "dependencies": [],
                "expected_outputs": ["register"]
            },
            {
                "node_id": "measured-conform",
                "operation_kind": "manifest-build",
                "recipe": evidence("conform-script", &recipe_b),
                "inputs": [evidence("timing-contract", &input_b)],
                "dependencies": ["source-register"],
                "expected_outputs": ["manifest"]
            },
            {
                "node_id": "release-notes",
                "operation_kind": "document-build",
                "recipe": evidence("notes-script", &recipe_c),
                "inputs": [],
                "dependencies": [],
                "expected_outputs": ["notes"]
            }
        ]
    });
    let empty_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "bertica-s1e01-proof",
        "nodes": []
    });

    let (_, first) = run_plan(root, "first", &graph, &empty_state);
    assert_eq!(
        status(&first, "source-register"),
        ("rebuild", "missing-prior-state")
    );
    assert_eq!(
        status(&first, "measured-conform"),
        ("blocked-dependency", "dependency-not-reusable")
    );
    let register_key = action_key(&first, "source-register");
    let notes_key = action_key(&first, "release-notes");

    let roots_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "bertica-s1e01-proof",
        "nodes": [
            {
                "node_id": "source-register",
                "action_key": register_key,
                "outputs": [evidence("register", &output_a)]
            },
            {
                "node_id": "release-notes",
                "action_key": notes_key,
                "outputs": [evidence("notes", &output_c)]
            }
        ]
    });
    let (_, second) = run_plan(root, "second", &graph, &roots_state);
    assert_eq!(
        status(&second, "source-register"),
        ("exact-byte-reuse", "action-and-outputs-match")
    );
    assert_eq!(
        status(&second, "measured-conform"),
        ("rebuild", "missing-prior-state")
    );
    let conform_key = action_key(&second, "measured-conform");

    let full_state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "bertica-s1e01-proof",
        "nodes": [
            {
                "node_id": "source-register",
                "action_key": action_key(&second, "source-register"),
                "outputs": [evidence("register", &output_a)]
            },
            {
                "node_id": "measured-conform",
                "action_key": conform_key,
                "outputs": [evidence("manifest", &output_b)]
            },
            {
                "node_id": "release-notes",
                "action_key": action_key(&second, "release-notes"),
                "outputs": [evidence("notes", &output_c)]
            }
        ]
    });
    let (third_path, third) = run_plan(root, "third", &graph, &full_state);
    assert_eq!(third["summary"]["exact_byte_reuse_count"], 3);
    assert_eq!(third["summary"]["rebuild_count"], 0);
    assert_eq!(third["authority"]["executes_builds"], false);
    assert_eq!(third["authority"]["grants_approval"], false);
    let serialized = fs::read_to_string(third_path).unwrap();
    assert!(!serialized.contains(&root.display().to_string()));
    assert!(!serialized.contains("source a v1"));

    fs::write(&input_a, b"source a v2").unwrap();
    let mut changed_graph = graph.clone();
    changed_graph["nodes"][0]["inputs"][0] = evidence("source-contract", &input_a);
    let (_, changed) = run_plan(root, "changed", &changed_graph, &full_state);
    assert_eq!(
        status(&changed, "source-register"),
        ("rebuild", "action-key-changed")
    );
    assert_eq!(
        status(&changed, "measured-conform"),
        ("blocked-dependency", "dependency-not-reusable")
    );
    assert_eq!(
        status(&changed, "release-notes"),
        ("exact-byte-reuse", "action-and-outputs-match")
    );
}

#[test]
fn rejects_tampered_outputs_from_reuse_and_blocks_dependents() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "recipe.py", b"recipe");
    let output = write_file(root, "output.bin", b"original");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "tamper-proof",
        "nodes": [{
            "node_id": "root",
            "operation_kind": "render",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": [],
            "expected_outputs": ["output"]
        }, {
            "node_id": "dependent",
            "operation_kind": "package",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": ["root"],
            "expected_outputs": ["package"]
        }]
    });
    let empty = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "tamper-proof",
        "nodes": []
    });
    let (_, initial) = run_plan(root, "initial", &graph, &empty);
    let state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "tamper-proof",
        "nodes": [{
            "node_id": "root",
            "action_key": action_key(&initial, "root"),
            "outputs": [evidence("output", &output)]
        }]
    });
    fs::write(&output, b"tampered").unwrap();
    let (_, plan) = run_plan(root, "tampered", &graph, &state);
    assert_eq!(status(&plan, "root"), ("rebuild", "output-mismatch"));
    assert_eq!(
        status(&plan, "dependent"),
        ("blocked-dependency", "dependency-not-reusable")
    );
}

#[test]
fn rejects_cycles_unknown_dependencies_and_declared_input_mismatches() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "recipe.py", b"recipe");
    let empty = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "invalid-graph",
        "nodes": []
    });
    let state_path = write_json(root, "invalid-state.json", &empty);

    let cycle = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "invalid-graph",
        "nodes": [{
            "node_id": "a",
            "operation_kind": "build",
            "recipe": evidence("recipe-a", &recipe),
            "inputs": [],
            "dependencies": ["b"],
            "expected_outputs": ["a-output"]
        }, {
            "node_id": "b",
            "operation_kind": "build",
            "recipe": evidence("recipe-b", &recipe),
            "inputs": [],
            "dependencies": ["a"],
            "expected_outputs": ["b-output"]
        }]
    });
    let cycle_path = write_json(root, "cycle.json", &cycle);
    let error = write_changed_only_plan(&cycle_path, &state_path, &root.join("cycle-plan.json"))
        .unwrap_err();
    assert!(error.to_string().contains("dependency cycle"));

    let unknown = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "invalid-graph",
        "nodes": [{
            "node_id": "a",
            "operation_kind": "build",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": ["missing"],
            "expected_outputs": ["output"]
        }]
    });
    let unknown_path = write_json(root, "unknown.json", &unknown);
    let error =
        write_changed_only_plan(&unknown_path, &state_path, &root.join("unknown-plan.json"))
            .unwrap_err();
    assert!(error.to_string().contains("unknown node"));

    let mut mismatched = unknown;
    mismatched["nodes"][0]["dependencies"] = json!([]);
    mismatched["nodes"][0]["recipe"]["sha256"] = json!("0".repeat(64));
    let mismatch_path = write_json(root, "mismatch.json", &mismatched);
    let error = write_changed_only_plan(
        &mismatch_path,
        &state_path,
        &root.join("mismatch-plan.json"),
    )
    .unwrap_err();
    assert!(error.to_string().contains("declared current bytes"));
}

#[test]
fn requires_json_output_and_never_overwrites_an_existing_plan() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "recipe.py", b"recipe");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "publication-proof",
        "nodes": [{
            "node_id": "node",
            "operation_kind": "build",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": [],
            "expected_outputs": ["output"]
        }]
    });
    let state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "publication-proof",
        "nodes": []
    });
    let graph_path = write_json(root, "graph.json", &graph);
    let state_path = write_json(root, "state.json", &state);

    let error =
        write_changed_only_plan(&graph_path, &state_path, &root.join("plan.txt")).unwrap_err();
    assert!(error.to_string().contains(".json extension"));

    let output = root.join("plan.json");
    fs::write(&output, b"owner data").unwrap();
    let error = write_changed_only_plan(&graph_path, &state_path, &output).unwrap_err();
    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(fs::read(output).unwrap(), b"owner data");
}

#[test]
fn exposes_the_changed_only_planner_through_the_cli() {
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .args(["changed-only-plan", "--help"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("<GRAPH>"));
    assert!(stdout.contains("<STATE>"));
    assert!(stdout.contains("--output-path"));
}

#[cfg(windows)]
#[test]
fn classifies_a_sharing_locked_prior_output_as_unavailable() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();
    let recipe = write_file(root, "recipe.py", b"recipe");
    let output = write_file(root, "output.bin", b"output");
    let graph = json!({
        "schema": "reel.changed-only-graph.v0.1",
        "graph_id": "locked-output-proof",
        "nodes": [{
            "node_id": "node",
            "operation_kind": "build",
            "recipe": evidence("recipe", &recipe),
            "inputs": [],
            "dependencies": [],
            "expected_outputs": ["output"]
        }]
    });
    let empty = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "locked-output-proof",
        "nodes": []
    });
    let (_, initial) = run_plan(root, "locked-initial", &graph, &empty);
    let state = json!({
        "schema": "reel.changed-only-state.v0.1",
        "graph_id": "locked-output-proof",
        "nodes": [{
            "node_id": "node",
            "action_key": action_key(&initial, "node"),
            "outputs": [evidence("output", &output)]
        }]
    });

    let _lock = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&output)
        .unwrap();
    let (_, plan) = run_plan(root, "locked", &graph, &state);
    assert_eq!(status(&plan, "node"), ("rebuild", "output-unavailable"));
}
