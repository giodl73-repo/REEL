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
    caption_presentation::PixelRect,
    production,
};

pub const COMPARISON_SCHEMA: &str = "reel.comparison.v0.1";
pub const COMPARISON_ARTIFACT_SCHEMA: &str = "reel.comparison-artifacts.v0.1";
pub const COMPARISON_RECEIPT_SCHEMA: &str = "reel.comparison-receipt.v0.1";
pub const COMPARISON_LAYOUT_SCHEMA: &str = "reel.comparison-layout.v0.1";
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
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
    pub slate_layout_policy: SlateLayoutPolicy,
    pub slate_layouts: Vec<SlateLayoutEvidence>,
    pub maximum_slate_occupied_screen_percent: f64,
    pub children: Vec<ComparisonChildEvidence>,
    pub tool_version: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlateLayoutPolicy {
    pub strategy: String,
    pub font_family: String,
    pub glyph_width_model: String,
    pub safe_margin_percent: u32,
    pub opening_maximum_lines: usize,
    pub variant_maximum_lines: usize,
    pub replay_maximum_lines: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SlateLayoutEvidence {
    pub role: String,
    pub variant_order: Option<usize>,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub font_size_px: u32,
    pub minimum_font_size_px: u32,
    pub line_spacing_px: u32,
    pub maximum_lines: usize,
    pub lines: Vec<String>,
    pub safe_area: PixelRect,
    pub bounding_box: PixelRect,
    pub inside_safe_area: bool,
    pub occupied_screen_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonLayoutImage {
    pub role: String,
    pub variant_order: Option<usize>,
    pub timestamp_ms: u64,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonLayoutPacket {
    pub schema: String,
    pub artifact_schema: String,
    pub artifact: String,
    pub artifact_sha256: String,
    pub receipt: String,
    pub receipt_sha256: String,
    pub video: String,
    pub video_sha256: String,
    pub width: u32,
    pub height: u32,
    pub measurement_scope: String,
    pub slate_layout_policy: SlateLayoutPolicy,
    pub slate_layouts: Vec<SlateLayoutEvidence>,
    pub maximum_slate_occupied_screen_percent: f64,
    pub images: Vec<ComparisonLayoutImage>,
    pub private: bool,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ComparisonLayoutCheckReport {
    pub schema: String,
    pub packet_sha256: String,
    pub artifact_sha256: String,
    pub receipt_sha256: String,
    pub video_sha256: String,
    pub images: usize,
    pub passed: bool,
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
    layout: &'a SlateLayoutEvidence,
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
    let (slate_layout_policy, slate_layouts) = build_slate_layouts(&contract, &children)?;
    let maximum_slate_occupied_screen_percent = slate_layouts
        .iter()
        .map(|layout| layout.occupied_screen_percent)
        .fold(0.0_f64, f64::max);

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
    render_comparison(
        &contract,
        base,
        &children,
        &slate_layouts,
        staging.path(),
        &staged_video,
    )?;
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
        slate_layout_policy,
        slate_layouts,
        maximum_slate_occupied_screen_percent,
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

pub fn write_layout_packet(
    artifact_path: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<ComparisonLayoutPacket> {
    let artifact_path = artifact_path.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve comparison artifact {}",
            artifact_path.as_ref().display()
        )
    })?;
    let verified = load_verified_artifact(&artifact_path)?;
    let output_dir = output_dir.as_ref();
    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        bail!(
            "comparison layout output directory must be absent or empty: {}",
            output_dir.display()
        );
    }
    let parent = output_dir
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new()
        .prefix(".reel-comparison-layout-")
        .tempdir_in(parent)?;
    let adapter = FfmpegAdapter;
    let mut images = Vec::new();
    for layout in verified
        .artifact
        .slate_layouts
        .iter()
        .filter(|layout| matches!(layout.role.as_str(), "opening" | "variant"))
    {
        let file = match layout.variant_order {
            Some(order) => format!("variant-{order:02}.png"),
            None => "opening.png".to_string(),
        };
        let path = staging.path().join(&file);
        let timestamp_ms = layout.start_ms + layout.duration_ms / 2;
        extract_comparison_frame(&adapter, &verified.video, timestamp_ms, &path)?;
        images.push(ComparisonLayoutImage {
            role: layout.role.clone(),
            variant_order: layout.variant_order,
            timestamp_ms,
            file,
            sha256: production::sha256_path(&path)?,
            bytes: fs::metadata(&path)?.len(),
        });
    }
    let packet = ComparisonLayoutPacket {
        schema: COMPARISON_LAYOUT_SCHEMA.to_string(),
        artifact_schema: verified.artifact.schema.clone(),
        artifact: artifact_path.display().to_string(),
        artifact_sha256: production::sha256_path(&artifact_path)?,
        receipt: verified.receipt_path.display().to_string(),
        receipt_sha256: production::sha256_path(&verified.receipt_path)?,
        video: verified.video.display().to_string(),
        video_sha256: verified.artifact.output_sha256.clone(),
        width: verified.artifact.width,
        height: verified.artifact.height,
        measurement_scope: "deterministic conservative glyph bounds and frames extracted from the verified local comparison video; private evidence, not OCR, device, translation, or human-legibility approval".to_string(),
        slate_layout_policy: verified.artifact.slate_layout_policy.clone(),
        slate_layouts: verified.artifact.slate_layouts.clone(),
        maximum_slate_occupied_screen_percent: verified
            .artifact
            .maximum_slate_occupied_screen_percent,
        images,
        private: true,
        verified: true,
    };
    fs::write(
        staging.path().join("layout.json"),
        format!("{}\n", serde_json::to_string_pretty(&packet)?),
    )?;
    if output_dir.exists() {
        fs::remove_dir(output_dir).with_context(|| {
            format!(
                "failed to replace empty comparison layout directory {}",
                output_dir.display()
            )
        })?;
    }
    fs::rename(staging.path(), output_dir).with_context(|| {
        format!(
            "failed to publish comparison layout packet {}",
            output_dir.display()
        )
    })?;
    Ok(packet)
}

pub fn check_layout_packet(output_dir: impl AsRef<Path>) -> Result<ComparisonLayoutCheckReport> {
    let output_dir = output_dir.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve comparison layout packet {}",
            output_dir.as_ref().display()
        )
    })?;
    let packet_path = output_dir.join("layout.json");
    let packet_bytes = fs::read(&packet_path)?;
    let packet: ComparisonLayoutPacket = serde_json::from_slice(&packet_bytes)
        .context("comparison layout packet is not valid strict JSON")?;
    if packet.schema != COMPARISON_LAYOUT_SCHEMA || !packet.private || !packet.verified {
        bail!("comparison layout packet is inconsistent");
    }
    let artifact_path = Path::new(&packet.artifact).canonicalize()?;
    let verified = load_verified_artifact(&artifact_path)?;
    if packet.artifact_schema != verified.artifact.schema
        || packet.artifact_sha256 != production::sha256_path(&artifact_path)?
        || Path::new(&packet.receipt).canonicalize()? != verified.receipt_path
        || packet.receipt_sha256 != production::sha256_path(&verified.receipt_path)?
        || Path::new(&packet.video).canonicalize()? != verified.video
        || packet.video_sha256 != verified.artifact.output_sha256
        || packet.width != verified.artifact.width
        || packet.height != verified.artifact.height
        || packet.slate_layout_policy != verified.artifact.slate_layout_policy
        || packet.slate_layouts != verified.artifact.slate_layouts
        || (packet.maximum_slate_occupied_screen_percent
            - verified.artifact.maximum_slate_occupied_screen_percent)
            .abs()
            > 1e-9
    {
        bail!("comparison layout packet does not match its verified artifact lineage");
    }
    let expected_images = packet
        .slate_layouts
        .iter()
        .filter(|layout| matches!(layout.role.as_str(), "opening" | "variant"))
        .count();
    if packet.images.len() != expected_images {
        bail!("comparison layout packet image coverage is inconsistent");
    }
    let expected_keys = packet
        .slate_layouts
        .iter()
        .filter(|layout| matches!(layout.role.as_str(), "opening" | "variant"))
        .map(|layout| (layout.role.as_str(), layout.variant_order))
        .collect::<BTreeSet<_>>();
    let image_keys = packet
        .images
        .iter()
        .map(|image| (image.role.as_str(), image.variant_order))
        .collect::<BTreeSet<_>>();
    let image_files = packet
        .images
        .iter()
        .map(|image| image.file.as_str())
        .collect::<BTreeSet<_>>();
    if image_keys != expected_keys || image_files.len() != packet.images.len() {
        bail!("comparison layout packet image identities are inconsistent");
    }
    for image in &packet.images {
        if Path::new(&image.file)
            .file_name()
            .and_then(|name| name.to_str())
            != Some(image.file.as_str())
            || !is_sha(&image.sha256)
        {
            bail!("comparison layout image record is unsafe or inconsistent");
        }
        let path = output_dir.join(&image.file);
        if production::sha256_path(&path)? != image.sha256
            || fs::metadata(&path)?.len() != image.bytes
        {
            bail!("comparison layout image {} was modified", image.file);
        }
        let expected = packet.slate_layouts.iter().find(|layout| {
            layout.role == image.role && layout.variant_order == image.variant_order
        });
        if expected
            .is_none_or(|layout| image.timestamp_ms != layout.start_ms + layout.duration_ms / 2)
        {
            bail!("comparison layout image timestamp is inconsistent");
        }
    }
    Ok(ComparisonLayoutCheckReport {
        schema: "reel.comparison-layout-check.v0.1".to_string(),
        packet_sha256: hash_bytes(&packet_bytes),
        artifact_sha256: packet.artifact_sha256,
        receipt_sha256: packet.receipt_sha256,
        video_sha256: packet.video_sha256,
        images: packet.images.len(),
        passed: true,
    })
}

