//! Source and render verification for granular imported semantic scenes.
//! This is deliberately distinct from a high-level scene-authoring build.

use crate::{scene_delivery, semantic_delivery};
use anyhow::{Context, Result, bail};
use reel_assembly::{Graph, SelectedPointer};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub const MANIFEST_SCHEMA: &str = "reel.imported-scene-proof-manifest.v1";
pub const RECEIPT_SCHEMA: &str = "reel.imported-scene-source-proof.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub episode_id: String,
    pub scene_id: String,
    pub language: String,
    pub source_target: String,
    pub source_graph: scene_delivery::FileRef,
    /// Original producer selection record; this is evidence, not an approval.
    pub source_selection: scene_delivery::FileRef,
    /// Frozen episode/scene source capture, checked independently of the job.
    pub source_capture: scene_delivery::FileRef,
    pub semantic_delivery: scene_delivery::FileRef,
    pub job: scene_delivery::FileRef,
    pub production: scene_delivery::FileRef,
    #[serde(default)]
    pub source_evidence: Vec<scene_delivery::FileRef>,
    #[serde(default)]
    pub presentation_role: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub episode_id: String,
    pub scene_id: String,
    pub language: String,
    pub imported_manifest_sha256: String,
    pub source_graph_sha256: String,
    pub source_selection_sha256: String,
    pub source_capture_sha256: String,
    pub selected_semantic_delivery_sha256: String,
    pub selected_delivery_job_sha256: String,
    pub production_manifest_sha256: String,
    pub contract_sha256: String,
    pub scene_delivery_receipt_sha256: String,
    pub master_sha256: String,
    pub master_bytes: u64,
    pub frames: u64,
    pub samples: u64,
    pub presentation_role: Option<String>,
}

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    serde_yaml::from_slice(&fs::read(path)?).with_context(|| format!("invalid {}", path.display()))
}

/// The first version accepts exact source graphs only. Technical transforms
/// need their own verified grammar; a caller cannot waive changed events.
fn validate_source(
    manifest: &Manifest,
    source: &Graph,
    selection: &SelectedPointer,
    capture: &serde_json::Value,
    semantic: &semantic_delivery::SemanticDelivery,
) -> Result<()> {
    if manifest.schema != MANIFEST_SCHEMA
        || manifest.episode_id.trim().is_empty()
        || manifest.scene_id.trim().is_empty()
        || !matches!(manifest.language.as_str(), "es" | "en")
        || capture.get("schema").and_then(|v| v.as_str())
            != Some("reel.imported-scene-source-capture.v1")
        || capture
            .get("selected_pointer_sha256")
            .and_then(|v| v.as_str())
            != Some(manifest.source_selection.sha256.as_str())
        || capture
            .get("selected_semantic_deliveries")
            .and_then(|items| items.get(&manifest.language))
            .and_then(|item| item.get("sha256"))
            .and_then(|v| v.as_str())
            != Some(manifest.semantic_delivery.sha256.as_str())
        || capture.get("episode_id").and_then(|v| v.as_str()) != Some(manifest.episode_id.as_str())
        || capture.get("scene_id").and_then(|v| v.as_str()) != Some(manifest.scene_id.as_str())
        || capture
            .pointer("/source_graph/sha256")
            .and_then(|v| v.as_str())
            != Some(manifest.source_graph.sha256.as_str())
        || capture
            .get("selected_delivery_jobs")
            .and_then(|jobs| jobs.get(&manifest.language))
            .and_then(|job| job.get("sha256"))
            .and_then(|hash| hash.as_str())
            != Some(manifest.job.sha256.as_str())
        || selection.selected_lock.logical_id != source.lock.logical_id
        || selection.selected_lock.sha256 != source.lock.sha256
        || serde_json::to_value(selection)? != serde_json::to_value(&semantic.pointer)?
    {
        bail!("imported source selection does not bind the exact graph and identity");
    }
    if semantic.scene_id.as_deref() != Some(manifest.scene_id.as_str())
        || semantic.language.as_deref() != Some(manifest.language.as_str())
        || semantic.target != manifest.source_target
        || manifest.source_target != manifest.scene_id
        || !source
            .nodes
            .iter()
            .any(|node| node.id == manifest.scene_id && node.inputs.is_empty())
        || serde_json::to_value(source)? != serde_json::to_value(&semantic.graph)?
    {
        bail!(
            "imported semantic delivery differs from the complete selected source scene graph/scope"
        );
    }
    Ok(())
}

