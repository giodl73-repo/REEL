//! Offline native candidate application. No media I/O, selection or publication.
use super::{project::Project, sequence::Sequence};
use crate::{Result, tiering};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing native {key}").into())
}
fn read(path: &Path) -> Result<Value> {
    Ok(serde_json::from_slice(&tiering::read_bounded(
        path,
        2 * 1024 * 1024,
    )?)?)
}
fn hash(bytes: &[u8]) -> String {
    tiering::digest(bytes)
}
/// Load only explicit controls and verify native scope, selections, cue clocks
/// and exact raw alignment identities. Descriptor hashes are checked by apply.
pub fn load(project: &Project) -> Result<(BTreeMap<String, Value>, Value)> {
    let snapshot = project.load()?;
    let docs = snapshot.controls;
    let hashes = snapshot.hashes;
    if docs["scene.json"]["schema"] != "reel.scene-authoring.v2"
        || docs["scene-bindings.json"]["schema"] != "reel.scene-asset-bindings.v1"
        || docs["scene.json"]["scene_id"] != docs["scene-bindings.json"]["scope_id"]
    {
        return Err("unsupported native input or scope".into());
    }
    let sequence = Sequence::new(&docs, project.is_multi_cue())?;
    for cue in &sequence.cues {
        let bound = &docs["scene-bindings.json"]["assets"][&cue.alignment_binding];
        if bound["sha256"] != hashes[&cue.alignment_role]
            || bound["bytes"] != snapshot.bytes[&cue.alignment_role]
        {
            return Err("native alignment binding differs from exact intake bytes".into());
        }
    }
    Ok((docs, hashes))
}
/// Apply a version-locked request into a new native candidate directory.
/// Sources are unchanged. Failed writes may leave a partial new directory;
/// cache registration, full delivery validation and approval remain external.
pub fn apply(project: &Path, request: &Path, out: &Path) -> Result<()> {
    let r = read(request)?;
    let project = Project::open(project)?;
    let (mut docs, hashes) = load(&project)?;
    if !project.accepts_edit(&r)
        || r["expected_inputs"] != hashes
        || r["scene_id"] != docs["scene.json"]["scene_id"]
    {
        return Err("stale or unsupported edit; source hashes differ".into());
    }
    let sequence = Sequence::new(&docs, project.is_multi_cue())?;
    let edited = r["rows"].as_array().ok_or("missing rows")?;
    let plan = sequence.plan(edited)?;
    let old_events = docs["scene.json"]["shared_events"]
        .as_array()
        .ok_or("events")?
        .clone();
    let lang = sequence.language.clone();
    let old_language = docs["scene.json"]["language_event_bindings"][&lang]
        .as_array()
        .ok_or("missing language events")?
        .clone();
    let mut events = vec![];
    for row in edited {
        let id = text(row, "semantic_id")?;
        let binding = text(row, "picture_binding")?;
        let mut e = old_events
            .iter()
            .find(|e| e["semantic_id"] == id)
            .ok_or("event absent")?
            .clone();
        let realization = old_language
            .iter()
            .find(|event| event["semantic_id"] == id)
            .ok_or("missing native language event")?;
        // Preserve shared defaults when the source deliberately chose a local
        // override. A no-op round trip must not rewrite the editorial layer.
        if realization["picture_binding_override"].is_null() {
            e["picture_binding"] = json!(binding);
        }
        events.push(e);
    }
    let mut language = vec![];
    for row in edited {
        let mut event = old_language
            .iter()
            .find(|e| e["semantic_id"] == row["semantic_id"])
            .ok_or("missing native language event")?
            .clone();
        if !event["picture_binding_override"].is_null() {
            event["picture_binding_override"] = row["picture_binding"].clone();
        }
        language.push(event);
    }
    let mut slot_assets: BTreeMap<String, Value> = BTreeMap::new();
    for (row, e) in edited.iter().zip(&language) {
        let slot = text(e, "picture_slot_id")?;
        let desired = &docs["scene-bindings.json"]["assets"][text(row, "picture_binding")?];
        if let Some(previous) = slot_assets.get(slot)
            && ["logical_id", "cache_uri", "sha256"]
                .iter()
                .any(|key| previous[*key] != desired[*key])
        {
            return Err("one selected picture slot cannot bind different edited assets".into());
        }
        slot_assets.insert(slot.to_owned(), desired.clone());
    }
    let mut graph_events = vec![];
    if let Some(existing) = docs["selected-graph.json"]["events"].as_array() {
        for ((row, e), timing) in edited.iter().zip(&language).zip(&plan.cells) {
            let event_id = text(e, "event_id")?;
            let mut event = existing
                .iter()
                .find(|x| x["event_id"] == event_id)
                .ok_or("missing native graph event")?
                .clone();
            event["phrase_start_seconds"] =
                json!(timing.start_sample as f64 / timing.sample_rate as f64);
            event["phrase_end_seconds"] =
                json!(timing.end_sample as f64 / timing.sample_rate as f64);
            let asset = &docs["scene-bindings.json"]["assets"][text(row, "picture_binding")?];
            for key in ["logical_id", "sha256"] {
                event["picture"][key] = asset[key].clone();
            }
            graph_events.push(event);
        }
    }
    docs.get_mut("scene.json").ok_or("scene")?["shared_events"] = json!(events);
    docs.get_mut("scene.json").ok_or("scene")?["language_event_bindings"][&lang] = json!(language);
    for cue in &sequence.cues {
        docs.get_mut(&cue.alignment_role).ok_or("alignment")?["semantic_markers"] =
            serde_json::to_value(&plan.markers[&cue.cue_id])?;
        let bytes = serde_json::to_vec(&docs[&cue.alignment_role])?;
        let digest = hash(&bytes);
        let asset = &mut docs.get_mut("scene-bindings.json").ok_or("bindings")?["assets"]
            [&cue.alignment_binding];
        if asset["sha256"] != digest {
            asset["selection_state"] = json!("selected-private-production");
        }
        asset["sha256"] = json!(digest);
        asset["bytes"] = json!(bytes.len());
        asset["cache_uri"] = json!(format!("cache://sha256/{digest}"));
    }
    let graph = docs.get_mut("selected-graph.json").ok_or("graph")?;
    if !graph_events.is_empty() {
        graph["events"] = json!(graph_events);
    }
    if let Some(nodes) = graph["nodes"].as_array_mut() {
        if nodes.len() != 1 {
            return Err("unsupported graph scope: requires one scene node".into());
        }
        nodes[0]["events"] = json!(
            language
                .iter()
                .map(|e| e["event_id"].clone())
                .collect::<Vec<_>>()
        );
        let retained_slots = slot_assets.keys().cloned().collect::<BTreeSet<_>>();
        let picture_slots = old_language
            .iter()
            .filter_map(|e| e["picture_slot_id"].as_str())
            .collect::<BTreeSet<_>>();
        nodes[0]["slots"]
            .as_array_mut()
            .ok_or("node slots")?
            .retain(|s| {
                !picture_slots.contains(s.as_str().unwrap_or(""))
                    || retained_slots.contains(s.as_str().unwrap_or(""))
            });
    }
    let slots = graph["slots"].as_array_mut().ok_or("graph slots")?;
    slots.retain(|s| {
        s["lane"] != "picture" || slot_assets.contains_key(s["slot_id"].as_str().unwrap_or(""))
    });
    let mut typed: reel_assembly::Graph = serde_json::from_value(graph.clone())?;
    let mut revisions = Vec::new();
    for slot in &typed.slots {
        if let Some(asset) = slot_assets.get(&slot.slot_id) {
            let current = slot
                .revisions
                .iter()
                .find(|revision| Some(&revision.revision_id) == slot.selected_revision_id.as_ref())
                .ok_or("missing selected picture revision")?;
            let desired = reel_assembly::Asset {
                logical_id: text(asset, "logical_id")?.into(),
                cache_uri: text(asset, "cache_uri")?.into(),
                sha256: text(asset, "sha256")?.into(),
            };
            if current.asset.logical_id != desired.logical_id
                || current.asset.sha256 != desired.sha256
                || current.asset.cache_uri != desired.cache_uri
            {
                let id = hash(&serde_json::to_vec(
                    &json!({"source_lock":typed.lock,"slot":slot.slot_id,"asset":desired}),
                )?);
                revisions.push(reel_assembly::SlotRevisionSelection {
                    slot_id: slot.slot_id.clone(),
                    revision: reel_assembly::Revision {
                        revision_id: format!("cel-edit-{id}"),
                        supersedes: Some(current.revision_id.clone()),
                        asset: desired,
                    },
                });
            }
        }
    }
    if revisions.is_empty() {
        typed.lock = reel_assembly::graph_lock(&typed, &typed.lock.logical_id)?;
    } else {
        typed = reel_assembly::append_selected_revisions(
            &typed,
            &reel_assembly::SlotRevisionBatchRequest {
                schema: reel_assembly::REVISION_BATCH_REQUEST_SCHEMA.into(),
                next_lock_logical_id: typed.lock.logical_id.clone(),
                revisions,
            },
        )?;
    }
    *graph = serde_json::to_value(typed)?;
    let lock = graph["lock"].clone();
    docs.get_mut("selected-pointer.json").ok_or("pointer")?["selected_lock"] = lock;
    project.write_draft(out, &docs, &r)?;
    Ok(())
}
