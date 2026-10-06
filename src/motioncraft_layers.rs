//! Frame-indexed alpha-layer validation independent of camera hold allowances.
use crate::adapters::still_animatic::{
    MAX_NEAR_STATIONARY_FRACTION, MIN_HOLD_STATIONARY_FRACTION, NEAR_STATIONARY_LUMA_THRESHOLD,
};
use crate::{
    motioncraft_cadence,
    scene_delivery::{self, ExternalLayerRenderMode, Span},
};
use anyhow::{Context, Result, bail};
use reel_assembly::motioncraft::Rect;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::Path,
    process::{Child, ChildStdout, Command, Stdio},
};

const COMPOSITION_TOLERANCE: f64 = 4.0;
const MIN_VISIBLE_FRACTION: f64 = 1.0 / 255.0;
const MAX_ROI_PIXELS: u64 = 2_073_600;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub job_sha256: String,
    pub render_receipt_sha256: String,
    pub layers: Vec<Expectation>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expectation {
    pub attachment_id: String,
    pub source_sha256: String,
    pub intervals: Vec<Interval>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interval {
    pub start_frame: u64,
    pub end_frame: u64,
    pub kind: Kind,
    pub region: Rect,
}
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Moving,
    Hold,
    Occluded,
}

#[derive(Clone, Copy)]
struct Roi {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}
fn roi(rect: &Rect, width: u32, height: u32) -> Result<Roi> {
    if [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .any(|v| !v.is_finite())
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.width <= 0.0
        || rect.height <= 0.0
        || rect.x + rect.width > 1.0
        || rect.y + rect.height > 1.0
    {
        bail!("invalid layer inspection region");
    }
    let x = (rect.x * f64::from(width)).floor() as u32;
    let y = (rect.y * f64::from(height)).floor() as u32;
    let w = ((rect.x + rect.width) * f64::from(width)).ceil() as u32 - x;
    let h = ((rect.y + rect.height) * f64::from(height)).ceil() as u32 - y;
    if u64::from(w) * u64::from(h) > MAX_ROI_PIXELS {
        bail!("layer region exceeds streaming frame budget");
    }
    Ok(Roi {
        x,
        y,
        width: w,
        height: h,
    })
}

/// Only two adjacent frames per stream are retained; runtime does not allocate
/// a movie-sized buffer. Child ownership also ensures failed checks stop decoders.
struct Frames {
    child: Child,
    stdout: ChildStdout,
    stride: usize,
    remaining: u64,
}
impl Frames {
    fn open_geometry(
        executable: &Path,
        video: &Path,
        start: u64,
        end: u64,
        region: Roi,
        normalize: &str,
    ) -> Result<Self> {
        Self::open_at(executable, video, start, end, region, normalize, None)
    }
    fn open_at(
        executable: &Path,
        video: &Path,
        start: u64,
        end: u64,
        region: Roi,
        normalize: &str,
        directory: Option<&Path>,
    ) -> Result<Self> {
        let filter = format!(
            "trim=start_frame={start}:end_frame={end},{normalize}crop={}:{}:{}:{},format=rgba",
            region.width, region.height, region.x, region.y
        );
        let mut command = Command::new(executable);
        let video = if let Some(directory) = directory {
            command.current_dir(directory);
            video.canonicalize()?
        } else {
            video.to_path_buf()
        };
        let mut child = command
            .args([
                "-v",
                "error",
                "-nostdin",
                "-protocol_whitelist",
                "file,pipe",
                "-i",
            ])
            .arg(&video)
            .args([
                "-vf",
                &filter,
                "-fps_mode",
                "passthrough",
                "-pix_fmt",
                "rgba",
                "-f",
                "rawvideo",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()?;
        let stdout = child
            .stdout
            .take()
            .context("layer decoder stdout unavailable")?;
        Ok(Self {
            child,
            stdout,
            stride: region.width as usize * region.height as usize * 4,
            remaining: end - start,
        })
    }
    fn next(&mut self) -> Result<Vec<u8>> {
        if self.remaining == 0 {
            bail!("layer decoder exhausted declared span");
        }
        let mut bytes = vec![0; self.stride];
        self.stdout
            .read_exact(&mut bytes)
            .context("layer decoder has missing native frames")?;
        self.remaining -= 1;
        Ok(bytes)
    }
    fn finish(&mut self) -> Result<()> {
        let mut extra = [0];
        if self.remaining != 0
            || self.stdout.read(&mut extra)? != 0
            || !self.child.wait()?.success()
        {
            bail!("layer decoder frame count or exit status differs");
        }
        Ok(())
    }
}

enum Carrier {
    Video(Frames),
    Ass { black: Frames, white: Frames },
}
struct CarrierFrame {
    rgba: Vec<u8>,
    ass_response: Option<(Vec<u8>, Vec<u8>)>,
}
impl CarrierFrame {
    fn expected(&self, pixel: usize, channel: usize, base: u8) -> f64 {
        if let Some((black, white)) = &self.ass_response {
            let offset = pixel * 4 + channel;
            let dark = f64::from(black[offset]);
            let transmission = ((f64::from(white[offset]) - dark) / 255.0).clamp(0.0, 1.0);
            dark + transmission * f64::from(base)
        } else {
            let offset = pixel * 4;
            let alpha = f64::from(self.rgba[offset + 3]) / 255.0;
            alpha * f64::from(self.rgba[offset + channel]) + (1.0 - alpha) * f64::from(base)
        }
    }
    fn transmission(&self, pixel: usize) -> f64 {
        if let Some((black, white)) = &self.ass_response {
            // Conservative across channels: motion cannot borrow transparency
            // from a channel in which the later presentation is opaque.
            (0..3)
                .map(|c| {
                    let offset = pixel * 4 + c;
                    ((f64::from(white[offset]) - f64::from(black[offset])) / 255.0).clamp(0.0, 1.0)
                })
                .fold(1.0, f64::min)
        } else {
            1.0 - f64::from(self.rgba[pixel * 4 + 3]) / 255.0
        }
    }
}
impl Carrier {
    fn ass(
        exe: &Path,
        root: &Path,
        before: &Path,
        range: (u64, u64),
        region: Roi,
        config: (usize, usize, bool),
    ) -> Result<Self> {
        let (index, count, font_bound) = config;
        let ass = if count == 1 {
            "presentation.ass".into()
        } else {
            format!("presentation-{index:03}.ass")
        };
        let font = if font_bound { ":fontsdir=fonts" } else { "" };
        let stream = |value: u8| {
            let filter = format!(
                "format=rgb24,lutrgb=r={value}:g={value}:b={value},format=yuv444p,ass={ass}{font},"
            );
            // Use the actual preceding picture's timestamps, including native
            // container rounding, rather than inventing a fresh text clock.
            Frames::open_at(exe, before, range.0, range.1, region, &filter, Some(root))
        };
        Ok(Self::Ass {
            black: stream(0)?,
            white: stream(255)?,
        })
    }
    fn next(&mut self) -> Result<CarrierFrame> {
        match self {
            Self::Video(frames) => Ok(CarrierFrame {
                rgba: frames.next()?,
                ass_response: None,
            }),
            Self::Ass { black, white } => {
                let a = black.next()?;
                let b = white.next()?;
                let mut rgba = Vec::with_capacity(a.len());
                for (dark, light) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
                    let transmission = (0..3)
                        .map(|c| (f64::from(light[c]) - f64::from(dark[c])) / 255.0)
                        .sum::<f64>()
                        / 3.0;
                    let alpha = (1.0 - transmission).clamp(0.0, 1.0);
                    for &channel in &dark[..3] {
                        rgba.push(if alpha > 0.0 {
                            (f64::from(channel) / alpha).round().clamp(0.0, 255.0) as u8
                        } else {
                            0
                        });
                    }
                    rgba.push((alpha * 255.0).round() as u8);
                }
                // Preserve native per-channel responses for composition;
                // rounded straight RGBA is only the cadence representation.
                Ok(CarrierFrame {
                    rgba,
                    ass_response: Some((a, b)),
                })
            }
        }
    }
    fn finish(&mut self) -> Result<()> {
        match self {
            Self::Video(frames) => frames.finish(),
            Self::Ass { black, white } => {
                black.finish()?;
                white.finish()
            }
        }
    }
}
impl Drop for Frames {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

struct Occluder {
    span: Span,
    frames: Option<Carrier>,
    before: Frames,
    after: Frames,
}
fn source_name(index: usize, count: usize) -> String {
    if count == 1 {
        "selected-overlay.mkv".into()
    } else {
        format!("selected-overlay-{index:03}.mkv")
    }
}
fn alpha_filter(width: u32, height: u32) -> String {
    format!(
        "format=rgba,pad=max(iw\\,{width}):max(ih\\,{height}):0:0:color=black@0,crop={width}:{height}:0:0,"
    )
}

fn measure(
    exe: &Path,
    root: &Path,
    geometry: (u32, u32),
    index: usize,
    spans: &[Span],
    interval: &Interval,
    ass: Option<(usize, bool)>,
) -> Result<Value> {
    let (width, height) = geometry;
    let span = &spans[index];
    let r = roi(&interval.region, width, height)?;
    let start = interval.start_frame;
    let end = interval.end_frame;
    let normalize = alpha_filter(width, height);
    let input = if index == 0 {
        "clean-picture.mkv".into()
    } else {
        format!("layered-picture-{:03}.mkv", index - 1)
    };
    let mut source = if ass.is_some_and(|(n, _)| n == index) {
        Carrier::ass(
            exe,
            root,
            &root.join(&input),
            (start, end),
            r,
            (index, spans.len(), ass.unwrap().1),
        )?
    } else {
        Carrier::Video(Frames::open_geometry(
            exe,
            &root.join(source_name(index, spans.len())),
            start - span.start_frame,
            end - span.start_frame,
            r,
            &normalize,
        )?)
    };
    let output = if index + 1 == spans.len() {
        "picture.mkv".into()
    } else {
        format!("layered-picture-{index:03}.mkv")
    };
    let mut before = Frames::open_geometry(exe, &root.join(input), start, end, r, "")?;
    let mut after = Frames::open_geometry(exe, &root.join(output), start, end, r, "")?;
    let mut later = Vec::new();
    for (n, other) in spans.iter().enumerate().skip(index + 1) {
        let a = start.max(other.start_frame);
        let b = end.min(other.end_frame);
        {
            later.push(Occluder {
                span: other.clone(),
                before: Frames::open_geometry(
                    exe,
                    &root.join(format!("layered-picture-{:03}.mkv", n - 1)),
                    start,
                    end,
                    r,
                    "",
                )?,
                after: Frames::open_geometry(
                    exe,
                    &root.join(if n + 1 == spans.len() {
                        "picture.mkv".into()
                    } else {
                        format!("layered-picture-{n:03}.mkv")
                    }),
                    start,
                    end,
                    r,
                    "",
                )?,
                frames: if a < b {
                    Some(if ass.is_some_and(|(index, _)| index == n) {
                        Carrier::ass(
                            exe,
                            root,
                            &root.join(format!("layered-picture-{:03}.mkv", n - 1)),
                            (a, b),
                            r,
                            (n, spans.len(), ass.unwrap().1),
                        )?
                    } else {
                        Carrier::Video(Frames::open_geometry(
                            exe,
                            &root.join(source_name(n, spans.len())),
                            a - other.start_frame,
                            b - other.start_frame,
                            r,
                            &normalize,
                        )?)
                    })
                } else {
                    None
                },
            });
        }
    }
    let mut previous: Option<Vec<u8>> = None;
    let mut stationary = 0u64;
    let mut visible_stationary = 0u64;
    let mut previous_transmission: Option<Vec<f64>> = None;
    let mut mismatched_frames = 0u64;
    let mut max_error = 0.0f64;
    let mut source_visibility = 0.0;
    let mut final_visibility = 0.0;
    let mut final_contrast = 0.0;
    let mut insufficient_frames = 0u64;
    let mut minimum_surviving_alpha = 1.0f64;
    let pixels = u64::from(r.width) * u64::from(r.height);
    for frame in start..end {
        let src = source.next()?;
        let base = before.next()?;
        let result = after.next()?;
        let mut transmission = vec![1.0; pixels as usize];
        let mut error = 0.0f64;
        for other in &mut later {
            let stage_before = other.before.next()?;
            let stage_after = other.after.next()?;
            if frame >= other.span.start_frame && frame < other.span.end_frame {
                let carrier = other
                    .frames
                    .as_mut()
                    .context("active layer has no decoder")?
                    .next()?;
                for (n, ((value, base_pixel), out_pixel)) in transmission
                    .iter_mut()
                    .zip(stage_before.chunks_exact(4))
                    .zip(stage_after.chunks_exact(4))
                    .enumerate()
                {
                    *value *= carrier.transmission(n);
                    for c in 0..3 {
                        let expected = carrier.expected(n, c, base_pixel[c]);
                        error = error.max((expected - f64::from(out_pixel[c])).abs());
                    }
                }
            } else {
                for (base_pixel, out_pixel) in stage_before
                    .chunks_exact(4)
                    .zip(stage_after.chunks_exact(4))
                {
                    for c in 0..3 {
                        error =
                            error.max((f64::from(base_pixel[c]) - f64::from(out_pixel[c])).abs());
                    }
                }
            }
        }
        let mut frame_visibility = 0.0;
        let mut frame_contrast = 0.0;
        let mut delta = 0.0;
        let mut visible_delta = 0.0;
        for (n, ((s, b), out)) in src
            .rgba
            .chunks_exact(4)
            .zip(base.chunks_exact(4))
            .zip(result.chunks_exact(4))
            .enumerate()
        {
            let alpha = f64::from(s[3]) / 255.0;
            source_visibility += alpha;
            frame_visibility += alpha * transmission[n];
            for c in 0..3 {
                let expected = src.expected(n, c, b[c]);
                error = error.max((expected - f64::from(out[c])).abs());
                frame_contrast +=
                    alpha * transmission[n] * (f64::from(s[c]) - f64::from(b[c])).abs() / 3.0;
            }
            if let Some(prev) = &previous {
                let p = &prev[n * 4..n * 4 + 4];
                let old_alpha = f64::from(p[3]) / 255.0;
                let mut pixel_delta = (f64::from(s[3]) - f64::from(p[3])).abs();
                for c in 0..3 {
                    pixel_delta += (alpha * f64::from(s[c]) - old_alpha * f64::from(p[c])).abs();
                }
                delta += pixel_delta;
                visible_delta +=
                    pixel_delta * transmission[n].min(previous_transmission.as_ref().unwrap()[n]);
            }
        }
        final_visibility += frame_visibility;
        final_contrast += frame_contrast;
        let visible_fraction = frame_visibility / pixels as f64;
        minimum_surviving_alpha = minimum_surviving_alpha.min(visible_fraction);
        if visible_fraction < MIN_VISIBLE_FRACTION
            || frame_contrast / (pixels as f64) < NEAR_STATIONARY_LUMA_THRESHOLD
        {
            insufficient_frames += 1;
        }
        if previous.is_some() && delta / (pixels as f64 * 4.0) < NEAR_STATIONARY_LUMA_THRESHOLD {
            stationary += 1;
        }
        if previous.is_some()
            && visible_delta / (pixels as f64 * 4.0) < NEAR_STATIONARY_LUMA_THRESHOLD
        {
            visible_stationary += 1;
        }
        previous_transmission = Some(transmission);
        if error > COMPOSITION_TOLERANCE {
            mismatched_frames += 1;
        }
        max_error = max_error.max(error);
        previous = Some(src.rgba);
    }
    source.finish()?;
    before.finish()?;
    after.finish()?;
    for other in &mut later {
        if let Some(frames) = &mut other.frames {
            frames.finish()?;
        }
        other.before.finish()?;
        other.after.finish()?;
    }
    let frames = end - start;
    let transitions = frames.saturating_sub(1);
    let denominator = frames as f64 * pixels as f64;
    let visible = final_visibility / denominator;
    let contrast = final_contrast / denominator;
    let stationary_fraction = if transitions == 0 {
        1.0
    } else {
        stationary as f64 / transitions as f64
    };
    let visible_stationary_fraction = if transitions == 0 {
        1.0
    } else {
        visible_stationary as f64 / transitions as f64
    };
    let inconclusive = insufficient_frames > 0
        || transitions == 0
        || source_visibility / denominator < MIN_VISIBLE_FRACTION
        || visible < MIN_VISIBLE_FRACTION
        || contrast < NEAR_STATIONARY_LUMA_THRESHOLD
        || interval.kind == Kind::Occluded;
    let temporal = match interval.kind {
        Kind::Moving => {
            stationary_fraction <= MAX_NEAR_STATIONARY_FRACTION
                && visible_stationary_fraction <= MAX_NEAR_STATIONARY_FRACTION
        }
        Kind::Hold => stationary_fraction >= MIN_HOLD_STATIONARY_FRACTION,
        Kind::Occluded => false,
    };
    let (status, passed) = if mismatched_frames > 0 {
        ("failed-composition-or-timing", Some(false))
    } else if interval.kind == Kind::Occluded && insufficient_frames < frames {
        ("failed-visibility-expectation", Some(false))
    } else if inconclusive {
        ("inconclusive-visibility-or-duration", None)
    } else if !temporal {
        ("failed-source-temporal-expectation", Some(false))
    } else {
        ("evaluated", Some(true))
    };
    Ok(
        json!({"start_frame":start,"end_frame_exclusive":end,"kind":interval.kind,"region":interval.region,
        "frames_evaluated":frames,"transitions":transitions,"stationary_fraction":stationary_fraction,"visible_stationary_fraction":visible_stationary_fraction,
        "mean_source_alpha":source_visibility/denominator,"mean_surviving_alpha":visible,"mean_final_contribution_contrast":contrast,
        "insufficient_visibility_frames":insufficient_frames,"minimum_surviving_alpha":minimum_surviving_alpha,
        "maximum_composition_error":max_error,"composition_mismatched_frames":mismatched_frames,"status":status,"passed":passed}),
    )
}

fn outside_span_mismatches(
    exe: &Path,
    root: &Path,
    geometry: (u32, u32, u64),
    index: usize,
    spans: &[Span],
    interval: &Interval,
) -> Result<u64> {
    let region = roi(&interval.region, geometry.0, geometry.1)?;
    let input = if index == 0 {
        "clean-picture.mkv".into()
    } else {
        format!("layered-picture-{:03}.mkv", index - 1)
    };
    let output = if index + 1 == spans.len() {
        "picture.mkv".into()
    } else {
        format!("layered-picture-{index:03}.mkv")
    };
    let mut mismatches = 0;
    for (start, end) in [
        (0, spans[index].start_frame),
        (spans[index].end_frame, geometry.2),
    ] {
        if start == end {
            continue;
        }
        let mut before = Frames::open_geometry(exe, &root.join(&input), start, end, region, "")?;
        let mut after = Frames::open_geometry(exe, &root.join(&output), start, end, region, "")?;
        for _ in start..end {
            let a = before.next()?;
            let b = after.next()?;
            if a.chunks_exact(4).zip(b.chunks_exact(4)).any(|(a, b)| {
                (0..3).any(|c| (f64::from(a[c]) - f64::from(b[c])).abs() > COMPOSITION_TOLERANCE)
            }) {
                mismatches += 1;
            }
        }
        before.finish()?;
        after.finish()?;
    }
    Ok(mismatches)
}

pub fn analyze(
    job_path: &Path,
    assets: &Path,
    render: &Path,
    request_path: &Path,
) -> Result<Value> {
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)?;
    let request_sha256 = crate::sha256_file(request_path)?;
    if request.schema != "reel.motioncraft-layer-expectations.v1"
        || request.job_sha256 != crate::sha256_file(job_path)?
        || request.render_receipt_sha256 != crate::sha256_file(&render.join("receipt.json"))?
    {
        bail!("layer expectations do not bind the exact job and render");
    }
    let (job, plan) = scene_delivery::plan(job_path, assets)?;
    let receipt = scene_delivery::check(job_path, assets, render)?;
    let layers: Vec<_> = job
        .external_layers
        .iter()
        .filter(|l| l.render_mode != ExternalLayerRenderMode::EvidenceOnly)
        .collect();
    if layers.is_empty() || layers.len() > 8 || request.layers.len() != layers.len() {
        bail!("declare each of 1..8 rendered layers exactly once");
    }
    let ids: BTreeSet<_> = request.layers.iter().map(|l| &l.attachment_id).collect();
    if ids.len() != layers.len() || layers.iter().any(|l| !ids.contains(&l.attachment_id)) {
        bail!("unknown, duplicate or missing layer expectation");
    }
    let executable = motioncraft_cadence::resolve_analyzer()?;
    let executable_sha256 = crate::sha256_file(&executable)?;
    let version = motioncraft_cadence::native_output(&executable, &["-version".into()])?;
    let ass = layers
        .iter()
        .enumerate()
        .find(|(_, layer)| layer.render_mode == ExternalLayerRenderMode::AssOverlay)
        .map(|(index, layer)| (index, layer.font.is_some()));
    if ass.is_some() && version.lines().next() != Some(receipt.ffmpeg_version.as_str()) {
        bail!("ASS opacity replay requires the rendered FFmpeg version");
    }
    let spans: Vec<_> = layers
        .iter()
        .map(|l| {
            plan.external_layer_spans
                .iter()
                .find(|s| s.attachment_id == l.attachment_id)
                .unwrap()
                .clone()
        })
        .collect();
    let mut rows = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        let expectation = request
            .layers
            .iter()
            .find(|e| e.attachment_id == layer.attachment_id)
            .unwrap();
        let source = layer.render_source.as_ref().unwrap_or(&layer.evidence);
        if expectation.source_sha256 != source.sha256 {
            bail!("layer expectation binds a stale selected carrier");
        }
        let span = &spans[index];
        if expectation.intervals.is_empty() || expectation.intervals.len() > 64 {
            bail!("layer requires 1..64 bounded intervals");
        }
        let mut cursor = span.start_frame;
        for interval in &expectation.intervals {
            if interval.start_frame != cursor
                || interval.end_frame <= cursor
                || interval.end_frame > span.end_frame
                || interval.end_frame - cursor > 432000
            {
                bail!("layer intervals must cover the exact native span without gaps/overlaps");
            }
            roi(&interval.region, job.width, job.height)?;
            cursor = interval.end_frame;
        }
        if cursor != span.end_frame {
            bail!("layer expectation leaves native frames uncovered");
        }
        if job.post_compose_camera.is_some() {
            rows.push(json!({"attachment_id":layer.attachment_id,"status":"needs-separate-composition-analysis","passed":null}));
            continue;
        }
        let intervals = expectation
            .intervals
            .iter()
            .map(|interval| {
                let mut row = measure(
                    &executable,
                    render,
                    (job.width, job.height),
                    index,
                    &spans,
                    interval,
                    ass,
                )?;
                let outside = outside_span_mismatches(
                    &executable,
                    render,
                    (job.width, job.height, plan.frame_count),
                    index,
                    &spans,
                    interval,
                )?;
                row["outside_active_span_mismatched_frames"] = json!(outside);
                if outside > 0 {
                    row["status"] = json!("failed-composition-or-timing");
                    row["passed"] = json!(false);
                }
                Ok(row)
            })
            .collect::<Result<Vec<_>>>()?;
        let passed = if intervals.iter().any(|row| row["passed"] == false) {
            Some(false)
        } else if intervals.iter().all(|row| row["passed"] == true) {
            Some(true)
        } else {
            None
        };
        rows.push(json!({"attachment_id":layer.attachment_id,"source_sha256":source.sha256,"start_sample":span.start_sample,
            "end_sample_exclusive":span.end_sample,"intervals":intervals,"passed":passed}));
    }
    if crate::sha256_file(&executable)? != executable_sha256
        || crate::sha256_file(request_path)? != request_sha256
    {
        bail!("layer analyzer or expectations changed during execution");
    }
    let passed = rows.iter().all(|row| row["passed"] == true);
    Ok(
        json!({"schema":"reel.motioncraft-layer-cadence.v1","job_sha256":plan.job_sha256,"render_receipt_sha256":request.render_receipt_sha256,
        "picture_sha256":crate::sha256_file(&render.join("picture.mkv"))?,"expectations_sha256":request_sha256,
        "analyzer_backend":"native","analyzer_executable":executable,"analyzer_executable_sha256":executable_sha256,
        "analyzer_ffmpeg_version":version.lines().next(),"near_stationary_threshold":NEAR_STATIONARY_LUMA_THRESHOLD,
        "maximum_moving_stationary_fraction":MAX_NEAR_STATIONARY_FRACTION,"minimum_hold_stationary_fraction":MIN_HOLD_STATIONARY_FRACTION,
        "composition_channel_tolerance":COMPOSITION_TOLERANCE,"minimum_visible_fraction":MIN_VISIBLE_FRACTION,
        "coordinate_space":"normalized full output before any post-compose camera","layers":rows,"passed":passed,
        "scope":"All declared native frames, selected premultiplied alpha source, independent RGB composition and later-layer visibility. Camera holds grant no layer exemptions. Not creative approval."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ffmpeg(exe: &Path, args: &[&str], output: &Path) {
        assert!(
            Command::new(exe)
                .args(["-v", "error", "-nostdin"])
                .args(args)
                .arg(output)
                .status()
                .unwrap()
                .success()
        );
    }
    #[test]
    #[ignore = "requires native FFmpeg; ASS opacity and final effect contribution"]
    fn ass_response_proves_visible_motion_and_rejects_hidden_or_wrong_composition() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let exe = motioncraft_cadence::resolve_analyzer().unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:r=24:d=1",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("clean-picture.mkv"),
        );
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=64x64:r=24:d=1,format=yuva444p,colorchannelmixer=aa=0.75",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
            ],
            &root.join("selected-overlay-000.mkv"),
        );
        ffmpeg(
            &exe,
            &[
                "-i",
                root.join("clean-picture.mkv").to_str().unwrap(),
                "-i",
                root.join("selected-overlay-000.mkv").to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v]overlay=format=auto",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("layered-picture-000.mkv"),
        );
        let render_ass = |drawing: &str| {
            fs::write(root.join("presentation-001.ass"),format!("[Script Info]\nScriptType: v4.00+\nPlayResX: 64\nPlayResY: 64\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Outline,Shadow,Alignment,MarginL,MarginR,MarginV,Encoding\nStyle: Text,Arial,12,&H00FFFFFF,&H00FFFFFF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,0,0,7,0,0,0,1\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\nDialogue: 0,0:00:00.00,0:00:01.00,Text,,0,0,0,,{{\\an7\\pos(0,0)\\bord0\\p1}}{drawing}\n")).unwrap();
            let picture = root.join("picture.mkv");
            if picture.exists() {
                fs::remove_file(&picture).unwrap();
            }
            assert!(
                Command::new(&exe)
                    .current_dir(root)
                    .args([
                        "-v",
                        "error",
                        "-nostdin",
                        "-i",
                        "layered-picture-000.mkv",
                        "-vf",
                        "ass=presentation-001.ass",
                        "-c:v",
                        "ffv1",
                        "-pix_fmt",
                        "yuv444p",
                        "-n",
                        "picture.mkv"
                    ])
                    .status()
                    .unwrap()
                    .success()
            );
        };
        let spans: Vec<_> = ["effect", "text"]
            .into_iter()
            .map(|id| Span {
                attachment_id: id.into(),
                start_sample: 0,
                end_sample: 48000,
                start_frame: 0,
                end_frame: 24,
            })
            .collect();
        let mut interval = Interval {
            start_frame: 0,
            end_frame: 24,
            kind: Kind::Moving,
            region: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 0.5,
            },
        };
        render_ass("m 0 48 l 66 48 66 66 0 66");
        let moving = measure(&exe, root, (64, 64), 0, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(moving["passed"], true, "{moving}");
        interval.kind = Kind::Hold;
        interval.region = Rect {
            x: 0.0,
            y: 0.75,
            width: 1.0,
            height: 0.25,
        };
        let text = measure(&exe, root, (64, 64), 1, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(text["passed"], true, "{text}");
        // Real glyph edges, a warm foreground and partial opacity must agree
        // with the native composition, not just opaque vector masks.
        render_ass("{\\p0\\c&H00AAD7E8&\\1a&H40&\\bord1}Read");
        interval.region = Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let glyph = measure(&exe, root, (64, 64), 1, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(glyph["passed"], true, "{glyph}");
        render_ass("m 0 0 l 66 0 66 66 0 66");
        interval.kind = Kind::Moving;
        interval.region = Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        };
        let hidden = measure(&exe, root, (64, 64), 0, &spans, &interval, Some((1, false))).unwrap();
        assert!(hidden["passed"].is_null(), "{hidden}");
        assert_eq!(hidden["insufficient_visibility_frames"], 24);
        fs::copy(
            root.join("layered-picture-000.mkv"),
            root.join("picture.mkv"),
        )
        .unwrap();
        let wrong = measure(&exe, root, (64, 64), 0, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(wrong["passed"], false, "{wrong}");
        assert_eq!(wrong["status"], "failed-composition-or-timing");
        // Motion in the covered half cannot borrow visibility from the static
        // half. Deliberately overshoot the mask edge to cover its antialiasing.
        fs::remove_file(root.join("selected-overlay-000.mkv")).unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "nullsrc=s=64x64:r=24:d=1,format=rgba,geq=r='if(lt(X,32),N*10,0)':g='if(lt(X,32),200,255)':b=0:a=191",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
            ],
            &root.join("selected-overlay-000.mkv"),
        );
        fs::remove_file(root.join("layered-picture-000.mkv")).unwrap();
        ffmpeg(
            &exe,
            &[
                "-i",
                root.join("clean-picture.mkv").to_str().unwrap(),
                "-i",
                root.join("selected-overlay-000.mkv").to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v]overlay=format=auto",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("layered-picture-000.mkv"),
        );
        render_ass("m 0 0 l 34 0 34 66 0 66");
        let partial =
            measure(&exe, root, (64, 64), 0, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(partial["passed"], false, "{partial}");
        assert_eq!(partial["status"], "failed-source-temporal-expectation");
        assert_eq!(partial["insufficient_visibility_frames"], 0, "{partial}");
        // A changing caption must not supply temporal evidence for a frozen
        // selected effect that remains visible elsewhere in the frame.
        fs::remove_file(root.join("selected-overlay-000.mkv")).unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=green:s=64x64:r=24:d=1,format=yuva444p,colorchannelmixer=aa=0.75",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
            ],
            &root.join("selected-overlay-000.mkv"),
        );
        fs::remove_file(root.join("layered-picture-000.mkv")).unwrap();
        ffmpeg(
            &exe,
            &[
                "-i",
                root.join("clean-picture.mkv").to_str().unwrap(),
                "-i",
                root.join("selected-overlay-000.mkv").to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v]overlay=format=auto",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("layered-picture-000.mkv"),
        );
        render_ass("{\\t(0,1000,\\1a&HCC&)}m 0 48 l 66 48 66 66 0 66");
        let frozen = measure(&exe, root, (64, 64), 0, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(frozen["passed"], false, "{frozen}");
        assert_eq!(frozen["status"], "failed-source-temporal-expectation");
        let caption =
            measure(&exe, root, (64, 64), 1, &spans, &interval, Some((1, false))).unwrap();
        assert_eq!(caption["passed"], true, "{caption}");
    }
    #[test]
    #[ignore = "requires the retained CAIMITOS mixed consumer closure and native FFmpeg"]
    fn retained_mixed_consumer_caption_and_effect_pass_native_temporal_analysis() {
        let root = std::env::var_os("REEL_MIXED_REGRESSION_ROOT")
            .map(std::path::PathBuf::from)
            .expect("set REEL_MIXED_REGRESSION_ROOT");
        let report = analyze(
            &root.join("job.json"),
            &root.join("assets"),
            &root.join("render"),
            &root.join("expectations.json"),
        )
        .unwrap();
        if let Some(path) = std::env::var_os("REEL_MIXED_REGRESSION_REPORT") {
            let path = std::path::PathBuf::from(path);
            assert!(!path.exists(), "regression report must be new");
            fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        assert_eq!(report["passed"], true, "{report}");
        assert_eq!(report["layers"][0]["passed"], true, "{report}");
        assert_eq!(report["layers"][1]["passed"], true, "{report}");
    }
    #[test]
    #[ignore = "requires native FFmpeg"]
    fn moving_alpha_layer_and_final_occlusion_are_measured_independently() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let exe = motioncraft_cadence::resolve_analyzer().unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:r=24:d=1",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("clean-picture.mkv"),
        );
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "testsrc2=s=64x64:r=24:d=1,format=yuva444p,colorchannelmixer=aa=0.75",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
            ],
            &root.join("selected-overlay-000.mkv"),
        );
        ffmpeg(
            &exe,
            &[
                "-i",
                root.join("clean-picture.mkv").to_str().unwrap(),
                "-i",
                root.join("selected-overlay-000.mkv").to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v]overlay=format=auto",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("layered-picture-000.mkv"),
        );
        fs::copy(
            root.join("selected-overlay-000.mkv"),
            root.join("selected-overlay.mkv"),
        )
        .unwrap();
        fs::copy(
            root.join("layered-picture-000.mkv"),
            root.join("picture.mkv"),
        )
        .unwrap();
        let span = Span {
            attachment_id: "moving".into(),
            start_sample: 0,
            end_sample: 48000,
            start_frame: 0,
            end_frame: 24,
        };
        let interval = Interval {
            start_frame: 0,
            end_frame: 24,
            kind: Kind::Moving,
            region: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        };
        let row = measure(
            &exe,
            root,
            (64, 64),
            0,
            std::slice::from_ref(&span),
            &interval,
            None,
        )
        .unwrap();
        assert_eq!(row["passed"], true, "{row}");
        let late_span = Span {
            start_sample: 24000,
            start_frame: 12,
            ..span.clone()
        };
        assert_eq!(
            outside_span_mismatches(&exe, root, (64, 64, 24), 0, &[late_span], &interval).unwrap(),
            12
        );
        fs::remove_file(root.join("picture.mkv")).unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x64:r=24:d=1,format=yuva444p",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuva444p",
            ],
            &root.join("selected-overlay-001.mkv"),
        );
        ffmpeg(
            &exe,
            &[
                "-i",
                root.join("layered-picture-000.mkv").to_str().unwrap(),
                "-i",
                root.join("selected-overlay-001.mkv").to_str().unwrap(),
                "-filter_complex",
                "[0:v][1:v]overlay=format=auto",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("picture.mkv"),
        );
        let spans = vec![
            span.clone(),
            Span {
                attachment_id: "cover".into(),
                ..span
            },
        ];
        let row = measure(&exe, root, (64, 64), 0, &spans, &interval, None).unwrap();
        assert!(row["passed"].is_null(), "{row}");
        assert_eq!(row["insufficient_visibility_frames"], 24, "{row}");
        fs::remove_file(root.join("picture.mkv")).unwrap();
        fs::copy(root.join("clean-picture.mkv"), root.join("picture.mkv")).unwrap();
        let row = measure(&exe, root, (64, 64), 0, &spans, &interval, None).unwrap();
        assert_eq!(row["status"], "failed-composition-or-timing", "{row}");
    }
    #[test]
    #[ignore = "requires native FFmpeg"]
    fn hidden_motion_cannot_borrow_visibility_from_a_static_region() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path();
        let exe = motioncraft_cadence::resolve_analyzer().unwrap();
        ffmpeg(
            &exe,
            &[
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=64x64:r=24:d=1",
                "-c:v",
                "ffv1",
                "-pix_fmt",
                "yuv444p",
            ],
            &root.join("clean-picture.mkv"),
        );
        for (name, cover) in [
            ("selected-overlay-000", false),
            ("selected-overlay-001", true),
        ] {
            let mut bytes = Vec::new();
            for frame in 0..24u8 {
                for _y in 0..64 {
                    for x in 0..64 {
                        let pixel = if cover {
                            [0, 0, 255, if x < 32 { 255 } else { 0 }]
                        } else if x < 32 {
                            [frame * 10, 200, 0, 191]
                        } else if x < 48 {
                            [0, 255, 0, 191]
                        } else {
                            [0, 0, 0, 0]
                        };
                        bytes.extend_from_slice(&pixel);
                    }
                }
            }
            let raw = root.join(format!("{name}.rgba"));
            fs::write(&raw, bytes).unwrap();
            ffmpeg(
                &exe,
                &[
                    "-f",
                    "rawvideo",
                    "-pix_fmt",
                    "rgba",
                    "-s",
                    "64x64",
                    "-r",
                    "24",
                    "-i",
                    raw.to_str().unwrap(),
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "bgra",
                ],
                &root.join(format!("{name}.mkv")),
            );
        }
        for index in 0..2 {
            let input = if index == 0 {
                "clean-picture.mkv"
            } else {
                "layered-picture-000.mkv"
            };
            let output = if index == 0 {
                "layered-picture-000.mkv"
            } else {
                "picture.mkv"
            };
            ffmpeg(
                &exe,
                &[
                    "-i",
                    root.join(input).to_str().unwrap(),
                    "-i",
                    root.join(format!("selected-overlay-{index:03}.mkv"))
                        .to_str()
                        .unwrap(),
                    "-filter_complex",
                    "[0:v][1:v]overlay=format=auto",
                    "-c:v",
                    "ffv1",
                    "-pix_fmt",
                    "yuv444p",
                ],
                &root.join(output),
            );
        }
        let spans: Vec<_> = (0..2)
            .map(|index| Span {
                attachment_id: format!("layer-{index}"),
                start_sample: 0,
                end_sample: 48000,
                start_frame: 0,
                end_frame: 24,
            })
            .collect();
        let mut interval = Interval {
            start_frame: 0,
            end_frame: 24,
            kind: Kind::Moving,
            region: Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            },
        };
        let row = measure(&exe, root, (64, 64), 0, &spans, &interval, None).unwrap();
        assert_eq!(row["status"], "failed-source-temporal-expectation", "{row}");
        assert_eq!(row["stationary_fraction"], 0.0, "{row}");
        assert_eq!(row["visible_stationary_fraction"], 1.0, "{row}");
        assert_eq!(row["insufficient_visibility_frames"], 0, "{row}");
        interval.region = Rect {
            x: 0.75,
            y: 0.0,
            width: 0.25,
            height: 1.0,
        };
        let row = measure(&exe, root, (64, 64), 0, &spans, &interval, None).unwrap();
        assert!(row["passed"].is_null(), "{row}");
        assert_eq!(row["insufficient_visibility_frames"], 24, "{row}");
        interval.region = Rect {
            x: 0.5,
            y: 0.0,
            width: 0.25,
            height: 1.0,
        };
        interval.end_frame = 1;
        interval.kind = Kind::Hold;
        let row = measure(&exe, root, (64, 64), 0, &spans, &interval, None).unwrap();
        assert!(row["passed"].is_null(), "{row}");
    }
}
