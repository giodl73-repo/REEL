//! Render a data-driven, editable-layer still sequence for episode presentation.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
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
    #[serde(default)]
    frames_per_picture: u32,
    #[serde(default)]
    total_frames: Option<u64>,
    #[serde(default)]
    audio_required: bool,
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
    #[serde(default)]
    picture_timing: Option<FileRef>,
    editable_layer: FileRef,
    #[serde(default)]
    audio: Option<FileRef>,
    #[serde(default)]
    audio_treatment: Option<AudioTreatment>,
    #[serde(default)]
    fonts: Vec<FileRef>,
}

/// Shared picture cuts refer to semantic IDs on one continuous source clock.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PictureTiming {
    schema: String,
    clock: FileRef,
    spans: Vec<PictureSpan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PictureSpan {
    start_anchor: String,
    end_anchor: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceClock {
    schema: String,
    source_sha256: String,
    sample_rate: u32,
    total_samples: u64,
    /// Evidence is retained and hashed, not interpreted as human approval.
    evidence: FileRef,
    anchors: Vec<ClockAnchor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockAnchor {
    id: String,
    sample: u64,
}

impl Template {
    fn frames(&self) -> u64 {
        self.total_frames
            .unwrap_or(self.picture_count as u64 * self.frames_per_picture as u64)
    }
}

fn compile_picture_frames(
    timing: &PictureTiming,
    clock: &SourceClock,
    template: &Template,
    source_sha256: &str,
) -> Result<Vec<u64>> {
    let frames = template.frames();
    let samples = frames * template.sample_rate as u64 / template.frame_rate as u64;
    if timing.schema != "reel.presentation-picture-timing.v1"
        || clock.schema != "reel.presentation-source-clock.v1"
        || clock.source_sha256 != source_sha256
        || clock.sample_rate != template.sample_rate
        || clock.total_samples != samples
        || timing.spans.len() != template.picture_count as usize
    {
        bail!("picture timing/source clock does not match selected audio and template");
    }
    let mut anchors = BTreeMap::new();
    for anchor in &clock.anchors {
        if anchor.id.trim().is_empty()
            || anchor.sample > samples
            || anchors.insert(anchor.id.as_str(), anchor.sample).is_some()
        {
            bail!("invalid or duplicate source clock anchor");
        }
    }
    let mut cursor = 0;
    let mut previous_sample = 0;
    let mut counts = Vec::new();
    for span in &timing.spans {
        let start = *anchors
            .get(span.start_anchor.as_str())
            .context("unknown picture start anchor")?;
        let end = *anchors
            .get(span.end_anchor.as_str())
            .context("unknown picture end anchor")?;
        if start != previous_sample || end <= start {
            bail!("picture source spans must be ordered, positive and gapless");
        }
        // One rounding of shared boundaries prevents independent-duration drift.
        let end_frame = ((end as u128 * template.frame_rate as u128
            + template.sample_rate as u128 / 2)
            / template.sample_rate as u128) as u64;
        if end_frame <= cursor {
            bail!("picture span collapses at the selected frame rate");
        }
        counts.push(end_frame - cursor);
        cursor = end_frame;
        previous_sample = end;
    }
    if previous_sample != samples || cursor != frames {
        bail!("picture source spans must close the entire selected audio clock");
    }
    Ok(counts)
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct AudioTreatment {
    #[serde(default)]
    gain_db: f64,
    #[serde(default)]
    fade_in_samples: u64,
    #[serde(default)]
    fade_out_samples: u64,
}

fn audio_envelope(treatment: Option<&AudioTreatment>, samples: u64) -> Result<String> {
    let Some(t) = treatment else {
        return Ok(String::new());
    };
    if !t.gain_db.is_finite()
        || !(-96.0..=12.0).contains(&t.gain_db)
        || t.fade_in_samples
            .checked_add(t.fade_out_samples)
            .is_none_or(|n| n > samples)
    {
        bail!("audio treatment exceeds bounded gain or presentation clock");
    }
    let mut filter = format!(",volume={}dB", t.gain_db);
    if t.fade_in_samples > 0 {
        filter.push_str(&format!(",afade=t=in:ss=0:ns={}", t.fade_in_samples));
    }
    if t.fade_out_samples > 0 {
        filter.push_str(&format!(
            ",afade=t=out:ss={}:ns={}",
            samples - t.fade_out_samples,
            t.fade_out_samples
        ));
    }
    Ok(filter)
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
    #[serde(default)]
    audio_treatment: Option<AudioTreatment>,
    #[serde(default)]
    font_sha256: Vec<String>,
    master_sha256: String,
    master_bytes: u64,
    review_sha256: String,
    review_bytes: u64,
    frames: u64,
    samples: u64,
    #[serde(default)]
    picture_frames: Vec<u64>,
    #[serde(default)]
    picture_timing_sha256: Option<String>,
    publication: String,
}

type LoadedPresentation = (
    Manifest,
    Template,
    Vec<PathBuf>,
    Option<PathBuf>,
    Vec<u8>,
    Vec<u64>,
);

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
    let frames = template.frames();
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

fn load(manifest_path: &Path, root: &Path) -> Result<LoadedPresentation> {
    let manifest_bytes = fs::read(manifest_path)?;
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    if !matches!(
        manifest.schema.as_str(),
        "reel.presentation-still-sequence.v1" | "reel.presentation-still-sequence.v2"
    ) || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.episode_id.is_empty()
        || manifest.role.is_empty()
    {
        bail!("invalid presentation manifest");
    }
    let (_, template_bytes) = verified(root, &manifest.template)?;
    let template: Template = serde_json::from_slice(&template_bytes)?;
    let v2 = manifest.schema == "reel.presentation-still-sequence.v2";
    if template.schema
        != if v2 {
            "reel.presentation-still-template.v2"
        } else {
            "reel.presentation-still-template.v1"
        }
        || template.template_id.is_empty()
        || template.width == 0
        || template.height == 0
        || !(1..=120).contains(&template.frame_rate)
        || !(8_000..=192_000).contains(&template.sample_rate)
        || !(1..=120).contains(&template.picture_count)
        || (v2
            && (template.total_frames.is_none()
                || template.frames_per_picture != 0
                || manifest.picture_timing.is_none()
                || manifest.audio.is_none()))
        || (!v2
            && (template.frames_per_picture == 0
                || template.total_frames.is_some()
                || manifest.picture_timing.is_some()))
        || manifest.pictures.len() != template.picture_count as usize
        || template.frames() == 0
        || template.frames() > 120 * template.frame_rate as u64
        || (template.frames() * template.sample_rate as u64) % template.frame_rate as u64 != 0
    {
        bail!("invalid presentation still template or picture count");
    }
    verified(root, &manifest.editable_layer)?;
    for font in &manifest.fonts {
        verified(root, font)?;
    }
    if manifest.audio_treatment.is_some() && manifest.audio.is_none() {
        bail!("audio treatment requires a selected audio source");
    }
    if template.audio_required && manifest.audio.is_none() {
        bail!("presentation template requires a selected audio source");
    }
    let samples = template.frames() * template.sample_rate as u64 / template.frame_rate as u64;
    audio_envelope(manifest.audio_treatment.as_ref(), samples)?;
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
    let picture_frames = if let Some(timing_ref) = &manifest.picture_timing {
        let (_, bytes) = verified(root, timing_ref)?;
        let timing: PictureTiming = serde_json::from_slice(&bytes)?;
        let (_, clock_bytes) = verified(root, &timing.clock)?;
        let clock: SourceClock = serde_json::from_slice(&clock_bytes)?;
        verified(root, &clock.evidence)?;
        let audio_ref = manifest
            .audio
            .as_ref()
            .context("source clock requires selected audio")?;
        let source = audio.as_ref().context("missing source audio")?;
        let source_facts = facts(source)?;
        let stream = source_facts["streams"]
            .as_array()
            .context("missing audio streams")?
            .iter()
            .find(|s| s["codec_type"] == "audio")
            .context("missing audio source stream")?;
        if stream["sample_rate"]
            .as_str()
            .and_then(|s| s.parse::<u32>().ok())
            != Some(template.sample_rate)
            || stream["channels"] != 2
        {
            bail!("source-clock audio must have the selected sample rate and stereo channels");
        }
        let decoded = Command::new("ffmpeg")
            .args(["-v", "error", "-i"])
            .arg(source)
            .args(["-map", "0:a:0", "-f", "s24le", "-acodec", "pcm_s24le", "-"])
            .output()?;
        if !decoded.status.success() || decoded.stdout.len() as u64 != samples * 6 {
            bail!("source-clock audio decoded length differs from the complete clock");
        }
        compile_picture_frames(&timing, &clock, &template, &audio_ref.sha256)?
    } else {
        vec![template.frames_per_picture as u64; pictures.len()]
    };
    Ok((
        manifest,
        template,
        pictures,
        audio,
        manifest_bytes,
        picture_frames,
    ))
}

fn build(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        bail!("output directory already exists");
    }
    let root = fs::canonicalize(root)?;
    let (manifest, template, pictures, audio, manifest_bytes, picture_frames) =
        load(manifest_path, &root)?;
    fs::create_dir_all(output_dir)?;
    let output_dir = fs::canonicalize(output_dir)?;
    fs::copy(
        root.join(&manifest.editable_layer.path),
        output_dir.join("editable-layer.ass"),
    )?;
    if !manifest.fonts.is_empty() {
        fs::create_dir(output_dir.join("fonts"))?;
        for (index, font) in manifest.fonts.iter().enumerate() {
            fs::copy(
                root.join(&font.path),
                output_dir.join(format!("fonts/font-{index}.ttf")),
            )?;
        }
    }
    let total_seconds = template.frames() as f64 / template.frame_rate as f64;
    let mut cmd = Command::new("ffmpeg");
    cmd.current_dir(&output_dir).args(["-y", "-v", "error"]);
    for (picture, frames) in pictures.iter().zip(&picture_frames) {
        let seconds_each = *frames as f64 / template.frame_rate as f64;
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
    for (i, frames) in picture_frames.iter().enumerate() {
        filter.push_str(&format!(
            "[{i}:v]scale=w='round(min({},{}*dar))':h='round(min({},{} / dar))',setsar=1,pad={}:{}:(ow-iw)/2:(oh-ih)/2,format=yuv444p,trim=end_frame={frames},setpts=PTS-STARTPTS[v{i}];",
            template.width, template.height, template.height, template.width, template.width, template.height
        ));
    }
    for i in 0..pictures.len() {
        filter.push_str(&format!("[v{i}]"));
    }
    filter.push_str(&format!(
        "concat=n={}:v=1:a=0,ass=editable-layer.ass{}[v];",
        pictures.len(),
        if manifest.fonts.is_empty() {
            ""
        } else {
            ":fontsdir=fonts"
        }
    ));
    let samples = template.frames() * template.sample_rate as u64 / template.frame_rate as u64;
    let envelope = audio_envelope(manifest.audio_treatment.as_ref(), samples)?;
    filter.push_str(&format!(
        "[{}:a]aresample={},aformat=channel_layouts=stereo,apad,atrim=duration={total_seconds:.9},asetpts=PTS-STARTPTS{envelope}[a]",
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
        &template.frames().to_string(),
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
        audio_treatment: manifest.audio_treatment,
        font_sha256: manifest
            .fonts
            .iter()
            .map(|font| font.sha256.clone())
            .collect(),
        master_sha256: hash(&master),
        master_bytes: master.len() as u64,
        review_sha256: hash(&review),
        review_bytes: review.len() as u64,
        frames,
        samples,
        picture_frames,
        picture_timing_sha256: manifest.picture_timing.map(|t| t.sha256),
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
    let (manifest, template, _, _, manifest_bytes, picture_frames) = load(manifest_path, &root)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(output_dir.join("receipt.json"))?)?;
    let master = fs::read(output_dir.join("master.mkv"))?;
    let review = fs::read(output_dir.join("review.mp4"))?;
    let layer = fs::read(output_dir.join("editable-layer.ass"))?;
    for (index, font) in manifest.fonts.iter().enumerate() {
        let bytes = fs::read(output_dir.join(format!("fonts/font-{index}.ttf")))?;
        if hash(&bytes) != font.sha256 || bytes.len() as u64 != font.bytes {
            bail!("retained font differs from selected font");
        }
    }
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
        || receipt.audio_treatment != manifest.audio_treatment
        || receipt.font_sha256
            != manifest
                .fonts
                .iter()
                .map(|font| font.sha256.clone())
                .collect::<Vec<_>>()
        || receipt.master_sha256 != hash(&master)
        || receipt.master_bytes != master.len() as u64
        || receipt.review_sha256 != hash(&review)
        || receipt.review_bytes != review.len() as u64
        || receipt.frames != frames
        || receipt.samples != samples
        || ((manifest.schema == "reel.presentation-still-sequence.v2"
            || !receipt.picture_frames.is_empty())
            && receipt.picture_frames != picture_frames)
        || receipt.picture_timing_sha256 != manifest.picture_timing.map(|t| t.sha256)
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
            bail!(
                "usage: reel-presentation-still-sequence <build|check> <manifest> --root <root> --output-dir <dir>"
            );
        }
    })();
    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod timing_tests {
    use super::*;

    fn fixture() -> (Template, PictureTiming, SourceClock) {
        let file = || FileRef {
            path: "evidence.json".into(),
            sha256: "a".repeat(64),
            bytes: 1,
            cache_uri: format!("cache://sha256/{}", "a".repeat(64)),
        };
        let template = Template {
            schema: "reel.presentation-still-template.v2".into(),
            template_id: "test".into(),
            width: 64,
            height: 64,
            frame_rate: 24,
            sample_rate: 48000,
            picture_count: 2,
            frames_per_picture: 0,
            total_frames: Some(48),
            audio_required: true,
        };
        let timing = PictureTiming {
            schema: "reel.presentation-picture-timing.v1".into(),
            clock: file(),
            spans: vec![
                PictureSpan {
                    start_anchor: "intro".into(),
                    end_anchor: "word".into(),
                },
                PictureSpan {
                    start_anchor: "word".into(),
                    end_anchor: "outro".into(),
                },
            ],
        };
        let clock = SourceClock {
            schema: "reel.presentation-source-clock.v1".into(),
            source_sha256: "source".into(),
            sample_rate: 48000,
            total_samples: 96000,
            evidence: file(),
            anchors: vec![
                ClockAnchor {
                    id: "intro".into(),
                    sample: 0,
                },
                ClockAnchor {
                    id: "word".into(),
                    sample: 12000,
                },
                ClockAnchor {
                    id: "outro".into(),
                    sample: 96000,
                },
            ],
        };
        (template, timing, clock)
    }

    #[test]
    fn semantic_boundaries_compile_to_variable_gapless_frames() {
        let (t, p, c) = fixture();
        assert_eq!(
            compile_picture_frames(&p, &c, &t, "source").unwrap(),
            vec![6, 42]
        );
    }

    #[test]
    fn shared_rounding_closes_non_frame_aligned_source_anchors() {
        let (t, p, mut c) = fixture();
        c.anchors[1].sample = 13001;
        assert_eq!(
            compile_picture_frames(&p, &c, &t, "source").unwrap(),
            vec![7, 41]
        );
    }

    #[test]
    fn rejects_wrong_source_unknown_duplicate_and_unclosed_anchors() {
        let (t, mut p, mut c) = fixture();
        assert!(compile_picture_frames(&p, &c, &t, "other").is_err());
        p.spans[0].end_anchor = "absent".into();
        assert!(compile_picture_frames(&p, &c, &t, "source").is_err());
        let (_, p, _) = fixture();
        c.anchors.push(ClockAnchor {
            id: "word".into(),
            sample: 20000,
        });
        assert!(compile_picture_frames(&p, &c, &t, "source").is_err());
        let (_, p, mut c) = fixture();
        c.anchors[2].sample = 90000;
        assert!(compile_picture_frames(&p, &c, &t, "source").is_err());
    }

    #[test]
    fn rejects_clock_gaps_and_sub_frame_spans() {
        let (t, mut p, mut c) = fixture();
        p.spans[1].start_anchor = "intro".into();
        assert!(compile_picture_frames(&p, &c, &t, "source").is_err());
        let (_, p, _) = fixture();
        c.anchors[1].sample = 1;
        assert!(compile_picture_frames(&p, &c, &t, "source").is_err());
    }
}
