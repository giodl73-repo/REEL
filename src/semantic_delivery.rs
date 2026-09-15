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
    /// Declares how every phrase-level semantic event in the selected closure
    /// is consumed by the cue-relative job.  This prevents a job from merely
    /// reusing the right bytes while disconnecting a language-local phrase
    /// trigger from its narration, picture, sound, or VFX attachments.
    #[serde(default)]
    pub event_bindings: Vec<EventDeliveryBinding>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventDeliveryBinding {
    pub event_id: String,
    pub narration_attachment_id: String,
    pub picture_attachment_id: String,
    /// Optional M/E audio attachments whose timing is motivated by this event.
    #[serde(default)]
    pub audio_attachment_ids: Vec<String>,
    /// Optional external/VFX attachments whose timing is motivated by this event.
    #[serde(default)]
    pub external_layer_attachment_ids: Vec<String>,
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

fn validate_event_bindings(
    selection: &SelectedClosure,
    job: &Job,
    bindings: &[EventDeliveryBinding],
) -> Result<()> {
    let events = selection
        .closure
        .semantic_events
        .iter()
        .map(|event| (event.event_id.as_str(), event))
        .collect::<std::collections::BTreeMap<_, _>>();
    let pictures = job
        .pictures
        .iter()
        .map(|picture| (picture.attachment_id.as_str(), picture))
        .collect::<std::collections::BTreeMap<_, _>>();
    let audio = job
        .audio
        .iter()
        .map(|audio| (audio.attachment_id.as_str(), audio))
        .collect::<std::collections::BTreeMap<_, _>>();
    let layers = job
        .external_layers
        .iter()
        .map(|layer| (layer.attachment_id.as_str(), layer))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut bound = BTreeSet::new();
    for binding in bindings {
        if !nonempty(&binding.event_id)
            || !nonempty(&binding.narration_attachment_id)
            || !nonempty(&binding.picture_attachment_id)
        {
            bail!("semantic event delivery binding has an empty identity");
        }
        if !bound.insert(binding.event_id.as_str()) {
            bail!(
                "semantic event {} is bound more than once",
                binding.event_id
            );
        }
        let event = events.get(binding.event_id.as_str()).with_context(|| {
            format!(
                "semantic event {} is not in the selected closure",
                binding.event_id
            )
        })?;
        let narration = audio
            .get(binding.narration_attachment_id.as_str())
            .with_context(|| {
                format!(
                    "semantic event {} narration attachment is absent",
                    binding.event_id
                )
            })?;
        if narration.bus != "D" || narration.source.sha256 != event.narration.sha256 {
            bail!(
                "semantic event {} narration attachment must be the selected D take",
                binding.event_id
            );
        }
        let picture = pictures
            .get(binding.picture_attachment_id.as_str())
            .with_context(|| {
                format!(
                    "semantic event {} picture attachment is absent",
                    binding.event_id
                )
            })?;
        if picture.source.sha256 != event.picture.sha256 {
            bail!(
                "semantic event {} picture attachment must be the selected picture",
                binding.event_id
            );
        }
        let mut referenced_audio = BTreeSet::new();
        for attachment_id in &binding.audio_attachment_ids {
            if !referenced_audio.insert(attachment_id.as_str())
                || !audio.contains_key(attachment_id.as_str())
            {
                bail!(
                    "semantic event {} has invalid audio attachment {}",
                    binding.event_id,
                    attachment_id
                );
            }
        }
        let mut referenced_layers = BTreeSet::new();
        for attachment_id in &binding.external_layer_attachment_ids {
            if !referenced_layers.insert(attachment_id.as_str())
                || !layers.contains_key(attachment_id.as_str())
            {
                bail!(
                    "semantic event {} has invalid external layer {}",
                    binding.event_id,
                    attachment_id
                );
            }
        }
    }
    if bound.len() != events.len() || events.keys().any(|event_id| !bound.contains(event_id)) {
        bail!("every selected semantic event must have exactly one delivery binding");
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
    validate_event_bindings(&selection, &job, &semantic.event_bindings)?;
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
        Asset, Disposition, GRAPH_SCHEMA, ImmutableRef, Lane, Node, POINTER_SCHEMA, Revision,
        SemanticEvent, Slot,
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

    fn event_graph() -> Graph {
        let mut graph = graph();
        let narration_hash = hash('c');
        graph.slots.push(Slot {
            slot_id: "narration".into(),
            beat_id: "beat".into(),
            lane: Lane::Narration,
            disposition: Disposition::Selected,
            selected_revision_id: Some("r1".into()),
            revisions: vec![Revision {
                revision_id: "r1".into(),
                supersedes: None,
                asset: Asset {
                    logical_id: "narration-es".into(),
                    cache_uri: format!("cache://sha256/{narration_hash}"),
                    sha256: narration_hash.clone(),
                },
            }],
        });
        graph.events.push(SemanticEvent {
            event_id: "event.es.phrase-1".into(),
            scene_id: "scene".into(),
            language: "es".into(),
            narration: ImmutableRef {
                logical_id: "narration-es".into(),
                sha256: narration_hash,
            },
            picture: ImmutableRef {
                logical_id: "cel".into(),
                sha256: hash('b'),
            },
            phrase_start_seconds: 1.0,
            phrase_end_seconds: 2.0,
        });
        graph.nodes[0].slots.push("narration".into());
        graph.nodes[0].events.push("event.es.phrase-1".into());
        graph
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

    #[test]
    fn semantic_events_require_exact_delivery_attachments() {
        let graph = event_graph();
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
        let picture = hash('b');
        let narration = hash('c');
        let job: Job = serde_yaml::from_str(&format!(
            r#"
schema: reel.scene-delivery.v0.1
id: scene
contract: {{ path: contract.yaml, sha256: {picture}, bytes: 1 }}
production_manifest_sha256: {picture}
width: 1920
height: 1080
max_composition_samples: 480000
pictures:
  - attachment_id: picture
    source: {{ path: cel.png, sha256: {picture}, bytes: 1 }}
    kind: still
    attention: beat
audio:
  - attachment_id: narration
    source: {{ path: narration.wav, sha256: {narration}, bytes: 1 }}
    bus: D
    cue_id: cue.es.001
buses: {{ D: {{ state: present, reason: narration }}, M: {{ state: intentional-silence, reason: none }}, E: {{ state: intentional-silence, reason: none }} }}
"#
        ))
        .unwrap();
        let binding = EventDeliveryBinding {
            event_id: "event.es.phrase-1".into(),
            narration_attachment_id: "narration".into(),
            picture_attachment_id: "picture".into(),
            audio_attachment_ids: vec![],
            external_layer_attachment_ids: vec![],
        };
        validate_event_bindings(&selection, &job, std::slice::from_ref(&binding)).unwrap();
        assert!(
            validate_event_bindings(&selection, &job, &[])
                .unwrap_err()
                .to_string()
                .contains("every selected semantic event")
        );
        let mut wrong = binding;
        wrong.narration_attachment_id = "picture".into();
        assert!(
            validate_event_bindings(&selection, &job, &[wrong])
                .unwrap_err()
                .to_string()
                .contains("narration attachment")
        );
    }

    #[test]
    fn plan_fails_closed_when_a_selected_event_is_not_declared_by_the_job() {
        let root = tempfile::tempdir().unwrap();
        let graph = event_graph();
        let picture = hash('b');
        let narration = hash('c');
        let job_path = root.path().join("scene.yaml");
        fs::write(
            &job_path,
            format!(
                r#"
schema: reel.scene-delivery.v0.1
id: scene
contract: {{ path: contract.yaml, sha256: {picture}, bytes: 1 }}
production_manifest_sha256: {picture}
width: 1920
height: 1080
max_composition_samples: 480000
pictures:
  - attachment_id: picture
    source: {{ path: cel.png, sha256: {picture}, bytes: 1 }}
    kind: still
    attention: beat
audio:
  - attachment_id: narration
    source: {{ path: narration.wav, sha256: {narration}, bytes: 1 }}
    bus: D
    cue_id: cue.es.001
buses: {{ D: {{ state: present, reason: narration }}, M: {{ state: intentional-silence, reason: none }}, E: {{ state: intentional-silence, reason: none }} }}
"#
            ),
        )
        .unwrap();
        let job_bytes = fs::read(&job_path).unwrap();
        let delivery_path = root.path().join("delivery.yaml");
        let mut delivery = SemanticDelivery {
            schema: SCHEMA.into(),
            id: "semantic-scene".into(),
            pointer: SelectedPointer {
                schema: POINTER_SCHEMA.into(),
                logical_id: "current".into(),
                selected_lock: graph.lock.clone(),
            },
            graph,
            target: "scene".into(),
            scene_delivery_job: FileRef {
                path: "scene.yaml".into(),
                sha256: crate::sha256_file(&job_path).unwrap(),
                bytes: job_bytes.len() as u64,
            },
            event_bindings: vec![EventDeliveryBinding {
                event_id: "event.es.phrase-1".into(),
                narration_attachment_id: "narration".into(),
                picture_attachment_id: "picture".into(),
                audio_attachment_ids: vec![],
                external_layer_attachment_ids: vec![],
            }],
        };
        fs::write(&delivery_path, serde_yaml::to_string(&delivery).unwrap()).unwrap();
        plan(&delivery_path).unwrap();
        delivery.event_bindings.clear();
        fs::write(&delivery_path, serde_yaml::to_string(&delivery).unwrap()).unwrap();
        assert!(
            plan(&delivery_path)
                .unwrap_err()
                .to_string()
                .contains("every selected semantic event")
        );
    }
}
