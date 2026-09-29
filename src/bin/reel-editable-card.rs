//! Render a cue-free, fixed-duration editable presentation card from a selected
//! REEL text template and its compiled ASS layer. Layout lives in the template.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRef {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    scene_id: String,
    episode_id: String,
    language: String,
    role: String,
    template: FileRef,
    source_text: FileRef,
    layer: FileRef,
    layer_receipt: FileRef,
    background_rgb: [u8; 3],
    frame_rate: u32,
    sample_rate: u32,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: String,
    scene_id: String,
    episode_id: String,
    language: String,
    role: String,
    template_id: String,
    template_definition_sha256: String,
    source_text_sha256: String,
    layer_sha256: String,
    layer_receipt_sha256: String,
    manifest_sha256: String,
    master_sha256: String,
    master_bytes: u64,
    review_sha256: String,
    review_bytes: u64,
    frames: u64,
    samples: u64,
    publication: String,
    technical_validation_state: String,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn read_verified(root: &Path, reference: &FileRef) -> Result<Vec<u8>> {
    let path = root.join(&reference.path);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if reference.bytes != bytes.len() as u64 || reference.sha256 != hash(&bytes) {
        bail!("input hash/bytes mismatch: {}", path.display());
    }
    Ok(bytes)
}

fn command(mut cmd: Command) -> Result<()> {
    let output = cmd.output()?;
    if !output.status.success() {
        bail!(
            "media command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn probe(path: &Path) -> Result<serde_json::Value> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-count_frames",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !output.status.success() {
        bail!("ffprobe failed: {}", path.display());
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn validate_media(
    path: &Path,
    width: u64,
    height: u64,
    rate: u32,
    frames: u64,
    samples: u64,
) -> Result<()> {
    let facts = probe(path)?;
    let streams = facts["streams"]
        .as_array()
        .context("missing media streams")?;
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("missing picture")?;
    let audio = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .context("missing audio")?;
    if video["width"] != width
        || video["height"] != height
        || video["nb_read_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(frames)
        || audio["sample_rate"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            != Some(rate)
        || audio["channels"] != 2
    {
        bail!("card media geometry/clock mismatch");
    }
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "s24le", "-acodec", "pcm_s24le", "-"])
        .output()?;
    if !decoded.status.success() || decoded.stdout.len() as u64 != samples * 6 {
        bail!("card audio sample count mismatch");
    }
    command({
        let mut c = Command::new("ffmpeg");
        c.args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "null", "-"]);
        c
    })?;
    Ok(())
}

fn build(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        bail!("output directory already exists");
    }
    let manifest_bytes = fs::read(manifest_path)?;
    let m: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if m.schema != "reel.editable-card-render.v1"
        || !(1..=120).contains(&m.frame_rate)
        || !(8_000..=192_000).contains(&m.sample_rate)
        || !matches!(m.language.as_str(), "es" | "en")
        || m.scene_id.is_empty()
        || m.role.is_empty()
    {
        bail!("invalid editable card manifest");
    }
    let template_bytes = read_verified(root, &m.template)?;
    let source_bytes = read_verified(root, &m.source_text)?;
    let layer_bytes = read_verified(root, &m.layer)?;
    let receipt_bytes = read_verified(root, &m.layer_receipt)?;
    let template: serde_json::Value = serde_json::from_slice(&template_bytes)?;
    let layer_receipt: serde_json::Value = serde_json::from_slice(&receipt_bytes)?;
    if template["schema"] != "reel.editable-text-template.v1"
        || template["kind"] != "chapter-title"
        || template["chapter"].is_null()
        || layer_receipt["schema"] != "reel.editable-layer-compile-receipt.v1"
        || layer_receipt["scene_id"] != m.scene_id
        || layer_receipt["language"] != m.language
        || layer_receipt["template_id"] != template["template_id"]
        || layer_receipt["template_definition_sha256"] != m.template.sha256
        || layer_receipt["source_text_sha256"] != hash(&source_bytes)
        || layer_receipt["ass_sha256"] != hash(&layer_bytes)
        || layer_receipt["ass_bytes"] != layer_bytes.len() as u64
    {
        bail!("editable card layer/template/source closure mismatch");
    }
    let width = template["canvas_width"]
        .as_u64()
        .context("template width")?;
    let height = template["canvas_height"]
        .as_u64()
        .context("template height")?;
    let duration = template["fixed_duration_seconds"]
        .as_u64()
        .context("fixed card duration")?;
    if width == 0 || height == 0 || duration == 0 || duration > 30 {
        bail!("invalid fixed card geometry/duration");
    }
    let frames = duration * u64::from(m.frame_rate);
    let samples = duration * u64::from(m.sample_rate);
    fs::create_dir_all(output_dir)?;
    let output_dir = fs::canonicalize(output_dir)?;
    fs::write(output_dir.join("layer.ass"), layer_bytes)?;
    let color = format!(
        "0x{:02x}{:02x}{:02x}",
        m.background_rgb[0], m.background_rgb[1], m.background_rgb[2]
    );
    let master = output_dir.join("master.mkv");
    command({
        let mut c = Command::new("ffmpeg");
        c.current_dir(&output_dir)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
            ])
            .arg(format!(
                "color=c={color}:s={width}x{height}:r={}:d={duration}",
                m.frame_rate
            ))
            .args(["-f", "lavfi", "-i"])
            .arg(format!("anullsrc=r={}:cl=stereo", m.sample_rate))
            .args(["-vf", "ass=layer.ass", "-frames:v"])
            .arg(frames.to_string())
            .args(["-t"])
            .arg(duration.to_string())
            .args([
                "-c:v",
                "ffv1",
                "-level",
                "3",
                "-g",
                "1",
                "-pix_fmt",
                "yuv444p",
                "-c:a",
                "pcm_s24le",
                "-ar",
            ])
            .arg(m.sample_rate.to_string())
            .args(["-ac", "2"])
            .arg(&master);
        c
    })?;
    validate_media(&master, width, height, m.sample_rate, frames, samples)?;
    let review = output_dir.join("review.mp4");
    command({
        let mut c = Command::new("ffmpeg");
        c.args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&master)
            .args([
                "-vf",
                "scale=448:252",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ar",
            ])
            .arg(m.sample_rate.to_string())
            .args(["-movflags", "+faststart"])
            .arg(&review);
        c
    })?;
    let master_bytes = fs::read(&master)?;
    let review_bytes = fs::read(&review)?;
    let receipt = Receipt {
        schema: "reel.presentation-master-receipt.v1".into(),
        scene_id: m.scene_id,
        episode_id: m.episode_id,
        language: m.language,
        role: m.role,
        template_id: template["template_id"].as_str().unwrap().into(),
        template_definition_sha256: m.template.sha256,
        source_text_sha256: m.source_text.sha256,
        layer_sha256: m.layer.sha256,
        layer_receipt_sha256: m.layer_receipt.sha256,
        manifest_sha256: hash(&manifest_bytes),
        master_sha256: hash(&master_bytes),
        master_bytes: master_bytes.len() as u64,
        review_sha256: hash(&review_bytes),
        review_bytes: review_bytes.len() as u64,
        frames,
        samples,
        publication: "not-authorized".into(),
        technical_validation_state: "rendered-and-decoded".into(),
    };
    fs::write(
        output_dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("{} {} frames {} samples", receipt.language, frames, samples);
    Ok(())
}

