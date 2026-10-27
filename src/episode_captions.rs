//! Caption sidecars on the verified native episode sample clock. No media execution.
use crate::{
    episode_conform::{Manifest as Conform, Receipt as ConformReceipt, SegmentKind},
    scene_delivery::{FileRef, checked_file},
    series::{SrtEntry, parse_srt, render_srt},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub language: String,
    pub conform_manifest: FileRef,
    pub conform_receipt: FileRef,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Track {
    pub segment_id: String,
    pub language: String,
    pub selected_master_sha256: String,
    pub source_caption: FileRef,
    /// SHA-256 of the UTF-8 JSON array of cue text strings, in source order.
    pub source_words_sha256: String,
    /// Existing owner selection/rights record; retained verbatim, never approved here.
    pub selection_evidence: FileRef,
    /// Original selected master clock, verified against native repeat-only evidence.
    pub source_frame_rate: crate::episode_conform::OutputFrameRate,
    /// Existing caption-family style, for example 70. No other VTT settings synthesized.
    #[serde(default)]
    pub vtt_line_percent: Option<u8>,
}

fn read(root: &Path, reference: &FileRef) -> Result<Vec<u8>> {
    Ok(fs::read(checked_file(root, reference)?)?)
}
fn sha(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn clock(n: u64, d: u64) -> Result<(u64, u64)> {
    if n == 0 || d == 0 || n > 1_000_000 || d > 1_000_000 || n > 120 * d {
        bail!("invalid caption frame clock");
    }
    Ok((n, d))
}
fn fps(value: &str) -> Result<(u64, u64)> {
    let (n, d) = value
        .split_once('/')
        .context("native FPS must be rational")?;
    clock(n.parse()?, d.parse()?)
}
fn same_clock(a: (u64, u64), b: (u64, u64)) -> bool {
    u128::from(a.0) * u128::from(b.1) == u128::from(b.0) * u128::from(a.1)
}
fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).context("caption clock overflow")
}
fn boundary(frame: u64, rate: u32, c: (u64, u64)) -> Result<u64> {
    u64::try_from(
        (u128::from(frame) * u128::from(rate) * u128::from(c.1) + u128::from(c.0) / 2)
            / u128::from(c.0),
    )
    .context("caption boundary overflow")
}
fn stamp(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1_000 % 60,
        ms % 1_000
    )
}

struct CaptionConform {
    episode_id: String,
    language: String,
    manifest_sha256: String,
    timestamps_verified: bool,
    decoded_master_matches_ordered_segments: bool,
    output_sample_rate: u32,
    total_frames: u64,
    total_samples: u64,
    segments: Vec<crate::episode_conform::SegmentReceipt>,
}

