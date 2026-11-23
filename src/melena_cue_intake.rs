//! Read-only verification of the exact recorded-cue package used for workflow prep.
use crate::tig_inputs::{Profile, Source};
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};
use tig_core::{Digest, Object};
use tig_store::existing::{ByteSource, ReadCache};

const CONTROL_MAX: usize = 4 * 1024 * 1024;
const CONTROL_TOTAL_MAX: u64 = 32 * 1024 * 1024;
const CUE_COUNT_MAX: usize = 4096;

#[derive(Deserialize)]
struct Package {
    schema: String,
    artifact_id: String,
    kind: String,
    public_release: bool,
    metadata: Metadata,
    assets: BTreeMap<String, AssetPin>,
}
#[derive(Deserialize, PartialEq, Eq)]
struct AssetPin {
    sha256: Digest,
    bytes: u64,
}
impl AssetPin {
    fn object(&self) -> Object {
        Object {
            sha256: self.sha256.clone(),
            bytes: self.bytes,
        }
    }
}
#[derive(Deserialize)]
struct Metadata {
    episode_id: String,
    ordered_source_scenes: Vec<ScenePin>,
}
#[derive(Deserialize)]
struct ScenePin {
    scene_id: String,
    sha256: Digest,
}
#[derive(Deserialize)]
struct Scene {
    schema: String,
    episode_id: String,
    scene_id: String,
    languages: BTreeMap<String, Language>,
}
#[derive(Deserialize)]
struct Language {
    cues: Vec<Cue>,
}
#[derive(Deserialize)]
struct Cue {
    cue_id: String,
    source_order: u64,
    spoken_text_sha256: Digest,
    #[serde(flatten)]
    audio: AssetPin,
}
#[derive(Deserialize)]
struct Performance {
    film_id: String,
    jobs: Vec<Job>,
}
#[derive(Deserialize)]
struct Job {
    cue_id: String,
    language_code: String,
    source_order: u64,
    source_blocks: Vec<u64>,
    text_sha256: Digest,
}

fn control(cache: &mut ReadCache, pin: &Object) -> Result<Vec<u8>> {
    ensure!(pin.bytes <= CONTROL_MAX as u64, "cue control too large");
    let mut bytes = Vec::new();
    cache
        .open_verified(pin)?
        .take(pin.bytes + 1)
        .read_to_end(&mut bytes)?;
    ensure!(Object::from_bytes(&bytes) == *pin, "cue control changed");
    Ok(bytes)
}

fn workflow_input(id: &str, pin: &Object) -> Value {
    json!({"logical_id":id,"sha256":pin.sha256,"bytes":pin.bytes,"media_type":"application/json"})
}

