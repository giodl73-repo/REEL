//! Session-local diagnostics over existing REEL contracts. Never a release receipt.
use reel_assembly::{Graph, SelectedPointer};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

pub const MAX_BYTES: usize = 131_072;
pub const MAX_POINTER_BYTES: usize = 8192;
pub const MAX_REQUEST_BYTES: usize = 2_097_152;
pub const MAX_REPORT_BYTES: usize = 2_097_152;
pub const MAX_NODES: usize = 128;
pub const MAX_ITEMS: usize = 256;
pub const MAX_REFERENCES: usize = 1024;
const SCOPE: &str = "Local dependency diagnostic containing caller identities, not a privacy-safe release receipt. Native validation and pointer matching do not verify cache bytes, media availability, provenance, rights, consent, creative selection or release approval. Unselected slots are omitted by native closure calculation; success does not establish production readiness. No media or cache URI is opened.";

fn admit(graph: &Graph) -> Result<(), String> {
    if graph.nodes.len() > MAX_NODES
        || graph.slots.len() > MAX_ITEMS
        || graph.events.len() > MAX_ITEMS
        || graph.presentation_targets.len() > MAX_ITEMS
    {
        return Err("Workbench admits at most 128 nodes and 256 slots, events and presentation targets each.".into());
    }
    let references: usize = graph
        .nodes
        .iter()
        .map(|n| n.inputs.len() + n.slots.len() + n.events.len())
        .sum();
    let revisions: usize = graph.slots.iter().map(|s| s.revisions.len()).sum();
    if references > MAX_REFERENCES || revisions > MAX_REFERENCES {
        return Err("Workbench admits at most 1024 total node references and 1024 retained revisions; nothing was truncated.".into());
    }
    Ok(())
}

fn failed(mut report: Value, stage: &str, error: impl ToString) -> Value {
    report["ok"] = json!(false);
    report["stage"] = json!(stage);
    report["error"] = json!(error.to_string());
    report
}

fn evaluate(
    graph_text: &str,
    target: &str,
    kind: &str,
    pointer_text: &str,
) -> Result<Value, String> {
    if graph_text.len() > MAX_BYTES || pointer_text.len() > MAX_POINTER_BYTES || target.len() > 256
    {
        return Err("Workbench input limit: 128 KiB graph, 8 KiB pointer and 256 UTF-8 target bytes; nothing was truncated.".into());
    }
    if !matches!(kind, "node" | "presentation") {
        return Err("Target kind must be node or presentation.".into());
    }
    let graph: Graph = serde_json::from_str(graph_text).map_err(|e| format!("Graph JSON: {e}"))?;
    admit(&graph)?;
    let pointer: Option<SelectedPointer> = if pointer_text.is_empty() {
        None
    } else {
        Some(serde_json::from_str(pointer_text).map_err(|e| format!("Pointer JSON: {e}"))?)
    };
    let mut report = json!({
        "ok":true,"scope":SCOPE,"input_bytes":graph_text.len(),
        "target":target,"target_kind":kind,"graph":graph,"pointer":pointer,
        "graph_valid":false,"closure":null,"presentation":null,"selected":null,
        "computed_graph_lock":null,"declared_lock_matches_computed":null,
        "pointer_matches_supplied_graph_lock":null
    });
    if let Err(error) = reel_assembly::validate_graph(&graph) {
        return Ok(failed(report, "native graph validation", error));
    }
    report["graph_valid"] = json!(true);
    let computed =
        reel_assembly::graph_lock(&graph, &graph.lock.logical_id).map_err(|e| e.to_string())?;
    report["declared_lock_matches_computed"] = json!(computed.sha256 == graph.lock.sha256);
    report["computed_graph_lock"] = json!(computed);
    if let Some(pointer) = &pointer {
        if let Err(error) = reel_assembly::validate_selected_graph(pointer, &graph) {
            report["pointer_matches_supplied_graph_lock"] = json!(false);
            return Ok(failed(report, "native pointer matching", error));
        }
        report["pointer_matches_supplied_graph_lock"] = json!(true);
    }
    let closure = if kind == "presentation" {
        match reel_assembly::presentation_closure(&graph, target) {
            Ok(presentation) => {
                let closure = presentation.closure.clone();
                report["presentation"] = json!(presentation);
                closure
            }
            Err(error) => return Ok(failed(report, "native presentation closure", error)),
        }
    } else {
        match reel_assembly::closure(&graph, target) {
            Ok(closure) => closure,
            Err(error) => return Ok(failed(report, "native node closure", error)),
        }
    };
    if let Some(pointer) = &pointer {
        match reel_assembly::selected_closure(pointer, &graph, &closure.target) {
            Ok(selected) => report["selected"] = json!(selected),
            Err(error) => return Ok(failed(report, "native selected closure", error)),
        }
    }
    report["closure"] = json!(closure);
    Ok(report)
}