/// Exact imports retain the original job bytes, including every picture,
/// M/E/VFX marker, offset, source trim and gain. Changed jobs require a separate
/// successor grammar. Also verify that the source phrases actually lie inside
/// their selected native D and primary-picture spans.
fn validate_native_timeline(inputs: &VerifiedInputs, asset_root: &Path) -> Result<()> {
    let root = &inputs.input_root;
    let semantic_path = scene_delivery::checked_file(root, &inputs.manifest.semantic_delivery)?;
    let semantic: semantic_delivery::SemanticDelivery = read(&semantic_path)?;
    let capture_path = scene_delivery::checked_file(root, &inputs.manifest.source_capture)?;
    let capture: serde_json::Value = read(&capture_path)?;
    let (job, plan) = scene_delivery::plan(&inputs.job_path, asset_root)?;
    for binding in &semantic.event_bindings {
        let event = semantic
            .graph
            .events
            .iter()
            .find(|event| event.event_id == binding.event_id)
            .context("imported binding lacks source event")?;
        if event.scene_id != inputs.manifest.scene_id || event.language != inputs.manifest.language
        {
            bail!("imported binding contains a foreign scene/language event");
        }
        let audio = job
            .audio
            .iter()
            .find(|audio| audio.attachment_id == binding.narration_attachment_id)
            .context("imported event lacks native narration job")?;
        let cue = capture
            .get("event_cue_ids")
            .and_then(|map| map.get(&event.event_id))
            .and_then(|value| value.as_str())
            .context("source capture lacks selected event cue identity")?;
        if audio.bus != "D"
            || audio.cue_id.as_deref() != Some(cue)
            || audio.source.sha256 != event.narration.sha256
        {
            bail!("imported event differs from original selected D cue");
        }
        let narration = plan
            .audio
            .iter()
            .find(|span| span.attachment_id == audio.attachment_id)
            .context("imported event lacks compiled narration span")?;
        let picture = plan
            .pictures
            .iter()
            .find(|span| span.attachment_id == binding.picture_attachment_id)
            .context("imported event lacks compiled primary picture span")?;
        let (start, end) = phrase_interval(
            event.phrase_start_seconds,
            event.phrase_end_seconds,
            plan.sample_rate,
            audio.source_start_sample,
            narration,
            picture,
        )?;
        for id in &binding.audio_attachment_ids {
            let audio = job
                .audio
                .iter()
                .find(|audio| audio.attachment_id == *id)
                .context("imported M/E attachment missing")?;
            let span = plan
                .audio
                .iter()
                .find(|span| span.attachment_id == *id)
                .context("imported M/E span missing")?;
            if audio.bus == "D" || span.start_sample >= end || span.end_sample <= start {
                bail!("imported M/E attachment misses its source phrase");
            }
        }
        for id in &binding.external_layer_attachment_ids {
            let span = plan
                .external_layer_spans
                .iter()
                .find(|span| span.attachment_id == *id)
                .context("imported VFX span missing")?;
            if span.start_sample >= end || span.end_sample <= start {
                bail!("imported VFX attachment misses its source phrase");
            }
        }
    }
    Ok(())
}

