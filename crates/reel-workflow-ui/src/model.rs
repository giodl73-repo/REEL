use crate::error;
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Board {
    pub schema: String,
    pub project: String,
    pub work_id: String,
    pub title: String,
    pub format: Format,
    pub source_commit: String,
    #[serde(default)]
    pub synthetic: bool,
    pub languages: Vec<Language>,
    pub scenes: Vec<Scene>,
    pub units: Vec<Unit>,
    pub cues: Vec<Cue>,
    pub media: BTreeMap<String, Media>,
    pub tasks: BTreeMap<String, Watch>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Film,
    Comic,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    pub id: String,
    pub label: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub id: String,
    pub title: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Unit {
    pub id: String,
    pub story_id: String,
    pub display_id: String,
    pub semantic_id: String,
    pub scene_id: String,
    pub title: String,
    pub versions: Vec<Version>,
    pub spans: Vec<Span>,
    #[serde(default)]
    pub task: Option<String>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Designation {
    Selected,
    Historical,
    Candidate,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub id: String,
    pub label: String,
    pub asset: Artifact,
    pub designation: Designation,
    pub language: Option<String>,
    pub preview: Option<String>,
    #[serde(default)]
    pub approval_reference: Option<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub language: String,
    pub cue_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Cue {
    pub id: String,
    pub semantic_id: String,
    pub language: String,
    pub title: String,
    pub text: String,
    pub selected_take: Artifact,
    pub preview: Option<String>,
    pub sample_rate: u32,
    pub samples: u64,
    pub markers: Vec<Marker>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub semantic_id: String,
    pub word: String,
    pub sample: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Media {
    pub artifact: Artifact,
    pub path: PathBuf,
    /// An explicitly declared derivative subject; not derivation verification.
    pub subject: Option<Artifact>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Watch {
    pub journal: PathBuf,
    pub ticket_sha256: String,
    pub task_key: String,
    pub attempt_id: String,
    pub outputs: BTreeMap<String, OutputPreview>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OutputPreview {
    pub unit_id: String,
    pub path: PathBuf,
    pub label: String,
}
fn text(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value
            .chars()
            .any(|c| c == '\0' || (c.is_control() && c != '\n' && c != '\t'))
}
fn id(value: &str) -> bool {
    reel_publication::valid_id(value)
}
fn hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn artifact(value: &Artifact) -> Result<()> {
    reel_publication::check_artifact(value).map_err(Into::into)
}
fn absolute(path: &std::path::Path) -> bool {
    path.is_absolute()
        && !path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
}
pub const MAX_CONTROL_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_PREVIEW_BYTES: u64 = 128 * 1024 * 1024;
pub fn mime(value: &str) -> Option<&'static str> {
    match value {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/webp" => Some("image/webp"),
        "audio/wav" => Some("audio/wav"),
        "audio/mpeg" => Some("audio/mpeg"),
        "video/mp4" => Some("video/mp4"),
        _ => None,
    }
}
impl Board {
    pub fn check(&self) -> Result<()> {
        if self.schema != "reel.workflow-board.v1"
            || !id(&self.project)
            || !id(&self.work_id)
            || !text(&self.title, 256)
            || ![40, 64].contains(&self.source_commit.len())
            || !self
                .source_commit
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.source_commit.bytes().all(|b| b == b'0')
            || self.languages.is_empty()
            || self.languages.len() > 32
            || self.scenes.is_empty()
            || self.scenes.len() > 256
            || self.units.is_empty()
            || self.units.len() > 2048
            || self.cues.len() > 2048
            || self.media.len() > 4096
            || self.tasks.len() > 128
        {
            return Err(error(ErrorKind::Invalid, "board-shape"));
        }
        let mut languages = BTreeSet::new();
        for language in &self.languages {
            if !id(&language.id) || !text(&language.label, 64) || !languages.insert(&language.id) {
                return Err(error(ErrorKind::Invalid, "board-language"));
            }
        }
        let mut scenes = BTreeSet::new();
        for scene in &self.scenes {
            if !id(&scene.id) || !text(&scene.title, 256) || !scenes.insert(&scene.id) {
                return Err(error(ErrorKind::Invalid, "board-scene"));
            }
        }
        let mut objects = BTreeMap::new();
        let mut total = 0u64;
        for (key, media) in &self.media {
            artifact(&media.artifact)?;
            if !id(key)
                || !absolute(&media.path)
                || mime(&media.artifact.media_type).is_none()
                || media.artifact.bytes > MAX_PREVIEW_BYTES
            {
                return Err(error(ErrorKind::Invalid, "board-preview"));
            }
            if let Some(subject) = &media.subject {
                artifact(subject)?;
            }
            if objects
                .insert(&media.artifact.sha256, media.artifact.bytes)
                .is_some_and(|n| n != media.artifact.bytes)
            {
                return Err(error(ErrorKind::Integrity, "board-object-alias"));
            }
            total = total
                .checked_add(media.artifact.bytes)
                .filter(|n| *n <= 64 * 1024 * 1024 * 1024)
                .ok_or_else(|| error(ErrorKind::Budget, "board-declared-previews"))?;
        }
        let mut cues = BTreeMap::new();
        for cue in &self.cues {
            artifact(&cue.selected_take)?;
            if !id(&cue.id)
                || !id(&cue.semantic_id)
                || !languages.contains(&cue.language)
                || !text(&cue.title, 256)
                || !text(&cue.text, 16384)
                || !(8000..=192000).contains(&cue.sample_rate)
                || cue.samples == 0
                || cue.samples > u64::from(cue.sample_rate) * 21600
                || cue.markers.len() > 1024
                || cues.insert((&cue.language, &cue.id), cue).is_some()
            {
                return Err(error(ErrorKind::Invalid, "board-cue"));
            }
            if let Some(preview) = &cue.preview {
                let media = self
                    .media
                    .get(preview)
                    .ok_or_else(|| error(ErrorKind::Missing, "board-cue-preview"))?;
                if media.artifact != cue.selected_take
                    || !media.artifact.media_type.starts_with("audio/")
                {
                    return Err(error(ErrorKind::Conflict, "board-cue-take-preview"));
                }
            }
            let mut marker_ids = BTreeSet::new();
            let mut previous = 0;
            for marker in &cue.markers {
                if !id(&marker.semantic_id)
                    || !text(&marker.word, 256)
                    || marker.sample < previous
                    || marker.sample >= cue.samples
                    || !marker_ids.insert(&marker.semantic_id)
                {
                    return Err(error(ErrorKind::Invalid, "board-cue-marker"));
                }
                previous = marker.sample;
            }
        }
        let mut units = BTreeSet::new();
        let mut stories = BTreeMap::new();
        for unit in &self.units {
            if !id(&unit.id)
                || !units.insert(&unit.id)
                || !id(&unit.story_id)
                || !id(&unit.display_id)
                || !id(&unit.semantic_id)
                || !scenes.contains(&unit.scene_id)
                || !text(&unit.title, 256)
                || unit.versions.len() > 32
                || unit.spans.len() > 256
                || unit
                    .task
                    .as_ref()
                    .is_some_and(|t| !self.tasks.contains_key(t))
                || stories
                    .insert(&unit.display_id, &unit.story_id)
                    .is_some_and(|s| s != &unit.story_id)
            {
                return Err(error(ErrorKind::Invalid, "board-unit"));
            }
            let mut versions = BTreeSet::new();
            let mut selected = BTreeSet::new();
            for version in &unit.versions {
                artifact(&version.asset)?;
                if !id(&version.id)
                    || !versions.insert(&version.id)
                    || !text(&version.label, 128)
                    || version
                        .language
                        .as_ref()
                        .is_some_and(|l| !languages.contains(l))
                    || (version.designation == Designation::Selected
                        && !selected.insert(&version.language))
                {
                    return Err(error(ErrorKind::Invalid, "board-version"));
                }
                if let Some(reference) = &version.approval_reference {
                    artifact(reference)?;
                }
                if let Some(preview) = &version.preview {
                    let media = self
                        .media
                        .get(preview)
                        .ok_or_else(|| error(ErrorKind::Missing, "board-unit-preview"))?;
                    if !media.artifact.media_type.starts_with("image/")
                        || (media.artifact != version.asset
                            && media.subject.as_ref() != Some(&version.asset))
                    {
                        return Err(error(ErrorKind::Conflict, "board-preview-subject"));
                    }
                }
            }
            for span in &unit.spans {
                let cue = cues
                    .get(&(&span.language, &span.cue_id))
                    .ok_or_else(|| error(ErrorKind::Missing, "board-span-cue"))?;
                if span.start_sample >= span.end_sample
                    || span.end_sample > cue.samples
                    || !cue
                        .markers
                        .iter()
                        .any(|m| m.semantic_id == unit.semantic_id && m.sample == span.start_sample)
                {
                    return Err(error(ErrorKind::Invalid, "board-span-clock"));
                }
            }
        }
        for (key, watch) in &self.tasks {
            if !id(key)
                || !absolute(&watch.journal)
                || !hash(&watch.ticket_sha256)
                || !hash(&watch.task_key)
                || !id(&watch.attempt_id)
                || watch.outputs.len() > 32
            {
                return Err(error(ErrorKind::Invalid, "board-watch"));
            }
            for (output, preview) in &watch.outputs {
                if !id(output)
                    || !units.contains(&preview.unit_id)
                    || !absolute(&preview.path)
                    || !text(&preview.label, 128)
                    || self
                        .units
                        .iter()
                        .find(|u| u.id == preview.unit_id)
                        .and_then(|u| u.task.as_ref())
                        != Some(key)
                {
                    return Err(error(ErrorKind::Invalid, "board-output-preview"));
                }
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| error(ErrorKind::Invalid, "board-json"))?
            .len() as u64
            > MAX_CONTROL_BYTES
        {
            return Err(error(ErrorKind::Budget, "board-control-bytes"));
        }
        Ok(())
    }
}
