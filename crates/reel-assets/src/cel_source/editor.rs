//! Offline native candidate application. No media I/O, selection or publication.
use super::{
    project::{Project, Snapshot},
    sequence::Sequence,
};
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
    let snapshot = load_snapshot(project)?;
    Ok((snapshot.controls, snapshot.hashes))
}
fn load_snapshot(project: &Project) -> Result<Snapshot> {
    let snapshot = project.load()?;
    let docs = &snapshot.controls;
    let hashes = &snapshot.hashes;
    if docs["scene.json"]["schema"] != "reel.scene-authoring.v2"
        || docs["scene-bindings.json"]["schema"] != "reel.scene-asset-bindings.v1"
        || docs["scene.json"]["scene_id"] != docs["scene-bindings.json"]["scope_id"]
    {
        return Err("unsupported native input or scope".into());
    }
    // Validate original native buffers too: Value parsing must not erase a
    // duplicated typed field that the downstream native parser would reject.
    super::inspect(
        &snapshot.raw["scene.json"],
        &snapshot.raw["scene-bindings.json"],
    )?;
    let _: reel_assembly::Graph = serde_json::from_slice(&snapshot.raw["selected-graph.json"])?;
    let _: reel_assembly::SelectedPointer =
        serde_json::from_slice(&snapshot.raw["selected-pointer.json"])?;
    for (role, raw) in &snapshot.raw {
        if role.starts_with("protected-graph:") {
            let _: reel_assembly::Graph = serde_json::from_slice(raw)?;
        } else if role.starts_with("protected-pointer:") {
            let _: reel_assembly::SelectedPointer = serde_json::from_slice(raw)?;
        }
    }
    let sequence = Sequence::for_project(docs, project)?;
    for cue in sequence.cues.iter().chain(&sequence.other_cues) {
        let _: reel_assembly::scene_authoring::NativeAlignment =
            serde_json::from_slice(&snapshot.raw[&cue.alignment_role])?;
        let bound = &docs["scene-bindings.json"]["assets"][&cue.alignment_binding];
        if bound["sha256"] != hashes[&cue.alignment_role]
            || bound["bytes"] != snapshot.bytes[&cue.alignment_role]
        {
            return Err("native alignment binding differs from exact intake bytes".into());
        }
    }
    Ok(snapshot)
}
/// Apply a version-locked request into a new native candidate directory.
/// Sources are unchanged. Failed writes may leave a partial new directory;
/// cache registration, full delivery validation and approval remain external.
pub fn apply(project: &Path, request: &Path, out: &Path) -> Result<()> {
    let r = read(request)?;
    let project = Project::open(project)?;
    let snapshot = load_snapshot(&project)?;
    let mut docs = snapshot.controls.clone();
    let hashes = &snapshot.hashes;
    let scoped = project.language().is_some();
    if !project.accepts_edit(&r)
        || r["expected_inputs"] != *hashes
        || r["scene_id"] != docs["scene.json"]["scene_id"]
    {
        return Err("stale or unsupported edit; source hashes differ".into());
    }
    let sequence = Sequence::for_project(&docs, &project)?;
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
        if !scoped && realization["picture_binding_override"].is_null() {
            e["picture_binding"] = json!(binding);
        }
        events.push(e);
    }
    if scoped {
        let retained: BTreeSet<_> = edited
            .iter()
            .filter_map(|row| row["semantic_id"].as_str())
            .collect();
        let source_ids: BTreeSet<_> = old_language
            .iter()
            .filter_map(|row| row["semantic_id"].as_str())
            .collect();
        let languages: Vec<_> = docs["scene.json"]["languages"]
            .as_object()
            .ok_or("languages")?
            .keys()
            .cloned()
            .collect();
        events = old_events
            .iter()
            .filter_map(|event| {
                let id = event["semantic_id"].as_str()?;
                if !source_ids.contains(id) || retained.contains(id) {
                    return Some(event.clone());
                }
                let declared = event["language_ids"]
                    .as_array()
                    .filter(|ids| !ids.is_empty());
                let remaining: Vec<_> = languages
                    .iter()
                    .filter(|id| {
                        **id != lang
                            && declared.is_none_or(|ids| ids.iter().any(|item| item == id.as_str()))
                    })
                    .cloned()
                    .collect();
                if remaining.is_empty() {
                    None
                } else {
                    let mut event = event.clone();
                    event["language_ids"] = json!(remaining);
                    Some(event)
                }
            })
            .collect();
    }
    let mut language = vec![];
    for row in edited {
        let mut event = old_language
            .iter()
            .find(|e| e["semantic_id"] == row["semantic_id"])
            .ok_or("missing native language event")?
            .clone();
        if (!scoped && !event["picture_binding_override"].is_null())
            || (scoped
                && row["picture_binding"]
                    != sequence.cells[text(row, "semantic_id")?].picture_binding)
        {
            event["picture_binding_override"] = row["picture_binding"].clone();
        }
        language.push(event);
    }
    let mut protected_slots: BTreeSet<String> = if scoped && !project.has_protected_graphs() {
        super::inspect(
            &serde_json::to_vec(&snapshot.controls["scene.json"])?,
            &serde_json::to_vec(&snapshot.controls["scene-bindings.json"])?,
        )?
        .cels
        .into_iter()
        .filter(|cell| cell.language != lang)
        .map(|cell| cell.picture_slot_id)
        .collect()
    } else {
        BTreeSet::new()
    };
    if scoped && !project.has_protected_graphs() {
        for (language, lane) in docs["scene.json"]["languages"]
            .as_object()
            .ok_or("languages")?
        {
            if language != &lang {
                for cue in lane["cues"].as_array().ok_or("cues")? {
                    protected_slots.insert(text(cue, "narration_slot_id")?.into());
                }
            }
        }
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
        if protected_slots.contains(slot) {
            let original = docs["selected-graph.json"]["slots"]
                .as_array()
                .ok_or("slots")?
                .iter()
                .find(|item| item["slot_id"] == slot)
                .ok_or("slot")?;
            let revision = original["revisions"]
                .as_array()
                .ok_or("revisions")?
                .iter()
                .find(|item| item["revision_id"] == original["selected_revision_id"])
                .ok_or("selected revision")?;
            if ["logical_id", "cache_uri", "sha256"]
                .iter()
                .any(|key| revision["asset"][*key] != desired[*key])
            {
                return Err("Picture slot is shared by another language; explicit slot separation is required".into());
            }
        }
        slot_assets.insert(slot.to_owned(), desired.clone());
    }
    let mut graph_events = if scoped {
        docs["selected-graph.json"]["events"]
            .as_array()
            .ok_or("events")?
            .iter()
            .filter(|event| event["language"] != lang)
            .cloned()
            .collect()
    } else {
        vec![]
    };
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
        let bytes = if docs[&cue.alignment_role] == snapshot.controls[&cue.alignment_role] {
            snapshot.raw[&cue.alignment_role].clone()
        } else {
            serde_json::to_vec(&docs[&cue.alignment_role])?
        };
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
        let source_ids: BTreeSet<_> = old_language
            .iter()
            .filter_map(|row| row["event_id"].as_str())
            .collect();
        let mut active: Vec<_> = if scoped {
            nodes[0]["events"]
                .as_array()
                .ok_or("node events")?
                .iter()
                .filter(|id| !source_ids.contains(id.as_str().unwrap_or("")))
                .cloned()
                .collect()
        } else {
            vec![]
        };
        active.extend(language.iter().map(|event| event["event_id"].clone()));
        nodes[0]["events"] = json!(active);
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
                    || protected_slots.contains(s.as_str().unwrap_or(""))
            });
    }
    let slots = graph["slots"].as_array_mut().ok_or("graph slots")?;
    slots.retain(|s| {
        s["lane"] != "picture"
            || slot_assets.contains_key(s["slot_id"].as_str().unwrap_or(""))
            || protected_slots.contains(s["slot_id"].as_str().unwrap_or(""))
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
    if scoped {
        preserve_other_lanes(&snapshot.controls, &mut docs, &sequence, &protected_slots)?;
    }
    Sequence::for_project(&docs, &project)?;
    project.write_draft_from_snapshot(out, &docs, &r, &snapshot)?;
    Ok(())
}

/// Refuse any semantic change to another lane, then retain its original graph
/// representation too (including explicit null/default spellings).
fn preserve_other_lanes(
    before: &BTreeMap<String, Value>,
    after: &mut BTreeMap<String, Value>,
    sequence: &Sequence,
    protected: &BTreeSet<String>,
) -> Result<()> {
    use reel_assembly::{self as assembly, scene_authoring as native};
    let old: native::Scene = serde_json::from_value(before["scene.json"].clone())?;
    let new: native::Scene = serde_json::from_value(after["scene.json"].clone())?;
    let old = native::materialize_scene(&old)?;
    let new = native::materialize_scene(&new)?;
    for language in old
        .languages
        .keys()
        .filter(|language| **language != sequence.language)
    {
        if serde_json::to_value(&old.languages[language])?
            != serde_json::to_value(
                new.languages
                    .get(language)
                    .ok_or("Protected language disappeared")?,
            )?
            || before["scene.json"]["language_event_bindings"][language]
                != after["scene.json"]["language_event_bindings"][language]
        {
            return Err("Edited candidate changed another language".into());
        }
    }
    for cue in &sequence.other_cues {
        if before[&cue.alignment_role] != after[&cue.alignment_role] {
            return Err("Edited candidate changed another alignment".into());
        }
    }
    let mut old_assets = before["scene-bindings.json"].clone();
    let mut new_assets = after["scene-bindings.json"].clone();
    for cue in &sequence.cues {
        old_assets["assets"]
            .as_object_mut()
            .ok_or("assets")?
            .remove(&cue.alignment_binding);
        new_assets["assets"]
            .as_object_mut()
            .ok_or("assets")?
            .remove(&cue.alignment_binding);
    }
    if old_assets != new_assets {
        return Err("Edited candidate changed unrelated asset bindings".into());
    }
    for (role, value) in before.iter().filter(|(role, _)| {
        role.starts_with("protected-graph:") || role.starts_with("protected-pointer:")
    }) {
        if after.get(role) != Some(value) {
            return Err("Edited candidate changed another language graph or pointer".into());
        }
    }
    let old_graph: assembly::Graph = serde_json::from_value(before["selected-graph.json"].clone())?;
    let new_graph: assembly::Graph = serde_json::from_value(after["selected-graph.json"].clone())?;
    for id in protected {
        if serde_json::to_value(old_graph.slots.iter().find(|slot| &slot.slot_id == id))?
            != serde_json::to_value(new_graph.slots.iter().find(|slot| &slot.slot_id == id))?
        {
            return Err("Edited candidate changed a protected picture slot".into());
        }
    }
    let others = |graph: &assembly::Graph| {
        serde_json::to_value(
            graph
                .events
                .iter()
                .filter(|event| event.language != sequence.language)
                .collect::<Vec<_>>(),
        )
    };
    if others(&old_graph)? != others(&new_graph)? {
        return Err("Edited candidate changed protected graph events".into());
    }
    let other_events: BTreeSet<_> = old_graph
        .events
        .iter()
        .filter(|event| event.language != sequence.language)
        .map(|event| event.event_id.as_str())
        .collect();
    let old_node = &old_graph.nodes[0];
    let new_node = &new_graph.nodes[0];
    if old_node
        .events
        .iter()
        .filter(|id| other_events.contains(id.as_str()))
        .collect::<Vec<_>>()
        != new_node
            .events
            .iter()
            .filter(|id| other_events.contains(id.as_str()))
            .collect::<Vec<_>>()
        || old_node
            .slots
            .iter()
            .filter(|id| protected.contains(*id))
            .collect::<Vec<_>>()
            != new_node
                .slots
                .iter()
                .filter(|id| protected.contains(*id))
                .collect::<Vec<_>>()
    {
        return Err("Edited candidate changed another language's active closure".into());
    }
    let graph = after.get_mut("selected-graph.json").ok_or("graph")?;
    for (role, id_key) in [("slots", "slot_id"), ("events", "event_id")] {
        for record in graph[role].as_array_mut().ok_or("graph records")? {
            let protected = if role == "slots" {
                protected.contains(record[id_key].as_str().unwrap_or(""))
            } else {
                record["language"] != sequence.language
            };
            if protected {
                *record = before["selected-graph.json"][role]
                    .as_array()
                    .ok_or("source records")?
                    .iter()
                    .find(|item| item[id_key] == record[id_key])
                    .ok_or("Protected graph record missing")?
                    .clone();
            }
        }
    }
    Ok(())
}
