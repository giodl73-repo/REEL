//! Indexed evidence from actual scene-engine picture output, never seek guesses.
use crate::scene_delivery::{Job, PictureMotion, Plan};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    process::Command,
};

pub fn samples(job: &Job, plan: &Plan) -> Result<BTreeMap<u64, BTreeSet<String>>> {
    let mut samples: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for (picture, span) in job.pictures.iter().zip(&plan.pictures) {
        if let Some(PictureMotion::PhasedCamera { plan: motion }) = &picture.motion {
            for sample in &motion.samples {
                let frame = span
                    .start_frame
                    .checked_add(sample.frame)
                    .context("review frame overflow")?;
                if frame >= span.end_frame || frame >= plan.frame_count {
                    bail!("motion evidence frame outside rendered span");
                }
                samples.entry(frame).or_default().extend(
                    sample
                        .reasons
                        .iter()
                        .map(|reason| format!("{}:{reason}", picture.attachment_id)),
                );
            }
        }
    }
    if samples.len() > 256
        || (samples.len() as u128) * u128::from(job.width) * u128::from(job.height) * 3
            > 512 * 1024 * 1024
    {
        bail!("motion evidence exceeds bounded extraction budget");
    }
    Ok(samples)
}

pub fn output_names(job: &Job, plan: &Plan) -> Result<Vec<String>> {
    let samples = samples(job, plan)?;
    if samples.is_empty() {
        return Ok(vec![]);
    }
    let mut names = samples
        .keys()
        .map(|frame| format!("motioncraft/frame-{frame:08}.png"))
        .collect::<Vec<_>>();
    names.extend(
        [
            "motioncraft/evidence.json",
            "motioncraft/contact-sheet.png",
            "motioncraft/quarter-speed.mp4",
        ]
        .map(String::from),
    );
    Ok(names)
}

fn metadata(job: &Job, plan: &Plan, root: &Path) -> Result<Value> {
    let version = Command::new("ffmpeg").arg("-version").output()?;
    if !version.status.success() {
        bail!("FFmpeg version unavailable");
    }
    Ok(json!({
        "schema":"reel.motioncraft-review.v1", "job_sha256":plan.job_sha256,
        "contract_sha256":plan.contract_sha256,"production_sha256":plan.production_sha256,
        "picture_sha256":crate::sha256_file(&root.join("picture.mkv"))?,
        "selected_picture_inputs":job.pictures.iter().map(|picture|json!({
            "attachment_id":picture.attachment_id,"sha256":picture.source.sha256,"bytes":picture.source.bytes
        })).collect::<Vec<_>>(),
        "camera_direction":job.pictures.iter().filter_map(|picture|match &picture.motion {
            Some(PictureMotion::PhasedCamera { plan:motion })=>Some(json!({
                "attachment_id":picture.attachment_id,
                "authored_direction":motion.direction,
                "execution_direction":motion.execution_direction.as_ref().unwrap_or(&motion.direction),
                "delivery_safe_area":motion.safe_area,
                "native_duration_samples":motion.duration_samples,
                "working_duration_residual_numerator":motion.duration_residual_numerator
            })),_=>None
        }).collect::<Vec<_>>(),
        "tool_version":env!("CARGO_PKG_VERSION"),
        "ffmpeg_version":String::from_utf8_lossy(&version.stdout).lines().next().unwrap_or_default(),
        "frames":samples(job,plan)?.into_iter().map(|(frame,reasons)|json!({
            "frame_index":frame,"timestamp_numerator":u128::from(frame)*u128::from(plan.fps_denominator),
            "timestamp_denominator":plan.fps_numerator,"reasons":reasons,
            "path":format!("frame-{frame:08}.png")
        })).collect::<Vec<_>>(),
        "scope":"sampled picture evidence; not exhaustive inspection or creative approval",
        "quarter_speed_audio":"intentionally omitted; review.mp4 retains native audio"
    }))
}

