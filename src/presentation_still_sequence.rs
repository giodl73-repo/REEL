//! Render a data-driven, editable-layer still sequence for episode presentation.

use crate::{
    episode_conform,
    scene_delivery::{self, InputFiles, MediaProgram},
};
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

fn pin(file: &FileRef) -> Result<scene_delivery::FileRef> {
    scene_delivery::control_name(&file.path)?;
    if file.cache_uri != format!("cache://sha256/{}", file.sha256) {
        bail!("file cache URI differs from its hash");
    }
    Ok(scene_delivery::FileRef {
        path: file.path.clone(),
        sha256: file.sha256.clone(),
        bytes: file.bytes,
    })
}

fn verified(files: &dyn InputFiles, file: &FileRef) -> Result<PathBuf> {
    files
        .verified_path(&pin(file)?)
        .context("file hash, bytes or cache URI mismatch")
}

fn read_control(files: &dyn InputFiles, file: &FileRef) -> Result<Vec<u8>> {
    scene_delivery::read_control(files, &pin(file)?)
}

fn command(files: &dyn InputFiles, program: MediaProgram) -> Result<Command> {
    episode_conform::command_with_inputs(files, program)
}

fn run(mut command: Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}

fn facts(path: &Path, files: &dyn InputFiles) -> Result<serde_json::Value> {
    let output = command(files, MediaProgram::Ffprobe)?
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

fn check_media(path: &Path, template: &Template, files: &dyn InputFiles) -> Result<(u64, u64)> {
    let streams = facts(path, files)?["streams"]
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
    crate::episode_delivery::verify_timestamps_with_inputs(
        path,
        template.sample_rate,
        u64::from(template.frame_rate),
        1,
        files,
    )?;
    let decoded = command(files, MediaProgram::Ffmpeg)?
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "s24le", "-acodec", "pcm_s24le", "-"])
        .output()?;
    if !decoded.status.success() || decoded.stdout.len() as u64 != samples * 6 {
        bail!("presentation audio sample count mismatch");
    }
    run({
        let mut c = command(files, MediaProgram::Ffmpeg)?;
        c.args(["-v", "error", "-i"])
            .arg(path)
            .args(["-f", "null", "-"]);
        c
    })?;
    Ok((frames, samples))
}

fn load(
    manifest_bytes: &[u8],
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
) -> Result<LoadedPresentation> {
    let manifest: Manifest = serde_json::from_slice(manifest_bytes)?;
    if !matches!(
        manifest.schema.as_str(),
        "reel.presentation-still-sequence.v1" | "reel.presentation-still-sequence.v2"
    ) || !matches!(manifest.language.as_str(), "es" | "en")
        || manifest.episode_id.is_empty()
        || manifest.role.is_empty()
    {
        bail!("invalid presentation manifest");
    }
    let template_bytes = read_control(controls, &manifest.template)?;
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
    verified(inputs, &manifest.editable_layer)?;
    for font in &manifest.fonts {
        verified(inputs, font)?;
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
        .map(|item| verified(inputs, item))
        .collect::<Result<Vec<_>>>()?;
    let audio = manifest
        .audio
        .as_ref()
        .map(|item| verified(inputs, item))
        .transpose()?;
    let picture_frames = if let Some(timing_ref) = &manifest.picture_timing {
        let bytes = read_control(controls, timing_ref)?;
        let timing: PictureTiming = serde_json::from_slice(&bytes)?;
        let clock_bytes = read_control(controls, &timing.clock)?;
        let clock: SourceClock = serde_json::from_slice(&clock_bytes)?;
        read_control(controls, &clock.evidence)?;
        let audio_ref = manifest
            .audio
            .as_ref()
            .context("source clock requires selected audio")?;
        let source = audio.as_ref().context("missing source audio")?;
        let source_facts = facts(source, inputs)?;
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
        let decoded = command(inputs, MediaProgram::Ffmpeg)?
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
        manifest_bytes.to_vec(),
        picture_frames,
    ))
}

