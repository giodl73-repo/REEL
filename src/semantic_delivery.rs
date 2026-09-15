//! REEL-owned bridge from a selected semantic graph to scene delivery.
//!
//! Projects own source interpretation, authority records, cache hydration and
//! the construction of the cue-relative scene-delivery job. REEL owns the
//! selection closure, media-binding proof, and invocation of the generic
//! FFmpeg scene-delivery renderer.

use crate::scene_delivery::{self, FileRef, Job};
use anyhow::{Context, Result, bail};
use reel_assembly::{Graph, SelectedClosure, SelectedPointer, selected_closure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

pub const SCHEMA: &str = "reel.semantic-delivery.v1";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticDelivery {
    pub schema: String,
    pub id: String,
    pub pointer: SelectedPointer,
    pub graph: Graph,
    pub target: String,
    /// A hash-bound cue-relative REEL scene-delivery job. Its media sources
    /// must be selected by this graph closure or named in its semantic events.
    pub scene_delivery_job: FileRef,
}

#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    pub schema: String,
    pub id: String,
    pub selection: SelectedClosure,
    pub scene_delivery_job_sha256: String,
}

fn nonempty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn checked_job(path: &Path, reference: &FileRef) -> Result<Job> {
    let base = path.parent().unwrap_or(Path::new("."));
    let job_path = scene_delivery::checked_file(base, reference)?;
    serde_yaml::from_slice(&fs::read(&job_path)?).context("invalid REEL scene-delivery job")
}

fn allowed_hashes(selection: &SelectedClosure) -> BTreeSet<String> {
    selection
        .closure
        .selected_assets
        .iter()
        .map(|asset| asset.sha256.clone())
        .chain(
            selection
                .closure
                .semantic_events
                .iter()
                .flat_map(|event| [event.narration.sha256.clone(), event.picture.sha256.clone()]),
        )
        .collect()
}

fn validate_job_media(selection: &SelectedClosure, job: &Job) -> Result<()> {
    let allowed = allowed_hashes(selection);
    for picture in &job.pictures {
        if !allowed.contains(&picture.source.sha256) {
            bail!(
                "picture {} is not selected by semantic closure",
                picture.attachment_id
            );
        }
    }
    for audio in &job.audio {
        if !allowed.contains(&audio.source.sha256) {
            bail!(
                "audio {} is not selected by semantic closure",
                audio.attachment_id
            );
        }
    }
    for layer in &job.external_layers {
        if !allowed.contains(&layer.evidence.sha256) {
            bail!(
                "external layer {} is not selected by semantic closure",
                layer.attachment_id
            );
        }
    }
    Ok(())
}

/// Validates one generic, selected semantic render input without rendering it.
/// This is the renderer handoff boundary: a project cannot replace media by
/// filename, path, or a candidate revision after REEL resolves the closure.
pub fn plan(path: &Path) -> Result<Plan> {
    let semantic: SemanticDelivery = serde_yaml::from_slice(&fs::read(path)?)?;
    if semantic.schema != SCHEMA || !nonempty(&semantic.id) || !nonempty(&semantic.target) {
        bail!("invalid semantic-delivery schema, id, or target");
    }
    let selection = selected_closure(&semantic.pointer, &semantic.graph, &semantic.target)?;
    let job = checked_job(path, &semantic.scene_delivery_job)?;
    validate_job_media(&selection, &job)?;
    Ok(Plan {
        schema: "reel.semantic-delivery-plan.v1".into(),
        id: semantic.id,
        selection,
        scene_delivery_job_sha256: semantic.scene_delivery_job.sha256,
    })
}