fn phrase_interval(
    start: f64,
    end: f64,
    rate: u32,
    source_start: u64,
    narration: &scene_delivery::Span,
    picture: &scene_delivery::Span,
) -> Result<(u64, u64)> {
    if !start.is_finite() || !end.is_finite() || start < 0.0 || end <= start || rate == 0 {
        bail!("invalid imported source phrase clock");
    }
    let sample = |seconds: f64| -> Result<u64> {
        let samples = seconds * f64::from(rate);
        if !samples.is_finite() || samples >= u64::MAX as f64 {
            bail!("imported phrase clock overflows");
        }
        Ok(samples.round() as u64)
    };
    let place = |seconds| -> Result<u64> {
        narration
            .start_sample
            .checked_add(sample(seconds)?)
            .and_then(|value| value.checked_sub(source_start))
            .context("imported phrase outside native source trim")
    };
    let (start, end) = (place(start)?, place(end)?);
    if start < narration.start_sample
        || end > narration.end_sample
        || start >= end
        || start < picture.start_sample
        || end > picture.end_sample
    {
        bail!("imported narration or picture misses source phrase");
    }
    Ok((start, end))
}

pub struct VerifiedInputs {
    pub input_root: std::path::PathBuf,
    pub manifest: Manifest,
    pub job_path: std::path::PathBuf,
    pub contract_sha256: String,
    pub manifest_sha256: String,
}

pub fn verify_inputs(
    input_root: &Path,
    manifest_ref: &scene_delivery::FileRef,
) -> Result<VerifiedInputs> {
    let manifest_path = scene_delivery::checked_file(input_root, manifest_ref)?;
    let manifest: Manifest = read(&manifest_path)?;
    let graph_path = scene_delivery::checked_file(input_root, &manifest.source_graph)?;
    let source: Graph = read(&graph_path)?;
    let selection_path = scene_delivery::checked_file(input_root, &manifest.source_selection)?;
    let selection: SelectedPointer = read(&selection_path)?;
    let capture_path = scene_delivery::checked_file(input_root, &manifest.source_capture)?;
    let capture: serde_json::Value = read(&capture_path)?;
    let semantic_path = scene_delivery::checked_file(input_root, &manifest.semantic_delivery)?;
    let semantic: semantic_delivery::SemanticDelivery = read(&semantic_path)?;
    validate_source(&manifest, &source, &selection, &capture, &semantic)?;
    let semantic_base = semantic_path
        .parent()
        .context("semantic delivery lacks parent")?;
    let semantic_job = scene_delivery::checked_file(semantic_base, &semantic.scene_delivery_job)?;
    let job_path = scene_delivery::checked_file(input_root, &manifest.job)?;
    if semantic.scene_delivery_job.sha256 != manifest.job.sha256
        || semantic.scene_delivery_job.bytes != manifest.job.bytes
        || crate::sha256_file(&semantic_job)? != manifest.job.sha256
    {
        bail!("imported proof job differs from semantic delivery job");
    }
    let job: scene_delivery::Job = read(&job_path)?;
    scene_delivery::checked_file(
        job_path.parent().context("job lacks parent")?,
        &job.contract,
    )?;
    scene_delivery::checked_file(input_root, &manifest.production)?;
    if job.production_manifest_sha256 != manifest.production.sha256 {
        bail!("imported production differs from selected job");
    }
    for evidence in &manifest.source_evidence {
        scene_delivery::checked_file(input_root, evidence)?;
    }
    // Checks every scoped event and rejects orphan picture/audio/VFX attachments.
    semantic_delivery::plan(&semantic_path)?;
    Ok(VerifiedInputs {
        input_root: input_root.to_path_buf(),
        manifest,
        job_path,
        contract_sha256: job.contract.sha256,
        manifest_sha256: manifest_ref.sha256.clone(),
    })
}

