//! Metadata-only dialogue profiles for already retained semantic deliveries.
//! Existing contracts, selection graphs and attachment identities are preserved.
use crate::scene_delivery::{FileRef, InputFiles, Job};
use crate::scene_delivery_compile::DialogueGain;
use crate::{cue_relative, scene_delivery, semantic_delivery};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub semantic: FileRef,
    pub production: FileRef,
    pub profile: FileRef,
    pub output_dir: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    /// Same exact-recording gain entries used by native scene authoring.
    pub dialogue_gain_by_cue: BTreeMap<String, DialogueGain>,
}

fn hash(raw: &[u8]) -> String {
    Sha256::digest(raw)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn bytes(value: &Value) -> Result<Vec<u8>> {
    let mut raw = serde_json::to_vec_pretty(value)?;
    raw.push(b'\n');
    Ok(raw)
}
fn name(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("control/output path must be relative without traversal");
    }
    Ok(())
}
fn pin(path: PathBuf, raw: &[u8]) -> FileRef {
    FileRef {
        path,
        sha256: hash(raw),
        bytes: raw.len() as u64,
    }
}

#[derive(Default)]
struct Controls(BTreeMap<PathBuf, Vec<u8>>);
impl Controls {
    fn insert(&mut self, reference: &FileRef, raw: Vec<u8>) -> Result<()> {
        name(&reference.path)?;
        if hash(&raw) != reference.sha256 || raw.len() as u64 != reference.bytes {
            bail!("control pin mismatch");
        }
        if let Some(prior) = self.0.get(&reference.path) {
            if prior != &raw {
                bail!("conflicting control names");
            }
        }
        self.0.insert(reference.path.clone(), raw);
        Ok(())
    }
    fn load(&mut self, root: &Path, reference: &FileRef) -> Result<Vec<u8>> {
        let raw = scene_delivery::read_control(root, reference)?;
        self.insert(reference, raw.clone())?;
        Ok(raw)
    }
}
impl InputFiles for Controls {
    fn verified_path(&self, _: &FileRef) -> Result<PathBuf> {
        bail!("retained dialogue compilation cannot open primary media")
    }
    fn control_bytes(&self, item: &FileRef) -> Result<Vec<u8>> {
        self.0
            .get(&item.path)
            .cloned()
            .context("missing held control")
    }
}

fn production_cue_index(
    production: &Value,
    cue: &str,
    audio: &scene_delivery::Audio,
) -> Result<usize> {
    let events = production["audio_events"]
        .as_array()
        .context("production audio events missing")?;
    let matching: Vec<_> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["id"].as_str() == Some(cue))
        .map(|(i, _)| i)
        .collect();
    if matching.len() != 1 {
        bail!("production cue {cue} is missing or ambiguous");
    }
    let event = &events[matching[0]];
    if !matches!(event["role"].as_str(), Some("narration" | "dialogue"))
        || !event["source"]
            .as_str()
            .is_some_and(|s| s.ends_with(&audio.source.sha256))
        || event.get("gain_db").and_then(Value::as_f64).unwrap_or(0.0) != audio.gain_db
    {
        bail!("production cue {cue} differs from retained narration");
    }
    Ok(matching[0])
}

