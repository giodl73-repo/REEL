//! Select an existing presentation master through scoped bindings, then make
//! a decoded-content-equivalent lossless segment for episode conform.

use crate::{episode_conform, episode_delivery, scene_delivery};
use anyhow::{Context, Result, bail};
use reel_assembly::scene_authoring::{ScopedBindings, TemplateCatalog};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, process::Stdio};

pub const SCHEMA: &str = "reel.presentation-adopt.v1";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub role: String,
    pub language: String,
    pub season_id: String,
    pub episode_id: String,
    pub template_id: String,
    pub catalog: scene_delivery::FileRef,
    pub template_definition: scene_delivery::FileRef,
    #[serde(default)]
    pub source_template: Option<scene_delivery::FileRef>,
    pub season_bindings: scene_delivery::FileRef,
    pub episode_bindings: scene_delivery::FileRef,
    pub source_binding: String,
    pub source: scene_delivery::FileRef,
    #[serde(default)]
    pub selection_evidence: Option<scene_delivery::FileRef>,
    #[serde(default)]
    pub evidence_hash_pointer: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    schema: String,
    template_id: String,
    kind: String,
    width: u64,
    height: u64,
    fps_numerator: u64,
    fps_denominator: u64,
    sample_rate: u32,
    duration_frames: u64,
    #[serde(default)]
    source_template_sha256: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub role: String,
    pub language: String,
    pub season_id: String,
    pub episode_id: String,
    pub template_id: String,
    pub template_definition_sha256: String,
    pub source_template_sha256: Option<String>,
    pub source_binding: String,
    pub source_logical_id: String,
    pub source_cache_uri: String,
    pub source_sha256: String,
    pub source_bytes: u64,
    pub selection_evidence_sha256: Option<String>,
    pub evidence_hash_pointer: Option<String>,
    pub master_sha256: String,
    pub master_bytes: u64,
    pub decoded_picture_sha256: String,
    pub decoded_audio_sha256: String,
    pub frames: u64,
    pub samples: u64,
    pub timestamps_verified: bool,
    pub source_content_matches_lossless_master: bool,
    pub technical_validation_state: String,
    pub creative_review_state: String,
    pub publication: String,
}

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn source_facts(path: &Path) -> Result<(u64, u64, String, u32)> {
    let output = episode_conform::command("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("FFprobe rejected selected presentation source");
    }
    let value: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    let streams = value["streams"]
        .as_array()
        .context("source lacks streams")?;
    let video = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audio = streams
        .iter()
        .filter(|stream| stream["codec_type"] == "audio")
        .collect::<Vec<_>>();
    if video.len() != 1 || audio.len() != 1 || audio[0]["channels"] != 2 {
        bail!("presentation source needs one picture and stereo audio stream");
    }
    let v = video[0];
    let a = audio[0];
    Ok((
        v["width"].as_u64().context("source width")?,
        v["height"].as_u64().context("source height")?,
        v["r_frame_rate"]
            .as_str()
            .context("source frame rate")?
            .into(),
        a["sample_rate"]
            .as_str()
            .context("source sample rate")?
            .parse()?,
    ))
}

