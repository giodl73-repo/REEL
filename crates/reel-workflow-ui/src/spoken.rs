//! Project checked spoken access manifests into the private workflow viewer.
use crate::{error, model::*};
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    project: String,
    work_id: String,
    title: String,
    source_commit: String,
    bundles: Vec<Bundle>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    path: PathBuf,
    sha256: String,
    waves: BTreeMap<String, Media>,
}

// Only the fields needed for this review projection are interpreted.
#[derive(Deserialize)]
struct Access {
    schema: String,
    sample_rate: u32,
    editions: Vec<Edition>,
}
#[derive(Deserialize)]
struct Edition {
    edition_id: String,
    language: String,
    wave: Wave,
    duration_samples: u64,
    entries: Vec<Entry>,
    chapters: Vec<Chapter>,
}
#[derive(Deserialize)]
struct Wave {
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct Entry {
    text: String,
}
#[derive(Deserialize)]
struct Chapter {
    module_id: String,
    start_sample: u64,
    end_sample: u64,
}

fn pinned(path: &Path, expected: &str) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|_| error(ErrorKind::Missing, "spoken-board-input"))?;
    let mut raw = Vec::new();
    file.take(MAX_CONTROL_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|_| error(ErrorKind::NeedsRecovery, "spoken-board-read"))?;
    if raw.len() as u64 > MAX_CONTROL_BYTES {
        return Err(error(ErrorKind::Budget, "spoken-board-input-bytes"));
    }
    if reel_workflow::digest(&raw) != expected {
        return Err(error(ErrorKind::Integrity, "spoken-board-input-pin"));
    }
    Ok(raw)
}

fn project(request: Request) -> Result<Board> {
    if request.schema != "reel.workflow-spoken-board.v1"
        || request.bundles.is_empty()
        || request.bundles.len() > 16
    {
        return Err(error(ErrorKind::Invalid, "spoken-board-request"));
    }
    let mut board = Board {
        schema: "reel.workflow-board.v1".into(),
        project: request.project,
        work_id: request.work_id,
        title: request.title,
        source_commit: request.source_commit,
        format: Format::Spoken,
        projection_request: None,
        approvals: Vec::new(),
        synthetic: false,
        languages: Vec::new(),
        scenes: Vec::new(),
        units: Vec::new(),
        cues: Vec::new(),
        media: BTreeMap::new(),
        tasks: BTreeMap::new(),
    };
    let mut languages = BTreeSet::new();
    for bundle in request.bundles {
        let raw = pinned(&bundle.path, &bundle.sha256)?;
        let access: Access = serde_json::from_slice(&raw)
            .map_err(|_| error(ErrorKind::Invalid, "spoken-board-access-json"))?;
        if access.schema != "reel.spoken-access.v1"
            || access.sample_rate != 48_000
            || bundle.waves.is_empty()
            || access.editions.len() > 16
        {
            return Err(error(ErrorKind::Invalid, "spoken-board-access-schema"));
        }
        let mut remaining = bundle.waves;
        for edition in access.editions {
            let Some(wave) = remaining.remove(&edition.edition_id) else {
                continue;
            };
            if wave.artifact.sha256 != edition.wave.sha256
                || wave.artifact.bytes != edition.wave.bytes
                || wave.artifact.media_type != "audio/wav"
                || edition.chapters.is_empty()
                || edition.chapters.len() > 16
                || board
                    .media
                    .insert(edition.edition_id.clone(), wave.clone())
                    .is_some()
            {
                return Err(error(ErrorKind::Conflict, "spoken-board-wave-or-edition"));
            }
            languages.insert(edition.language.clone());
            board.scenes.push(Scene {
                id: edition.edition_id.clone(),
                title: edition.edition_id.clone(),
            });
            let mut markers = Vec::new();
            for (index, chapter) in edition.chapters.into_iter().enumerate() {
                let id = format!("{}-chapter-{}", edition.edition_id, index + 1);
                markers.push(Marker {
                    semantic_id: id.clone(),
                    word: chapter.module_id.clone(),
                    sample: chapter.start_sample,
                });
                board.units.push(Unit {
                    id: id.clone(),
                    display_id: id.clone(),
                    story_id: id.clone(),
                    semantic_id: id,
                    scene_id: edition.edition_id.clone(),
                    title: chapter.module_id,
                    versions: vec![],
                    review_spans: vec![],
                    task: None,
                    spans: vec![Span {
                        language: edition.language.clone(),
                        cue_id: edition.edition_id.clone(),
                        start_sample: chapter.start_sample,
                        end_sample: chapter.end_sample,
                    }],
                });
            }
            let text = edition
                .entries
                .into_iter()
                .map(|entry| entry.text)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            let text = if text.is_empty() {
                "No spoken transcript entries".into()
            } else {
                text
            };
            board.cues.push(Cue {
                id: edition.edition_id.clone(),
                semantic_id: edition.edition_id.clone(),
                language: edition.language,
                title: edition.edition_id.clone(),
                text,
                selected_take: wave.artifact,
                preview: Some(edition.edition_id),
                sample_rate: access.sample_rate,
                samples: edition.duration_samples,
                markers,
            });
        }
        if !remaining.is_empty() {
            return Err(error(ErrorKind::Missing, "spoken-board-edition"));
        }
    }
    board.languages = languages
        .into_iter()
        .map(|id| Language {
            label: id.to_uppercase(),
            id,
        })
        .collect();
    Ok(board)
}