pub fn build(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    build_manifest(&fs::read(manifest_path)?, &root, &root, output_dir)
}

/// Render the original declarative still-sequence format from exact guarded
/// control and primary providers. Keep both providers alive throughout the
/// operation. An explicit refusal cannot fall back to a source checkout or PATH.
pub fn build_with_control_files(
    manifest: &scene_delivery::FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    output_dir: &Path,
) -> Result<()> {
    scene_delivery::control_name(&manifest.path)?;
    let bytes = scene_delivery::read_control(controls, manifest)?;
    build_manifest(&bytes, controls, inputs, output_dir)
}

fn build_manifest(
    raw: &[u8],
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    output_dir: &Path,
) -> Result<()> {
    if output_dir.exists() {
        bail!("output directory already exists");
    }
    let loaded = load(raw, controls, inputs)?;
    fs::create_dir_all(output_dir)?;
    let output_dir = fs::canonicalize(output_dir)?;
    prepare_layers(&loaded.0, inputs, &output_dir)?;
    let mut cmd = render_command(&loaded, inputs, &output_dir, "ffv1")?;
    cmd.arg(output_dir.join("master.mkv"));
    run(cmd)?;
    let (manifest, template, _, _, manifest_bytes, picture_frames) = loaded;
    let (frames, samples) = check_media(&output_dir.join("master.mkv"), &template, inputs)?;
    write_review(&output_dir, inputs)?;
    let master = output_dir.join("master.mkv");
    let review = output_dir.join("review.mp4");
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
        master_sha256: episode_conform::file_sha(&master)?,
        master_bytes: fs::metadata(&master)?.len(),
        review_sha256: episode_conform::file_sha(&review)?,
        review_bytes: fs::metadata(&review)?.len(),
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

fn prepare_layers(manifest: &Manifest, inputs: &dyn InputFiles, output_dir: &Path) -> Result<()> {
    fs::copy(
        verified(inputs, &manifest.editable_layer)?,
        output_dir.join("editable-layer.ass"),
    )?;
    if !manifest.fonts.is_empty() {
        fs::create_dir(output_dir.join("fonts"))?;
        for (index, font) in manifest.fonts.iter().enumerate() {
            fs::copy(
                verified(inputs, font)?,
                output_dir.join(format!("fonts/font-{index}.ttf")),
            )?;
        }
    }
    Ok(())
}

fn render_command(
    loaded: &LoadedPresentation,
    inputs: &dyn InputFiles,
    output_dir: &Path,
    video_codec: &str,
) -> Result<Command> {
    let (manifest, template, pictures, audio, _, picture_frames) = loaded;
    let total_seconds = template.frames() as f64 / template.frame_rate as f64;
    let mut cmd = command(inputs, MediaProgram::Ffmpeg)?;
    cmd.current_dir(output_dir)
        .args(["-y", "-v", "error", "-nostdin"]);
    if video_codec == "rawvideo" {
        cmd.args(["-filter_complex_threads", "1"]);
    }
    for (picture, frames) in pictures.iter().zip(picture_frames) {
        let seconds_each = *frames as f64 / template.frame_rate as f64;
        if video_codec == "rawvideo" {
            cmd.args(["-threads", "1"]);
        }
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
    if let Some(source) = audio {
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
        "-c:v",
        video_codec,
        "-pix_fmt",
        "yuv444p",
        "-c:a",
        "pcm_s24le",
        "-ar",
        &template.sample_rate.to_string(),
        "-ac",
        "2",
    ]);
    if video_codec == "rawvideo" {
        cmd.args(["-threads:v", "1"]);
    }
    if video_codec == "ffv1" {
        cmd.args(["-frames:v", &template.frames().to_string()]);
    }
    Ok(cmd)
}

fn write_review(output_dir: &Path, inputs: &dyn InputFiles) -> Result<()> {
    run({
        let mut c = command(inputs, MediaProgram::Ffmpeg)?;
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
    })
}

pub fn check(manifest_path: &Path, root: &Path, output_dir: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    check_manifest(&fs::read(manifest_path)?, &root, &root, output_dir)
}

/// Recheck a generated output directory against cache-backed producer inputs.
/// This is technical receipt correspondence, not creative approval or a proof
/// that the source movie's full production lineage has been adopted by Tig.
pub fn check_with_control_files(
    manifest: &scene_delivery::FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    output_dir: &Path,
) -> Result<()> {
    scene_delivery::control_name(&manifest.path)?;
    let bytes = scene_delivery::read_control(controls, manifest)?;
    check_manifest(&bytes, controls, inputs, output_dir)
}

fn check_manifest(
    raw: &[u8],
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    output_dir: &Path,
) -> Result<()> {
    let loaded = load(raw, controls, inputs)?;
    let fonts = (0..loaded.0.fonts.len())
        .map(|i| observe(&output_dir.join(format!("fonts/font-{i}.ttf"))))
        .collect::<Result<Vec<_>>>()?;
    check_outputs(
        loaded,
        inputs,
        ObservedOutputs {
            receipt: bounded_receipt(&output_dir.join("receipt.json"))?,
            master: output_dir.join("master.mkv"),
            review: output_dir.join("review.mp4"),
            layer: observe(&output_dir.join("editable-layer.ass"))?,
            fonts,
        },
    )
}

/// Exact retained output subjects, independent of their physical directory.
/// Fonts are in the producer manifest's declared order. These pins and a
/// successful check do not confer approval, custody or native job currency.
#[derive(Clone, Copy)]
pub struct RetainedOutputs<'a> {
    pub receipt: &'a scene_delivery::FileRef,
    pub master: &'a scene_delivery::FileRef,
    pub review: &'a scene_delivery::FileRef,
    pub editable_layer: &'a scene_delivery::FileRef,
    pub fonts: &'a [scene_delivery::FileRef],
}