pub fn import(profile: &Path, id: &str, output: &Path) -> Result<Value> {
    let profile = Profile::read(profile)?;
    let commit = profile.commit.clone();
    let mut objects = ReadCache::new(profile.byte_source.clone())?;
    let mut source = Source::open(profile)?;
    let reference = source.authoring_file(id)?;
    let (raw, _held) = source.authoring_snapshot(&reference, CONTROL_MAX)?;
    let package_pin = Object::from_bytes(&raw);
    let package: Package = serde_json::from_slice(&raw)?;
    ensure!(
        package.schema == "caminos.artifact-package.v1"
            && package.artifact_id == id
            && package.kind == "recorded-episode-cue-draft"
            && !package.public_release
            && (1..=128).contains(&package.metadata.ordered_source_scenes.len()),
        "recorded cue package contract"
    );
    let performance_pin = package
        .assets
        .get("source-performance-plan")
        .context("performance plan pin")?;
    let performance_raw = control(&mut objects, &performance_pin.object())?;
    let performance: Performance = serde_json::from_slice(&performance_raw)?;
    ensure!(
        performance.film_id == package.metadata.episode_id,
        "performance episode identity"
    );
    let mut jobs = BTreeMap::new();
    for job in &performance.jobs {
        ensure!(
            jobs.insert((&job.language_code, &job.cue_id), job)
                .is_none(),
            "duplicate performance cue"
        );
    }

    let mut control_total = package_pin.bytes + performance_pin.bytes;
    let mut seen_scenes = BTreeSet::new();
    let mut seen_cues = BTreeSet::new();
    let mut inventory = Vec::new();
    let mut files = vec![
        ("reviewed-cues.json".to_string(), raw),
        ("performance-plan.json".to_string(), performance_raw),
    ];
    for (index, declared) in package.metadata.ordered_source_scenes.iter().enumerate() {
        ensure!(seen_scenes.insert(&declared.scene_id), "duplicate scene");
        let key = format!("source-scene/{}", declared.scene_id);
        let pin = package.assets.get(&key).context("scene asset pin")?;
        ensure!(
            pin.sha256 == declared.sha256,
            "scene hash disagrees with package"
        );
        ensure!(pin.bytes <= CONTROL_MAX as u64, "scene control too large");
        control_total += pin.bytes;
        ensure!(
            control_total <= CONTROL_TOTAL_MAX,
            "cue controls exceed total byte budget"
        );
        let scene_raw = control(&mut objects, &pin.object())?;
        let scene: Scene = serde_json::from_slice(&scene_raw)?;
        ensure!(
            scene.schema == "caimitos.cue-scene-draft.v1"
                && scene.episode_id == package.metadata.episode_id
                && scene.scene_id == declared.scene_id
                && !scene.languages.is_empty(),
            "scene identity contract"
        );

        let mut cues = Vec::new();
        for (language, ordered) in &scene.languages {
            let mut previous = None;
            ensure!(!ordered.cues.is_empty(), "empty scene cue list");
            for cue in &ordered.cues {
                ensure!(
                    previous.is_none_or(|order| order < cue.source_order),
                    "scene cue order"
                );
                previous = Some(cue.source_order);
                let key = format!("cue/{language}/{}", cue.cue_id);
                ensure!(
                    seen_cues.len() < CUE_COUNT_MAX && seen_cues.insert(key.clone()),
                    "duplicate or excessive package cues"
                );
                ensure!(
                    package.assets.get(&key) == Some(&cue.audio),
                    "scene cue audio pin disagrees with package"
                );
                let job = jobs
                    .get(&(language, &cue.cue_id))
                    .context("cue absent from performance plan")?;
                ensure!(
                    job.text_sha256 == cue.spoken_text_sha256
                        && job.source_order == cue.source_order,
                    "cue differs from performance plan"
                );
                drop(
                    objects
                        .open_verified(&cue.audio.object())
                        .context("retained cue bytes")?,
                );
                cues.push(json!({"cue_id":cue.cue_id,"language":language,"source_blocks":job.source_blocks,
                    "order":cue.source_order,"audio":cue.audio.object(),"text_sha256":cue.spoken_text_sha256}));
            }
        }
        let logical_id = format!("scene-{}-source", index + 1);
        files.push((format!("{logical_id}.json"), scene_raw));
        inventory.push(json!({"id":scene.scene_id,"source":pin.object(),"cues":cues,
            "inputs":[workflow_input(&logical_id,&pin.object()),workflow_input("reviewed-cues",&package_pin),workflow_input("performance-plan",&performance_pin.object())]}));
    }
    ensure!(
        package
            .assets
            .keys()
            .filter(|key| key.starts_with("cue/"))
            .eq(seen_cues.iter()),
        "package cue coverage"
    );
    ensure!(
        package
            .assets
            .keys()
            .filter(|key| key.starts_with("source-scene/"))
            .count()
            == seen_scenes.len(),
        "package scene coverage"
    );

    let evidence = json!({"schema":"reel.melena-cue-intake.v1","episode":package.metadata.episode_id,
        "source_commit":commit,"source_checkpoint":source.checkpoint(),
        "source_record":id,"package":package_pin,"scenes":inventory,
        "takes_selected":false,"playback_listened":false,"timing_qualified":false});
    let output = std::path::absolute(output)?;
    std::fs::create_dir(&output).context("create new cue intake directory")?;
    let mut bindings = BTreeMap::new();
    for (name, bytes) in files {
        let path = output.join(&name);
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?
            .write_all(&bytes)?;
        bindings.insert(name.trim_end_matches(".json").to_string(), path);
    }
    bindings.insert("inventory".to_string(), output.join("inventory.json"));
    for (name, value) in [
        ("file-bindings.json", serde_json::to_value(bindings)?),
        ("inventory.json", evidence),
    ] {
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?
            .write_all(&serde_json::to_vec_pretty(&value)?)?;
    }
    Ok(json!({"scenes":seen_scenes.len(),"cues":seen_cues.len(),"payloads_verified":true}))
}
