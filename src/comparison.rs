use std::{
    collections::BTreeSet,
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::Builder;

use crate::{
    adapters::{
        ffmpeg::FfmpegAdapter,
        still_animatic::{self, AnimaticReceipt, AnimaticRenderReport},
    },
    audio_quality::AudioCheckReport,
    production,
};

pub const COMPARISON_SCHEMA: &str = "reel.comparison.v0.1";
pub const COMPARISON_ARTIFACT_SCHEMA: &str = "reel.comparison-artifacts.v0.1";
pub const COMPARISON_RECEIPT_SCHEMA: &str = "reel.comparison-receipt.v0.1";
const DIMENSIONS: [&str; 7] = [
    "captions",
    "motion",
    "voice",
    "mix",
    "visual-treatment",
    "duration",
    "stream-facts",
];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonContract {
    pub schema: String,
    pub id: String,
    pub opening: OpeningSlate,
    pub variants: Vec<ComparisonVariant>,
    pub label_mode: String,
    pub blind_seed: Option<String>,
    pub changed_dimension: String,
    pub fixed_dimensions: Vec<String>,
    pub variant_slate_duration_ms: u64,
    pub protected_silence_ms: u64,
    pub chime: Option<PathBuf>,
    #[serde(default)]
    pub replay: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpeningSlate {
    pub title: String,
    pub instructions: String,
    pub duration_ms: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonVariant {
    pub id: String,
    pub label: Option<String>,
    pub video: PathBuf,
    pub receipt: PathBuf,
    pub artifact: Option<PathBuf>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonChildEvidence {
    pub order: usize,
    pub id: String,
    pub presented_label: String,
    pub video_sha256: String,
    pub receipt_sha256: String,
    pub source_artifact_sha256: String,
    pub receipt: AnimaticReceipt,
    pub local_artifact_sha256: Option<String>,
    pub duration_ms: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub audio_streams: usize,
    pub audio_signature_sha256: Option<String>,
    pub voice_signature_sha256: Option<String>,
    pub mix_signature_sha256: Option<String>,
    pub motion_signature_sha256: Option<String>,
    pub visual_signature_sha256: Option<String>,
    pub caption_signature_sha256: Option<String>,
    pub replayed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonArtifactReport {
    pub schema: String,
    pub comparison_id: String,
    pub contract_sha256: String,
    pub output: String,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub label_mode: String,
    pub blind_seed_sha256: Option<String>,
    pub chime_sha256: Option<String>,
    pub changed_dimension: String,
    pub fixed_dimensions: Vec<String>,
    pub inclusion_order_is_approval: bool,
    pub children: Vec<ComparisonChildEvidence>,
    pub tool_version: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonReceipt {
    pub schema: String,
    pub source_artifact_schema: String,
    pub source_artifact_sha256: String,
    pub tool_version: String,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub duration_ms: u64,
    pub audio_streams: usize,
    pub children: usize,
    pub child_receipt_sha256: Vec<String>,
    pub changed_dimension: String,
    pub fixed_dimensions: Vec<String>,
    pub blinded: bool,
    pub blind_seed_sha256: Option<String>,
    pub inclusion_order_is_approval: bool,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonComposeReport {
    pub schema: String,
    pub output: String,
    pub artifact: String,
    pub receipt: String,
    pub output_sha256: String,
    pub children: usize,
    pub duration_ms: u64,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonReceiptCheckReport {
    pub schema: String,
    pub receipt_sha256: String,
    pub video_sha256: String,
    pub output_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_ms: u64,
    pub audio_streams: usize,
    pub children: usize,
    pub changed_dimension: String,
    pub passed: bool,
}

struct LoadedChild {
    evidence: ComparisonChildEvidence,
    video: PathBuf,
}

struct SlateRenderOptions<'a> {
    text: &'a str,
    duration_ms: u64,
    width: u32,
    height: u32,
    fps: u32,
    chime: Option<&'a Path>,
    protected_ms: u64,
    output: &'a Path,
}

pub fn compose(
    contract_path: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<ComparisonComposeReport> {
    let contract_path = contract_path.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve comparison contract {}",
            contract_path.as_ref().display()
        )
    })?;
    let contract_bytes = fs::read(&contract_path)?;
    let contract: ComparisonContract = serde_yaml::from_slice(&contract_bytes)
        .context("comparison contract is not valid strict YAML")?;
    validate_contract(&contract)?;
    let base = contract_path.parent().unwrap_or_else(|| Path::new("."));
    let labels = presented_labels(&contract)?;
    let mut children = Vec::new();
    for (index, variant) in contract.variants.iter().enumerate() {
        children.push(load_child(
            base,
            variant,
            &labels[index],
            index + 1,
            contract.replay,
        )?);
    }
    if children
        .iter()
        .map(|child| &child.evidence.receipt_sha256)
        .collect::<BTreeSet<_>>()
        .len()
        != children.len()
        || children
            .iter()
            .map(|child| &child.evidence.video_sha256)
            .collect::<BTreeSet<_>>()
            .len()
            != children.len()
    {
        bail!("comparison variants must reference distinct verified receipts and videos");
    }
    enforce_fixed_dimensions(&contract.fixed_dimensions, &children)?;
    enforce_changed_dimension(&contract.changed_dimension, &children)?;

    let output = output.as_ref();
    let artifact_path = output.with_extension("comparison.artifacts.json");
    let receipt_path = output.with_extension("comparison.receipt.json");
    for path in [output, artifact_path.as_path(), receipt_path.as_path()] {
        if path.exists() {
            bail!(
                "refusing to overwrite comparison output: {}",
                path.display()
            );
        }
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new()
        .prefix(".reel-comparison-")
        .tempdir_in(parent)?;
    let staged_video = staging.path().join("comparison.mp4");
    render_comparison(&contract, base, &children, staging.path(), &staged_video)?;
    let probe = probe_delivery(&staged_video)?;
    let expected_duration_ms = contract.opening.duration_ms
        + children
            .iter()
            .map(|child| {
                contract.variant_slate_duration_ms
                    + child.evidence.duration_ms
                    + if contract.replay {
                        contract.variant_slate_duration_ms + child.evidence.duration_ms
                    } else {
                        0
                    }
            })
            .sum::<u64>();
    let frame_ms = (1000.0 / probe.fps).ceil() as u64;
    if probe.duration_ms.abs_diff(expected_duration_ms) > frame_ms {
        bail!(
            "comparison duration {}ms differs from declared segment total {}ms",
            probe.duration_ms,
            expected_duration_ms
        );
    }
    let output_sha256 = production::sha256_path(&staged_video)?;
    let output_bytes = fs::metadata(&staged_video)?.len();
    let blind_seed_sha256 = contract.blind_seed.as_deref().map(hash_text);
    let artifact = ComparisonArtifactReport {
        schema: COMPARISON_ARTIFACT_SCHEMA.to_string(),
        comparison_id: contract.id.clone(),
        contract_sha256: hash_bytes(&contract_bytes),
        output: output
            .canonicalize()
            .unwrap_or_else(|_| {
                parent
                    .canonicalize()
                    .unwrap_or_else(|_| parent.to_path_buf())
                    .join(output.file_name().unwrap_or_default())
            })
            .display()
            .to_string(),
        output_sha256: output_sha256.clone(),
        output_bytes,
        width: probe.width,
        height: probe.height,
        fps: probe.fps.round() as u32,
        duration_ms: probe.duration_ms,
        label_mode: contract.label_mode.clone(),
        blind_seed_sha256: blind_seed_sha256.clone(),
        chime_sha256: contract
            .chime
            .as_ref()
            .map(|path| resolve(base, path).and_then(production::sha256_path))
            .transpose()?,
        changed_dimension: contract.changed_dimension.clone(),
        fixed_dimensions: contract.fixed_dimensions.clone(),
        inclusion_order_is_approval: false,
        children: children
            .iter()
            .map(|child| child.evidence.clone())
            .collect(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        verified: true,
    };
    let staged_artifact = staging.path().join("comparison.artifacts.json");
    fs::write(
        &staged_artifact,
        format!("{}\n", serde_json::to_string_pretty(&artifact)?),
    )?;
    let receipt = ComparisonReceipt {
        schema: COMPARISON_RECEIPT_SCHEMA.to_string(),
        source_artifact_schema: COMPARISON_ARTIFACT_SCHEMA.to_string(),
        source_artifact_sha256: production::sha256_path(&staged_artifact)?,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        output_sha256: output_sha256.clone(),
        output_bytes,
        width: probe.width,
        height: probe.height,
        fps: probe.fps.round() as u32,
        duration_ms: probe.duration_ms,
        audio_streams: probe.audio_streams,
        children: children.len(),
        child_receipt_sha256: children
            .iter()
            .map(|child| child.evidence.receipt_sha256.clone())
            .collect(),
        changed_dimension: contract.changed_dimension,
        fixed_dimensions: contract.fixed_dimensions,
        blinded: contract.label_mode == "blinded",
        blind_seed_sha256,
        inclusion_order_is_approval: false,
        verified: true,
    };
    validate_receipt(&receipt)?;
    let staged_receipt = staging.path().join("comparison.receipt.json");
    fs::write(
        &staged_receipt,
        format!("{}\n", serde_json::to_string_pretty(&receipt)?),
    )?;
    publish_group([
        (&staged_video, output),
        (&staged_artifact, artifact_path.as_path()),
        (&staged_receipt, receipt_path.as_path()),
    ])?;
    Ok(ComparisonComposeReport {
        schema: "reel.comparison-compose.v0.1".to_string(),
        output: output.display().to_string(),
        artifact: artifact_path.display().to_string(),
        receipt: receipt_path.display().to_string(),
        output_sha256,
        children: children.len(),
        duration_ms: probe.duration_ms,
        verified: true,
    })
}

pub fn check_receipt(
    receipt_path: impl AsRef<Path>,
    video: impl AsRef<Path>,
) -> Result<ComparisonReceiptCheckReport> {
    let receipt_path = receipt_path.as_ref().canonicalize()?;
    let video = video.as_ref().canonicalize()?;
    let receipt: ComparisonReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("comparison receipt is not valid strict JSON")?;
    validate_receipt(&receipt)?;
    let video_sha256 = production::sha256_path(&video)?;
    if video_sha256 != receipt.output_sha256 || fs::metadata(&video)?.len() != receipt.output_bytes
    {
        bail!("comparison video does not match receipt hash or byte length");
    }
    let probe = probe_delivery(&video)?;
    let frame_ms = (1000.0 / f64::from(receipt.fps)).ceil() as u64;
    if probe.width != receipt.width
        || probe.height != receipt.height
        || (probe.fps - f64::from(receipt.fps)).abs() > 0.001
        || probe.duration_ms.abs_diff(receipt.duration_ms) > frame_ms
        || probe.audio_streams != receipt.audio_streams
    {
        bail!("comparison video stream facts do not match receipt");
    }
    Ok(ComparisonReceiptCheckReport {
        schema: "reel.comparison-receipt-check.v0.1".to_string(),
        receipt_sha256: production::sha256_path(&receipt_path)?,
        video_sha256,
        output_bytes: receipt.output_bytes,
        width: probe.width,
        height: probe.height,
        fps: probe.fps,
        duration_ms: probe.duration_ms,
        audio_streams: probe.audio_streams,
        children: receipt.children,
        changed_dimension: receipt.changed_dimension,
        passed: true,
    })
}

fn validate_contract(contract: &ComparisonContract) -> Result<()> {
    if contract.schema != COMPARISON_SCHEMA {
        bail!("unsupported comparison schema {}", contract.schema);
    }
    require_text("comparison id", &contract.id)?;
    require_text("opening title", &contract.opening.title)?;
    require_text("opening instructions", &contract.opening.instructions)?;
    if contract.variants.len() < 2 || contract.variants.len() > 26 {
        bail!("comparison requires 2..=26 variants");
    }
    if !(500..=30_000).contains(&contract.opening.duration_ms)
        || !(500..=15_000).contains(&contract.variant_slate_duration_ms)
        || contract.protected_silence_ms * 2 >= contract.variant_slate_duration_ms
    {
        bail!("comparison slate or protected-silence timing is infeasible");
    }
    if !DIMENSIONS.contains(&contract.changed_dimension.as_str()) {
        bail!(
            "unsupported changed dimension {}",
            contract.changed_dimension
        );
    }
    let mut fixed = BTreeSet::new();
    for dimension in &contract.fixed_dimensions {
        if !DIMENSIONS.contains(&dimension.as_str()) || !fixed.insert(dimension) {
            bail!("invalid or duplicate fixed dimension {dimension}");
        }
        if dimension == &contract.changed_dimension {
            bail!("changed dimension cannot also be fixed");
        }
    }
    let mut ids = BTreeSet::new();
    for variant in &contract.variants {
        require_text("variant id", &variant.id)?;
        if !ids.insert(&variant.id) {
            bail!("duplicate comparison variant id {}", variant.id);
        }
        if let Some(label) = &variant.label {
            require_text("variant label", label)?;
        }
    }
    match contract.label_mode.as_str() {
        "descriptive"
            if contract.blind_seed.is_none()
                && contract.variants.iter().all(|variant| {
                    variant
                        .label
                        .as_ref()
                        .is_some_and(|label| !label.trim().is_empty())
                }) => {}
        "blinded"
            if contract
                .blind_seed
                .as_ref()
                .is_some_and(|seed| !seed.trim().is_empty())
                && contract
                    .variants
                    .iter()
                    .all(|variant| variant.label.is_none()) => {}
        _ => bail!("label mode, labels, and blind seed are inconsistent"),
    }
    let labels = presented_labels(contract)?;
    if labels.iter().collect::<BTreeSet<_>>().len() != labels.len() {
        bail!("comparison presented labels must be unique");
    }
    Ok(())
}

fn presented_labels(contract: &ComparisonContract) -> Result<Vec<String>> {
    if contract.label_mode == "descriptive" {
        return Ok(contract
            .variants
            .iter()
            .map(|variant| variant.label.clone().expect("validated label"))
            .collect());
    }
    let seed = contract
        .blind_seed
        .as_deref()
        .expect("validated blind seed");
    let mut ranked = contract
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| (hash_text(&format!("{seed}\0{}", variant.id)), index))
        .collect::<Vec<_>>();
    ranked.sort();
    let mut labels = vec![String::new(); contract.variants.len()];
    for (label_index, (_, variant_index)) in ranked.into_iter().enumerate() {
        labels[variant_index] = ((b'A' + label_index as u8) as char).to_string();
    }
    Ok(labels)
}

fn load_child(
    base: &Path,
    variant: &ComparisonVariant,
    label: &str,
    order: usize,
    replayed: bool,
) -> Result<LoadedChild> {
    let video = resolve(base, &variant.video)?;
    let receipt_path = resolve(base, &variant.receipt)?;
    still_animatic::check_animatic_receipt(&receipt_path, &video)?;
    let receipt: AnimaticReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)?;
    let mut evidence = ComparisonChildEvidence {
        order,
        id: variant.id.clone(),
        presented_label: label.to_string(),
        video_sha256: receipt.output_sha256.clone(),
        receipt_sha256: production::sha256_path(&receipt_path)?,
        source_artifact_sha256: receipt.source_artifact_sha256.clone(),
        receipt: receipt.clone(),
        local_artifact_sha256: None,
        duration_ms: receipt.duration_ms,
        width: receipt.width,
        height: receipt.height,
        fps: receipt.fps,
        audio_streams: receipt.audio_streams,
        audio_signature_sha256: None,
        voice_signature_sha256: None,
        mix_signature_sha256: None,
        motion_signature_sha256: None,
        visual_signature_sha256: None,
        caption_signature_sha256: None,
        replayed,
    };
    if let Some(artifact_path) = &variant.artifact {
        let artifact_path = resolve(base, artifact_path)?;
        still_animatic::check_animatic(&artifact_path)?;
        let bytes = fs::read(&artifact_path)?;
        if production::sha256_path(&artifact_path)? != receipt.source_artifact_sha256 {
            bail!("child artifact does not match its receipt");
        }
        let artifact: AnimaticRenderReport = serde_json::from_slice(&bytes)?;
        evidence.local_artifact_sha256 = Some(hash_bytes(&bytes));
        evidence.audio_signature_sha256 = artifact
            .inputs
            .iter()
            .find(|input| input.kind == "audio")
            .map(|input| input.sha256.clone());
        evidence.motion_signature_sha256 = Some(hash_serialized(&artifact.motion)?);
        evidence.visual_signature_sha256 = Some(hash_serialized(
            &artifact
                .inputs
                .iter()
                .filter(|input| input.kind == "visual")
                .map(|input| &input.sha256)
                .collect::<Vec<_>>(),
        )?);
        evidence.caption_signature_sha256 = artifact
            .captions
            .as_ref()
            .map(|lineage| {
                hash_serialized(&(&lineage.captions_sha256, &lineage.presentation_sha256))
            })
            .transpose()?;
        evidence.voice_signature_sha256 = evidence.audio_signature_sha256.clone();
        evidence.mix_signature_sha256 = evidence.audio_signature_sha256.clone();
        if let Some(report_input) = artifact
            .inputs
            .iter()
            .find(|input| input.kind == "audio-check-report")
        {
            let checked: AudioCheckReport = serde_json::from_slice(&fs::read(&report_input.path)?)?;
            if let Some(stems) = checked.stem_margin {
                evidence.voice_signature_sha256 = Some(stems.narration.sha256);
                evidence.mix_signature_sha256 = Some(stems.effects_music.sha256);
            }
        }
    }
    Ok(LoadedChild { evidence, video })
}

fn enforce_fixed_dimensions(fixed: &[String], children: &[LoadedChild]) -> Result<()> {
    let first = &children[0].evidence;
    for dimension in fixed {
        for child in &children[1..] {
            let matches = match dimension.as_str() {
                "duration" => child.evidence.duration_ms == first.duration_ms,
                "stream-facts" => {
                    child.evidence.width == first.width
                        && child.evidence.height == first.height
                        && child.evidence.fps == first.fps
                        && child.evidence.audio_streams == first.audio_streams
                }
                "voice" => required_equal(
                    &first.voice_signature_sha256,
                    &child.evidence.voice_signature_sha256,
                )?,
                "mix" => required_equal(
                    &first.mix_signature_sha256,
                    &child.evidence.mix_signature_sha256,
                )?,
                "motion" => required_equal(
                    &first.motion_signature_sha256,
                    &child.evidence.motion_signature_sha256,
                )?,
                "visual-treatment" => required_equal(
                    &first.visual_signature_sha256,
                    &child.evidence.visual_signature_sha256,
                )?,
                "captions" => required_equal(
                    &first.caption_signature_sha256,
                    &child.evidence.caption_signature_sha256,
                )?,
                _ => false,
            };
            if !matches {
                bail!(
                    "fixed dimension {dimension} differs for variant {}",
                    child.evidence.id
                );
            }
        }
    }
    if children.iter().any(|child| {
        child.evidence.width != first.width
            || child.evidence.height != first.height
            || child.evidence.fps != first.fps
    }) {
        bail!("comparison variants require identical delivery geometry and frame rate");
    }
    Ok(())
}

fn required_equal(left: &Option<String>, right: &Option<String>) -> Result<bool> {
    match (left, right) {
        (Some(left), Some(right)) => Ok(left == right),
        _ => bail!("declared fixed dimension lacks local artifact evidence"),
    }
}

fn enforce_changed_dimension(dimension: &str, children: &[LoadedChild]) -> Result<()> {
    let signatures = children
        .iter()
        .map(|child| match dimension {
            "captions" => child.evidence.caption_signature_sha256.clone(),
            "motion" => child.evidence.motion_signature_sha256.clone(),
            "voice" => child.evidence.voice_signature_sha256.clone(),
            "mix" => child.evidence.mix_signature_sha256.clone(),
            "visual-treatment" => child.evidence.visual_signature_sha256.clone(),
            "duration" => Some(child.evidence.duration_ms.to_string()),
            "stream-facts" => Some(format!(
                "{}x{}@{}:a{}",
                child.evidence.width,
                child.evidence.height,
                child.evidence.fps,
                child.evidence.audio_streams
            )),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| anyhow!("changed dimension {dimension} lacks local artifact evidence"))?;
    if signatures.iter().collect::<BTreeSet<_>>().len() < 2 {
        bail!("declared changed dimension {dimension} does not differ across variants");
    }
    Ok(())
}

fn render_comparison(
    contract: &ComparisonContract,
    base: &Path,
    children: &[LoadedChild],
    staging: &Path,
    output: &Path,
) -> Result<()> {
    let adapter = FfmpegAdapter;
    let width = children[0].evidence.width;
    let height = children[0].evidence.height;
    let fps = children[0].evidence.fps;
    let chime = contract
        .chime
        .as_ref()
        .map(|path| resolve(base, path))
        .transpose()?;
    if let Some(chime) = &chime {
        let duration_ms =
            (adapter.ffprobe_duration(chime)?.parse::<f64>()? * 1000.0).round() as u64;
        if duration_ms + contract.protected_silence_ms * 2 > contract.variant_slate_duration_ms {
            bail!("chime and protected silence do not fit variant slate");
        }
    }
    let mut segments = Vec::new();
    let opening = staging.join("opening.mp4");
    render_slate(
        &adapter,
        SlateRenderOptions {
            text: &format!(
                "{}\n\n{}",
                contract.opening.title, contract.opening.instructions
            ),
            duration_ms: contract.opening.duration_ms,
            width,
            height,
            fps,
            chime: None,
            protected_ms: 0,
            output: &opening,
        },
    )?;
    segments.push(opening);
    for child in children {
        let slate = staging.join(format!("slate-{:02}.mp4", child.evidence.order));
        render_slate(
            &adapter,
            SlateRenderOptions {
                text: &format!(
                    "VARIANT {}\n\nOne declared dimension changes: {}",
                    child.evidence.presented_label, contract.changed_dimension
                ),
                duration_ms: contract.variant_slate_duration_ms,
                width,
                height,
                fps,
                chime: chime.as_deref(),
                protected_ms: contract.protected_silence_ms,
                output: &slate,
            },
        )?;
        segments.push(slate);
        let normalized = staging.join(format!("variant-{:02}.mp4", child.evidence.order));
        normalize_child(&adapter, child, &normalized)?;
        segments.push(normalized.clone());
        if contract.replay {
            let replay_slate =
                staging.join(format!("replay-slate-{:02}.mp4", child.evidence.order));
            render_slate(
                &adapter,
                SlateRenderOptions {
                    text: &format!("REPLAY {}", child.evidence.presented_label),
                    duration_ms: contract.variant_slate_duration_ms,
                    width,
                    height,
                    fps,
                    chime: None,
                    protected_ms: 0,
                    output: &replay_slate,
                },
            )?;
            segments.push(replay_slate);
            segments.push(normalized);
        }
    }
    let concat = staging.join("segments.txt");
    let mut list = String::new();
    for segment in &segments {
        list.push_str(&format!("file '{}'\n", adapter.path_for_concat(segment)?));
    }
    fs::write(&concat, list)?;
    adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-f".to_string(),
            "concat".to_string(),
            "-safe".to_string(),
            "0".to_string(),
            "-i".to_string(),
        ],
        &[
            adapter.path_argument(&concat)?,
            "-c".to_string(),
            "copy".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            adapter.path_argument(output)?,
        ],
    )?;
    Ok(())
}

fn render_slate(adapter: &FfmpegAdapter, options: SlateRenderOptions<'_>) -> Result<()> {
    let SlateRenderOptions {
        text,
        duration_ms,
        width,
        height,
        fps,
        chime,
        protected_ms,
        output,
    } = options;
    let text_path = output.with_extension("txt");
    fs::write(&text_path, text)?;
    let seconds = duration_ms as f64 / 1000.0;
    let draw = format!(
        "drawtext=textfile='{}':fontcolor=white:fontsize={}:line_spacing=14:x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.35:boxborderw=16,format=yuv420p",
        adapter.path_argument(&text_path)?,
        (height / 18).clamp(24, 64)
    );
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!("color=c=0x172033:s={width}x{height}:r={fps}:d={seconds:.3}"),
        "-f".to_string(),
        "lavfi".to_string(),
        "-i".to_string(),
        format!("anullsrc=r=48000:cl=stereo:d={seconds:.3}"),
    ];
    if let Some(chime) = chime {
        args.extend(["-i".to_string(), adapter.path_argument(chime)?]);
        args.extend([
            "-filter_complex".to_string(),
            format!("[0:v]{draw}[v];[2:a]adelay={protected_ms}|{protected_ms},apad,atrim=duration={seconds:.3}[ch];[1:a][ch]amix=inputs=2:duration=first:normalize=0[a]"),
            "-map".to_string(), "[v]".to_string(), "-map".to_string(), "[a]".to_string(),
        ]);
    } else {
        args.extend([
            "-vf".to_string(),
            draw,
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "1:a:0".to_string(),
        ]);
    }
    args.extend(encode_args(fps, seconds, output, adapter)?);
    adapter.run_ffmpeg(&args, &[])?;
    Ok(())
}

fn normalize_child(adapter: &FfmpegAdapter, child: &LoadedChild, output: &Path) -> Result<()> {
    let seconds = child.evidence.duration_ms as f64 / 1000.0;
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
        "-i".to_string(),
        adapter.path_argument(&child.video)?,
    ];
    if child.evidence.audio_streams == 0 {
        args.extend([
            "-f".to_string(),
            "lavfi".to_string(),
            "-i".to_string(),
            format!("anullsrc=r=48000:cl=stereo:d={seconds:.3}"),
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "1:a:0".to_string(),
        ]);
    } else {
        args.extend([
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "0:a:0".to_string(),
        ]);
    }
    args.extend(encode_args(child.evidence.fps, seconds, output, adapter)?);
    adapter.run_ffmpeg(&args, &[])?;
    Ok(())
}

fn encode_args(
    fps: u32,
    seconds: f64,
    output: &Path,
    adapter: &FfmpegAdapter,
) -> Result<Vec<String>> {
    Ok(vec![
        "-t".to_string(),
        format!("{seconds:.3}"),
        "-c:v".to_string(),
        "libx264".to_string(),
        "-preset".to_string(),
        "medium".to_string(),
        "-crf".to_string(),
        "18".to_string(),
        "-pix_fmt".to_string(),
        "yuv420p".to_string(),
        "-r".to_string(),
        fps.to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "128k".to_string(),
        "-ar".to_string(),
        "48000".to_string(),
        "-ac".to_string(),
        "2".to_string(),
        adapter.path_argument(output)?,
    ])
}

struct DeliveryProbe {
    width: u32,
    height: u32,
    fps: f64,
    duration_ms: u64,
    audio_streams: usize,
}
fn probe_delivery(path: &Path) -> Result<DeliveryProbe> {
    let raw = FfmpegAdapter.ffprobe_json(path)?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let streams = value["streams"]
        .as_array()
        .ok_or_else(|| anyhow!("ffprobe omitted streams"))?;
    let video = streams
        .iter()
        .find(|stream| stream["codec_type"] == "video")
        .ok_or_else(|| anyhow!("comparison output has no video"))?;
    if video["codec_name"] != "h264" || video["pix_fmt"] != "yuv420p" {
        bail!("comparison output is not H.264 yuv420p");
    }
    let fps = parse_fraction(video["r_frame_rate"].as_str().unwrap_or("0/1"))?;
    let duration_ms = (value["format"]["duration"]
        .as_str()
        .ok_or_else(|| anyhow!("ffprobe omitted duration"))?
        .parse::<f64>()?
        * 1000.0)
        .round() as u64;
    Ok(DeliveryProbe {
        width: video["width"].as_u64().unwrap_or_default() as u32,
        height: video["height"].as_u64().unwrap_or_default() as u32,
        fps,
        duration_ms,
        audio_streams: streams
            .iter()
            .filter(|stream| stream["codec_type"] == "audio")
            .count(),
    })
}

fn validate_receipt(receipt: &ComparisonReceipt) -> Result<()> {
    if receipt.schema != COMPARISON_RECEIPT_SCHEMA
        || receipt.source_artifact_schema != COMPARISON_ARTIFACT_SCHEMA
        || !is_sha(&receipt.source_artifact_sha256)
        || !is_sha(&receipt.output_sha256)
        || receipt.children < 2
        || receipt.child_receipt_sha256.len() != receipt.children
        || receipt
            .child_receipt_sha256
            .iter()
            .any(|hash| !is_sha(hash))
        || receipt.width == 0
        || receipt.height == 0
        || receipt.fps == 0
        || receipt.duration_ms == 0
        || receipt.output_bytes == 0
        || receipt.audio_streams != 1
        || !DIMENSIONS.contains(&receipt.changed_dimension.as_str())
        || receipt.fixed_dimensions.iter().any(|dimension| {
            !DIMENSIONS.contains(&dimension.as_str()) || dimension == &receipt.changed_dimension
        })
        || receipt
            .fixed_dimensions
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != receipt.fixed_dimensions.len()
        || receipt.inclusion_order_is_approval
        || !receipt.verified
    {
        bail!("comparison receipt is inconsistent");
    }
    if receipt.blinded != receipt.blind_seed_sha256.is_some() {
        bail!("comparison receipt blind evidence is inconsistent");
    }
    if receipt
        .blind_seed_sha256
        .as_ref()
        .is_some_and(|hash| !is_sha(hash))
    {
        bail!("comparison receipt blind seed hash is invalid");
    }
    Ok(())
}

fn publish_group<const N: usize>(paths: [(&Path, &Path); N]) -> Result<()> {
    let mut published = Vec::new();
    for (source, target) in paths {
        if let Err(error) = fs::rename(source, target) {
            for path in published {
                let _ = fs::remove_file(path);
            }
            return Err(error).with_context(|| {
                format!("failed to publish comparison file {}", target.display())
            });
        }
        published.push(target.to_path_buf());
    }
    Ok(())
}

fn resolve(base: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        path.canonicalize()
    } else {
        base.join(path).canonicalize()
    }
    .with_context(|| format!("failed to resolve comparison input {}", path.display()))
}
fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 500 {
        bail!("{name} must contain 1..=500 characters");
    }
    Ok(())
}
fn hash_serialized(value: &impl Serialize) -> Result<String> {
    Ok(hash_bytes(&serde_json::to_vec(value)?))
}
fn hash_text(value: &str) -> String {
    hash_bytes(value.as_bytes())
}
fn hash_bytes(value: &[u8]) -> String {
    let mut output = String::with_capacity(64);
    for byte in Sha256::digest(value) {
        write!(&mut output, "{byte:02x}").expect("writing to string cannot fail");
    }
    output
}
fn is_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
fn parse_fraction(value: &str) -> Result<f64> {
    let (n, d) = value
        .split_once('/')
        .ok_or_else(|| anyhow!("invalid frame rate"))?;
    let d = d.parse::<f64>()?;
    if d == 0.0 {
        bail!("invalid frame rate");
    }
    Ok(n.parse::<f64>()? / d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blind_labels_are_seeded_and_do_not_follow_declared_ids() {
        let contract = ComparisonContract {
            schema: COMPARISON_SCHEMA.to_string(),
            id: "test".to_string(),
            opening: OpeningSlate {
                title: "Review".to_string(),
                instructions: "Compare one dimension.".to_string(),
                duration_ms: 1000,
            },
            variants: ["one", "two", "three"]
                .into_iter()
                .map(|id| ComparisonVariant {
                    id: id.to_string(),
                    label: None,
                    video: "v".into(),
                    receipt: "r".into(),
                    artifact: None,
                })
                .collect(),
            label_mode: "blinded".to_string(),
            blind_seed: Some("seed-42".to_string()),
            changed_dimension: "captions".to_string(),
            fixed_dimensions: vec!["duration".to_string()],
            variant_slate_duration_ms: 1000,
            protected_silence_ms: 100,
            chime: None,
            replay: false,
        };
        assert_eq!(
            presented_labels(&contract).unwrap(),
            presented_labels(&contract).unwrap()
        );
        assert_eq!(
            presented_labels(&contract)
                .unwrap()
                .into_iter()
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["A".to_string(), "B".to_string(), "C".to_string()])
        );
    }
}