fn selected_pixels(job: &Job, plan: &Plan, root: &Path) -> Result<Vec<u8>> {
    let samples = samples(job, plan)?;
    let select = samples
        .keys()
        .map(|frame| format!("eq(n\\,{frame})"))
        .collect::<Vec<_>>()
        .join("+");
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-nostdin", "-i"])
        .arg(root.join("picture.mkv"))
        .args([
            "-vf",
            &format!("select={select}"),
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()?;
    if !output.status.success() {
        bail!(
            "motion evidence decode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let expected = samples.len() as u64 * u64::from(job.width) * u64::from(job.height) * 3;
    if output.stdout.len() as u64 != expected {
        bail!("motion evidence selected frame count mismatch");
    }
    Ok(output.stdout)
}

pub fn render(job: &Job, plan: &Plan, root: &Path) -> Result<()> {
    let samples = samples(job, plan)?;
    if samples.is_empty() {
        return Ok(());
    }
    let directory = root.join("motioncraft");
    fs::create_dir(&directory)?;
    let pixels = selected_pixels(job, plan, root)?;
    let stride = job.width as usize * job.height as usize * 3;
    let thumb_width =
        ((320u64 * u64::from(job.width)) / u64::from(job.width.max(job.height))).max(1) as u32;
    let thumb_height =
        ((320u64 * u64::from(job.height)) / u64::from(job.width.max(job.height))).max(1) as u32;
    let rows = (samples.len() as u32).div_ceil(4);
    let mut contact = image::RgbImage::new(4 * thumb_width, rows * thumb_height);
    for (index, frame) in samples.keys().enumerate() {
        let source = image::RgbImage::from_raw(
            job.width,
            job.height,
            pixels[index * stride..(index + 1) * stride].to_vec(),
        )
        .context("motion evidence RGB geometry mismatch")?;
        source.save(directory.join(format!("frame-{frame:08}.png")))?;
        let thumbnail = image::imageops::resize(
            &source,
            thumb_width,
            thumb_height,
            image::imageops::FilterType::Triangle,
        );
        image::imageops::replace(
            &mut contact,
            &thumbnail,
            (index as i64 % 4) * i64::from(thumb_width),
            (index as i64 / 4) * i64::from(thumb_height),
        );
    }
    contact.save(directory.join("contact-sheet.png"))?;
    fs::write(
        directory.join("evidence.json"),
        serde_json::to_vec_pretty(&metadata(job, plan, root)?)?,
    )?;
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-nostdin", "-i"])
        .arg(root.join("picture.mkv"))
        .args([
            "-vf",
            "setpts=4*PTS",
            "-an",
            "-fps_mode",
            "vfr",
            "-c:v",
            "libx264",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
        ])
        .arg(directory.join("quarter-speed.mp4"))
        .output()?;
    if !output.status.success() {
        bail!(
            "quarter-speed evidence render failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

pub fn check(job: &Job, plan: &Plan, root: &Path, decode: bool) -> Result<()> {
    let samples = samples(job, plan)?;
    if samples.is_empty() {
        return Ok(());
    }
    let report: Value = serde_json::from_slice(&fs::read(root.join("motioncraft/evidence.json"))?)?;
    let mut expected = metadata(job, plan, root)?;
    // Producer provenance is retained across a consumer runtime upgrade. The
    // enclosing delivery receipt pins these report bytes; current decoding
    // still independently verifies indexed pixels.
    for field in ["tool_version", "ffmpeg_version"] {
        if report[field]
            .as_str()
            .is_none_or(|value| value.trim().is_empty())
        {
            bail!("motion review lacks producer tool identity");
        }
        expected[field] = report[field].clone();
    }
    if report != expected {
        bail!("motion review metadata differs from selected scene");
    }
    if !decode {
        return Ok(());
    }
    let pixels = selected_pixels(job, plan, root)?;
    let stride = job.width as usize * job.height as usize * 3;
    for (index, frame) in samples.keys().enumerate() {
        let saved = image::open(root.join(format!("motioncraft/frame-{frame:08}.png")))?.to_rgb8();
        if saved.width() != job.width
            || saved.height() != job.height
            || saved.as_raw().as_slice() != &pixels[index * stride..(index + 1) * stride]
        {
            bail!("motion review pixels differ from rendered frame {frame}");
        }
    }
    Ok(())
}