fn receipt(
    inputs: &VerifiedInputs,
    native: &scene_delivery::Receipt,
    output: &Path,
) -> Result<Receipt> {
    let master = native
        .outputs
        .get("master.mkv")
        .context("native receipt lacks master")?;
    let m = &inputs.manifest;
    Ok(Receipt {
        schema: RECEIPT_SCHEMA.into(),
        episode_id: m.episode_id.clone(),
        scene_id: m.scene_id.clone(),
        language: m.language.clone(),
        imported_manifest_sha256: inputs.manifest_sha256.clone(),
        source_graph_sha256: m.source_graph.sha256.clone(),
        source_selection_sha256: m.source_selection.sha256.clone(),
        source_capture_sha256: m.source_capture.sha256.clone(),
        selected_semantic_delivery_sha256: m.semantic_delivery.sha256.clone(),
        selected_delivery_job_sha256: m.job.sha256.clone(),
        production_manifest_sha256: m.production.sha256.clone(),
        contract_sha256: inputs.contract_sha256.clone(),
        scene_delivery_receipt_sha256: crate::sha256_file(&output.join("receipt.json"))?,
        master_sha256: master.sha256.clone(),
        master_bytes: master.bytes,
        frames: native.delivery_frames,
        samples: native.content_samples,
        presentation_role: m.presentation_role.clone(),
    })
}

/// Issues a distinct proof only after actual source checks and full native
/// output verification. No render or media selection is performed here.
pub fn verify_render(
    input_root: &Path,
    manifest_ref: &scene_delivery::FileRef,
    asset_root: &Path,
    output: &Path,
) -> Result<Receipt> {
    let inputs = verify_inputs(input_root, manifest_ref)?;
    validate_native_timeline(&inputs, asset_root)?;
    let native = scene_delivery::check(&inputs.job_path, asset_root, output)?;
    let proof = receipt(&inputs, &native, output)?;
    let destination = output.join("imported-scene-source-proof.json");
    if destination.exists() {
        bail!("imported proof already exists; retain it and use a new output alias");
    }
    fs::write(destination, serde_json::to_vec_pretty(&proof)?)?;
    Ok(proof)
}

