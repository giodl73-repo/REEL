//! Explicit producer authoring: split one native hold into two stable cels.
//! No media lookup, registry admission, selection, rendering or approval.
use super::{editor, project::Project, sequence::Sequence};
use crate::{Result, tiering};
use reel_assembly::{self as assembly, scene_authoring as native};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    scene_id: String,
    language: String,
    #[serde(default)]
    expected_project: Option<String>,
    #[serde(deserialize_with = "super::project::unique_files")]
    expected_inputs: BTreeMap<String, String>,
    after_semantic_id: String,
    #[serde(default)]
    split_after_frames: Option<u64>,
    #[serde(default)]
    split_after_samples: Option<u64>,
    shared_event: native::SharedEvent,
    language_event: native::LanguageEventBinding,
    #[serde(default)]
    human_approval: bool,
    #[serde(default, rename = "notes")]
    _notes: String,
}

fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing native {key}").into())
}
fn new_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.trim() == id && !id.chars().any(char::is_control)
}

/// Create a fresh native source with one additional cel inside an existing hold.
/// Caller-authored event fragments use the native scene types. Only an already
/// selected or explicitly admitted picture binding is eligible. All validation
/// precedes output creation; failed I/O may leave a partial new directory.
pub fn apply(project: &Path, request: &Path, output: &Path) -> Result<()> {
    let bytes = tiering::read_bounded(request, 2 * 1024 * 1024)?;
    let r: Request = serde_json::from_slice(&bytes)?;
    let project = Project::open(project)?;
    let snapshot = editor::load_snapshot(&project)?;
    let mut docs = snapshot.controls.clone();
    let sequence = Sequence::for_project(&docs, &project)?;
    if r.schema != "reel.cel-sorter-insert.v1"
        || r.scene_id != docs["scene.json"]["scene_id"]
        || r.language != sequence.language
        || r.expected_project.as_deref() != project.descriptor_sha256()
        || serde_json::to_value(&r.expected_inputs)? != snapshot.hashes
        || r.human_approval
    {
        return Err("Stale or unsupported insertion scope/locks; approval is not granted".into());
    }
    let local = &r.language_event;
    let shared = &r.shared_event;
    super::score::validate(&docs, &shared.score)?;
    super::effects::validate(&docs, &project, &sequence, shared, local)?;
    if [
        &shared.semantic_id,
        &local.event_id,
        &local.picture_slot_id,
        &local.semantic_trigger_id,
    ]
    .into_iter()
    .any(|id| !new_id(id))
        || shared.semantic_id != local.semantic_id
        || shared.language_ids != [sequence.language.clone()]
        || local.supersedes_event_id.is_some()
        || !sequence.picture_bindings.contains(&shared.picture_binding)
        || local
            .picture_binding_override
            .as_ref()
            .is_some_and(|key| !sequence.picture_bindings.contains(key))
    {
        return Err("Invalid fresh cel identities, language scope or picture admission".into());
    }
    let anchor = sequence
        .cells
        .get(&r.after_semantic_id)
        .ok_or("Unknown source cel")?;
    if anchor.cue_id != local.cue_id {
        return Err("Insertion cannot cross a native cue".into());
    }
    let source_shared = docs["scene.json"]["shared_events"]
        .as_array()
        .ok_or("Shared events")?;
    let anchor_shared = source_shared
        .iter()
        .find(|event| event["semantic_id"] == r.after_semantic_id)
        .ok_or("Missing shared source cel")?;
    if source_shared
        .iter()
        .any(|event| event["semantic_id"] == shared.semantic_id)
        || anchor_shared["canonical_cue_id"] != shared.canonical_cue_id
        || docs["scene.json"]["language_event_bindings"]
            .as_object()
            .ok_or("Language events")?
            .values()
            .any(|events| {
                events.as_array().is_some_and(|events| {
                    events
                        .iter()
                        .any(|event| event["event_id"] == local.event_id)
                })
            })
    {
        return Err("Cel identity already exists or canonical cue differs".into());
    }
    for (role, graph) in &docs {
        if role != "selected-graph.json" && !role.starts_with("protected-graph:") {
            continue;
        }
        if graph["slots"]
            .as_array()
            .ok_or("Graph slots")?
            .iter()
            .any(|slot| slot["slot_id"] == local.picture_slot_id)
            || graph["events"]
                .as_array()
                .ok_or("Graph events")?
                .iter()
                .any(|event| event["event_id"] == local.event_id)
        {
            return Err("New graph slot/event identity is already used".into());
        }
    }
    let rows = sequence.rows();
    let rows = rows.as_array().ok_or("Native rows")?;
    let row_index = rows
        .iter()
        .position(|row| row["semantic_id"] == r.after_semantic_id)
        .ok_or("Source row")?;
    let plan = sequence.plan(rows)?;
    // Insertion must not repair stale clocks on unrelated source events.
    for timing in &plan.cells {
        let id = &sequence.cells[&timing.semantic_id].event_id;
        let event = snapshot.controls["selected-graph.json"]["events"]
            .as_array()
            .ok_or("Graph events")?
            .iter()
            .find(|event| event["event_id"] == *id)
            .ok_or("Source graph event")?;
        for (key, sample) in [
            ("phrase_start_seconds", timing.start_sample),
            ("phrase_end_seconds", timing.end_sample),
        ] {
            let seconds = event[key].as_f64().ok_or("Source graph clock")?;
            if (seconds * f64::from(timing.sample_rate)).round() != sample as f64 {
                return Err(
                    "Source graph clock differs from native alignment; normalize explicitly first"
                        .into(),
                );
            }
        }
    }
    let span = &plan.cells[row_index];
    let cue = sequence
        .cues
        .iter()
        .find(|cue| cue.cue_id == local.cue_id)
        .ok_or("Native cue")?;
    let offset = if project.is_multi_cue() {
        if r.split_after_frames.is_some() {
            return Err("Sample-clock insertion requires samples only".into());
        }
        r.split_after_samples.ok_or("Missing split_after_samples")?
    } else {
        if r.split_after_samples.is_some() {
            return Err("Frame-clock insertion requires frames only".into());
        }
        let frames = rows[row_index]["frames"]
            .as_u64()
            .ok_or("Native frame hold")?;
        let duration = span.end_sample - span.start_sample;
        if frames == 0 || duration % frames != 0 {
            return Err("Nonintegral source frame clock".into());
        }
        r.split_after_frames
            .ok_or("Missing split_after_frames")?
            .checked_mul(duration / frames)
            .ok_or("Split clock overflow")?
    };
    if offset == 0 || offset >= span.end_sample - span.start_sample {
        return Err("Split must be strictly inside the source hold".into());
    }
    let split = span
        .start_sample
        .checked_add(offset)
        .ok_or("Split sample overflow")?;
    let markers = docs[&cue.alignment_role]["semantic_markers"]
        .as_object()
        .ok_or("Native markers")?;
    if markers.contains_key(&local.semantic_trigger_id) {
        return Err("Semantic trigger identity is already used".into());
    }
    let shared_index = source_shared
        .iter()
        .position(|event| event["semantic_id"] == r.after_semantic_id)
        .ok_or("Source shared position")?;
    docs.get_mut("scene.json").ok_or("Scene")?["shared_events"]
        .as_array_mut()
        .ok_or("Shared events")?
        .insert(shared_index + 1, serde_json::to_value(shared)?);
    let events = docs.get_mut("scene.json").ok_or("Scene")?["language_event_bindings"]
        [&sequence.language]
        .as_array_mut()
        .ok_or("Language events")?;
    let index = events
        .iter()
        .position(|event| event["semantic_id"] == r.after_semantic_id)
        .ok_or("Source language position")?;
    events.insert(index + 1, serde_json::to_value(local)?);
    let alignment = docs.get_mut(&cue.alignment_role).ok_or("Alignment")?;
    alignment["semantic_markers"]
        .as_object_mut()
        .ok_or("Markers")?
        .insert(local.semantic_trigger_id.clone(), json!(split));
    let alignment_bytes = serde_json::to_vec(alignment)?;
    let digest = tiering::digest(&alignment_bytes);
    let binding = &mut docs.get_mut("scene-bindings.json").ok_or("Bindings")?["assets"]
        [&cue.alignment_binding];
    binding["sha256"] = json!(digest);
    binding["bytes"] = json!(alignment_bytes.len());
    binding["cache_uri"] = json!(format!("cache://sha256/{digest}"));
    binding["selection_state"] = json!("selected-private-production");
    let picture = docs["scene-bindings.json"]["assets"][local
        .picture_binding_override
        .as_ref()
        .unwrap_or(&shared.picture_binding)]
    .clone();
    let graph = docs.get_mut("selected-graph.json").ok_or("Graph")?;
    if graph["nodes"].as_array().ok_or("Graph nodes")?.len() != 1 {
        return Err("Insertion requires one native scene node".into());
    }
    let old_slots = graph["slots"].as_array().ok_or("Graph slots")?.clone();
    let old_slot = old_slots
        .iter()
        .find(|slot| slot["slot_id"] == anchor.picture_slot_id)
        .ok_or("Source slot")?;
    let revision = format!(
        "cel-new-{}",
        tiering::digest(&serde_json::to_vec(
            &json!({"source_lock":graph["lock"],"slot":local.picture_slot_id,"asset":picture})
        )?)
    );
    graph["slots"].as_array_mut().ok_or("Graph slots")?.push(json!({"slot_id":local.picture_slot_id,"beat_id":old_slot["beat_id"],"lane":"picture","disposition":"selected","selected_revision_id":revision,"revisions":[{"revision_id":revision,"asset":{"logical_id":picture["logical_id"],"sha256":picture["sha256"],"cache_uri":picture["cache_uri"]}}]}));
    let graph_events = graph["events"].as_array_mut().ok_or("Graph events")?;
    let index = graph_events
        .iter()
        .position(|event| event["event_id"] == anchor.event_id)
        .ok_or("Source graph event")?;
    let mut event = graph_events[index].clone();
    graph_events[index]["phrase_end_seconds"] = json!(split as f64 / span.sample_rate as f64);
    event["event_id"] = json!(local.event_id);
    event["picture"] = json!({"logical_id":picture["logical_id"],"sha256":picture["sha256"]});
    event["phrase_start_seconds"] = json!(split as f64 / span.sample_rate as f64);
    event["phrase_end_seconds"] = json!(span.end_sample as f64 / span.sample_rate as f64);
    graph_events.insert(index + 1, event);
    let node = &mut graph["nodes"][0];
    node["slots"]
        .as_array_mut()
        .ok_or("Node slots")?
        .push(json!(local.picture_slot_id));
    let node_events = node["events"].as_array_mut().ok_or("Node events")?;
    let index = node_events
        .iter()
        .position(|event| event == &anchor.event_id)
        .ok_or("Source active graph event")?;
    node_events.insert(index + 1, json!(local.event_id));
    let typed: assembly::Graph = serde_json::from_value(graph.clone())?;
    graph["lock"] = serde_json::to_value(assembly::graph_lock(&typed, &typed.lock.logical_id)?)?;
    let lock = graph["lock"].clone();
    docs.get_mut("selected-pointer.json").ok_or("Pointer")?["selected_lock"] = lock;
    let protected = old_slots
        .iter()
        .map(|slot| text(slot, "slot_id").map(str::to_owned))
        .collect::<Result<_>>()?;
    editor::preserve_other_lanes(&snapshot.controls, &mut docs, &sequence, &protected)?;
    Sequence::for_project(&docs, &project)?;
    project.write_draft_from_snapshot(
        output,
        &docs,
        &serde_json::from_slice::<Value>(&bytes)?,
        &snapshot,
    )?;
    Ok(())
}