pub fn export(path: &Path, pin: &str, output: &Path) -> Result<Artifact> {
    let raw = pinned(path, pin)?;
    let request = serde_json::from_slice(&raw)
        .map_err(|_| error(ErrorKind::Invalid, "spoken-board-request-json"))?;
    let mut board = project(request)?;
    board.projection_request = Some(Artifact {
        logical_id: "workflow/spoken-board-request".into(),
        sha256: pin.into(),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    });
    board.check()?;
    let raw = serde_json::to_vec_pretty(&board)
        .map_err(|_| error(ErrorKind::Invalid, "spoken-board-json"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|_| error(ErrorKind::Conflict, "spoken-board-new-output"))?;
    file.write_all(&raw)
        .and_then(|_| file.sync_all())
        .map_err(|_| error(ErrorKind::NeedsRecovery, "spoken-board-output-write"))?;
    Ok(Artifact {
        logical_id: "workflow/spoken-board".into(),
        sha256: reel_workflow::digest(&raw),
        bytes: raw.len() as u64,
        media_type: "application/json".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn projects_new_and_reused_editions_and_refuses_wrong_audio() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = |edition: &str, language: &str, hash: &str| {
            let path = temp.path().join(format!("{edition}.json"));
            let raw = serde_json::to_vec(&json!({
                "schema":"reel.spoken-access.v1","sample_rate":48000,"editions":[{
                    "edition_id":edition,"language":language,
                    "wave":{"sha256":hash,"bytes":288044},"duration_samples":48000,
                    "entries":[{"text":format!("Transcript {language}")}],
                    "chapters":[{"module_id":"opening","start_sample":0,"end_sample":24000},
                        {"module_id":"closing","start_sample":24000,"end_sample":48000}]
                }]
            }))
            .unwrap();
            std::fs::write(&path, &raw).unwrap();
            json!({"path":path,"sha256":reel_workflow::digest(&raw),
                "waves":{(edition):{"path":temp.path().join(format!("{edition}.wav")),
                    "artifact":{"logical_id":format!("audio/{edition}"),"sha256":hash,
                        "bytes":288044,"media_type":"audio/wav"}}}})
        };
        let mut request = json!({"schema":"reel.workflow-spoken-board.v1",
            "project":"fixture","work_id":"programme","title":"Bilingual programme",
            "source_commit":"a".repeat(40),
            "bundles":[bundle("english","en",&"b".repeat(64)),bundle("spanish","es",&"c".repeat(64))]
        });
        let request_path = temp.path().join("request.json");
        let invoke = |request: &Value, output: &Path| {
            let raw = serde_json::to_vec(request).unwrap();
            std::fs::write(&request_path, &raw).unwrap();
            export(&request_path, &reel_workflow::digest(&raw), output)
        };
        let output = temp.path().join("board.json");
        let pin = invoke(&request, &output).unwrap();
        let raw = std::fs::read(output).unwrap();
        assert_eq!(pin.sha256, reel_workflow::digest(&raw));
        let board: Board = serde_json::from_slice(&raw).unwrap();
        assert_eq!(board.format, Format::Spoken);
        assert_eq!(board.cues.len(), 2);
        assert_eq!(board.units.len(), 4);
        assert_eq!(board.cues[1].language, "es");
        assert_eq!(board.cues[1].text, "Transcript es");
        assert_eq!(board.cues[1].markers[1].sample, 24000);
        request["bundles"][1]["waves"]["spanish"]["artifact"]["sha256"] = "d".repeat(64).into();
        let rejected = temp.path().join("rejected.json");
        assert_eq!(
            invoke(&request, &rejected).unwrap_err().kind,
            ErrorKind::Conflict
        );
        assert!(!rejected.exists());
    }
}