/// Rechecks both the original source closure and actual scene output. Episode
/// conform cannot accept a renamed native receipt as an imported source proof.
pub fn recheck(
    input_root: &Path,
    manifest_ref: &scene_delivery::FileRef,
    asset_root: &Path,
    output: &Path,
    selected: &serde_json::Value,
) -> Result<scene_delivery::Receipt> {
    let inputs = verify_inputs(input_root, manifest_ref)?;
    validate_native_timeline(&inputs, asset_root)?;
    let native = scene_delivery::check(&inputs.job_path, asset_root, output)?;
    if serde_json::to_value(receipt(&inputs, &native, output)?)? != *selected {
        bail!("imported source proof differs from reverified source/job/receipt/master");
    }
    Ok(native)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (
        Manifest,
        Graph,
        SelectedPointer,
        serde_json::Value,
        semantic_delivery::SemanticDelivery,
    ) {
        let hash = "a".repeat(64);
        let file = json!({"path":"source.json","sha256":hash,"bytes":1});
        let graph: Graph = serde_json::from_value(json!({
            "schema":"reel.assembly-graph.v1",
            "lock":{"logical_id":"selected","sha256":hash},
            "slots":[], "events":[],
            "nodes":[{"id":"scene-1","inputs":[],"slots":[],"events":[]}]
        }))
        .unwrap();
        let selection: SelectedPointer = serde_json::from_value(json!({
            "schema":"reel.selected-pointer.v1", "logical_id":"episode-selection",
            "selected_lock":{"logical_id":"selected","sha256":hash}
        }))
        .unwrap();
        let manifest: Manifest = serde_json::from_value(json!({
            "schema":MANIFEST_SCHEMA,"episode_id":"episode-1","scene_id":"scene-1",
            "language":"es","source_target":"scene-1","source_graph":file,
            "source_selection":file,"source_capture":file,"semantic_delivery":file,
            "job":file,"production":file
        }))
        .unwrap();
        let capture = json!({"schema":"reel.imported-scene-source-capture.v1",
            "episode_id":"episode-1","scene_id":"scene-1","selected_pointer_sha256":hash,
            "source_graph":{"sha256":hash},"selected_delivery_jobs":{"es":{"sha256":hash}},
            "selected_semantic_deliveries":{"es":{"sha256":hash}}});
        let semantic: semantic_delivery::SemanticDelivery = serde_json::from_value(json!({
            "schema":"reel.semantic-delivery.v1","id":"delivery","pointer":selection,
            "graph":graph,"target":"scene-1","scene_id":"scene-1","language":"es",
            "scene_delivery_job":file,"event_bindings":[]
        }))
        .unwrap();
        (manifest, graph, selection, capture, semantic)
    }

    #[test]
    fn identity_accepts_exact_source_pointer_and_capture() {
        let (m, g, p, c, s) = fixture();
        validate_source(&m, &g, &p, &c, &s).unwrap();
    }

    #[test]
    fn identity_rejects_wrong_episode_scene_or_graph_capture() {
        for pointer in [
            "/episode_id",
            "/scene_id",
            "/source_graph/sha256",
            "/selected_delivery_jobs/es/sha256",
            "/selected_pointer_sha256",
            "/selected_semantic_deliveries/es/sha256",
        ] {
            let (m, g, p, mut c, s) = fixture();
            *c.pointer_mut(pointer).unwrap() = json!("different");
            assert!(validate_source(&m, &g, &p, &c, &s).is_err());
        }
    }

    #[test]
    fn identity_rejects_reselected_lock_or_pointer() {
        let (m, g, mut p, c, s) = fixture();
        p.selected_lock.sha256 = "b".repeat(64);
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
        let (m, g, mut p, c, s) = fixture();
        p.logical_id = "another-selection".into();
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
    }

    #[test]
    fn identity_rejects_aggregate_target() {
        let (mut m, g, p, c, mut s) = fixture();
        m.source_target = "episode-root".into();
        s.target = m.source_target.clone();
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
        let (m, mut g, p, c, mut s) = fixture();
        g.nodes[0].inputs.push("sibling".into());
        s.graph = g.clone();
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
    }

    #[test]
    fn identity_rejects_foreign_language_and_changed_graph() {
        let (mut m, g, p, c, mut s) = fixture();
        m.language = "fr".into();
        s.language = Some("fr".into());
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
        let (m, g, p, c, mut s) = fixture();
        s.graph.nodes[0].slots.push("changed".into());
        assert!(validate_source(&m, &g, &p, &c, &s).is_err());
    }

    fn span(start_sample: u64, end_sample: u64) -> scene_delivery::Span {
        scene_delivery::Span {
            attachment_id: "test".into(),
            start_sample,
            end_sample,
            start_frame: 0,
            end_frame: 1,
        }
    }

    #[test]
    fn phrase_clock_places_source_trim_on_native_timeline() {
        assert_eq!(
            phrase_interval(
                2.0,
                3.0,
                48000,
                48000,
                &span(96000, 240000),
                &span(144000, 192000)
            )
            .unwrap(),
            (144000, 192000)
        );
    }

    #[test]
    fn phrase_clock_rejects_shifted_picture_outside_cue_and_invalid_values() {
        let narration = span(0, 48000);
        assert!(phrase_interval(0.0, 1.0, 48000, 0, &narration, &span(1, 48000)).is_err());
        assert!(phrase_interval(0.0, 1.001, 48000, 0, &narration, &span(0, 96000)).is_err());
        assert!(phrase_interval(0.0, 0.5, 48000, 1, &narration, &span(0, 48000)).is_err());
        for (start, end) in [
            (f64::NAN, 1.0),
            (0.0, f64::INFINITY),
            (-1.0, 1.0),
            (1.0, 1.0),
            (0.0, f64::MAX),
        ] {
            assert!(phrase_interval(start, end, 48000, 0, &narration, &narration).is_err());
        }
    }
}
