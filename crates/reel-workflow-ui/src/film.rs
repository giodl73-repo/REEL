//! Read-only projection through the existing exact-commit film inspector.
use crate::{error, model::*};
use reel_workflow::{Artifact, ErrorKind, Result, film_inspect};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    path::Path,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AssetBinding {
    /// Exact logical ID, hash, length and declared media type of the subject.
    pub asset: Artifact,
    pub preview: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub inspection: film_inspect::Request,
    pub title: String,
    pub scene_title: String,
    #[serde(default)]
    pub language_labels: BTreeMap<String, String>,
    #[serde(default)]
    pub language_order: Vec<String>,
    pub assets: Vec<AssetBinding>,
    pub media: BTreeMap<String, Media>,
    #[serde(default)]
    pub tasks: BTreeMap<String, Watch>,
}
/// Retain only the bounded metadata already admitted and verified by inspection.
struct Capture<'a, R> {
    source: &'a mut R,
    text: BTreeMap<String, Vec<u8>>,
}
impl<R: film_inspect::Reader> film_inspect::Reader for Capture<'_, R> {
    fn read(&mut self, commit: &str, path: &str, maximum: u64) -> Result<Vec<u8>> {
        let raw = self.source.read(commit, path, maximum)?;
        self.text.insert(path.into(), raw.clone());
        Ok(raw)
    }
}
pub fn project(request: &Request, reader: &mut impl film_inspect::Reader) -> Result<Board> {
    if request.schema != "reel.workflow-film-board.v1" {
        return Err(error(ErrorKind::Invalid, "film-board-request-schema"));
    }
    if request.assets.len() > 4096
        || request.media.len() > 4096
        || request.tasks.len() > 128
        || request.language_labels.len() > 32
        || request.language_order.len() > 32
        || serde_json::to_vec(request)
            .map_err(|_| error(ErrorKind::Invalid, "film-board-json"))?
            .len() as u64
            > MAX_CONTROL_BYTES
    {
        return Err(error(ErrorKind::Budget, "film-board-request"));
    }
    let mut capture = Capture {
        source: reader,
        text: BTreeMap::new(),
    };
    let sidecar = film_inspect::inspect(&request.inspection, &mut capture)?;
    if !request.language_order.is_empty()
        && (request.language_order.len() != sidecar.lanes.len()
            || request.language_order.iter().collect::<BTreeSet<_>>()
                != sidecar.lanes.keys().collect::<BTreeSet<_>>())
    {
        return Err(error(ErrorKind::Conflict, "film-board-language-order"));
    }
    let mut declared = BTreeSet::new();
    for binding in &request.assets {
        reel_publication::check_artifact(&binding.asset)?;
        if !declared.insert((
            &binding.asset.logical_id,
            &binding.asset.sha256,
            binding.asset.bytes,
        )) {
            return Err(error(ErrorKind::Conflict, "film-board-duplicate-subject"));
        }
    }
    let binding = |id: &str, sha: &str, bytes: u64| -> Result<&AssetBinding> {
        request
            .assets
            .iter()
            .find(|a| a.asset.logical_id == id && a.asset.sha256 == sha && a.asset.bytes == bytes)
            .ok_or_else(|| error(ErrorKind::Missing, "film-board-exact-subject"))
    };
    if request
        .language_labels
        .keys()
        .any(|l| !sidecar.lanes.contains_key(l))
    {
        return Err(error(ErrorKind::Conflict, "film-board-language-label"));
    }
    let mut board = Board {
        schema: "reel.workflow-board.v1".into(),
        project: sidecar.request.project.clone(),
        work_id: sidecar.episode.episode_id.clone(),
        title: request.title.clone(),
        format: Format::Film,
        source_commit: sidecar.request.source_commit.clone(),
        projection_request: None,
        approvals: sidecar
            .request
            .approvals
            .iter()
            .map(|a| ApprovalReference {
                subject: a.subject.clone(),
                evidence: a.evidence.artifact.clone(),
            })
            .collect(),
        synthetic: false,
        languages: vec![],
        scenes: vec![Scene {
            id: sidecar.authored_scene.scene_id.clone(),
            title: request.scene_title.clone(),
        }],
        units: vec![],
        cues: vec![],
        media: request.media.clone(),
        tasks: request.tasks.clone(),
    };
    for (language, lane) in &sidecar.lanes {
        board.languages.push(Language {
            id: language.clone(),
            label: request
                .language_labels
                .get(language)
                .unwrap_or(language)
                .clone(),
        });
        for inspected in &lane.cues {
            let cue = &inspected.cue;
            let evidence = &sidecar.request.cues[language][&cue.cue_id].text;
            let text = String::from_utf8(capture.text[&evidence.path].clone())
                .map_err(|_| error(ErrorKind::Invalid, "film-board-text"))?;
            let source = &inspected.selected_take;
            let a = binding(&source.logical_id, &source.sha256, source.bytes)?;
            if !a.asset.media_type.starts_with("audio/") {
                return Err(error(ErrorKind::Conflict, "film-board-take-type"));
            }
            let mut markers = Vec::new();
            for event in lane.events.iter().filter(|e| e.event.cue_id == cue.cue_id) {
                let words = sidecar.authored_scene.language_event_bindings[language]
                    .iter()
                    .find(|b| b.event_id == event.event.event_id)
                    .and_then(|b| b.spoken_trigger_phrase.clone())
                    .ok_or_else(|| error(ErrorKind::Missing, "film-board-trigger-phrase"))?;
                markers.push(Marker {
                    semantic_id: event.semantic_id.clone(),
                    word: words,
                    sample: event.start_sample,
                });
            }
            markers.sort_by_key(|m| m.sample);
            board.cues.push(Cue {
                id: cue.cue_id.clone(),
                semantic_id: cue.source_id.clone(),
                language: language.clone(),
                title: cue.cue_id.clone(),
                text,
                selected_take: a.asset.clone(),
                preview: a.preview.clone(),
                sample_rate: inspected.alignment.sample_rate,
                samples: inspected.alignment.cue_end_sample,
                markers,
            });
        }
        for event in &lane.events {
            let unit_index = match board.units.iter().position(|u| u.id == event.semantic_id) {
                Some(i) => i,
                None => {
                    board.units.push(Unit {
                        id: event.semantic_id.clone(),
                        story_id: event.identity.story_id.clone(),
                        display_id: event.identity.cel_id.clone(),
                        semantic_id: event.semantic_id.clone(),
                        scene_id: sidecar.authored_scene.scene_id.clone(),
                        title: event.semantic_id.clone(),
                        versions: vec![],
                        spans: vec![],
                        review_spans: vec![],
                        task: None,
                    });
                    board.units.len() - 1
                }
            };
            let unit = &mut board.units[unit_index];
            if unit.story_id != event.identity.story_id
                || unit.display_id != event.identity.cel_id
                || unit.spans.iter().any(|s| s.language == *language)
            {
                return Err(error(ErrorKind::Conflict, "film-board-event-identity"));
            }
            let source = &event.selected_picture;
            let a = binding(&source.logical_id, &source.sha256, source.bytes)?;
            let version_id = format!(
                "v-{}",
                reel_workflow::canonical_digest(&(language, &a.asset))?
            );
            unit.versions.push(Version {
                id: version_id,
                label: format!("{} · selected", language.to_uppercase()),
                asset: a.asset.clone(),
                designation: Designation::Selected,
                language: Some(language.clone()),
                preview: a.preview.clone(),
                approval_reference: None,
            });
            let cue = lane
                .cues
                .iter()
                .find(|c| c.cue.cue_id == event.event.cue_id)
                .ok_or_else(|| error(ErrorKind::Missing, "film-board-event-cue"))?;
            let end = lane
                .events
                .iter()
                .filter(|e| {
                    e.event.cue_id == event.event.cue_id && e.start_sample > event.start_sample
                })
                .map(|e| e.start_sample)
                .min()
                .unwrap_or(cue.alignment.cue_end_sample);
            if lane
                .events
                .iter()
                .filter(|e| {
                    e.event.cue_id == event.event.cue_id && e.start_sample == event.start_sample
                })
                .count()
                != 1
            {
                return Err(error(ErrorKind::Conflict, "film-board-ambiguous-cut"));
            }
            unit.spans.push(Span {
                language: language.clone(),
                cue_id: event.event.cue_id.clone(),
                start_sample: event.start_sample,
                end_sample: end,
            });
        }
    }
    if !request.language_order.is_empty() {
        board.languages.sort_by_key(|l| {
            request
                .language_order
                .iter()
                .position(|id| id == &l.id)
                .unwrap()
        });
    }
    // Canonical shared-event order is independent of language spelling or clocks.
    board.units.sort_by_key(|unit| {
        sidecar
            .authored_scene
            .shared_events
            .iter()
            .position(|e| e.semantic_id == unit.semantic_id)
            .unwrap_or(usize::MAX)
    });
    for (key, watch) in &board.tasks {
        for preview in watch.outputs.values() {
            let unit = board
                .units
                .iter_mut()
                .find(|u| u.id == preview.unit_id)
                .ok_or_else(|| error(ErrorKind::Missing, "film-board-watch-unit"))?;
            if unit.task.as_ref().is_some_and(|t| t != key) {
                return Err(error(ErrorKind::Conflict, "film-board-watch-alias"));
            }
            unit.task = Some(key.clone());
        }
    }
    board.check()?;
    Ok(board)
}
/// No preview files are opened. The original pinned controls are inspected via Git.
pub fn export(
    request_path: &Path,
    pin: &str,
    repository: &Path,
    output: &Path,
) -> Result<Artifact> {
    export_with(request_path, pin, output, |request| {
        project(request, &mut film_inspect::GitReader { repository })
    })
}