fn check(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    let m: Manifest = serde_json::from_slice(&fs::read(manifest_path)?)?;
    read_verified(root, &m.template)?;
    read_verified(root, &m.source_text)?;
    read_verified(root, &m.layer)?;
    read_verified(root, &m.layer_receipt)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(output_dir.join("receipt.json"))?)?;
    let master = output_dir.join("master.mkv");
    let review = output_dir.join("review.mp4");
    let master_bytes = fs::read(&master)?;
    let review_bytes = fs::read(&review)?;
    if receipt.schema != "reel.presentation-master-receipt.v1"
        || receipt.manifest_sha256 != hash(&fs::read(manifest_path)?)
        || receipt.language != m.language
        || receipt.role != m.role
        || receipt.scene_id != m.scene_id
        || receipt.master_sha256 != hash(&master_bytes)
        || receipt.master_bytes != master_bytes.len() as u64
        || receipt.review_sha256 != hash(&review_bytes)
        || receipt.review_bytes != review_bytes.len() as u64
    {
        bail!("card receipt/output closure mismatch");
    }
    let template: serde_json::Value = serde_json::from_slice(&read_verified(root, &m.template)?)?;
    validate_media(
        &master,
        template["canvas_width"].as_u64().unwrap(),
        template["canvas_height"].as_u64().unwrap(),
        m.sample_rate,
        receipt.frames,
        receipt.samples,
    )?;
    command({
        let mut c = Command::new("ffmpeg");
        c.args(["-v", "error", "-i"])
            .arg(&review)
            .args(["-f", "null", "-"]);
        c
    })?;
    println!("editable card verified");
    Ok(())
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result =
        if let [_, action, manifest, root_flag, root, output_flag, output] = args.as_slice() {
            if root_flag == "--root" && output_flag == "--output-dir" {
                match action.as_str() {
                    "build" => build(Path::new(manifest), Path::new(root), Path::new(output)),
                    "check" => check(Path::new(manifest), Path::new(root), Path::new(output)),
                    _ => usage(),
                }
            } else {
                usage()
            }
        } else {
            usage()
        };
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

fn usage() -> Result<()> {
    bail!(
        "usage: reel-editable-card <build|check> <manifest.json> --root <input-root> --output-dir <dir>"
    )
}