/// Full native primary-reference comparison. This capability is not a native
/// job enrollment, original-producer assertion, creative approval or custody.
pub struct PrimaryComparison {
    manifest: scene_delivery::FileRef,
    master: scene_delivery::FileRef,
    yuv: String,
    rgb: String,
    audio: String,
    staged_bytes: u64,
}
impl PrimaryComparison {
    pub fn manifest(&self) -> &scene_delivery::FileRef {
        &self.manifest
    }
    pub fn master(&self) -> &scene_delivery::FileRef {
        &self.master
    }
    pub fn picture_sha256(&self) -> (&str, &str) {
        (&self.yuv, &self.rgb)
    }
    pub fn audio_sha256(&self) -> &str {
        &self.audio
    }
    pub fn staged_bytes(&self) -> u64 {
        self.staged_bytes
    }
}

/// Reconstruct every expected native picture/audio sample into stream hashes,
/// then compare the retained master. Only ASS/fonts are copied into owned
/// temporary scratch, bounded by their exact declared lengths. No movie is
/// created. Keep all provider guards alive; the caller supplies admitted scratch.
pub fn compare_retained_to_primaries(
    manifest: &scene_delivery::FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    outputs: &dyn InputFiles,
    selected: RetainedOutputs<'_>,
    stage_parent: &Path,
    max_staged_bytes: u64,
) -> Result<PrimaryComparison> {
    scene_delivery::control_name(&manifest.path)?;
    staging_bytes(
        &scene_delivery::read_control(controls, manifest)?,
        max_staged_bytes,
    )?;
    check_retained_with_files(manifest, controls, inputs, outputs, selected)?;
    compare_master_to_primaries(
        manifest,
        controls,
        inputs,
        outputs,
        selected.master,
        stage_parent,
        max_staged_bytes,
    )
}