struct VerifiedComparisonArtifact {
    artifact: ComparisonArtifactReport,
    receipt_path: PathBuf,
    video: PathBuf,
}

fn load_verified_artifact(artifact_path: &Path) -> Result<VerifiedComparisonArtifact> {
    let artifact_bytes = fs::read(artifact_path)?;
    let artifact: ComparisonArtifactReport = serde_json::from_slice(&artifact_bytes)
        .context("comparison artifact is not valid strict JSON")?;
    validate_artifact_layout(&artifact)?;
    let receipt_path = comparison_receipt_path(artifact_path)?;
    let receipt: ComparisonReceipt = serde_json::from_slice(&fs::read(&receipt_path)?)
        .context("comparison receipt is not valid strict JSON")?;
    validate_receipt(&receipt)?;
    let artifact_sha256 = hash_bytes(&artifact_bytes);
    if receipt.source_artifact_sha256 != artifact_sha256
        || receipt.source_artifact_schema != artifact.schema
    {
        bail!("comparison artifact does not match its receipt");
    }
    let video = Path::new(&artifact.output).canonicalize()?;
    check_receipt(&receipt_path, &video)?;
    if artifact.output_sha256 != receipt.output_sha256
        || artifact.output_bytes != receipt.output_bytes
        || artifact.width != receipt.width
        || artifact.height != receipt.height
        || artifact.fps != receipt.fps
        || artifact.duration_ms.abs_diff(receipt.duration_ms)
            > (1000.0 / f64::from(receipt.fps)).ceil() as u64
    {
        bail!("comparison artifact delivery facts do not match its receipt");
    }
    Ok(VerifiedComparisonArtifact {
        artifact,
        receipt_path,
        video,
    })
}