pub fn build(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    output: &Path,
) -> Result<Receipt> {
    if output.exists() {
        bail!("presentation output must be a new directory");
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    if manifest.schema != SCHEMA
        || manifest.role.is_empty()
        || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.season_id.is_empty()
        || manifest.episode_id.is_empty()
        || manifest.template_id.is_empty()
        || manifest.source_binding.is_empty()
    {
        bail!("invalid presentation adoption manifest");
    }
    let read = |item: &scene_delivery::FileRef| -> Result<Vec<u8>> {
        Ok(fs::read(scene_delivery::checked_file(input_root, item)?)?)
    };
    let catalog: TemplateCatalog = serde_json::from_slice(&read(&manifest.catalog)?)?;
    let definition_bytes = read(&manifest.template_definition)?;
    let definition: Template = serde_json::from_slice(&definition_bytes)?;
    match (
        &definition.source_template_sha256,
        &manifest.source_template,
    ) {
        (Some(expected), Some(source)) if *expected == source.sha256 => {
            read(source)?;
        }
        (None, None) => {}
        _ => bail!("presentation source template binding is incomplete or stale"),
    }
    let selected_template = catalog
        .templates
        .iter()
        .find(|template| template.template_id == manifest.template_id)
        .context("presentation template is not selected")?;
    if catalog.schema != "reel.scene-template-catalog.v1"
        || definition.schema != "reel.selected-presentation-master-template.v1"
        || definition.template_id != manifest.template_id
        || definition.kind != manifest.role
        || selected_template.kind != manifest.role
        || selected_template.definition_sha256 != sha(&definition_bytes)
        || definition.width == 0
        || definition.height == 0
        || definition.fps_numerator == 0
        || definition.fps_denominator == 0
        || definition.sample_rate == 0
        || definition.duration_frames == 0
    {
        bail!("selected presentation template definition mismatch");
    }
    let season: ScopedBindings = serde_json::from_slice(&read(&manifest.season_bindings)?)?;
    let episode: ScopedBindings = serde_json::from_slice(&read(&manifest.episode_bindings)?)?;
    if season.schema != "reel.scene-asset-bindings.v1"
        || episode.schema != "reel.scene-asset-bindings.v1"
        || season.scope_id != manifest.season_id
        || episode.scope_id != manifest.episode_id
    {
        bail!("invalid scoped source bindings");
    }
    let found = [&season, &episode]
        .into_iter()
        .filter_map(|scope| scope.assets.get(&manifest.source_binding))
        .collect::<Vec<_>>();
    if found.len() != 1 {
        bail!("source binding must resolve in exactly one scope");
    }
    let selected = found[0];
    if !matches!(
        selected.selection_state.as_str(),
        "selected-private-production" | "principal-approved" | "release-cleared"
    ) || selected.sha256 != manifest.source.sha256
        || selected.bytes != manifest.source.bytes
        || selected.cache_uri != format!("cache://sha256/{}", selected.sha256)
    {
        bail!("presentation source differs from selected cache binding");
    }
    let selection_evidence_sha256 = match (
        &manifest.selection_evidence,
        &manifest.evidence_hash_pointer,
    ) {
        (Some(evidence), Some(pointer)) => {
            let bytes = read(evidence)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)?;
            let selected_hash = value
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .context("selection evidence pointer must name a hash or cache URI")?;
            let selected_hash = selected_hash
                .strip_prefix("cache://sha256/")
                .unwrap_or(selected_hash);
            if selected_hash != selected.sha256 {
                bail!("selection evidence differs from selected source binding");
            }
            Some(evidence.sha256.clone())
        }
        (None, None) => None,
        _ => bail!("selection evidence and hash pointer must be supplied together"),
    };
    let source = scene_delivery::checked_file(asset_root, &manifest.source)?;
    let (width, height, fps, sample_rate) = source_facts(&source)?;
    let expected_fps = format!(
        "{}/{}",
        definition.fps_numerator, definition.fps_denominator
    );
    if (width, height, fps.as_str(), sample_rate)
        != (
            definition.width,
            definition.height,
            expected_fps.as_str(),
            definition.sample_rate,
        )
    {
        bail!("presentation source differs from selected template media policy");
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".presentation-adopt-")
        .tempdir_in(parent)?;
    let master = stage.path().join("master.mkv");
    let status = episode_conform::command("ffmpeg")
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(&source)
        .args([
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-fps_mode",
            "passthrough",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv444p",
            "-c:a",
            "pcm_s24le",
            "-ac",
            "2",
            "-ar",
        ])
        .arg(definition.sample_rate.to_string())
        .arg(&master)
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        bail!("presentation lossless conversion failed");
    }
    let facts = episode_conform::probe(&master)?;
    if (
        facts.width,
        facts.height,
        facts.fps.as_str(),
        facts.sample_rate,
    ) != (
        definition.width,
        definition.height,
        expected_fps.as_str(),
        definition.sample_rate,
    ) {
        bail!("converted presentation media policy mismatch");
    }
    let (source_picture_sha, source_picture_bytes) =
        episode_conform::decoded_digest(&source, true)?;
    let (source_audio_sha, source_audio_bytes) = episode_conform::decoded_digest(&source, false)?;
    let (picture_sha, picture_bytes) = episode_conform::decoded_digest(&master, true)?;
    let (audio_sha, audio_bytes) = episode_conform::decoded_digest(&master, false)?;
    let frame_bytes = definition.width * definition.height * 3;
    if picture_sha != source_picture_sha
        || picture_bytes != source_picture_bytes
        || audio_sha != source_audio_sha
        || audio_bytes != source_audio_bytes
        || picture_bytes % frame_bytes != 0
        || audio_bytes % 6 != 0
    {
        bail!("lossless presentation changed decoded source picture or audio");
    }
    let frames = picture_bytes / frame_bytes;
    let samples = audio_bytes / 6;
    let expected_samples = (u128::from(definition.duration_frames)
        * u128::from(definition.sample_rate)
        * u128::from(definition.fps_denominator))
        / u128::from(definition.fps_numerator);
    if frames != definition.duration_frames
        || u128::from(samples).abs_diff(expected_samples)
            > u128::from(definition.sample_rate) * u128::from(definition.fps_denominator)
                / u128::from(definition.fps_numerator)
    {
        bail!("presentation duration differs from selected template");
    }
    episode_delivery::verify_timestamps(
        &master,
        definition.sample_rate,
        definition.fps_numerator,
        definition.fps_denominator,
    )?;
    let receipt = Receipt {
        schema: "reel.presentation-master-receipt.v1".into(),
        role: manifest.role,
        language: manifest.language,
        season_id: manifest.season_id,
        episode_id: manifest.episode_id,
        template_id: manifest.template_id,
        template_definition_sha256: sha(&definition_bytes),
        source_template_sha256: definition.source_template_sha256,
        source_binding: manifest.source_binding,
        source_logical_id: selected.logical_id.clone(),
        source_cache_uri: selected.cache_uri.clone(),
        source_sha256: selected.sha256.clone(),
        source_bytes: selected.bytes,
        selection_evidence_sha256,
        evidence_hash_pointer: manifest.evidence_hash_pointer,
        master_sha256: episode_conform::file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
        decoded_picture_sha256: picture_sha,
        decoded_audio_sha256: audio_sha,
        frames,
        samples,
        timestamps_verified: true,
        source_content_matches_lossless_master: true,
        technical_validation_state: "decoded-source-equivalent".into(),
        creative_review_state: "open; existing source selection does not grant creative approval"
            .into(),
        publication: "not-authorized".into(),
    };
    fs::write(
        stage.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    fs::rename(stage.path(), output)?;
    Ok(receipt)
}