/// Renders through REEL's existing FFmpeg scene-delivery implementation after
/// the semantic graph closure and every hydrated media binding validate.
pub fn render(path: &Path, asset_root: &Path, output: &Path) -> Result<scene_delivery::Receipt> {
    let semantic: SemanticDelivery = serde_yaml::from_slice(&fs::read(path)?)?;
    let _plan = plan(path)?;
    let base = path.parent().unwrap_or(Path::new("."));
    let job_path = scene_delivery::checked_file(base, &semantic.scene_delivery_job)?;
    scene_delivery::render(&job_path, asset_root, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_assembly::{
        Asset, Disposition, GRAPH_SCHEMA, ImmutableRef, Lane, Node, POINTER_SCHEMA, Revision, Slot,
    };

    fn hash(letter: char) -> String {
        std::iter::repeat_n(letter, 64).collect()
    }
    fn graph() -> Graph {
        let asset_hash = hash('b');
        Graph {
            schema: GRAPH_SCHEMA.into(),
            lock: ImmutableRef {
                logical_id: "lock".into(),
                sha256: hash('a'),
            },
            slots: vec![Slot {
                slot_id: "picture".into(),
                beat_id: "beat".into(),
                lane: Lane::Picture,
                disposition: Disposition::Selected,
                selected_revision_id: Some("r1".into()),
                revisions: vec![Revision {
                    revision_id: "r1".into(),
                    supersedes: None,
                    asset: Asset {
                        logical_id: "cel".into(),
                        cache_uri: format!("cache://sha256/{asset_hash}"),
                        sha256: asset_hash,
                    },
                }],
            }],
            events: vec![],
            nodes: vec![Node {
                id: "scene".into(),
                inputs: vec![],
                slots: vec!["picture".into()],
                events: vec![],
            }],
            presentation_targets: vec![],
        }
    }

    #[test]
    fn rejects_media_not_selected_by_the_semantic_closure() {
        let graph = graph();
        let selection = selected_closure(
            &SelectedPointer {
                schema: POINTER_SCHEMA.into(),
                logical_id: "current".into(),
                selected_lock: graph.lock.clone(),
            },
            &graph,
            "scene",
        )
        .unwrap();
        let wrong = hash('c');
        let job: Job = serde_yaml::from_str(&format!(r#"
schema: reel.scene-delivery.v0.1
id: scene
contract: {{ path: contract.yaml, sha256: {wrong}, bytes: 1 }}
production_manifest_sha256: {wrong}
width: 1920
height: 1080
max_composition_samples: 480000
pictures:
  - attachment_id: picture
    source: {{ path: cel.png, sha256: {wrong}, bytes: 1 }}
    kind: still
    attention: beat
audio: []
buses: {{ D: {{ state: intentional-silence, reason: none }}, M: {{ state: intentional-silence, reason: none }}, E: {{ state: intentional-silence, reason: none }} }}
"#)).unwrap();
        assert!(
            validate_job_media(&selection, &job)
                .unwrap_err()
                .to_string()
                .contains("not selected")
        );
    }

    #[test]
    fn accepts_media_selected_by_the_semantic_closure() {
        let graph = graph();
        let selection = selected_closure(
            &SelectedPointer {
                schema: POINTER_SCHEMA.into(),
                logical_id: "current".into(),
                selected_lock: graph.lock.clone(),
            },
            &graph,
            "scene",
        )
        .unwrap();
        let selected = hash('b');
        let job: Job = serde_yaml::from_str(&format!(
            r#"
schema: reel.scene-delivery.v0.1
id: scene
contract: {{ path: contract.yaml, sha256: {selected}, bytes: 1 }}
production_manifest_sha256: {selected}
width: 1920
height: 1080
max_composition_samples: 480000
pictures:
  - attachment_id: picture
    source: {{ path: cel.png, sha256: {selected}, bytes: 1 }}
    kind: still
    attention: beat
audio: []
buses: {{ D: {{ state: intentional-silence, reason: none }}, M: {{ state: intentional-silence, reason: none }}, E: {{ state: intentional-silence, reason: none }} }}
"#
        ))
        .unwrap();
        validate_job_media(&selection, &job).unwrap();
    }
}