fn comparison_receipt_path(artifact_path: &Path) -> Result<PathBuf> {
    let file = artifact_path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("comparison artifact filename is not UTF-8"))?;
    let stem = file
        .strip_suffix(".comparison.artifacts.json")
        .ok_or_else(|| anyhow!("comparison artifact must end in .comparison.artifacts.json"))?;
    Ok(artifact_path.with_file_name(format!("{stem}.comparison.receipt.json")))
}

fn validate_artifact_layout(artifact: &ComparisonArtifactReport) -> Result<()> {
    if artifact.schema != COMPARISON_ARTIFACT_SCHEMA
        || !artifact.verified
        || artifact.children.len() < 2
        || artifact.width == 0
        || artifact.height == 0
        || !is_sha(&artifact.output_sha256)
        || !is_sha(&artifact.contract_sha256)
        || artifact.slate_layouts.is_empty()
        || artifact.slate_layouts[0].role != "opening"
    {
        bail!("comparison artifact layout evidence is inconsistent");
    }
    let expected_layouts = 1
        + artifact.children.len()
        + if artifact.children.iter().any(|child| child.replayed) {
            artifact.children.len()
        } else {
            0
        };
    if artifact.slate_layouts.len() != expected_layouts {
        bail!("comparison artifact slate coverage is inconsistent");
    }
    for layout in &artifact.slate_layouts {
        if !matches!(layout.role.as_str(), "opening" | "variant" | "replay")
            || layout.lines.is_empty()
            || layout.lines.len() > layout.maximum_lines
            || layout.font_size_px < layout.minimum_font_size_px
            || !layout.inside_safe_area
            || !rect_inside(&layout.safe_area, artifact.width, artifact.height)
            || !rect_inside_rect(&layout.bounding_box, &layout.safe_area)
            || layout.duration_ms == 0
        {
            bail!("comparison artifact contains an invalid slate layout");
        }
    }
    let maximum = artifact
        .slate_layouts
        .iter()
        .map(|layout| layout.occupied_screen_percent)
        .fold(0.0_f64, f64::max);
    if (maximum - artifact.maximum_slate_occupied_screen_percent).abs() > 1e-9 {
        bail!("comparison artifact maximum slate occupancy is inconsistent");
    }
    Ok(())
}