/// Independently compare one exact master with the current primary render.
/// Old receipts, reviews and output-directory layouts are not prerequisites.
/// Original producer/approval evidence remains separate; this comparison is
/// neither a rewritten production history nor native job admission.
pub fn compare_master_to_primaries(
    manifest: &scene_delivery::FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    outputs: &dyn InputFiles,
    master: &scene_delivery::FileRef,
    stage_parent: &Path,
    max_staged_bytes: u64,
) -> Result<PrimaryComparison> {
    scene_delivery::control_name(&manifest.path)?;
    scene_delivery::control_name(&master.path)?;
    let raw = scene_delivery::read_control(controls, manifest)?;
    let staged_bytes = staging_bytes(&raw, max_staged_bytes)?;
    let loaded = load(&raw, controls, inputs)?;
    let master_path = outputs.verified_path(master)?;
    check_media(&master_path, &loaded.1, inputs)?;
    let stage = tempfile::Builder::new()
        .prefix(".presentation-primary-check-")
        .tempdir_in(stage_parent)?;
    prepare_layers(&loaded.0, inputs, stage.path())?;
    let mut matched = Vec::new();
    for pixel_format in ["yuv444p", "rgb24"] {
        let mut reference = render_command(&loaded, inputs, stage.path(), "rawvideo")?;
        reference.args(["-pix_fmt", pixel_format]);
        let expected = stream_hashes(reference)?;
        let mut retained = command(inputs, MediaProgram::Ffmpeg)?;
        retained
            .args(["-v", "error", "-nostdin", "-i"])
            .arg(&master_path)
            .args([
                "-map",
                "0:v:0",
                "-map",
                "0:a:0",
                "-c:v",
                "rawvideo",
                "-threads:v",
                "1",
                "-pix_fmt",
                pixel_format,
                "-c:a",
                "pcm_s24le",
            ]);
        let actual = stream_hashes(retained)?;
        if actual != expected {
            bail!("retained presentation differs from primary reference ({pixel_format})");
        }
        matched.push(actual);
    }
    if matched[0].1 != matched[1].1 {
        bail!("presentation primary comparison audio is inconsistent");
    }
    Ok(PrimaryComparison {
        manifest: manifest.clone(),
        master: master.clone(),
        yuv: matched[0].0.clone(),
        rgb: matched[1].0.clone(),
        audio: matched[0].1.clone(),
        staged_bytes,
    })
}

fn staging_bytes(raw: &[u8], maximum: u64) -> Result<u64> {
    let parsed: Manifest = serde_json::from_slice(raw)?;
    let bytes = parsed
        .fonts
        .iter()
        .try_fold(parsed.editable_layer.bytes, |sum, font| {
            sum.checked_add(font.bytes)
                .context("presentation staging byte count overflow")
        })?;
    if bytes > maximum {
        bail!("presentation primary comparison exceeds staging budget");
    }
    Ok(bytes)
}

fn stream_hashes(mut cmd: Command) -> Result<(String, String)> {
    let result = cmd
        .args(["-f", "streamhash", "-hash", "sha256", "-"])
        .output()?;
    if !result.status.success() {
        bail!("presentation stream hashing failed");
    }
    let output =
        std::str::from_utf8(&result.stdout).context("invalid presentation stream hashes")?;
    parse_stream_hashes(output)
}

