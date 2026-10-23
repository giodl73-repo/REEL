//! Metadata-only native cel discovery. No media resolution, timing inference or
//! editing authority. Reports are private; summaries deliberately omit identity.
use crate::{Result, tiering, valid_oid};
use reel_assembly::scene_authoring as native;
use serde::Serialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, path::Path};
pub mod project;

const CONTROL_BYTES: usize = 1024 * 1024;
const MAX_EVENTS: usize = 8192;

#[derive(Serialize)]
pub struct Cel {
    pub language: String,
    pub cue_id: String,
    pub event_id: String,
    pub shared_semantic_id: Option<String>,
    pub semantic_trigger_id: String,
    pub picture_slot_id: String,
    pub picture_binding: String,
    pub asset: native::AssetRef,
}

/// Original control locks, not a claim of selected media custody or build readiness.
#[derive(Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub scene_id: String,
    pub scene_sha256: String,
    pub bindings_sha256: String,
    pub authoring_state: String,
    pub languages: Vec<String>,
    pub cels: Vec<Cel>,
    pub imported_evidence: bool,
}
impl Report {
    pub fn summary(&self) -> Value {
        json!({"schema":"reel.cel-source-summary.v1","languages":self.languages.len(),
            "cels":self.cels.len(),"imported_evidence":self.imported_evidence,
            "media_read":false,"render_ready":false,"selection_changed":false})
    }
}

/// Read exactly two caller-supplied native controls. No catalog convention,
/// source-repository lookup, cache scan, implicit hydration or movie requirement.
pub fn read(scene: &Path, bindings: &Path) -> Result<Report> {
    inspect(
        &tiering::read_bounded(scene, CONTROL_BYTES as u64)?,
        &tiering::read_bounded(bindings, CONTROL_BYTES as u64)?,
    )
}

/// Reuse native V1/V2 materialization, including V2 language overrides and cue
/// scope. Unknown native fields reject rather than being silently discarded.
pub fn inspect(scene_bytes: &[u8], bindings_bytes: &[u8]) -> Result<Report> {
    if scene_bytes.len() > CONTROL_BYTES || bindings_bytes.len() > CONTROL_BYTES {
        return Err("Cel source control exceeds byte budget".into());
    }
    let scene: native::Scene = serde_json::from_slice(scene_bytes)?;
    let bindings: native::ScopedBindings = serde_json::from_slice(bindings_bytes)?;
    if !matches!(
        scene.schema.as_str(),
        native::SCENE_SCHEMA | native::SCENE_SCHEMA_V2
    ) || bindings.schema != native::BINDINGS_SCHEMA
        || scene.scene_id.trim().is_empty()
        || scene.scene_id != bindings.scope_id
        || !matches!(
            scene.authoring_state.as_str(),
            "imported-evidence" | "ready-for-private-build"
        )
        || scene.languages.is_empty()
        || scene.languages.len() > 16
        || bindings.assets.len() > MAX_EVENTS
    {
        return Err("Unsupported cel source schema, state, scope or count".into());
    }
    let total_events = scene
        .languages
        .values()
        .map(|lane| lane.events.len())
        .sum::<usize>()
        + scene
            .language_event_bindings
            .values()
            .map(Vec::len)
            .sum::<usize>();
    if total_events > MAX_EVENTS
        || scene.shared_events.len() > MAX_EVENTS
        || scene.languages.values().any(|lane| lane.cues.len() > 4096)
    {
        return Err("Cel source event/cue budget exceeded".into());
    }
    for (key, asset) in &bindings.assets {
        if key.trim().is_empty()
            || asset.logical_id.trim().is_empty()
            || !valid_oid(&asset.sha256)
            || asset.cache_uri != format!("cache://sha256/{}", asset.sha256)
            || asset.selection_state.trim().is_empty()
        {
            return Err("Invalid scoped asset identity".into());
        }
    }
    let materialized = native::materialize_scene(&scene)?;
    let mut cels = Vec::new();
    for (language, lane) in &materialized.languages {
        if scene.authoring_state == "ready-for-private-build"
            && (lane.cues.is_empty() || lane.events.is_empty())
        {
            return Err("Ready cel source has an empty language lane".into());
        }
        let cues: BTreeSet<_> = lane.cues.iter().map(|cue| cue.cue_id.as_str()).collect();
        if language.trim().is_empty()
            || cues.len() != lane.cues.len()
            || cues.iter().any(|id| id.trim().is_empty())
        {
            return Err("Empty language or duplicate/empty cue identity".into());
        }
        let mut ids = BTreeSet::new();
        for event in &lane.events {
            if event.event_id.trim().is_empty()
                || !ids.insert(&event.event_id)
                || !cues.contains(event.cue_id.as_str())
                || event.semantic_trigger_id.trim().is_empty()
                || event.picture_slot_id.trim().is_empty()
            {
                return Err("Ambiguous or mis-scoped native cel event".into());
            }
            let key = event
                .picture_binding
                .as_ref()
                .ok_or("Cel event has no picture binding")?;
            let asset = bindings
                .assets
                .get(key)
                .ok_or("Cel picture binding absent from scene scope")?;
            cels.push(Cel {
                shared_semantic_id: scene
                    .language_event_bindings
                    .get(language)
                    .and_then(|events| {
                        events
                            .iter()
                            .find(|binding| binding.event_id == event.event_id)
                    })
                    .map(|binding| binding.semantic_id.clone()),
                language: language.clone(),
                cue_id: event.cue_id.clone(),
                event_id: event.event_id.clone(),
                semantic_trigger_id: event.semantic_trigger_id.clone(),
                picture_slot_id: event.picture_slot_id.clone(),
                picture_binding: key.clone(),
                asset: asset.clone(),
            });
        }
    }
    if cels.len() > MAX_EVENTS
        || (scene.authoring_state == "ready-for-private-build" && cels.is_empty())
    {
        return Err("Ready cel source needs bounded picture events".into());
    }
    Ok(Report {
        schema: "reel.cel-source-report.v1",
        scene_id: scene.scene_id,
        scene_sha256: tiering::digest(scene_bytes),
        bindings_sha256: tiering::digest(bindings_bytes),
        imported_evidence: scene.authoring_state == "imported-evidence",
        authoring_state: scene.authoring_state,
        languages: scene.languages.into_keys().collect(),
        cels,
    })
}