fn rect_inside(rect: &PixelRect, width: u32, height: u32) -> bool {
    u64::from(rect.x) + u64::from(rect.width) <= u64::from(width)
        && u64::from(rect.y) + u64::from(rect.height) <= u64::from(height)
}

fn rect_inside_rect(inner: &PixelRect, outer: &PixelRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && u64::from(inner.x) + u64::from(inner.width)
            <= u64::from(outer.x) + u64::from(outer.width)
        && u64::from(inner.y) + u64::from(inner.height)
            <= u64::from(outer.y) + u64::from(outer.height)
}

fn extract_comparison_frame(
    adapter: &FfmpegAdapter,
    video: &Path,
    timestamp_ms: u64,
    output: &Path,
) -> Result<()> {
    adapter.run_ffmpeg(
        &[
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "error".to_string(),
            "-y".to_string(),
            "-ss".to_string(),
            format!("{:.3}", timestamp_ms as f64 / 1000.0),
            "-i".to_string(),
        ],
        &[
            adapter.path_argument(video)?,
            "-frames:v".to_string(),
            "1".to_string(),
            adapter.path_argument(output)?,
        ],
    )?;
    Ok(())
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

fn build_slate_layouts(
    contract: &ComparisonContract,
    children: &[LoadedChild],
) -> Result<(SlateLayoutPolicy, Vec<SlateLayoutEvidence>)> {
    let width = children[0].evidence.width;
    let height = children[0].evidence.height;
    let policy = SlateLayoutPolicy {
        strategy: "word-wrap-then-one-pixel-scale-v1".to_string(),
        font_family: "ffmpeg-default-sans".to_string(),
        glyph_width_model: "conservative-one-em-per-unicode-scalar-v1".to_string(),
        safe_margin_percent: 8,
        opening_maximum_lines: 10,
        variant_maximum_lines: 6,
        replay_maximum_lines: 4,
    };
    let mut layouts = Vec::new();
    let mut cursor_ms = 0;
    layouts.push(fit_slate(
        "opening",
        None,
        &format!(
            "{}\n\n{}",
            contract.opening.title, contract.opening.instructions
        ),
        cursor_ms,
        contract.opening.duration_ms,
        (width, height),
        policy.opening_maximum_lines,
    )?);
    cursor_ms += contract.opening.duration_ms;
    for child in children {
        layouts.push(fit_slate(
            "variant",
            Some(child.evidence.order),
            &format!(
                "VARIANT {}\n\nOne declared dimension changes: {}",
                child.evidence.presented_label, contract.changed_dimension
            ),
            cursor_ms,
            contract.variant_slate_duration_ms,
            (width, height),
            policy.variant_maximum_lines,
        )?);
        cursor_ms += contract.variant_slate_duration_ms + child.evidence.duration_ms;
        if contract.replay {
            layouts.push(fit_slate(
                "replay",
                Some(child.evidence.order),
                &format!("REPLAY {}", child.evidence.presented_label),
                cursor_ms,
                contract.variant_slate_duration_ms,
                (width, height),
                policy.replay_maximum_lines,
            )?);
            cursor_ms += contract.variant_slate_duration_ms + child.evidence.duration_ms;
        }
    }
    Ok((policy, layouts))
}

fn fit_slate(
    role: &str,
    variant_order: Option<usize>,
    text: &str,
    start_ms: u64,
    duration_ms: u64,
    geometry: (u32, u32),
    maximum_lines: usize,
) -> Result<SlateLayoutEvidence> {
    let (width, height) = geometry;
    let margin_x = width.saturating_mul(8).div_ceil(100);
    let margin_y = height.saturating_mul(8).div_ceil(100);
    let safe_area = PixelRect {
        x: margin_x,
        y: margin_y,
        width: width.saturating_sub(margin_x.saturating_mul(2)),
        height: height.saturating_sub(margin_y.saturating_mul(2)),
    };
    let initial_font = (height / 18).clamp(24, 64);
    let minimum_font = (height / 36).clamp(18, 32).min(initial_font);
    for font_size_px in (minimum_font..=initial_font).rev() {
        let padding = (font_size_px / 2).max(8);
        let available_width = safe_area.width.saturating_sub(padding.saturating_mul(2));
        let capacity = (available_width / font_size_px).max(1) as usize;
        let lines = wrap_slate_text(text, capacity);
        if lines.len() > maximum_lines {
            continue;
        }
        let line_spacing_px = (font_size_px / 3).max(6);
        let longest = lines
            .iter()
            .map(|line| line.chars().count() as u32)
            .max()
            .unwrap_or_default();
        let text_width = longest.saturating_mul(font_size_px);
        let text_height = (lines.len() as u32)
            .saturating_mul(font_size_px)
            .saturating_add((lines.len().saturating_sub(1) as u32).saturating_mul(line_spacing_px));
        let box_width = text_width.saturating_add(padding.saturating_mul(2));
        let box_height = text_height.saturating_add(padding.saturating_mul(2));
        if box_width > safe_area.width || box_height > safe_area.height {
            continue;
        }
        let bounding_box = PixelRect {
            x: safe_area.x + (safe_area.width - box_width) / 2,
            y: safe_area.y + (safe_area.height - box_height) / 2,
            width: box_width,
            height: box_height,
        };
        let occupied_screen_percent = f64::from(box_width) * f64::from(box_height) * 100.0
            / (f64::from(width) * f64::from(height));
        return Ok(SlateLayoutEvidence {
            role: role.to_string(),
            variant_order,
            start_ms,
            duration_ms,
            font_size_px,
            minimum_font_size_px: minimum_font,
            line_spacing_px,
            maximum_lines,
            lines,
            safe_area,
            bounding_box,
            inside_safe_area: true,
            occupied_screen_percent,
        });
    }
    bail!(
        "comparison {role} slate text cannot fit {}x{} safe area at minimum font size {minimum_font}px and {maximum_lines}-line limit",
        safe_area.width,
        safe_area.height
    )
}

fn wrap_slate_text(text: &str, capacity: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        if paragraph.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in paragraph.split_whitespace() {
            let chunks = split_word(word, capacity);
            for chunk in chunks {
                let projected = current.chars().count()
                    + usize::from(!current.is_empty())
                    + chunk.chars().count();
                if projected <= capacity {
                    if !current.is_empty() {
                        current.push(' ');
                    }
                    current.push_str(&chunk);
                } else {
                    if !current.is_empty() {
                        lines.push(std::mem::take(&mut current));
                    }
                    current = chunk;
                }
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn split_word(word: &str, capacity: usize) -> Vec<String> {
    let chars = word.chars().collect::<Vec<_>>();
    if chars.len() <= capacity {
        return vec![word.to_string()];
    }
    chars
        .chunks(capacity)
        .map(|chunk| chunk.iter().collect())
        .collect()
}

fn render_comparison(
    contract: &ComparisonContract,
    base: &Path,
    children: &[LoadedChild],
    layouts: &[SlateLayoutEvidence],
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
    let mut layouts = layouts.iter();
    let opening = staging.join("opening.mp4");
    render_slate(
        &adapter,
        SlateRenderOptions {
            layout: layouts.next().expect("opening layout was preflighted"),
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
                layout: layouts.next().expect("variant layout was preflighted"),
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
                    layout: layouts.next().expect("replay layout was preflighted"),
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
        layout,
        width,
        height,
        fps,
        chime,
        protected_ms,
        output,
    } = options;
    let text_path = output.with_extension("txt");
    fs::write(&text_path, layout.lines.join("\n"))?;
    let seconds = layout.duration_ms as f64 / 1000.0;
    let padding = (layout.font_size_px / 2).max(8);
    let draw = format!(
        "drawtext=textfile='{}':fontcolor=white:fontsize={}:line_spacing={}:x=(w-text_w)/2:y=(h-text_h)/2:box=1:boxcolor=black@0.35:boxborderw={},format=yuv420p",
        adapter.path_argument(&text_path)?,
        layout.font_size_px,
        layout.line_spacing_px,
        padding
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

    #[test]
    fn long_slate_copy_wraps_deterministically_inside_safe_bounds() {
        let text = "A deliberately long comparison title that used to cross both horizontal edges\n\nCompare only the declared caption presentation treatment. Keep voice, timing, motion, mix, and visual treatment fixed; inclusion does not imply preference or approval.";
        let first = fit_slate("opening", None, text, 0, 2_000, (1280, 720), 10).unwrap();
        let second = fit_slate("opening", None, text, 0, 2_000, (1280, 720), 10).unwrap();
        assert_eq!(first, second);
        assert!(first.lines.len() > 2);
        assert!(rect_inside_rect(&first.bounding_box, &first.safe_area));
        assert!(first.font_size_px >= first.minimum_font_size_px);
    }

    #[test]
    fn infeasible_slate_copy_is_rejected_at_the_policy_floor() {
        let error = fit_slate(
            "variant",
            Some(1),
            &"x".repeat(500),
            0,
            1_000,
            (1280, 720),
            6,
        )
        .unwrap_err();
        assert!(error.to_string().contains("cannot fit"));
        assert!(error.to_string().contains("minimum font size"));
    }
}