fn parse_stream_hashes(output: &str) -> Result<(String, String)> {
    let lines = output.lines().map(str::trim).collect::<Vec<_>>();
    // FFmpeg 8.1 emits the original index/type/algorithm form. Newer builds
    // emit versioned stream-checksum headers and index/byte-count/hash records.
    let version2 = lines.first() == Some(&"#format: stream checksums");
    if version2
        && ![
            "#version: 2",
            "#hash: SHA256",
            "#media_type 0: video",
            "#codec_id 0: rawvideo",
            "#media_type 1: audio",
            "#codec_id 1: pcm_s24le",
        ]
        .iter()
        .all(|required| lines.contains(required))
    {
        bail!("unsupported presentation stream hash headers");
    }
    let mut hashes = BTreeMap::new();
    for line in lines {
        if version2 && line.starts_with('#') {
            continue;
        }
        let parts = line.split(',').map(str::trim).collect::<Vec<_>>();
        if parts.len() != 3 || !matches!(parts[0], "0" | "1") {
            bail!("unexpected presentation stream hash subject");
        }
        let digest = if version2 {
            if parts[1].parse::<u64>().ok().is_none_or(|bytes| bytes == 0) {
                bail!("empty or invalid presentation stream size");
            }
            parts[2]
        } else {
            if !matches!((parts[0], parts[1]), ("0", "v") | ("1", "a")) {
                bail!("unexpected presentation stream hash type");
            }
            let (algorithm, digest) = parts[2]
                .split_once('=')
                .context("missing presentation stream digest")?;
            if !algorithm.eq_ignore_ascii_case("sha256") {
                bail!("unsupported presentation stream hash algorithm");
            }
            digest
        };
        if digest.len() != 64
            || !digest.bytes().all(|b| b.is_ascii_hexdigit())
            || hashes
                .insert(parts[0], digest.to_ascii_lowercase())
                .is_some()
        {
            bail!("invalid or duplicate presentation stream digest");
        }
    }
    if hashes.len() != 2 {
        bail!("incomplete presentation stream hashes");
    }
    Ok((hashes["0"].clone(), hashes["1"].clone()))
}

/// Recheck directly from guarded control, primary and retained-output readers.
/// No original source/output directory, staging view or media copy is needed.
pub fn check_retained_with_files(
    manifest: &scene_delivery::FileRef,
    controls: &dyn InputFiles,
    inputs: &dyn InputFiles,
    outputs: &dyn InputFiles,
    selected: RetainedOutputs<'_>,
) -> Result<()> {
    scene_delivery::control_name(&manifest.path)?;
    let raw = scene_delivery::read_control(controls, manifest)?;
    scene_delivery::control_name(&selected.receipt.path)?;
    let receipt = scene_delivery::read_control(outputs, selected.receipt)?;
    let declared: Receipt = serde_json::from_slice(&receipt)?;
    if selected.master.sha256 != declared.master_sha256
        || selected.master.bytes != declared.master_bytes
        || selected.review.sha256 != declared.review_sha256
        || selected.review.bytes != declared.review_bytes
        || selected.editable_layer.sha256 != declared.editable_layer_sha256
        || selected.fonts.len() != declared.font_sha256.len()
        || selected
            .fonts
            .iter()
            .zip(&declared.font_sha256)
            .any(|(f, sha)| &f.sha256 != sha)
    {
        bail!("retained output pins differ from producer receipt");
    }
    let loaded = load(&raw, controls, inputs)?;
    if selected.fonts.len() != loaded.0.fonts.len()
        || selected
            .fonts
            .iter()
            .zip(&loaded.0.fonts)
            .any(|(f, source)| f.sha256 != source.sha256 || f.bytes != source.bytes)
        || selected.editable_layer.sha256 != loaded.0.editable_layer.sha256
        || selected.editable_layer.bytes != loaded.0.editable_layer.bytes
    {
        bail!("retained layer/font subjects differ from producer");
    }
    let resolve = |item: &scene_delivery::FileRef| -> Result<PathBuf> {
        scene_delivery::control_name(&item.path)?;
        outputs.verified_path(item)
    };
    let master = resolve(selected.master)?;
    let review = resolve(selected.review)?;
    let layer = observe(&resolve(selected.editable_layer)?)?;
    let fonts = selected
        .fonts
        .iter()
        .map(|f| observe(&resolve(f)?))
        .collect::<Result<Vec<_>>>()?;
    check_outputs(
        loaded,
        inputs,
        ObservedOutputs {
            receipt,
            master,
            review,
            layer,
            fonts,
        },
    )
}

struct ObservedOutputs {
    receipt: Vec<u8>,
    master: PathBuf,
    review: PathBuf,
    layer: (String, u64),
    fonts: Vec<(String, u64)>,
}

