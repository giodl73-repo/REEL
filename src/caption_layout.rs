use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::{
    adapters::{
        ffmpeg::FfmpegAdapter,
        still_animatic::{self, AnimaticRenderReport},
    },
    caption_presentation::{CaptionLineage, PixelRect},
    production,
    series::parse_srt,
};

pub const CAPTION_LAYOUT_SCHEMA: &str = "reel.caption-layout.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptionLayoutCue {
    pub srt_index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub caption_region: PixelRect,
    pub speaker_badge_region: Option<PixelRect>,
    pub caption_inside_frame: bool,
    pub speaker_badge_inside_frame: Option<bool>,
    pub caption_badge_intersection: bool,
    pub occupied_screen_percent: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptionLayoutImage {
    pub role: String,
    pub srt_index: Option<usize>,
    pub timestamp_ms: Option<u64>,
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CaptionLayoutReport {
    pub schema: String,
    pub artifact_schema: String,
    pub artifact_sha256: String,
    pub video_sha256: String,
    pub caption_lineage_schema: String,
    pub caption_presentation_sha256: String,
    pub width: u32,
    pub height: u32,
    pub caption_font_size_px: u32,
    pub speaker_badge_font_size_px: u32,
    pub caption_margin_x_px: u32,
    pub caption_margin_bottom_px: u32,
    pub speaker_badge_margin_x_px: u32,
    pub speaker_badge_margin_top_px: u32,
    pub caption_text_color: String,
    pub caption_outline_px: u32,
    pub speaker_badge_text_color: String,
    pub speaker_badge_background: String,
    pub speaker_badge_padding_px: u32,
    pub representative_strategy: String,
    pub measurement_scope: String,
    pub cues: Vec<CaptionLayoutCue>,
    pub maximum_occupied_screen_percent: f64,
    pub images: Vec<CaptionLayoutImage>,
    pub passed: bool,
}

pub fn write_packet(
    artifact_manifest: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<CaptionLayoutReport> {
    let artifact_manifest = artifact_manifest.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve artifact report {}",
            artifact_manifest.as_ref().display()
        )
    })?;
    let verified = still_animatic::check_animatic(&artifact_manifest)?;
    let artifact_bytes = fs::read(&artifact_manifest)?;
    let artifact: AnimaticRenderReport =
        serde_json::from_slice(&artifact_bytes).context("artifact report is not valid JSON")?;
    let lineage = artifact
        .captions
        .as_ref()
        .ok_or_else(|| anyhow!("caption layout evidence requires caption presentation lineage"))?;
    let captions = artifact
        .inputs
        .iter()
        .find(|input| input.kind == "captions")
        .ok_or_else(|| anyhow!("artifact report has no captions input"))?;
    let entries = parse_srt(&fs::read_to_string(&captions.path)?)?;
    if entries.is_empty() {
        bail!("caption layout evidence requires at least one caption cue");
    }

    let cues = layout_cues(lineage, &entries, artifact.width, artifact.height);
    if cues.iter().any(|cue| {
        !cue.caption_inside_frame
            || cue.speaker_badge_inside_frame == Some(false)
            || cue.caption_badge_intersection
    }) {
        bail!("caption layout regions fail frame or overlap safety checks");
    }

    let output_dir = output_dir.as_ref();
    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        bail!(
            "caption layout output directory must be absent or empty: {}",
            output_dir.display()
        );
    }
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = Builder::new()
        .prefix(".reel-caption-layout-")
        .tempdir_in(parent)
        .context("failed to create caption layout staging directory")?;
    let selected = representative_indexes(entries.len());
    let adapter = FfmpegAdapter;
    let video = Path::new(&artifact.output).canonicalize()?;
    let mut images = Vec::new();
    let roles = ["first", "middle", "last"];
    let mut frame_paths = Vec::new();
    for (position, entry_index) in selected.iter().enumerate() {
        let entry = &entries[*entry_index];
        let role = roles[position.min(roles.len() - 1)];
        let file = format!("{role}.png");
        let path = staging.path().join(&file);
        let timestamp_ms = entry.start_ms + (entry.end_ms - entry.start_ms) / 2;
        extract_frame(&adapter, &video, timestamp_ms, &path)?;
        images.push(image_record(
            role,
            Some(entry.index),
            Some(timestamp_ms),
            &file,
            &path,
        )?);
        frame_paths.push(path);
    }
    let contact_file = "contact-sheet.png";
    let contact_path = staging.path().join(contact_file);
    render_contact_sheet(&adapter, &frame_paths, &contact_path)?;
    images.push(image_record(
        "contact-sheet",
        None,
        None,
        contact_file,
        &contact_path,
    )?);

    let maximum_occupied_screen_percent = cues
        .iter()
        .map(|cue| cue.occupied_screen_percent)
        .fold(0.0_f64, f64::max);
    let report = CaptionLayoutReport {
        schema: CAPTION_LAYOUT_SCHEMA.to_string(),
        artifact_schema: artifact.schema,
        artifact_sha256: production::sha256_path(&artifact_manifest)?,
        video_sha256: verified.output_sha256,
        caption_lineage_schema: lineage.schema.clone(),
        caption_presentation_sha256: lineage.presentation_sha256.clone(),
        width: artifact.width,
        height: artifact.height,
        caption_font_size_px: lineage.style.caption_font_size,
        speaker_badge_font_size_px: lineage.style.badge_font_size_px,
        caption_margin_x_px: lineage.style.caption_region.x,
        caption_margin_bottom_px: artifact.height.saturating_sub(
            lineage
                .style
                .caption_region
                .y
                .saturating_add(lineage.style.caption_region.height),
        ),
        speaker_badge_margin_x_px: lineage.style.badge_region.x,
        speaker_badge_margin_top_px: lineage.style.badge_region.y,
        caption_text_color: lineage.style.caption_text_color.clone(),
        caption_outline_px: lineage.style.caption_outline_px,
        speaker_badge_text_color: lineage.style.badge_text_color.clone(),
        speaker_badge_background: lineage.style.badge_background.clone(),
        speaker_badge_padding_px: lineage.style.badge_padding_px,
        representative_strategy: "caption-cue-first-middle-last-midpoint-v1".to_string(),
        measurement_scope: "declared caption and speaker-badge presentation regions; no OCR, translation, glyph-bound, device, or human-legibility claim".to_string(),
        cues,
        maximum_occupied_screen_percent,
        images,
        passed: true,
    };
    fs::write(
        staging.path().join("layout.json"),
        format!("{}\n", serde_json::to_string_pretty(&report)?),
    )?;
    if output_dir.exists() {
        fs::remove_dir(output_dir).with_context(|| {
            format!(
                "failed to replace empty output directory {}",
                output_dir.display()
            )
        })?;
    }
    fs::rename(staging.path(), output_dir).with_context(|| {
        format!(
            "failed to publish caption layout packet {}",
            output_dir.display()
        )
    })?;
    Ok(report)
}

