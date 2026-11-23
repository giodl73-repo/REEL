//! Project checked spoken access manifests into the private workflow viewer.
use crate::{error, model::*};
use reel_workflow::{Artifact, ErrorKind, Result};
use serde::{Deserialize, Serialize};
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

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    path: PathBuf,
    sha256: String,
    waves: BTreeMap<String, WaveBinding>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WaveBinding {
    media: Media,
    compiled_recipe_sha256: String,
    execution_recipe_sha256: String,
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
    compiled_recipe_sha256: String,
    execution_recipe_sha256: String,
    wave: Wave,
    transcript: Option<Wave>,
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
#[serde(deny_unknown_fields)]
struct ExpectedEdition {
    compiled_recipe_sha256: String,
    execution_recipe_sha256: String,
}

pub fn bind_retained(
    project: &Path,
    result: &Path,
    result_pin: &str,
    expectation: &Path,
    expectation_pin: &str,
) -> Result<serde_json::Value> {
    let result: serde_json::Value = serde_json::from_slice(&pinned(result, result_pin)?)
        .map_err(|_| error(ErrorKind::Invalid, "spoken-board-result-json"))?;
    let mut expected: BTreeMap<String, ExpectedEdition> =
        serde_json::from_slice(&pinned(expectation, expectation_pin)?)
            .map_err(|_| error(ErrorKind::Invalid, "spoken-board-expected-json"))?;
    if result["schema"] != "caminos.production-job-result.v1"
        || result["workspace_retained"] != false
        || expected.is_empty()
        || expected.len() > 16
    {
        return Err(error(ErrorKind::Invalid, "spoken-board-retained-result"));
    }
    let registry = reel_assets::generic_objects(project)
        .map_err(|_| error(ErrorKind::Missing, "spoken-board-retained-registry"))?;
    let config = reel_assets::config(project)
        .map_err(|_| error(ErrorKind::Missing, "spoken-board-storage-config"))?;
    let resolve = |object: &serde_json::Value| -> Result<(Wave, PathBuf)> {
        let pin: Wave = serde_json::from_value(serde_json::json!({
            "sha256":object["sha256"],"bytes":object["bytes"]
        }))
        .map_err(|_| error(ErrorKind::Invalid, "spoken-board-retained-pin"))?;
        if registry.get(&pin.sha256) != Some(&pin.bytes) {
            return Err(error(
                ErrorKind::Missing,
                "spoken-board-unregistered-output",
            ));
        }
        let path = reel_assets::active_existing(&config, &pin.sha256, pin.bytes)
            .map_err(|_| error(ErrorKind::Integrity, "spoken-board-retained-bytes"))?
            .ok_or_else(|| error(ErrorKind::Missing, "spoken-board-retained-object"))?;
        Ok((pin, path))
    };
    resolve(&result["retirement_evidence"])?;
    let outputs = &result["outputs"];
    let (access_pin, path) = resolve(&outputs["spoken-access/access.json"])?;
    let access: Access = serde_json::from_slice(&pinned(&path, &access_pin.sha256)?)
        .map_err(|_| error(ErrorKind::Invalid, "spoken-board-access-json"))?;
    if access.schema != "reel.spoken-access.v1"
        || access.sample_rate != 48_000
        || access.editions.len() > 16
    {
        return Err(error(ErrorKind::Invalid, "spoken-board-access-schema"));
    }
    let mut waves = BTreeMap::new();
    for (index, edition) in access.editions.into_iter().enumerate() {
        let Some(desired) = expected.remove(&edition.edition_id) else {
            continue;
        };
        if !exact_recipe(
            &desired.compiled_recipe_sha256,
            &edition.compiled_recipe_sha256,
        ) || !exact_recipe(
            &desired.execution_recipe_sha256,
            &edition.execution_recipe_sha256,
        ) {
            return Err(error(ErrorKind::Conflict, "spoken-board-recipe"));
        }
        let (wave, wave_path) =
            resolve(&outputs[format!("spoken-candidate/edition-{index:03}.wav")])?;
        let (transcript, _) = resolve(&outputs[format!("spoken-access/edition-{index:03}.txt")])?;
        if wave.sha256 != edition.wave.sha256
            || wave.bytes != edition.wave.bytes
            || edition
                .transcript
                .is_none_or(|pin| pin.sha256 != transcript.sha256 || pin.bytes != transcript.bytes)
        {
            return Err(error(
                ErrorKind::Integrity,
                "spoken-board-retained-access-pin",
            ));
        }
        waves.insert(
            edition.edition_id.clone(),
            WaveBinding {
                media: Media {
                    path: wave_path,
                    subject: None,
                    artifact: Artifact {
                        logical_id: format!("spoken/{}", edition.edition_id),
                        sha256: wave.sha256,
                        bytes: wave.bytes,
                        media_type: "audio/wav".into(),
                    },
                },
                compiled_recipe_sha256: desired.compiled_recipe_sha256,
                execution_recipe_sha256: desired.execution_recipe_sha256,
            },
        );
    }
    if !expected.is_empty() {
        return Err(error(ErrorKind::Missing, "spoken-board-edition"));
    }
    serde_json::to_value(Bundle {
        path,
        sha256: access_pin.sha256,
        waves,
    })
    .map_err(|_| error(ErrorKind::Invalid, "spoken-board-binding-json"))
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
            let Some(binding) = remaining.remove(&edition.edition_id) else {
                continue;
            };
            if !exact_recipe(
                &binding.compiled_recipe_sha256,
                &edition.compiled_recipe_sha256,
            ) || !exact_recipe(
                &binding.execution_recipe_sha256,
                &edition.execution_recipe_sha256,
            ) {
                return Err(error(ErrorKind::Conflict, "spoken-board-recipe"));
            }
            let wave = binding.media;
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

fn exact_recipe(expected: &str, observed: &str) -> bool {
    expected.len() == 64
        && expected
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && expected == observed
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
    fn retained_binding_reopens_registry_objects_and_rejects_missing_transcript_registration() {
        let fixture = tempfile::tempdir().unwrap();
        let root = fixture.path();
        let cache = root.join("cache");
        std::fs::create_dir(root.join(".local")).unwrap();
        std::fs::create_dir(root.join("registry")).unwrap();
        std::fs::write(
            root.join(".local/storage-rust.json"),
            serde_json::to_vec(&json!({"cache_roots":[cache]})).unwrap(),
        )
        .unwrap();
        let put = |bytes: &[u8]| {
            let hash = reel_workflow::digest(bytes);
            let object = reel_assets::object(&cache, &hash);
            std::fs::create_dir_all(object.parent().unwrap()).unwrap();
            std::fs::write(object, bytes).unwrap();
            json!({"sha256":hash,"bytes":bytes.len()})
        };
        let wave = put(b"audio bytes for the registry binding test");
        let text = put(b"hello");
        let evidence = put(b"retired workspace evidence");
        let expected = json!({"english":{
            "compiled_recipe_sha256":"1".repeat(64),"execution_recipe_sha256":"2".repeat(64)
        }});
        let access = put(&serde_json::to_vec(&json!({
            "schema":"reel.spoken-access.v1","sample_rate":48000,"editions":[{
                "edition_id":"english","language":"en","compiled_recipe_sha256":"1".repeat(64),
                "execution_recipe_sha256":"2".repeat(64),"wave":wave,"transcript":text,
                "duration_samples":48000,"entries":[{"text":"hello"}],"chapters":[
                    {"module_id":"opening","start_sample":0,"end_sample":48000}]
            }]
        }))
        .unwrap());
        let result = json!({"schema":"caminos.production-job-result.v1","workspace_retained":false,
            "retirement_evidence":evidence,"outputs":{"spoken-access/access.json":access,
                "spoken-candidate/edition-000.wav":wave,"spoken-access/edition-000.txt":text}
        });
        let write = |path: &Path, value: &Value| {
            let bytes = serde_json::to_vec(value).unwrap();
            std::fs::write(path, &bytes).unwrap();
            reel_workflow::digest(&bytes)
        };
        let result_path = root.join("result.json");
        let expectation_path = root.join("expectation.json");
        let result_pin = write(&result_path, &result);
        let expectation_pin = write(&expectation_path, &expected);
        let register = |objects: &[&Value]| {
            let rows: String = objects
                .iter()
                .map(|object| {
                    format!(
                        "{}\t{}\n",
                        object["sha256"].as_str().unwrap(),
                        object["bytes"].as_u64().unwrap()
                    )
                })
                .collect();
            std::fs::write(root.join("registry/objects.tsv"), rows).unwrap();
        };
        let bind = || {
            bind_retained(
                root,
                &result_path,
                &result_pin,
                &expectation_path,
                &expectation_pin,
            )
        };
        register(&[&evidence, &access, &wave, &text]);
        assert_eq!(
            bind().unwrap()["waves"]["english"]["media"]["artifact"]["sha256"],
            wave["sha256"]
        );
        register(&[&evidence, &access, &wave]);
        assert_eq!(bind().unwrap_err().code, "spoken-board-unregistered-output");
    }

    #[test]
    fn projects_new_and_reused_editions_and_refuses_wrong_audio() {
        let temp = tempfile::tempdir().unwrap();
        let bundle = |edition: &str, language: &str, hash: &str| {
            let path = temp.path().join(format!("{edition}.json"));
            let raw = serde_json::to_vec(&json!({
                "schema":"reel.spoken-access.v1","sample_rate":48000,"editions":[{
                    "edition_id":edition,"language":language,
                    "compiled_recipe_sha256":"1".repeat(64),
                    "execution_recipe_sha256":"2".repeat(64),
                    "wave":{"sha256":hash,"bytes":288044},"duration_samples":48000,
                    "entries":[{"text":format!("Transcript {language}")}],
                    "chapters":[{"module_id":"opening","start_sample":0,"end_sample":24000},
                        {"module_id":"closing","start_sample":24000,"end_sample":48000}]
                }]
            }))
            .unwrap();
            std::fs::write(&path, &raw).unwrap();
            json!({"path":path,"sha256":reel_workflow::digest(&raw),
                "waves":{(edition):{"compiled_recipe_sha256":"1".repeat(64),
                    "execution_recipe_sha256":"2".repeat(64),
                    "media":{"path":temp.path().join(format!("{edition}.wav")),
                        "artifact":{"logical_id":format!("audio/{edition}"),"sha256":hash,
                            "bytes":288044,"media_type":"audio/wav"}}}}})
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
        let mut stale = request.clone();
        stale["bundles"][1]["waves"]["spanish"]["compiled_recipe_sha256"] = "9".repeat(64).into();
        assert_eq!(
            invoke(&stale, &temp.path().join("stale.json"))
                .unwrap_err()
                .code,
            "spoken-board-recipe"
        );
        request["bundles"][1]["waves"]["spanish"]["media"]["artifact"]["sha256"] =
            "d".repeat(64).into();
        let rejected = temp.path().join("rejected.json");
        assert_eq!(
            invoke(&request, &rejected).unwrap_err().kind,
            ErrorKind::Conflict
        );
        assert!(!rejected.exists());
    }
}