fn observe(path: &Path) -> Result<(String, u64)> {
    Ok((episode_conform::file_sha(path)?, fs::metadata(path)?.len()))
}

fn bounded_receipt(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut raw = Vec::new();
    fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > 4 * 1024 * 1024 {
        bail!("presentation receipt exceeds metadata budget");
    }
    Ok(raw)
}

fn check_outputs(
    loaded: LoadedPresentation,
    inputs: &dyn InputFiles,
    outputs: ObservedOutputs,
) -> Result<()> {
    let (manifest, template, _, _, manifest_bytes, picture_frames) = loaded;
    let ObservedOutputs {
        receipt,
        master,
        review,
        layer,
        fonts,
    } = outputs;
    let receipt: Receipt = serde_json::from_slice(&receipt)?;
    if fonts.len() != manifest.fonts.len()
        || fonts
            .iter()
            .zip(&manifest.fonts)
            .any(|((sha, bytes), f)| sha != &f.sha256 || *bytes != f.bytes)
    {
        bail!("retained font differs from selected font");
    }
    let (frames, samples) = check_media(&master, &template, inputs)?;
    if receipt.schema != "reel.presentation-still-sequence-receipt.v1"
        || receipt.episode_id != manifest.episode_id
        || receipt.language != manifest.language
        || receipt.role != manifest.role
        || receipt.template_id != template.template_id
        || receipt.manifest_sha256 != hash(&manifest_bytes)
        || receipt.template_sha256 != manifest.template.sha256
        || receipt.picture_sha256
            != manifest
                .pictures
                .iter()
                .map(|p| p.sha256.clone())
                .collect::<Vec<_>>()
        || receipt.editable_layer_sha256 != layer.0
        || manifest.editable_layer.sha256 != layer.0
        || manifest.editable_layer.bytes != layer.1
        || receipt.audio_sha256 != manifest.audio.map(|a| a.sha256)
        || receipt.audio_treatment != manifest.audio_treatment
        || receipt.font_sha256
            != manifest
                .fonts
                .iter()
                .map(|font| font.sha256.clone())
                .collect::<Vec<_>>()
        || receipt.master_sha256 != episode_conform::file_sha(&master)?
        || receipt.master_bytes != fs::metadata(&master)?.len()
        || receipt.review_sha256 != episode_conform::file_sha(&review)?
        || receipt.review_bytes != fs::metadata(&review)?.len()
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
        let mut c = command(inputs, MediaProgram::Ffmpeg)?;
        c.args(["-v", "error", "-i"])
            .arg(&review)
            .args(["-f", "null", "-"]);
        c
    })?;
    println!("presentation still sequence verified");
    Ok(())
}

pub fn cli() {
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

    #[test]
    fn stream_hash_protocols_bind_both_exact_streams_and_sha256() {
        let video = "a".repeat(64);
        let audio = "b".repeat(64);
        let legacy = format!("0,v,SHA256={video}\n1,a,SHA256={audio}\n");
        let current = format!(
            "#format: stream checksums\n#version: 2\n#hash: SHA256\n#media_type 0: video\n#codec_id 0: rawvideo\n#media_type 1: audio\n#codec_id 1: pcm_s24le\n0, 123, {video}\n1, 456, {audio}\n"
        );
        assert_eq!(
            parse_stream_hashes(&legacy).unwrap(),
            (video.clone(), audio.clone())
        );
        assert_eq!(parse_stream_hashes(&current).unwrap(), (video, audio));
        for bad in [
            legacy.replace("1,a", "0,v"),
            legacy.replace("SHA256", "SHA512"),
            legacy.lines().next().unwrap().into(),
            current.replace("#version: 2", "#version: 3"),
            current.replace("pcm_s24le", "pcm_s16le"),
            current.replace("0, 123", "0, 0"),
        ] {
            assert!(parse_stream_hashes(&bad).is_err());
        }
    }

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