/// Independently rerun the selected adoption and compare decoded content with
/// an existing master. This intentionally redoes the full source conversion;
/// callers can cache the checked result at a higher build-graph layer.
pub fn check(
    manifest_path: &Path,
    input_root: &Path,
    asset_root: &Path,
    master_path: &Path,
    receipt_path: &Path,
    stage_parent: &Path,
) -> Result<()> {
    let selected: Receipt = serde_json::from_slice(&fs::read(receipt_path)?)?;
    if selected.schema != "reel.presentation-master-receipt.v1"
        || selected.technical_validation_state != "decoded-source-equivalent"
        || !selected.timestamps_verified
        || !selected.source_content_matches_lossless_master
        || selected.master_sha256 != episode_conform::file_sha(master_path)?
        || selected.master_bytes != fs::metadata(master_path)?.len()
    {
        bail!("selected presentation adoption receipt or master is invalid");
    }
    fs::create_dir_all(stage_parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".presentation-recheck-")
        .tempdir_in(stage_parent)?;
    let rebuilt = build(
        manifest_path,
        input_root,
        asset_root,
        &stage.path().join("rebuilt"),
    )?;
    let mut selected_value = serde_json::to_value(&selected)?;
    let mut rebuilt_value = serde_json::to_value(&rebuilt)?;
    for value in [&mut selected_value, &mut rebuilt_value] {
        value
            .as_object_mut()
            .context("adoption receipt is not an object")?
            .remove("master_sha256");
        value
            .as_object_mut()
            .context("adoption receipt is not an object")?
            .remove("master_bytes");
    }
    if selected_value != rebuilt_value {
        bail!("selected presentation adoption differs from current source selection");
    }
    let manifest: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    let definition: Template = serde_json::from_slice(&fs::read(scene_delivery::checked_file(
        input_root,
        &manifest.template_definition,
    )?)?)?;
    let facts = episode_conform::probe(master_path)?;
    if (
        facts.width,
        facts.height,
        facts.fps.as_str(),
        facts.sample_rate,
    ) != (
        definition.width,
        definition.height,
        format!(
            "{}/{}",
            definition.fps_numerator, definition.fps_denominator
        )
        .as_str(),
        definition.sample_rate,
    ) {
        bail!("selected presentation master differs from adopted media policy");
    }
    episode_delivery::verify_timestamps(
        master_path,
        definition.sample_rate,
        definition.fps_numerator,
        definition.fps_denominator,
    )?;
    let (picture_sha, picture_bytes) = episode_conform::decoded_digest(master_path, true)?;
    let (audio_sha, audio_bytes) = episode_conform::decoded_digest(master_path, false)?;
    if picture_sha != rebuilt.decoded_picture_sha256
        || audio_sha != rebuilt.decoded_audio_sha256
        || picture_bytes == 0
        || audio_bytes == 0
    {
        bail!("selected presentation master differs from rechecked source content");
    }
    Ok(())
}