fn layout_cues(
    lineage: &CaptionLineage,
    entries: &[crate::series::SrtEntry],
    width: u32,
    height: u32,
) -> Vec<CaptionLayoutCue> {
    let badge_indexes = lineage
        .label_events
        .iter()
        .map(|event| event.srt_index)
        .collect::<BTreeSet<_>>();
    let frame_area = f64::from(width) * f64::from(height);
    entries
        .iter()
        .map(|entry| {
            let caption = lineage.style.caption_region.clone();
            let badge = badge_indexes
                .contains(&entry.index)
                .then(|| lineage.style.badge_region.clone());
            let intersection = badge
                .as_ref()
                .is_some_and(|badge| intersects(&caption, badge));
            let occupied = rect_area(&caption) + badge.as_ref().map(rect_area).unwrap_or_default()
                - if intersection {
                    intersection_area(&caption, badge.as_ref().expect("badge exists"))
                } else {
                    0.0
                };
            CaptionLayoutCue {
                srt_index: entry.index,
                start_ms: entry.start_ms,
                end_ms: entry.end_ms,
                caption_region: caption.clone(),
                speaker_badge_region: badge.clone(),
                caption_inside_frame: inside_frame(&caption, width, height),
                speaker_badge_inside_frame: badge
                    .as_ref()
                    .map(|rect| inside_frame(rect, width, height)),
                caption_badge_intersection: intersection,
                occupied_screen_percent: occupied * 100.0 / frame_area,
            }
        })
        .collect()
}

fn representative_indexes(length: usize) -> Vec<usize> {
    vec![0, (length - 1) / 2, length - 1]
}

fn extract_frame(
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
            "-vf".to_string(),
            "scale=480:270:force_original_aspect_ratio=decrease,pad=480:270:(ow-iw)/2:(oh-ih)/2:color=black".to_string(),
            adapter.path_argument(output)?,
        ],
    )?;
    Ok(())
}

fn render_contact_sheet(adapter: &FfmpegAdapter, inputs: &[PathBuf], output: &Path) -> Result<()> {
    let mut args = vec![
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "error".to_string(),
        "-y".to_string(),
    ];
    for input in inputs {
        args.push("-i".to_string());
        args.push(adapter.path_argument(input)?);
    }
    args.extend([
        "-filter_complex".to_string(),
        format!("hstack=inputs={}", inputs.len()),
        "-frames:v".to_string(),
        "1".to_string(),
        adapter.path_argument(output)?,
    ]);
    adapter.run_ffmpeg(&args, &[])?;
    Ok(())
}

fn image_record(
    role: &str,
    srt_index: Option<usize>,
    timestamp_ms: Option<u64>,
    file: &str,
    path: &Path,
) -> Result<CaptionLayoutImage> {
    Ok(CaptionLayoutImage {
        role: role.to_string(),
        srt_index,
        timestamp_ms,
        file: file.to_string(),
        sha256: production::sha256_path(path)?,
        bytes: fs::metadata(path)?.len(),
    })
}

fn inside_frame(rect: &PixelRect, width: u32, height: u32) -> bool {
    u64::from(rect.x) + u64::from(rect.width) <= u64::from(width)
        && u64::from(rect.y) + u64::from(rect.height) <= u64::from(height)
}

fn rect_area(rect: &PixelRect) -> f64 {
    f64::from(rect.width) * f64::from(rect.height)
}

fn intersects(left: &PixelRect, right: &PixelRect) -> bool {
    left.x < right.x.saturating_add(right.width)
        && left.x.saturating_add(left.width) > right.x
        && left.y < right.y.saturating_add(right.height)
        && left.y.saturating_add(left.height) > right.y
}

fn intersection_area(left: &PixelRect, right: &PixelRect) -> f64 {
    let width = left
        .x
        .saturating_add(left.width)
        .min(right.x.saturating_add(right.width))
        .saturating_sub(left.x.max(right.x));
    let height = left
        .y
        .saturating_add(left.height)
        .min(right.y.saturating_add(right.height))
        .saturating_sub(left.y.max(right.y));
    f64::from(width) * f64::from(height)
}

#[cfg(test)]
mod tests {
    use super::representative_indexes;

    #[test]
    fn representative_selection_is_deterministic() {
        assert_eq!(representative_indexes(1), vec![0, 0, 0]);
        assert_eq!(representative_indexes(2), vec![0, 0, 1]);
        assert_eq!(representative_indexes(11), vec![0, 5, 10]);
    }
}
