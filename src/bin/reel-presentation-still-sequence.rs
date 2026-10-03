//! Render a data-driven, editable-layer still sequence for episode presentation.

use anyhow::{bail, Context, Result};
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
    cache_uri: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Template {
    schema: String,
    template_id: String,
    width: u32,
    height: u32,
    frame_rate: u32,
    sample_rate: u32,
    picture_count: u32,
    frames_per_picture: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    episode_id: String,
    language: String,
    role: String,
    template: FileRef,
    pictures: Vec<FileRef>,
    editable_layer: FileRef,
    #[serde(default)]
    audio: Option<FileRef>,
}

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: String,
    episode_id: String,
    language: String,
    role: String,
    template_id: String,
    template_sha256: String,
    manifest_sha256: String,
    picture_sha256: Vec<String>,
    editable_layer_sha256: String,
    audio_sha256: Option<String>,
    master_sha256: String,
    master_bytes: u64,
    review_sha256: String,
    review_bytes: u64,
    frames: u64,
    samples: u64,
    publication: String,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn verified(root: &Path, file: &FileRef) -> Result<(PathBuf, Vec<u8>)> {
    if file.path.is_absolute()
        || file
            .path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        bail!("file reference must be root-relative");
    }
    let path = root.join(&file.path);
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    if hash(&bytes) != file.sha256
        || bytes.len() as u64 != file.bytes
        || file.cache_uri != format!("cache://sha256/{}", file.sha256)
    {
        bail!("file hash, bytes or cache URI mismatch: {}", path.display());
    }
    Ok((fs::canonicalize(path)?, bytes))
}

fn run(mut command: Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn facts(path: &Path) -> Result<serde_json::Value> {
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

fn check_media(path: &Path, template: &Template) -> Result<(u64, u64)> {
    let streams = facts(path)?["streams"]
        .as_array()
        .context("missing streams")?
        .clone();
    let video = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .context("missing video")?;
    let audio = streams
        .iter()
        .find(|s| s["codec_type"] == "audio")
        .context("missing audio")?;
    let frames = template.picture_count as u64 * template.frames_per_picture as u64;
    let samples = frames * template.sample_rate as u64 / template.frame_rate as u64;
    if video["codec_name"] != "ffv1"
        || video["width"] != template.width
        || video["height"] != template.height
        || video["nb_read_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(frames)
        || audio["codec_name"] != "pcm_s24le"
        || audio["sample_rate"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            != Some(template.sample_rate)
        || audio["channels"] != 2
    {
        bail!("presentation picture/audio format or frame count mismatch");
    }
    let decoded = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "s24le", "-acodec", "pcm_s24le", "-"])
        .output()?;
    if !decoded.status.success() || decoded.stdout.len() as u64 != samples * 6 {
        bail!("presentation audio sample count mismatch");
    }
    run({
        let mut c = Command::new("ffmpeg");
        c.args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "null", "-"]);
        c
    })?;
    Ok((frames, samples))
}

fn load(
    manifest_path: &Path,
    root: &Path,
) -> Result<(Manifest, Template, Vec<PathBuf>, Option<PathBuf>, Vec<u8>)> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if manifest.schema != "reel.presentation-still-sequence.v1"
        || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.episode_id.is_empty()
        || manifest.role.is_empty()
    {
        bail!("invalid presentation manifest");
    }
    let (_, template_bytes) = verified(root, &manifest.template)?;
    let template: Template = serde_json::from_slice(&template_bytes)?;
    if template.schema != "reel.presentation-still-template.v1"
        || template.template_id.is_empty()
        || template.width == 0
        || template.height == 0
        || !(1..=120).contains(&template.frame_rate)
        || !(8_000..=192_000).contains(&template.sample_rate)
        || !(1..=120).contains(&template.picture_count)
        || template.frames_per_picture == 0
        || manifest.pictures.len() != template.picture_count as usize
        || template.picture_count as u64 * template.frames_per_picture as u64
            > 120 * template.frame_rate as u64
        || (template.picture_count as u64
            * template.frames_per_picture as u64
            * template.sample_rate as u64)
            % template.frame_rate as u64
            != 0
    {
        bail!("invalid presentation still template or picture count");
    }
    verified(root, &manifest.editable_layer)?;
    let pictures = manifest
        .pictures
        .iter()
        .map(|item| verified(root, item).map(|x| x.0))
        .collect::<Result<Vec<_>>>()?;
    let audio = manifest
        .audio
        .as_ref()
        .map(|item| verified(root, item).map(|x| x.0))
        .transpose()?;
    Ok((manifest, template, pictures, audio, manifest_bytes))
}