pub fn export_with(
    request_path: &Path,
    pin: &str,
    output: &Path,
    project_request: impl FnOnce(&Request) -> Result<Board>,
) -> Result<Artifact> {
    let file = std::fs::File::open(request_path)
        .map_err(|_| error(ErrorKind::Missing, "film-board-request-file"))?;
    let mut raw = Vec::new();
    file.take(MAX_CONTROL_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| error(ErrorKind::Missing, "film-board-request-read"))?;
    if raw.len() as u64 > MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "film-board-request-bytes"));
    }
    if reel_workflow::digest(&raw) != pin {
        return Err(error(ErrorKind::Integrity, "film-board-request-pin"));
    }
    let request: Request = serde_json::from_slice(&raw)
        .map_err(|_| error(ErrorKind::Invalid, "film-board-request-json"))?;
    let mut board = project_request(&request)?;
    board.projection_request = Some(Artifact {
        logical_id: "workflow/film-board-request".into(),
        sha256: pin.into(),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    board.check()?;
    let raw = serde_json::to_vec_pretty(&board)
        .map_err(|_| error(ErrorKind::Invalid, "film-board-output-json"))?;
    if raw.len() as u64 > MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "film-board-output-bytes"));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| error(ErrorKind::Conflict, "film-board-new-output"))?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "film-board-output-write"))?;
    Ok(Artifact {
        logical_id: "workflow/film-board".into(),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    })
}