#[wasm_bindgen]
pub fn inspect_assembly(graph_text: &str, target: &str, kind: &str, pointer_text: &str) -> String {
    let value = evaluate(graph_text, target, kind, pointer_text).unwrap_or_else(
        |error| json!({"ok":false,"stage":"workbench admission","error":error,"scope":SCOPE}),
    );
    let text = serde_json::to_string_pretty(&value).expect("JSON values serialize");
    if text.len() > MAX_REPORT_BYTES {
        return serde_json::to_string_pretty(&json!({"ok":false,"stage":"workbench report budget","error":"Report exceeds 2 MiB; reduce input. Nothing was truncated.","scope":SCOPE})).expect("JSON values serialize");
    }
    text
}

/// Native CLI transport only; nested graph/pointer parsing uses original types.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub graph_text: String,
    pub target: String,
    pub kind: String,
    pub pointer_text: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLE: &str = include_str!("../fixtures/graph.json");
    fn report(text: &str, target: &str, pointer: &str) -> Value {
        serde_json::from_str(&inspect_assembly(text, target, "node", pointer)).unwrap()
    }
    #[test]
    fn native_closure_omits_unselected_slots_without_claiming_readiness() {
        let r = report(EXAMPLE, "episode", "");
        assert_eq!(r["ok"], true);
        assert_eq!(r["closure"]["node_ids"], json!(["episode", "scene"]));
        assert_eq!(r["closure"]["selected_assets"].as_array().unwrap().len(), 2);
        assert_eq!(r["graph"]["slots"][1]["disposition"], "unselected");
        assert_eq!(r["declared_lock_matches_computed"], false);
        assert_eq!(r["pointer_matches_supplied_graph_lock"], Value::Null);
    }
    #[test]
    fn matching_declared_pointer_does_not_imply_computed_lock_identity() {
        let graph: Graph = serde_json::from_str(EXAMPLE).unwrap();
        let pointer = json!({"schema":"reel.selected-pointer.v1","logical_id":"current-review","selected_lock":graph.lock});
        let r = report(EXAMPLE, "episode", &pointer.to_string());
        assert_eq!(r["ok"], true);
        assert_eq!(r["pointer_matches_supplied_graph_lock"], true);
        assert_eq!(r["declared_lock_matches_computed"], false);
        assert_eq!(r["selected"]["closure"], r["closure"]);
    }
    #[test]
    fn reachable_cycles_are_native_closure_errors_not_graph_validation_errors() {
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["nodes"][0]["inputs"] = json!(["episode"]);
        let r = report(&v.to_string(), "episode", "");
        assert_eq!(r["ok"], false);
        assert_eq!(r["graph_valid"], true);
        assert!(r["error"].as_str().unwrap().contains("cycle"));
    }
    #[test]
    fn graph_extensions_pointer_errors_and_resource_excess_reject_whole() {
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["unexpected"] = json!(true);
        assert_eq!(report(&v.to_string(), "episode", "")["ok"], false);
        assert_eq!(report(EXAMPLE, "episode", " ")["ok"], false);
        assert_eq!(
            report(&" ".repeat(MAX_BYTES + 1), "episode", "")["ok"],
            false
        );
        let mut v: Value = serde_json::from_str(EXAMPLE).unwrap();
        v["nodes"] = json!(vec![v["nodes"][0].clone(); MAX_NODES + 1]);
        assert_eq!(
            report(&v.to_string(), "episode", "")["stage"],
            "workbench admission"
        );
    }
}