/// Compile a distinct immutable metadata bundle. This never renders or opens media.
pub fn compile(root: &Path, request: &Request) -> Result<Value> {
    if request.schema != "reel.retained-dialogue-gain-compile.v1" {
        bail!("invalid retained dialogue compile schema");
    }
    name(Path::new(&request.output_dir))?;
    let mut controls = Controls::default();
    let semantic_raw = controls.load(root, &request.semantic)?;
    let semantic: semantic_delivery::SemanticDelivery = serde_yaml::from_slice(&semantic_raw)?;
    let mut semantic_value: Value = serde_yaml::from_slice(&semantic_raw)?;
    let job_ref = scene_delivery::scoped_control(&request.semantic, &semantic.scene_delivery_job)?;
    let job_raw = controls.load(root, &job_ref)?;
    let job: Job = serde_yaml::from_slice(&job_raw)?;
    if job.schema != "reel.scene-delivery.v0.1"
        || job.id.trim().is_empty()
        || job.width == 0
        || job.height == 0
        || job.max_composition_samples == 0
    {
        bail!("invalid retained scene delivery job");
    }
    let mut job_value: Value = serde_yaml::from_slice(&job_raw)?;
    let contract_ref = scene_delivery::scoped_control(&job_ref, &job.contract)?;
    let contract_raw = controls.load(root, &contract_ref)?;
    let contract: cue_relative::Contract = serde_yaml::from_slice(&contract_raw)?;
    let expected_production = scene_delivery::scoped_control(
        &contract_ref,
        &FileRef {
            path: contract.production_manifest.clone(),
            sha256: job.production_manifest_sha256.clone(),
            bytes: request.production.bytes,
        },
    )?;
    if expected_production != request.production {
        bail!("production differs from retained job scope");
    }
    let production_raw = controls.load(root, &request.production)?;
    let mut production: Value = serde_yaml::from_slice(&production_raw)?;
    let profile_raw = controls.load(root, &request.profile)?;
    let profile: Profile = serde_yaml::from_slice(&profile_raw)?;
    if profile.schema != "reel.dialogue-gain-profile.v1" || profile.dialogue_gain_by_cue.is_empty()
    {
        bail!("invalid or empty retained dialogue gain profile");
    }
    crate::scene_delivery_compile::validate_dialogue_gains(&profile.dialogue_gain_by_cue)?;
    let baseline_plan = semantic_delivery::plan_with_control_files(&request.semantic, &controls)?;
    let baseline_clock = cue_relative::compile_with_manifest_bytes(&contract, &production_raw)?;
    // Unlisted D cues must be coherent too: preserving stale provenance is not
    // a valid successor merely because its gain was outside the requested mask.
    let mut narration_cues = BTreeSet::new();
    for audio in job.audio.iter().filter(|audio| audio.bus == "D") {
        if let Some(cue) = &audio.cue_id {
            if !narration_cues.insert(cue.as_str()) {
                bail!("ambiguous retained D cue {cue}");
            }
            production_cue_index(&production, cue, audio)?;
        }
    }
    let mut changes = Vec::new();
    for (cue, gain) in &profile.dialogue_gain_by_cue {
        let audio: Vec<_> = job
            .audio
            .iter()
            .enumerate()
            .filter(|(_, a)| a.cue_id.as_ref() == Some(cue))
            .collect();
        if audio.len() != 1
            || audio[0].1.bus != "D"
            || audio[0].1.source.sha256 != gain.source_sha256
        {
            bail!(
                "dialogue gain cue {cue} is missing, ambiguous, non-D or bound to another source"
            );
        }
        let (audio_index, audio) = audio[0];
        let event_index = production_cue_index(&production, cue, audio)?;
        job_value["audio"][audio_index]["gain_db"] = json!(gain.gain_db);
        production["audio_events"][event_index]["gain_db"] = json!(gain.gain_db);
        changes.push(json!({"cue_id": cue, "source_sha256": gain.source_sha256, "before_gain_db": audio.gain_db, "gain_db": gain.gain_db}));
    }
    let candidate_production = bytes(&production)?;
    let mut before = serde_json::to_value(baseline_clock)?;
    let mut after = serde_json::to_value(cue_relative::compile_with_manifest_bytes(
        &contract,
        &candidate_production,
    )?)?;
    before
        .as_object_mut()
        .unwrap()
        .remove("production_manifest_sha256");
    after
        .as_object_mut()
        .unwrap()
        .remove("production_manifest_sha256");
    if before != after {
        bail!("dialogue profile changed the retained clock");
    }
    let production_pin = pin(request.production.path.clone(), &candidate_production);
    job_value["production_manifest_sha256"] = json!(production_pin.sha256);
    let candidate_job = bytes(&job_value)?;
    let job_pin = pin(job_ref.path.clone(), &candidate_job);
    semantic_value["scene_delivery_job"]["sha256"] = json!(job_pin.sha256);
    semantic_value["scene_delivery_job"]["bytes"] = json!(job_pin.bytes);
    let candidate_semantic = bytes(&semantic_value)?;
    let semantic_pin = pin(request.semantic.path.clone(), &candidate_semantic);
    controls
        .0
        .insert(production_pin.path.clone(), candidate_production);
    controls.0.insert(job_pin.path.clone(), candidate_job);
    controls
        .0
        .insert(semantic_pin.path.clone(), candidate_semantic);
    let candidate_plan = semantic_delivery::plan_with_control_files(&semantic_pin, &controls)?;
    if serde_json::to_value(baseline_plan.selection)?
        != serde_json::to_value(candidate_plan.selection)?
    {
        bail!("dialogue profile changed the retained selection");
    }
    let receipt = json!({
        "schema": "reel.retained-dialogue-gain-compile-receipt.v1",
        "inputs": {"semantic": request.semantic, "job": job_ref, "contract": contract_ref, "production": request.production, "profile": request.profile},
        "outputs": {"semantic": semantic_pin, "job": job_pin, "contract": contract_ref, "production": production_pin, "profile": request.profile},
        "changes": changes, "contract_bytes_preserved": true, "clock_preserved": true,
        "selection_preserved": true, "primary_media_reads": 0, "media_rendered": 0,
        "human_approval": false
    });
    let root = root.canonicalize()?;
    if controls.0.contains_key(Path::new("compile-receipt.json")) {
        bail!("receipt filename collides with input control");
    }
    let output = root.join(&request.output_dir);
    let parent = output.parent().context("output parent missing")?;
    // Never create an output through an existing link outside the metadata root.
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor.parent().context("output ancestor missing")?;
    }
    if !ancestor.canonicalize()?.starts_with(&root) {
        bail!("output escapes metadata root");
    }
    fs::create_dir_all(parent)?;
    if !parent.canonicalize()?.starts_with(&root) {
        bail!("output escapes metadata root");
    }
    fs::create_dir(&output).context("output already exists or cannot be exclusively claimed")?;
    for (relative, raw) in &controls.0 {
        let target = output.join(relative);
        fs::create_dir_all(target.parent().context("control parent missing")?)?;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(target)?
            .write_all(raw)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("compile-receipt.json"))?
        .write_all(&bytes(&receipt)?)?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reel_assembly::{GRAPH_SCHEMA, POINTER_SCHEMA};

    fn write(root: &Path, path: &str, value: &Value) -> FileRef {
        let raw = bytes(value).unwrap();
        fs::write(root.join(path), &raw).unwrap();
        pin(path.into(), &raw)
    }
    fn fixture() -> (tempfile::TempDir, Request) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let pic = "b".repeat(64);
        let voice = "c".repeat(64);
        let overlay = "d".repeat(64);
        let font = "e".repeat(64);
        let production = write(
            root,
            "production.json",
            &json!({
                "manifest_version":"reel.manifest.v0.2", "work":"fixture", "title":"Fixture",
                "audio_events":[
                    {"id":"cue1", "role":"narration", "source":format!("objects/{voice}"), "start_seconds":0},
                    {"id":"cue2", "role":"narration", "source":format!("objects/{voice}"), "start_seconds":1}
                ]
            }),
        );
        let contract = write(
            root,
            "contract.json",
            &json!({
                "schema":"reel.audio-event-relative-assembly.v0.1", "id":"fixture",
                "production_manifest":"production.json", "sample_rate":48000,
                "frame_rate":{"numerator":24,"denominator":1},
                "cues":[{"cue_id":"cue1","duration_samples":48000},{"cue_id":"cue2","duration_samples":48000}]
            }),
        );
        let job = write(
            root,
            "delivery.json",
            &json!({
                "schema":"reel.scene-delivery.v0.1", "id":"fixture", "contract":contract,
                "production_manifest_sha256":production.sha256, "width":1280,"height":720,"max_composition_samples":480000,
                "pictures":[
                    {"attachment_id":"picture-0","source":{"path":"absent-picture","sha256":pic,"bytes":1},"kind":"still","attention":"a"},
                    {"attachment_id":"picture-1","source":{"path":"absent-picture","sha256":pic,"bytes":1},"kind":"still","attention":"b"}
                ],
                "audio":[
                    {"attachment_id":"d-one","cue_id":"cue1","bus":"D","source":{"path":"absent-audio","sha256":voice,"bytes":1},"source_start_sample":17,"fade_in_samples":23,"fade_out_samples":29},
                    {"attachment_id":"d-two","cue_id":"cue2","bus":"D","source":{"path":"absent-audio","sha256":voice,"bytes":1}}
                ],
                "external_layers":[{"attachment_id":"title-old-id","reason":"retain exact title","render_mode":"ass-overlay",
                    "evidence":{"path":"absent-ass","sha256":overlay,"bytes":1},"font":{"path":"absent-font","sha256":font,"bytes":1}}],
                "buses":{"D":{"state":"present","reason":"narration"},"M":{"state":"intentional-silence","reason":"none"},"E":{"state":"intentional-silence","reason":"none"}}
            }),
        );
        let lock = json!({"logical_id":"lock","sha256":"a".repeat(64)});
        let mut slots = Vec::new();
        for (id, lane, sha) in [
            ("cel", "picture", &pic),
            ("voice", "narration", &voice),
            ("title", "presentation", &overlay),
            ("font", "presentation", &font),
        ] {
            slots.push(json!({"slot_id":id,"beat_id":"beat","lane":lane,"disposition":"selected","selected_revision_id":"r1",
                "revisions":[{"revision_id":"r1","asset":{"logical_id":id,"sha256":sha,"cache_uri":format!("cache://sha256/{sha}")}}]}));
        }
        let events: Vec<_> = (1..=2).map(|i| json!({"event_id":format!("event{i}"),"scene_id":"fixture","language":"es",
            "narration":{"logical_id":"voice","sha256":voice},"picture":{"logical_id":"cel","sha256":pic},"phrase_start_seconds":0,"phrase_end_seconds":1})).collect();
        let semantic = write(
            root,
            "semantic.json",
            &json!({
                "schema":"reel.semantic-delivery.v1","id":"fixture","target":"fixture","scene_id":"fixture","language":"es",
                "pointer":{"schema":POINTER_SCHEMA,"logical_id":"current","selected_lock":lock},
                "graph":{"schema":GRAPH_SCHEMA,"lock":lock,"slots":slots,"events":events,
                    "nodes":[{"id":"fixture","inputs":[],"slots":["cel","voice","title","font"],"events":["event1","event2"]}],"presentation_targets":[]},
                "scene_delivery_job":job,
                "event_bindings":[
                    {"event_id":"event1","narration_attachment_id":"d-one","picture_attachment_id":"picture-0","additional_picture_attachment_ids":["picture-1"],"external_layer_attachment_ids":["title-old-id"]},
                    {"event_id":"event2","narration_attachment_id":"d-two","picture_attachment_id":"picture-1"}
                ]
            }),
        );
        let profile = write(
            root,
            "profile.json",
            &json!({"schema":"reel.dialogue-gain-profile.v1",
            "dialogue_gain_by_cue":{"cue1":{"source_sha256":voice,"gain_db":2.5}}}),
        );
        (
            dir,
            Request {
                schema: "reel.retained-dialogue-gain-compile.v1".into(),
                semantic,
                production,
                profile,
                output_dir: "candidate".into(),
            },
        )
    }
    fn change_profile(root: &Path, request: &mut Request, entry: Value) {
        request.profile = write(
            root,
            "profile.json",
            &json!({"schema":"reel.dialogue-gain-profile.v1","dialogue_gain_by_cue":entry}),
        );
    }
    fn change_job(root: &Path, request: &mut Request, change: impl FnOnce(&mut Value)) {
        let mut job: Value =
            serde_json::from_slice(&fs::read(root.join("delivery.json")).unwrap()).unwrap();
        change(&mut job);
        let updated = write(root, "delivery.json", &job);
        let mut semantic: Value =
            serde_json::from_slice(&fs::read(root.join("semantic.json")).unwrap()).unwrap();
        semantic["scene_delivery_job"] = serde_json::to_value(updated).unwrap();
        request.semantic = write(root, "semantic.json", &semantic);
    }
    #[test]
    fn rejects_non_dialogue_ambiguous_cues_and_unselected_font() {
        for kind in 0..3 {
            let (dir, mut request) = fixture();
            change_job(dir.path(), &mut request, |job| match kind {
                0 => job["audio"][0]["bus"] = json!("M"),
                1 => job["audio"][1]["cue_id"] = json!("cue1"),
                _ => job["external_layers"][0]["font"]["sha256"] = json!("f".repeat(64)),
            });
            assert!(compile(dir.path(), &request).is_err());
            assert!(!dir.path().join("candidate").exists());
        }
    }
    #[test]
    fn dialogue_role_and_nonzero_baseline_use_absolute_saved_gain() {
        let (dir, mut request) = fixture();
        let mut production: Value =
            serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap()).unwrap();
        production["audio_events"][0]["role"] = json!("dialogue");
        production["audio_events"][0]["gain_db"] = json!(1.5);
        request.production = write(dir.path(), "production.json", &production);
        let production_sha = request.production.sha256.clone();
        change_job(dir.path(), &mut request, |job| {
            job["production_manifest_sha256"] = json!(production_sha);
            job["audio"][0]["gain_db"] = json!(1.5);
        });
        compile(dir.path(), &request).unwrap();
        let job: Value =
            serde_json::from_slice(&fs::read(dir.path().join("candidate/delivery.json")).unwrap())
                .unwrap();
        assert_eq!(job["audio"][0]["gain_db"], 2.5);
        let after: Value = serde_json::from_slice(
            &fs::read(dir.path().join("candidate/production.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(after["audio_events"][0]["role"], "dialogue");
        assert_eq!(after["audio_events"][0]["gain_db"], 2.5);
    }
    #[test]
    fn refuses_stale_source_on_unlisted_dialogue_cue() {
        let (dir, mut request) = fixture();
        let mut production: Value =
            serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap()).unwrap();
        production["audio_events"][1]["source"] = json!(format!("objects/{}", "f".repeat(64)));
        request.production = write(dir.path(), "production.json", &production);
        let production_sha = request.production.sha256.clone();
        change_job(dir.path(), &mut request, |job| {
            job["production_manifest_sha256"] = json!(production_sha)
        });
        let error = compile(dir.path(), &request).unwrap_err().to_string();
        assert!(error.contains("cue2 differs from retained narration"));
        assert!(!dir.path().join("candidate").exists());
    }
    #[test]
    fn preserves_legacy_cuts_font_offsets_unlisted_cues_and_exact_replay_without_media() {
        let (dir, mut request) = fixture();
        let receipt = compile(dir.path(), &request).unwrap();
        assert_eq!(receipt["primary_media_reads"], 0);
        assert_eq!(
            fs::read(dir.path().join("contract.json")).unwrap(),
            fs::read(dir.path().join("candidate/contract.json")).unwrap()
        );
        let before: Value =
            serde_json::from_slice(&fs::read(dir.path().join("delivery.json")).unwrap()).unwrap();
        let mut after: Value =
            serde_json::from_slice(&fs::read(dir.path().join("candidate/delivery.json")).unwrap())
                .unwrap();
        assert_eq!(after["audio"][0]["gain_db"], 2.5);
        after["audio"][0].as_object_mut().unwrap().remove("gain_db");
        after["production_manifest_sha256"] = before["production_manifest_sha256"].clone();
        assert_eq!(before, after);
        request.output_dir = "replay".into();
        assert_eq!(receipt, compile(dir.path(), &request).unwrap());
        for name in [
            "semantic.json",
            "delivery.json",
            "contract.json",
            "production.json",
            "profile.json",
            "compile-receipt.json",
        ] {
            assert_eq!(
                fs::read(dir.path().join("candidate").join(name)).unwrap(),
                fs::read(dir.path().join("replay").join(name)).unwrap()
            );
        }
    }
    #[test]
    fn rejects_wrong_source_unknown_cue_and_out_of_range_before_output() {
        for entry in [
            json!({"cue1":{"source_sha256":"f".repeat(64),"gain_db":1}}),
            json!({"missing":{"source_sha256":"c".repeat(64),"gain_db":1}}),
            json!({"cue1":{"source_sha256":"c".repeat(64),"gain_db":61}}),
        ] {
            let (dir, mut request) = fixture();
            change_profile(dir.path(), &mut request, entry);
            assert!(compile(dir.path(), &request).is_err());
            assert!(!dir.path().join("candidate").exists());
        }
    }
    #[test]
    fn refuses_corrupt_control_escape_and_output_collision() {
        let (dir, mut request) = fixture();
        request.output_dir = "../escape".into();
        assert!(compile(dir.path(), &request).is_err());
        request.output_dir = "candidate".into();
        let raw = fs::read(dir.path().join("profile.json")).unwrap();
        fs::write(dir.path().join("profile.json"), b"corrupt").unwrap();
        assert!(compile(dir.path(), &request).is_err());
        fs::write(dir.path().join("profile.json"), raw).unwrap();
        compile(dir.path(), &request).unwrap();
        let original = fs::read(dir.path().join("candidate/compile-receipt.json")).unwrap();
        assert!(compile(dir.path(), &request).is_err());
        assert_eq!(
            original,
            fs::read(dir.path().join("candidate/compile-receipt.json")).unwrap()
        );
    }
}