/// All inputs are bounded FileRefs in one explicitly staged root. No cache lookup.
/// Writes only to a new or empty directory after every dependency and cue has passed.
pub fn assemble(manifest_path: &Path, root: &Path, output: &Path) -> Result<()> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    let conform_bytes = read(root, &manifest.conform_manifest)?;
    let conform: Conform = serde_json::from_slice(&conform_bytes)?;
    let receipt_bytes = read(root, &manifest.conform_receipt)?;
    // Keep the timing consumer independent of the physical master representation.
    // Deserialize each version with its own typed contract; never synthesize v1
    // master hashes for an ordered immutable part set.
    let schema: serde_json::Value = serde_json::from_slice(&receipt_bytes)?;
    let receipt = if schema["schema"] == crate::episode_conform::parts::RECEIPT {
        let r: crate::episode_conform::parts::Receipt = serde_json::from_slice(&receipt_bytes)?;
        CaptionConform {
            episode_id: r.episode_id,
            language: r.language,
            manifest_sha256: r.manifest_sha256,
            timestamps_verified: r.timestamps_verified,
            decoded_master_matches_ordered_segments: r.compact_ordered_content_verified
                && r.preencode_pcm_identity_verified,
            output_sample_rate: r.output_sample_rate,
            total_frames: r.total_frames,
            total_samples: r.total_samples,
            segments: r.segments,
        }
    } else {
        let r: ConformReceipt = serde_json::from_slice(&receipt_bytes)?;
        if r.schema != "reel.episode-conform-receipt.v1" {
            bail!("unsupported native conform receipt");
        }
        CaptionConform {
            episode_id: r.episode_id,
            language: r.language,
            manifest_sha256: r.manifest_sha256,
            timestamps_verified: r.timestamps_verified,
            decoded_master_matches_ordered_segments: r.decoded_master_matches_ordered_segments,
            output_sample_rate: r.output_sample_rate,
            total_frames: r.total_frames,
            total_samples: r.total_samples,
            segments: r.segments,
        }
    };
    if manifest.schema != "reel.episode-captions.v1"
        || conform.schema != "reel.episode-conform.v1"
        || manifest.language != conform.language
        || manifest.language != receipt.language
        || receipt.episode_id != conform.episode_id
        || receipt.manifest_sha256 != sha(&conform_bytes)
        || !receipt.timestamps_verified
        || !receipt.decoded_master_matches_ordered_segments
        || conform.output_sample_rate == 0
        || receipt.output_sample_rate != conform.output_sample_rate
        || conform.segments.is_empty()
        || conform.segments.len() != receipt.segments.len()
        || manifest.tracks.len() != conform.segments.len()
    {
        bail!("caption manifest does not bind a complete verified native conform");
    }
    let mut frame_cursor = 0;
    let mut sample_cursor = 0;
    let mut ids = BTreeSet::new();
    let mut cues: Vec<SrtEntry> = Vec::new();
    let mut vtt = String::from("WEBVTT\n\n");
    let mut lineage = Vec::new();
    let mut output_clock = None;
    for ((segment, native), track) in conform
        .segments
        .iter()
        .zip(&receipt.segments)
        .zip(&manifest.tracks)
    {
        let source_clock = clock(
            u64::from(track.source_frame_rate.numerator),
            u64::from(track.source_frame_rate.denominator),
        )?;
        if track.source_frame_rate.policy != "repeat-only" {
            bail!("unsupported source frame clock policy");
        }
        let c = match &conform.output_frame_rate {
            Some(c) if c.policy == "repeat-only" => {
                clock(u64::from(c.numerator), u64::from(c.denominator))?
            }
            Some(_) => bail!("unsupported native output clock"),
            None => source_clock,
        };
        if output_clock.is_some_and(|previous| !same_clock(previous, c)) {
            bail!("native output clock changes between segments");
        }
        output_clock = Some(c);
        let kind = match segment.kind {
            SegmentKind::Scene => "scene",
            SegmentKind::EpisodePresentation => "episode-presentation",
        };
        if !ids.insert(&segment.id)
            || track.segment_id != segment.id
            || native.id != segment.id
            || native.kind != kind
            || track.language != manifest.language
            || track.selected_master_sha256 != segment.master.sha256
            || native.selected_master_sha256 != segment.master.sha256
            || native.selected_master_bytes != segment.master.bytes
            || native.source_receipt_sha256 != segment.source_receipt.sha256
            || native.start_frame != frame_cursor
            || native.start_sample != sample_cursor
            || native.output_sample_rate != receipt.output_sample_rate
            || native.input_sample_rate == 0
            || native.input_frames == 0
            || native.input_samples == 0
            || native.tail_hold_frames != segment.tail_hold_frames
            || track.vtt_line_percent.is_some_and(|line| line > 100)
            || (segment.kind == SegmentKind::Scene && !native.upstream_delivery_verified)
            || (segment.kind == SegmentKind::EpisodePresentation
                && !native.upstream_presentation_verified)
        {
            bail!(
                "caption track disagrees with ordered native segment metadata: {}",
                segment.id
            );
        }
        let expected_frames = u64::try_from(
            (u128::from(native.input_frames) * u128::from(c.0) * u128::from(source_clock.1))
                .div_ceil(u128::from(c.1) * u128::from(source_clock.0)),
        )?;
        if u128::from(c.0) * u128::from(source_clock.1)
            < u128::from(source_clock.0) * u128::from(c.1)
            || add(expected_frames, native.tail_hold_frames)? != native.frames
        {
            bail!("native caption picture count mismatch");
        }
        match &native.frame_rate_verification {
            Some(e)
                if same_clock(fps(&e.input_fps)?, source_clock)
                    && same_clock(fps(&e.output_fps)?, c)
                    && e.input_frames == native.input_frames
                    && e.output_frames == expected_frames
                    && e.repeated_frames == expected_frames - native.input_frames
                    && e.transformed_source_matches_selected => {}
            None if same_clock(source_clock, c) => {}
            _ => bail!("source caption clock differs from native repeat-only evidence"),
        }
        if u128::from(native.tail_hold_frames) * u128::from(c.1) > u128::from(c.0) * 60 {
            bail!("native caption silent hold exceeds conform limit");
        }
        match conform.audio_frame_conform.as_deref() {
            None if native.audio_padding_samples == 0 => {}
            Some("pad-silence-to-picture-boundaries") => {}
            _ => bail!("native caption padding disagrees with conform policy"),
        }
        let end_frame = add(frame_cursor, native.frames)?;
        let hold_samples = boundary(end_frame, receipt.output_sample_rate, c)?
            .checked_sub(boundary(
                end_frame - native.tail_hold_frames,
                receipt.output_sample_rate,
                c,
            )?)
            .context("invalid hold clock")?;
        let content_samples = native
            .samples
            .checked_sub(native.tail_hold_samples)
            .and_then(|v| v.checked_sub(native.audio_padding_samples))
            .context("invalid native caption sample counts")?;
        if native.tail_hold_samples != hold_samples || content_samples == 0 {
            bail!("native caption hold/sample count mismatch");
        }
        if conform.audio_frame_conform.as_deref() == Some("pad-silence-to-picture-boundaries")
            && native.samples
                != boundary(end_frame, receipt.output_sample_rate, c)?
                    - boundary(frame_cursor, receipt.output_sample_rate, c)?
        {
            bail!("native caption sample/frame boundary mismatch");
        }
        // Resampling may differ by one sample; no other native duration discrepancy is accepted.
        let resampled = (u128::from(native.input_samples) * u128::from(receipt.output_sample_rate))
            .div_ceil(u128::from(native.input_sample_rate));
        if u128::from(content_samples).abs_diff(resampled) > 1 {
            bail!("native caption source sample duration mismatch");
        }
        read(root, &track.selection_evidence)?;
        let source = read(root, &track.source_caption)?;
        let entries = parse_srt(std::str::from_utf8(&source)?)?;
        if entries.first().is_some_and(|e| e.index != 1)
            || sha(&serde_json::to_vec(
                &entries.iter().map(|e| &e.text).collect::<Vec<_>>(),
            )?) != track.source_words_sha256
        {
            bail!("selected source caption words/index mismatch");
        }
        let first_cue = cues.len() + 1;
        for entry in entries {
            if u128::from(entry.end_ms) * u128::from(source_clock.0)
                > u128::from(native.input_frames) * u128::from(source_clock.1) * 1000
                || u128::from(entry.end_ms) * u128::from(receipt.output_sample_rate)
                    > u128::from(content_samples) * 1000
            {
                bail!(
                    "source caption exceeds content or leaks into silent hold: {}",
                    segment.id
                );
            }
            let offset = u128::from(sample_cursor) * 1000;
            let rate = u128::from(receipt.output_sample_rate);
            // Inward rounding keeps millisecond exports outside protected silence.
            let start_ms =
                u64::try_from((offset + u128::from(entry.start_ms) * rate).div_ceil(rate))?;
            let end_ms = u64::try_from((offset + u128::from(entry.end_ms) * rate) / rate)?;
            if end_ms <= start_ms || cues.last().is_some_and(|last| last.end_ms > start_ms) {
                bail!("assembled captions overlap or round to zero duration");
            }
            let index = cues.len() + 1;
            let style = track
                .vtt_line_percent
                .map(|line| format!(" line:{line}%"))
                .unwrap_or_default();
            vtt.push_str(&format!(
                "{index}\n{} --> {}{style}\n{}\n\n",
                stamp(start_ms),
                stamp(end_ms),
                entry.text
            ));
            cues.push(SrtEntry {
                index,
                start_ms,
                end_ms,
                text: entry.text,
            });
        }
        lineage.push(serde_json::json!({"segment_id":segment.id,"source_caption":track.source_caption,"selection_evidence":track.selection_evidence,"source_words_sha256":track.source_words_sha256,"start_sample":native.start_sample,"content_samples":content_samples,"tail_hold_samples":native.tail_hold_samples,"first_cue":first_cue,"cue_count":cues.len()+1-first_cue}));
        frame_cursor = end_frame;
        sample_cursor = add(sample_cursor, native.samples)?;
    }
    if frame_cursor != receipt.total_frames || sample_cursor != receipt.total_samples {
        bail!("caption conform totals mismatch");
    }
    let srt = render_srt(&cues);
    let report = serde_json::json!({"schema":"reel.episode-captions-receipt.v1","manifest_sha256":sha(&manifest_bytes),"conform_manifest":manifest.conform_manifest,"conform_receipt":manifest.conform_receipt,"episode_id":receipt.episode_id,"language":manifest.language,"output_sample_rate":receipt.output_sample_rate,"total_samples":sample_cursor,"cues":cues.len(),"srt":{"sha256":sha(srt.as_bytes()),"bytes":srt.len()},"vtt":{"sha256":sha(vtt.as_bytes()),"bytes":vtt.len()},"tracks":lineage,"publication":"not-authorized","creative_review_state":"open; no human approval inferred"});
    if output.exists() {
        if !output.is_dir() || fs::read_dir(output)?.next().is_some() {
            bail!("caption output must be a new or empty directory");
        }
    } else {
        fs::create_dir(output).context("creating caption output directory")?;
    }
    let write_new = |name: &str, bytes: &[u8]| -> Result<()> {
        use std::io::Write;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(name))?;
        file.write_all(bytes)?;
        Ok(())
    };
    write_new("captions.srt", srt.as_bytes())?;
    write_new("captions.vtt", vtt.as_bytes())?;
    write_new("caption-timing.json", &serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