fn build(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        bail!("output directory already exists");
    }
    let root = fs::canonicalize(root)?;
    let (manifest, template, pictures, audio, manifest_bytes) = load(manifest_path, &root)?;
    fs::create_dir_all(output_dir)?;
    let output_dir = fs::canonicalize(output_dir)?;
    fs::copy(
        root.join(&manifest.editable_layer.path),
        output_dir.join("editable-layer.ass"),
    )?;
    let seconds_each = template.frames_per_picture as f64 / template.frame_rate as f64;
    let total_seconds = template.picture_count as f64 * seconds_each;
    let mut cmd = Command::new("ffmpeg");
    cmd.current_dir(&output_dir).args(["-y", "-v", "error"]);
    for picture in &pictures {
        cmd.args([
            "-loop",
            "1",
            "-framerate",
            &template.frame_rate.to_string(),
            "-t",
            &format!("{seconds_each:.9}"),
            "-i",
        ])
        .arg(picture);
    }
    if let Some(source) = &audio {
        cmd.arg("-i").arg(source);
    } else {
        cmd.args(["-f", "lavfi", "-i"])
            .arg(format!("anullsrc=r={}:cl=stereo", template.sample_rate));
    }
    let mut filter = String::new();
    for i in 0..pictures.len() {
        filter.push_str(&format!(
            "[{i}:v]scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,format=yuv444p,trim=duration={seconds_each:.9},setpts=PTS-STARTPTS[v{i}];",
            template.width, template.height, template.width, template.height
        ));
    }
    for i in 0..pictures.len() {
        filter.push_str(&format!("[v{i}]"));
    }
    filter.push_str(&format!(
        "concat=n={}:v=1:a=0,ass=editable-layer.ass[v];",
        pictures.len()
    ));
    filter.push_str(&format!(
        "[{}:a]aresample={},aformat=channel_layouts=stereo,apad,atrim=duration={total_seconds:.9},asetpts=PTS-STARTPTS[a]",
        pictures.len(), template.sample_rate
    ));
    cmd.args([
        "-filter_complex",
        &filter,
        "-map",
        "[v]",
        "-map",
        "[a]",
        "-frames:v",
        &(template.picture_count * template.frames_per_picture).to_string(),
        "-c:v",
        "ffv1",
        "-pix_fmt",
        "yuv444p",
        "-c:a",
        "pcm_s24le",
        "-ar",
        &template.sample_rate.to_string(),
        "-ac",
        "2",
    ])
    .arg(output_dir.join("master.mkv"));
    run(cmd)?;
    let (frames, samples) = check_media(&output_dir.join("master.mkv"), &template)?;
    run({
        let mut c = Command::new("ffmpeg");
        c.args(["-y", "-v", "error", "-i"])
            .arg(output_dir.join("master.mkv"))
            .args([
                "-vf",
                "scale=448:252",
                "-c:v",
                "libx264",
                "-preset",
                "medium",
                "-crf",
                "18",
                "-pix_fmt",
                "yuv420p",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-movflags",
                "+faststart",
            ])
            .arg(output_dir.join("review.mp4"));
        c
    })?;
    let master = fs::read(output_dir.join("master.mkv"))?;
    let review = fs::read(output_dir.join("review.mp4"))?;
    let receipt = Receipt {
        schema: "reel.presentation-still-sequence-receipt.v1".into(),
        episode_id: manifest.episode_id,
        language: manifest.language,
        role: manifest.role,
        template_id: template.template_id,
        template_sha256: manifest.template.sha256,
        manifest_sha256: hash(&manifest_bytes),
        picture_sha256: manifest.pictures.iter().map(|p| p.sha256.clone()).collect(),
        editable_layer_sha256: manifest.editable_layer.sha256,
        audio_sha256: manifest.audio.map(|a| a.sha256),
        master_sha256: hash(&master),
        master_bytes: master.len() as u64,
        review_sha256: hash(&review),
        review_bytes: review.len() as u64,
        frames,
        samples,
        publication: "not-authorized".into(),
    };
    fs::write(
        output_dir.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!("{} {} {} frames", receipt.role, receipt.language, frames);
    Ok(())
}

fn check(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let (manifest, template, _, _, manifest_bytes) = load(manifest_path, &root)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(output_dir.join("receipt.json"))?)?;
    let master = fs::read(output_dir.join("master.mkv"))?;
    let review = fs::read(output_dir.join("review.mp4"))?;
    let layer = fs::read(output_dir.join("editable-layer.ass"))?;
    let (frames, samples) = check_media(&output_dir.join("master.mkv"), &template)?;
    if receipt.schema != "reel.presentation-still-sequence-receipt.v1"
        || receipt.manifest_sha256 != hash(&manifest_bytes)
        || receipt.template_sha256 != manifest.template.sha256
        || receipt.picture_sha256
            != manifest
                .pictures
                .iter()
                .map(|p| p.sha256.clone())
                .collect::<Vec<_>>()
        || receipt.editable_layer_sha256 != hash(&layer)
        || receipt.audio_sha256 != manifest.audio.map(|a| a.sha256)
        || receipt.master_sha256 != hash(&master)
        || receipt.master_bytes != master.len() as u64
        || receipt.review_sha256 != hash(&review)
        || receipt.review_bytes != review.len() as u64
        || receipt.frames != frames
        || receipt.samples != samples
    {
        bail!("presentation still receipt/output mismatch");
    }
    run({
        let mut c = Command::new("ffmpeg");
        c.args(["-v", "error", "-i"])
            .arg(output_dir.join("review.mp4"))
            .args(["-f", "null", "-"]);
        c
    })?;
    println!("presentation still sequence verified");
    Ok(())
}

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let result = (|| -> Result<()> {
        if let [_, action, manifest, root_flag, root, output_flag, output] = args.as_slice() {
            if root_flag != "--root" || output_flag != "--output-dir" {
                bail!("invalid flags");
            }
            match action.as_str() {
                "build" => build(Path::new(manifest), Path::new(root), Path::new(output)),
                "check" => check(Path::new(manifest), Path::new(root), Path::new(output)),
                _ => bail!("expected build or check"),
            }
        } else {
            bail!("usage: reel-presentation-still-sequence <build|check> <manifest> --root <root> --output-dir <dir>");
        }
    })();
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}
