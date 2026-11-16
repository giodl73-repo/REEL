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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceReconciliationProfile {
    pub schema: String,
    pub source_repairs_by_cue: BTreeMap<String, SourceRepair>,
    /// Explicitly reconcile producer bookkeeping to the existing job's gain.
    /// This never changes the selected job audio or its rendered level.
    #[serde(default)]
    pub baseline_gain_repairs_by_cue: BTreeMap<String, BaselineGainRepair>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineGainRepair {
    /// Effective old gain; an absent producer gain field means zero.
    pub expected_production_gain_db: f64,
    pub source_sha256: String,
    pub retained_gain_db: f64,
    pub reason: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRepair {
    pub expected_production_source: String,
    pub source_sha256: String,
    pub replacement_source: String,
    pub reason: String,
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
    require_source_match: bool,
    require_gain_match: bool,
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
        || (require_source_match
            && !event["source"]
                .as_str()
                .is_some_and(|s| s.ends_with(&audio.source.sha256)))
        || (require_gain_match
            && event.get("gain_db").and_then(Value::as_f64).unwrap_or(0.0) != audio.gain_db)
    {
        bail!("production cue {cue} differs from retained narration");
    }
    Ok(matching[0])
}

/// Compile a distinct immutable metadata bundle. This never renders or opens media.
pub fn compile(root: &Path, request: &Request) -> Result<Value> {
    compile_operation(root, request, false)
}

/// Reconcile explicitly named stale producer references to already selected
/// recordings and saved gains. This never changes the retained job audio.
pub fn reconcile_sources(root: &Path, request: &Request) -> Result<Value> {
    compile_operation(root, request, true)
}

fn compile_operation(root: &Path, request: &Request, reconcile: bool) -> Result<Value> {
    let schema = if reconcile {
        "reel.retained-dialogue-source-reconcile.v1"
    } else {
        "reel.retained-dialogue-gain-compile.v1"
    };
    if request.schema != schema {
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
    let profile: Option<Profile> = if reconcile {
        None
    } else {
        let profile: Profile = serde_yaml::from_slice(&profile_raw)?;
        if profile.schema != "reel.dialogue-gain-profile.v1"
            || profile.dialogue_gain_by_cue.is_empty()
        {
            bail!("invalid or empty retained dialogue gain profile");
        }
        crate::scene_delivery_compile::validate_dialogue_gains(&profile.dialogue_gain_by_cue)?;
        Some(profile)
    };
    let baseline_plan = semantic_delivery::plan_with_control_files(&request.semantic, &controls)?;
    let baseline_clock = cue_relative::compile_with_manifest_bytes(&contract, &production_raw)?;
    let mut changes = Vec::new();
    if reconcile {
        let repairs: SourceReconciliationProfile = serde_yaml::from_slice(&profile_raw)?;
        if repairs.schema != "reel.dialogue-source-reconciliation-profile.v1"
            || (repairs.source_repairs_by_cue.is_empty()
                && repairs.baseline_gain_repairs_by_cue.is_empty())
        {
            bail!("invalid or empty source reconciliation profile");
        }
        for (cue, repair) in repairs.baseline_gain_repairs_by_cue {
            let audio: Vec<_> = job
                .audio
                .iter()
                .filter(|a| a.cue_id.as_ref() == Some(&cue))
                .collect();
            if audio.len() != 1
                || audio[0].bus != "D"
                || audio[0].source.sha256 != repair.source_sha256
                || !repair.expected_production_gain_db.is_finite()
                || !repair.retained_gain_db.is_finite()
                || repair.retained_gain_db.abs() > 60.0
                || repair.retained_gain_db != audio[0].gain_db
                || repair.reason.trim().is_empty()
                || repair.reason.len() > 1024
            {
                bail!(
                    "baseline gain reconciliation cue {cue} differs from the selected D recording and saved gain"
                );
            }
            let index = production_cue_index(&production, &cue, audio[0], false, false)?;
            let current = match production["audio_events"][index].get("gain_db") {
                None => 0.0,
                Some(value) => value
                    .as_f64()
                    .filter(|gain| gain.is_finite())
                    .context("producer baseline gain must be finite numeric metadata")?,
            };
            if current != repair.expected_production_gain_db || current == repair.retained_gain_db {
                bail!("baseline gain reconciliation cue {cue} does not bind an exact stale gain");
            }
            changes.push(json!({"kind":"producer-baseline-gain-reconciliation","cue_id":cue,"source_sha256":repair.source_sha256,"before_production_gain_db":current,"retained_gain_db":repair.retained_gain_db,"reason":repair.reason}));
            production["audio_events"][index]["gain_db"] = json!(repair.retained_gain_db);
        }
        for (cue, repair) in repairs.source_repairs_by_cue {
            let audio: Vec<_> = job
                .audio
                .iter()
                .filter(|a| a.cue_id.as_ref() == Some(&cue))
                .collect();
            if audio.len() != 1
                || audio[0].bus != "D"
                || audio[0].source.sha256 != repair.source_sha256
                || repair.reason.trim().is_empty()
                || repair.reason.len() > 1024
            {
                bail!(
                    "source reconciliation cue {cue} is missing, ambiguous, non-D or differs from the selected recording"
                );
            }
            let audio = audio[0];
            let prefix = audio
                .source
                .sha256
                .get(..2)
                .context("invalid selected source hash")?;
            let canonical = format!("objects/sha256/{}/{}", prefix, audio.source.sha256);
            let direct = audio
                .source
                .path
                .to_str()
                .context("non-Unicode selected source")?;
            let index = production_cue_index(&production, &cue, audio, false, true)?;
            let current = production["audio_events"][index]["source"]
                .as_str()
                .context("production source missing")?;
            if current != repair.expected_production_source
                || current.ends_with(&audio.source.sha256)
                || (repair.replacement_source != canonical && repair.replacement_source != direct)
            {
                bail!(
                    "source reconciliation cue {cue} does not bind an exact stale source and selected replacement"
                );
            }
            changes.push(json!({"cue_id":cue,"before_source":current,"source":repair.replacement_source,"source_sha256":repair.source_sha256,"reason":repair.reason}));
            production["audio_events"][index]["source"] = json!(repair.replacement_source);
        }
    }
    // Unlisted D cues must be coherent too: preserving stale provenance is not
    // a valid successor merely because its gain was outside the requested mask.
    let mut narration_cues = BTreeSet::new();
    for audio in job.audio.iter().filter(|audio| audio.bus == "D") {
        if let Some(cue) = &audio.cue_id {
            if !narration_cues.insert(cue.as_str()) {
                bail!("ambiguous retained D cue {cue}");
            }
            production_cue_index(&production, cue, audio, true, true)?;
        }
    }
    for (cue, gain) in profile
        .as_ref()
        .into_iter()
        .flat_map(|p| &p.dialogue_gain_by_cue)
    {
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
        let event_index = production_cue_index(&production, cue, audio, true, true)?;
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
        "schema": if reconcile { "reel.retained-dialogue-source-reconcile-receipt.v1" } else { "reel.retained-dialogue-gain-compile-receipt.v1" },
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
    fn reconciliation_fixture(broken: &[usize], repair: &[usize]) -> (tempfile::TempDir, Request) {
        let (dir, mut request) = fixture();
        let mut production: Value =
            serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap()).unwrap();
        let stale = format!("objects/{}", "f".repeat(64));
        for index in broken {
            production["audio_events"][*index]["source"] = json!(stale);
        }
        request.production = write(dir.path(), "production.json", &production);
        let production_sha = request.production.sha256.clone();
        change_job(dir.path(), &mut request, |job| {
            job["production_manifest_sha256"] = json!(production_sha)
        });
        let repairs: serde_json::Map<String, Value> = repair.iter().map(|index| (format!("cue{}",index+1), json!({
            "expected_production_source":stale,"source_sha256":"c".repeat(64),
            "replacement_source":format!("objects/sha256/cc/{}","c".repeat(64)),
            "reason":"Correct producer metadata to the recording already selected by the retained job"
        }))).collect();
        request.profile = write(
            dir.path(),
            "profile.json",
            &json!({
                "schema":"reel.dialogue-source-reconciliation-profile.v1","source_repairs_by_cue":repairs
            }),
        );
        request.schema = "reel.retained-dialogue-source-reconcile.v1".into();
        request.output_dir = "reconciled".into();
        (dir, request)
    }
    #[test]
    fn explicit_source_reconciliation_preserves_job_media_and_enables_separate_gains() {
        let (dir, request) = reconciliation_fixture(&[1], &[1]);
        let before_job: Value =
            serde_json::from_slice(&fs::read(dir.path().join("delivery.json")).unwrap()).unwrap();
        let before_production: Value =
            serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap()).unwrap();
        let receipt = reconcile_sources(dir.path(), &request).unwrap();
        assert_eq!(
            receipt["schema"],
            "reel.retained-dialogue-source-reconcile-receipt.v1"
        );
        let root = dir.path().join("reconciled");
        let mut after_job: Value =
            serde_json::from_slice(&fs::read(root.join("delivery.json")).unwrap()).unwrap();
        after_job["production_manifest_sha256"] = before_job["production_manifest_sha256"].clone();
        assert_eq!(after_job, before_job);
        let mut after_production: Value =
            serde_json::from_slice(&fs::read(root.join("production.json")).unwrap()).unwrap();
        assert_eq!(
            after_production["audio_events"][1]["source"],
            format!("objects/sha256/cc/{}", "c".repeat(64))
        );
        after_production["audio_events"][1]["source"] =
            before_production["audio_events"][1]["source"].clone();
        assert_eq!(after_production, before_production);
        assert_eq!(
            fs::read(dir.path().join("contract.json")).unwrap(),
            fs::read(root.join("contract.json")).unwrap()
        );
        let profile = write(
            &root,
            "gain-profile.json",
            &json!({"schema":"reel.dialogue-gain-profile.v1",
            "dialogue_gain_by_cue":{"cue1":{"source_sha256":"c".repeat(64),"gain_db":2.5}}}),
        );
        let gain_request = Request {
            schema: "reel.retained-dialogue-gain-compile.v1".into(),
            semantic: serde_json::from_value(receipt["outputs"]["semantic"].clone()).unwrap(),
            production: serde_json::from_value(receipt["outputs"]["production"].clone()).unwrap(),
            profile,
            output_dir: "gain".into(),
        };
        let gained = compile(&root, &gain_request).unwrap();
        assert_eq!(gained["changes"][0]["gain_db"], 2.5);
        assert_eq!(
            fs::read(dir.path().join("production.json")).unwrap(),
            bytes(&before_production).unwrap()
        );
    }
    #[test]
    fn source_reconciliation_replay_is_byte_exact_and_never_overwrites() {
        let (dir, mut request) = reconciliation_fixture(&[1], &[1]);
        let first = reconcile_sources(dir.path(), &request).unwrap();
        request.output_dir = "replay".into();
        assert_eq!(first, reconcile_sources(dir.path(), &request).unwrap());
        for name in [
            "semantic.json",
            "delivery.json",
            "contract.json",
            "production.json",
            "profile.json",
            "compile-receipt.json",
        ] {
            assert_eq!(
                fs::read(dir.path().join("reconciled").join(name)).unwrap(),
                fs::read(dir.path().join("replay").join(name)).unwrap()
            );
        }
        assert!(reconcile_sources(dir.path(), &request).is_err());
    }
    #[test]
    fn source_reconciliation_refuses_wrong_expected_or_selected_source_and_undocumented_locator() {
        for (key, value) in [
            ("expected_production_source", json!("wrong")),
            ("source_sha256", json!("f".repeat(64))),
            (
                "replacement_source",
                json!(format!("unproven/{}", "c".repeat(64))),
            ),
            ("reason", json!("")),
        ] {
            let (dir, mut request) = reconciliation_fixture(&[1], &[1]);
            let mut profile: Value =
                serde_json::from_slice(&fs::read(dir.path().join("profile.json")).unwrap())
                    .unwrap();
            profile["source_repairs_by_cue"]["cue2"][key] = value;
            request.profile = write(dir.path(), "profile.json", &profile);
            assert!(reconcile_sources(dir.path(), &request).is_err());
            assert!(!dir.path().join("reconciled").exists());
        }
    }
    #[test]
    fn source_reconciliation_refuses_undeclared_stale_cues_without_partial_output() {
        let (dir, request) = reconciliation_fixture(&[0, 1], &[0]);
        let original = fs::read(dir.path().join("production.json")).unwrap();
        let error = reconcile_sources(dir.path(), &request)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cue2 differs from retained narration"));
        assert!(!dir.path().join("reconciled").exists());
        assert_eq!(
            original,
            fs::read(dir.path().join("production.json")).unwrap()
        );
    }

    fn baseline_fixture(both: bool, stale_source: bool) -> (tempfile::TempDir, Request) {
        let (dir, mut request) = if stale_source {
            reconciliation_fixture(&[0], &[0])
        } else {
            reconciliation_fixture(&[], &[])
        };
        change_job(dir.path(), &mut request, |job| {
            job["audio"][0]["gain_db"] = json!(0.010299956639812);
            if both {
                job["audio"][1]["gain_db"] = json!(0.010299956639812);
            }
        });
        let mut profile: Value =
            serde_json::from_slice(&fs::read(dir.path().join("profile.json")).unwrap()).unwrap();
        profile["baseline_gain_repairs_by_cue"] = json!({"cue1":{
            "expected_production_gain_db":0.0,"source_sha256":"c".repeat(64),
            "retained_gain_db":0.010299956639812,
            "reason":"Reconcile producer bookkeeping to the retained job without changing audio"
        }});
        request.profile = write(dir.path(), "profile.json", &profile);
        (dir, request)
    }

    #[test]
    fn explicit_baseline_repair_preserves_job_audio_and_exact_replay_without_media() {
        let (dir, mut request) = baseline_fixture(false, false);
        let before_job: Value =
            serde_json::from_slice(&fs::read(dir.path().join("delivery.json")).unwrap()).unwrap();
        let before_production: Value =
            serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap()).unwrap();
        let receipt = reconcile_sources(dir.path(), &request).unwrap();
        assert_eq!(receipt["primary_media_reads"], 0);
        assert_eq!(receipt["changes"].as_array().unwrap().len(), 1);
        assert_eq!(
            receipt["changes"][0]["kind"],
            "producer-baseline-gain-reconciliation"
        );
        let root = dir.path().join("reconciled");
        let mut after_job: Value =
            serde_json::from_slice(&fs::read(root.join("delivery.json")).unwrap()).unwrap();
        after_job["production_manifest_sha256"] = before_job["production_manifest_sha256"].clone();
        assert_eq!(after_job, before_job);
        let mut after_production: Value =
            serde_json::from_slice(&fs::read(root.join("production.json")).unwrap()).unwrap();
        assert_eq!(
            after_production["audio_events"][0]["gain_db"],
            0.010299956639812
        );
        after_production["audio_events"][0]
            .as_object_mut()
            .unwrap()
            .remove("gain_db");
        assert_eq!(after_production, before_production);
        assert_eq!(
            fs::read(root.join("contract.json")).unwrap(),
            fs::read(dir.path().join("contract.json")).unwrap()
        );
        request.output_dir = "replay".into();
        assert_eq!(receipt, reconcile_sources(dir.path(), &request).unwrap());
        for name in [
            "semantic.json",
            "delivery.json",
            "production.json",
            "contract.json",
            "profile.json",
            "compile-receipt.json",
        ] {
            assert_eq!(
                fs::read(root.join(name)).unwrap(),
                fs::read(dir.path().join("replay").join(name)).unwrap()
            );
        }
    }

    #[test]
    fn baseline_repair_and_source_repair_enable_a_separate_gain_compile() {
        let (dir, request) = baseline_fixture(false, true);
        let receipt = reconcile_sources(dir.path(), &request).unwrap();
        assert_eq!(receipt["changes"].as_array().unwrap().len(), 2);
        let root = dir.path().join("reconciled");
        let mut gain_request = Request {
            schema: "reel.retained-dialogue-gain-compile.v1".into(),
            semantic: serde_json::from_value(receipt["outputs"]["semantic"].clone()).unwrap(),
            production: serde_json::from_value(receipt["outputs"]["production"].clone()).unwrap(),
            profile: request.profile,
            output_dir: "gain".into(),
        };
        change_profile(
            &root,
            &mut gain_request,
            json!({"cue1":{"source_sha256":"c".repeat(64),"gain_db":2.5}}),
        );
        let result = compile(&root, &gain_request).unwrap();
        assert_eq!(result["changes"][0]["before_gain_db"], 0.010299956639812);
        assert_eq!(result["changes"][0]["gain_db"], 2.5);
    }

    #[test]
    fn baseline_repair_refuses_wrong_expected_gain_source_target_and_empty_reason() {
        for (key, value) in [
            ("expected_production_gain_db", json!(0.5)),
            ("source_sha256", json!("f".repeat(64))),
            ("retained_gain_db", json!(0.0103)),
            ("retained_gain_db", json!(61.0)),
            ("retained_gain_db", Value::Null),
            ("reason", json!(" ")),
        ] {
            let (dir, mut request) = baseline_fixture(false, false);
            let mut profile: Value =
                serde_json::from_slice(&fs::read(dir.path().join("profile.json")).unwrap())
                    .unwrap();
            profile["baseline_gain_repairs_by_cue"]["cue1"][key] = value;
            request.profile = write(dir.path(), "profile.json", &profile);
            assert!(reconcile_sources(dir.path(), &request).is_err());
            assert!(!dir.path().join("reconciled").exists());
        }
    }

    #[test]
    fn undeclared_baseline_difference_refuses_before_output_and_without_changing_inputs() {
        let (dir, request) = baseline_fixture(true, false);
        let original = fs::read(dir.path().join("production.json")).unwrap();
        let error = reconcile_sources(dir.path(), &request)
            .unwrap_err()
            .to_string();
        assert!(error.contains("cue2 differs from retained narration"));
        assert!(!dir.path().join("reconciled").exists());
        assert_eq!(
            original,
            fs::read(dir.path().join("production.json")).unwrap()
        );
    }

    #[test]
    fn baseline_repair_cannot_rewrite_a_coherent_gain_or_change_a_non_d_event() {
        for non_d in [false, true] {
            let (dir, mut request) = baseline_fixture(false, false);
            change_job(dir.path(), &mut request, |job| {
                if non_d {
                    job["audio"][0]["bus"] = json!("M");
                } else {
                    job["audio"][0]["gain_db"] = json!(0.0);
                }
            });
            if !non_d {
                let mut profile: Value =
                    serde_json::from_slice(&fs::read(dir.path().join("profile.json")).unwrap())
                        .unwrap();
                profile["baseline_gain_repairs_by_cue"]["cue1"]["retained_gain_db"] = json!(0.0);
                request.profile = write(dir.path(), "profile.json", &profile);
            }
            assert!(reconcile_sources(dir.path(), &request).is_err());
            assert!(!dir.path().join("reconciled").exists());
        }
    }

    #[test]
    fn baseline_repair_rejects_invalid_producer_gain_and_wrong_role() {
        for invalid in [Value::Null, json!("0"), json!("wrong-role")] {
            let (dir, mut request) = baseline_fixture(false, false);
            let mut production: Value =
                serde_json::from_slice(&fs::read(dir.path().join("production.json")).unwrap())
                    .unwrap();
            if invalid == json!("wrong-role") {
                production["audio_events"][0]["role"] = json!("music");
            } else {
                production["audio_events"][0]["gain_db"] = invalid;
            }
            request.production = write(dir.path(), "production.json", &production);
            let sha = request.production.sha256.clone();
            change_job(dir.path(), &mut request, |job| {
                job["production_manifest_sha256"] = json!(sha);
            });
            assert!(reconcile_sources(dir.path(), &request).is_err());
            assert!(!dir.path().join("reconciled").exists());
        }
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
