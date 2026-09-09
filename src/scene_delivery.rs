//! Hash-bound scene delivery directly from the cue-relative compiler.
//! Media interpretation and creative selection remain with the consumer.
use crate::cue_relative::{self, Target};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRef {
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub schema: String,
    pub id: String,
    pub contract: FileRef,
    pub production_manifest_sha256: String,
    pub width: u32,
    pub height: u32,
    pub max_composition_samples: u64,
    pub pictures: Vec<Picture>,
    pub audio: Vec<Audio>,
    pub buses: BTreeMap<String, BusPolicy>,
    /// Exact render attachments intentionally owned by another delivery layer.
    #[serde(default)]
    pub external_layers: Vec<ExternalLayer>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalLayer {
    pub attachment_id: String,
    pub reason: String,
    pub evidence: FileRef,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BusPolicy {
    pub state: BusState,
    pub reason: String,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BusState {
    Present,
    IntentionalSilence,
    Held,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Picture {
    pub attachment_id: String,
    pub source: FileRef,
    pub kind: PictureKind,
    /// Offset after resampling the source to the declared delivery frame rate.
    #[serde(default)]
    pub source_start_frame: u64,
    pub attention: String,
    #[serde(default)]
    pub crop: Option<Crop>,
    #[serde(default)]
    pub stillness_exception: Option<Exception>,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PictureKind {
    Still,
    Video,
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    pub reason: String,
    pub decision: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Audio {
    pub attachment_id: String,
    pub source: FileRef,
    pub bus: String,
    #[serde(default)]
    pub cue_id: Option<String>,
    #[serde(default)]
    pub source_start_sample: u64,
    #[serde(default)]
    pub gain_db: f64,
    #[serde(default)]
    pub fade_in_samples: u64,
    #[serde(default)]
    pub fade_out_samples: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Span {
    pub attachment_id: String,
    pub start_sample: u64,
    pub end_sample: u64,
    pub start_frame: u64,
    pub end_frame: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub id: String,
    pub job_sha256: String,
    pub contract_sha256: String,
    pub production_sha256: String,
    pub compiled_sha256: String,
    pub sample_rate: u32,
    pub fps_numerator: u64,
    pub fps_denominator: u64,
    pub duration_samples: u64,
    pub frame_count: u64,
    pub pictures: Vec<Span>,
    pub audio: Vec<Span>,
    pub external_layers: Vec<String>,
    pub buses: BTreeMap<String, BusPolicy>,
    pub creative_authority: String,
}

pub(crate) fn checked_file(root: &Path, item: &FileRef) -> Result<PathBuf> {
    if item.path.is_absolute()
        || item
            .path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        bail!("asset path must be a relative local path");
    }
    let root = root.canonicalize()?;
    // Windows may accept mismatched case that later fails on Linux. Verify the
    // exact directory entries as well as resolved bytes on every platform.
    let mut exact = root.clone();
    for component in item.path.components() {
        let name = component.as_os_str();
        if !fs::read_dir(&exact)?.any(|entry| entry.is_ok_and(|entry| entry.file_name() == name)) {
            bail!(
                "missing or case-mismatched asset path: {}",
                item.path.display()
            );
        }
        exact.push(name);
    }
    let path = root
        .join(&item.path)
        .canonicalize()
        .context("missing declared file")?;
    if !path.starts_with(&root) || !path.is_file() {
        bail!("asset escapes its root");
    }
    if fs::metadata(&path)?.len() != item.bytes || crate::sha256_file(&path)? != item.sha256 {
        bail!("file hash/byte mismatch: {}", item.path.display());
    }
    Ok(path)
}

fn nonempty(s: &str) -> bool {
    !s.trim().is_empty()
}

pub fn plan(job_path: &Path, asset_root: &Path) -> Result<(Job, Plan)> {
    let job: Job = serde_yaml::from_slice(&fs::read(job_path)?)?;
    if job.schema != "reel.scene-delivery.v0.1" || !nonempty(&job.id) {
        bail!("invalid scene-delivery schema/id");
    }
    if job.width == 0
        || job.height == 0
        || job.width > 8192
        || job.height > 8192
        || job.width % 2 != 0
        || job.height % 2 != 0
        || job.max_composition_samples == 0
    {
        bail!("invalid delivery dimensions/composition limit");
    }
    let base = job_path.parent().unwrap_or(Path::new("."));
    let contract_path = checked_file(base, &job.contract)?;
    let contract = cue_relative::load(&contract_path)?;
    let contract_base = contract_path.parent().unwrap();
    checked_file(
        contract_base,
        &FileRef {
            path: contract.production_manifest.clone(),
            sha256: job.production_manifest_sha256.clone(),
            bytes: fs::metadata(contract_base.join(&contract.production_manifest))?.len(),
        },
    )?;
    let compiled = cue_relative::compile(&contract, contract_path.parent().unwrap())?;
    if compiled.production_manifest_sha256 != job.production_manifest_sha256 {
        bail!("production manifest differs from the delivery selection");
    }
    let mut attached = BTreeMap::new();
    for a in &compiled.attachments {
        attached.insert(a.id.as_str(), a);
    }
    let mut used = BTreeSet::new();
    let mut take = |id: &str| -> Result<&cue_relative::CompiledAttachment> {
        if !used.insert(id.to_string()) {
            bail!("attachment consumed more than once: {id}");
        }
        attached
            .get(id)
            .copied()
            .with_context(|| format!("unknown attachment {id}"))
    };
    let frame = |sample: u64, ceil: bool| -> Result<u64> {
        let n = u128::from(sample) * u128::from(compiled.frame_rate.numerator);
        let d = u128::from(compiled.sample_rate) * u128::from(compiled.frame_rate.denominator);
        Ok(u64::try_from(if ceil { n.div_ceil(d) } else { n / d })?)
    };
    let span = |a: &cue_relative::CompiledAttachment| -> Result<Span> {
        Ok(Span {
            attachment_id: a.id.clone(),
            start_sample: a.start_sample,
            end_sample: a.end_sample,
            start_frame: frame(a.start_sample, false)?,
            end_frame: frame(a.end_sample, a.end_sample == compiled.duration_samples)?,
        })
    };
    let mut pictures = Vec::new();
    let mut prior: Option<&Picture> = None;
    let mut unchanged_start = 0;
    let mut cursor = 0;
    for p in &job.pictures {
        let a = take(&p.attachment_id)?;
        if !matches!(a.target, Target::Shot { .. } | Target::Cel { .. }) {
            bail!("picture must bind a shot or cel");
        }
        if !nonempty(&p.attention) || a.start_sample != cursor || a.end_sample <= a.start_sample {
            bail!("picture attention missing or coverage gap/overlap");
        }
        checked_file(asset_root, &p.source)?;
        if p.kind == PictureKind::Still && p.source_start_frame != 0 {
            bail!("still pictures cannot have a source frame offset");
        }
        if let Some(c) = &p.crop {
            if c.width == 0 || c.height == 0 {
                bail!("empty crop");
            }
        }
        if prior.is_none_or(|old| old.source.sha256 != p.source.sha256 || old.crop != p.crop) {
            unchanged_start = a.start_sample;
        }
        if a.end_sample - unchanged_start > job.max_composition_samples
            && !p
                .stillness_exception
                .as_ref()
                .is_some_and(|e| nonempty(&e.reason) && nonempty(&e.decision))
        {
            bail!("unchanged composition exceeds limit: {}", p.attachment_id);
        }
        let s = span(a)?;
        if s.end_frame <= s.start_frame {
            bail!("composition has no delivery frame: {}", p.attachment_id);
        }
        pictures.push(s);
        cursor = a.end_sample;
        prior = Some(p);
    }
    if cursor != compiled.duration_samples || pictures.is_empty() {
        bail!("picture does not cover the complete scene");
    }
    if job
        .buses
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != BTreeSet::from(["D", "E", "M"])
    {
        bail!("declare exactly D, M and E bus policies");
    }
    let mut audio = Vec::new();
    let mut dialogue = BTreeSet::new();
    for event in &job.audio {
        let a = take(&event.attachment_id)?;
        if !matches!(a.target, Target::Audio { .. } | Target::Sonic { .. }) {
            bail!("audio must bind an audio/sonic attachment");
        }
        if !job.buses.contains_key(&event.bus)
            || !event.gain_db.is_finite()
            || event.gain_db.abs() > 60.0
        {
            bail!("invalid audio bus/gain");
        }
        let duration = a.end_sample - a.start_sample;
        if duration == 0
            || event.fade_in_samples > duration
            || event.fade_out_samples > duration
            || event.fade_in_samples.saturating_add(event.fade_out_samples) > duration
        {
            bail!("invalid audio span/fades");
        }
        checked_file(asset_root, &event.source)?;
        if event.bus == "D" {
            let id = event.cue_id.as_deref().context("D event needs cue_id")?;
            let c = compiled
                .cues
                .iter()
                .find(|c| c.cue_id == id)
                .context("D cue is unknown")?;
            if !dialogue.insert(id.to_string())
                || c.start_sample != a.start_sample
                || c.end_sample != a.end_sample
                || event.source_start_sample != 0
            {
                bail!("D event must consume one complete native cue exactly once");
            }
        } else if event.cue_id.is_some() {
            bail!("only D events declare cue_id");
        }
        audio.push(span(a)?);
    }
    for (bus, policy) in &job.buses {
        if !nonempty(&policy.reason) || policy.state == BusState::Held {
            bail!("bus {bus} is held or lacks a reason");
        }
        let present = job.audio.iter().any(|a| a.bus == *bus);
        if present != (policy.state == BusState::Present) {
            bail!("bus {bus} policy and consumed audio disagree");
        }
    }
    if job.buses["D"].state == BusState::Present && dialogue.len() != compiled.cues.len() {
        bail!("D bus must cover every native cue");
    }
    let mut external_layers = Vec::new();
    for layer in &job.external_layers {
        let a = take(&layer.attachment_id)?;
        if !matches!(
            a.target,
            Target::Beat { .. }
                | Target::Camera { .. }
                | Target::Effect { .. }
                | Target::Caption { .. }
                | Target::Title { .. }
        ) || !nonempty(&layer.reason)
        {
            bail!("invalid external layer; primary picture/audio cannot be omitted");
        }
        checked_file(asset_root, &layer.evidence)?;
        external_layers.push(layer.attachment_id.clone());
    }
    if used.len() != attached.len() {
        bail!("unconsumed compiled attachments; declare external layers explicitly");
    }
    let plan = Plan {
        schema: "reel.scene-delivery-plan.v0.1".into(),
        id: job.id.clone(),
        job_sha256: crate::sha256_file(job_path)?,
        contract_sha256: job.contract.sha256.clone(),
        production_sha256: compiled.production_manifest_sha256.clone(),
        compiled_sha256: <sha2::Sha256 as sha2::Digest>::digest(serde_json::to_vec(&compiled)?)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        sample_rate: compiled.sample_rate,
        fps_numerator: compiled.frame_rate.numerator,
        fps_denominator: compiled.frame_rate.denominator,
        duration_samples: compiled.duration_samples,
        frame_count: frame(compiled.duration_samples, true)?,
        pictures,
        audio,
        external_layers,
        buses: job.buses.clone(),
        creative_authority: "not-granted; external layers are not certified as rendered".into(),
    };
    Ok((job, plan))
}

pub(crate) fn ffmpeg(args: &[String]) -> Result<()> {
    let mut local_args = Vec::new();
    for argument in args {
        if argument == "-i" {
            local_args.extend(["-protocol_whitelist".to_string(), "file,pipe".to_string()]);
        }
        local_args.push(argument.clone());
    }
    let output = Command::new("ffmpeg")
        .args(["-hide_banner", "-v", "error", "-nostdin", "-n"])
        .args(local_args)
        .output()?;
    if !output.status.success() {
        bail!("FFmpeg failed: {}", String::from_utf8_lossy(&output.stderr));
    }
    Ok(())
}
pub(crate) fn probe(path: &Path) -> Result<serde_json::Value> {
    let o = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-show_streams",
            "-of",
            "json",
        ])
        .arg(path)
        .output()?;
    if !o.status.success() {
        bail!("ffprobe failed");
    }
    Ok(serde_json::from_slice(&o.stdout)?)
}
fn arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn finish_pcm(float_path: &Path, output: &Path) -> Result<()> {
    // Reject overload before PCM24 quantization can hide clipping. This is not
    // a loudness/true-peak or intelligibility approval; audio-quality still owns it.
    let result = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(float_path)
        .args(["-af", "astats=metadata=0:reset=0", "-f", "null", "-"])
        .output()?;
    if !result.status.success() {
        bail!("float mix inspection failed");
    }
    let text = String::from_utf8_lossy(&result.stderr);
    let peaks: Vec<f64> = text
        .lines()
        .filter_map(|line| line.split_once("Peak level dB: "))
        .map(|(_, value)| value.trim().parse::<f64>())
        .collect::<std::result::Result<_, _>>()?;
    if peaks.is_empty() || peaks.iter().any(|p| p.is_nan() || *p >= 0.0) {
        bail!("audio overload or missing peak evidence; revise explicit gains");
    }
    ffmpeg(&[
        "-i".into(),
        arg(float_path),
        "-c:a".into(),
        "pcm_s24le".into(),
        arg(output),
    ])?;
    fs::remove_file(float_path)?;
    Ok(())
}
fn pcm_samples(path: &Path, sr: u32) -> Result<u64> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(path)
        .args([
            "-map",
            "0:a:0",
            "-ar",
            &sr.to_string(),
            "-ac",
            "2",
            "-f",
            "s24le",
            "-",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut out = child.stdout.take().context("PCM pipe missing")?;
    let mut total = 0u64;
    let mut buf = [0u8; 65536];
    loop {
        let n = out.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
    }
    let status = child.wait_with_output()?;
    if !status.status.success() || total % 6 != 0 {
        bail!("PCM decode failed");
    }
    Ok(total / 6)
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub plan: Plan,
    pub outputs: BTreeMap<String, FileRef>,
    pub content_samples: u64,
    pub delivery_frames: u64,
    pub publication: String,
}

pub fn render(job_path: &Path, asset_root: &Path, output: &Path) -> Result<Receipt> {
    let (job, plan) = plan(job_path, asset_root)?;
    if output.exists() {
        bail!("output already exists; use a new delivery directory");
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".scene-delivery-")
        .tempdir_in(parent)?;
    let root = stage.path();
    let fps = format!("{}/{}", plan.fps_numerator, plan.fps_denominator);
    let mut inputs = Vec::new();
    let mut filters = Vec::new();
    for (i, (p, s)) in job.pictures.iter().zip(&plan.pictures).enumerate() {
        let path = checked_file(asset_root, &p.source)?;
        let info = probe(&path)?;
        let stream = info["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "video"))
            .context("picture has no video stream")?;
        if let Some(c) = &p.crop {
            if u64::from(c.x) + u64::from(c.width) > stream["width"].as_u64().unwrap_or(0)
                || u64::from(c.y) + u64::from(c.height) > stream["height"].as_u64().unwrap_or(0)
            {
                bail!("crop outside source");
            }
        }
        if p.kind == PictureKind::Still {
            inputs.extend(["-loop".into(), "1".into(), "-framerate".into(), fps.clone()]);
        }
        inputs.extend(["-i".into(), arg(&path)]);
        let crop = p
            .crop
            .as_ref()
            .map(|c| format!("crop={}:{}:{}:{},", c.width, c.height, c.x, c.y))
            .unwrap_or_default();
        let source_end = p
            .source_start_frame
            .checked_add(s.end_frame - s.start_frame)
            .context("source frame offset overflow")?;
        filters.push(format!("[{i}:v]{crop}scale={}:{}:force_original_aspect_ratio=decrease,pad={}:{}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},trim=start_frame={}:end_frame={source_end},setpts=PTS-STARTPTS[v{i}]",job.width,job.height,job.width,job.height,p.source_start_frame));
    }
    filters.push(format!(
        "{}concat=n={}:v=1:a=0[v]",
        (0..job.pictures.len())
            .map(|i| format!("[v{i}]"))
            .collect::<String>(),
        job.pictures.len()
    ));
    inputs.extend([
        "-filter_complex".into(),
        filters.join(";"),
        "-map".into(),
        "[v]".into(),
        "-an".into(),
        "-c:v".into(),
        "ffv1".into(),
        "-pix_fmt".into(),
        "yuv444p".into(),
        "-r".into(),
        fps.clone(),
        arg(&root.join("picture.mkv")),
    ]);
    ffmpeg(&inputs)?;
    for bus in ["D", "M", "E"] {
        let mut inputs = Vec::new();
        let mut filters = Vec::new();
        let mut labels = String::new();
        let mut count = 0;
        for (event, s) in job
            .audio
            .iter()
            .zip(&plan.audio)
            .filter(|(a, _)| a.bus == bus)
        {
            let path = checked_file(asset_root, &event.source)?;
            let info = probe(&path)?;
            let streams = info["streams"]
                .as_array()
                .context("audio streams missing")?;
            let aud = streams
                .iter()
                .find(|s| s["codec_type"] == "audio")
                .context("audio stream missing")?;
            if aud["sample_rate"].as_str() != Some(&plan.sample_rate.to_string()) {
                bail!("audio must be preconformed to the scene sample rate");
            }
            let samples = pcm_samples(&path, plan.sample_rate)?;
            let n = s.end_sample - s.start_sample;
            if event
                .source_start_sample
                .checked_add(n)
                .is_none_or(|end| end > samples)
                || (bus == "D" && samples != n)
            {
                bail!("native audio duration mismatch or insufficient source samples");
            }
            inputs.extend(["-i".into(), arg(&path)]);
            let mut f = format!(
                "[{count}:a]aformat=sample_fmts=fltp:channel_layouts=stereo,atrim=start_sample={}:end_sample={},asetpts=PTS-STARTPTS,volume={}dB",
                event.source_start_sample,
                event.source_start_sample + n,
                event.gain_db
            );
            if event.fade_in_samples > 0 {
                f.push_str(&format!(",afade=t=in:ss=0:ns={}", event.fade_in_samples));
            }
            if event.fade_out_samples > 0 {
                f.push_str(&format!(
                    ",afade=t=out:ss={}:ns={}",
                    n - event.fade_out_samples,
                    event.fade_out_samples
                ));
            }
            f.push_str(&format!(
                ",adelay={}S:all=1,apad=whole_len={},atrim=end_sample={}[a{count}]",
                s.start_sample, plan.duration_samples, plan.duration_samples
            ));
            filters.push(f);
            labels.push_str(&format!("[a{count}]"));
            count += 1;
        }
        if count == 0 {
            inputs.extend([
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                format!("anullsrc=r={}:cl=stereo", plan.sample_rate),
            ]);
            filters.push(format!(
                "[0:a]atrim=end_sample={}[out]",
                plan.duration_samples
            ));
        } else {
            filters.push(format!(
                "{labels}amix=inputs={count}:normalize=0:duration=longest,atrim=end_sample={}[out]",
                plan.duration_samples
            ));
        }
        inputs.extend([
            "-filter_complex".into(),
            filters.join(";"),
            "-map".into(),
            "[out]".into(),
            "-ar".into(),
            plan.sample_rate.to_string(),
            "-c:a".into(),
            "pcm_f32le".into(),
            arg(&root.join(format!("{bus}-float.wav"))),
        ]);
        ffmpeg(&inputs)?;
        finish_pcm(
            &root.join(format!("{bus}-float.wav")),
            &root.join(format!("{bus}.wav")),
        )?;
    }
    let mut mix = Vec::new();
    for bus in ["D", "M", "E"] {
        mix.extend(["-i".into(), arg(&root.join(format!("{bus}.wav")))]);
    }
    mix.extend([
        "-filter_complex".into(),
        format!(
            "[0:a][1:a][2:a]amix=inputs=3:normalize=0,atrim=end_sample={}[a]",
            plan.duration_samples
        ),
        "-map".into(),
        "[a]".into(),
        "-c:a".into(),
        "pcm_f32le".into(),
        arg(&root.join("mix-float.wav")),
    ]);
    ffmpeg(&mix)?;
    finish_pcm(&root.join("mix-float.wav"), &root.join("mix.wav"))?;
    ffmpeg(&[
        "-i".into(),
        arg(&root.join("picture.mkv")),
        "-i".into(),
        arg(&root.join("mix.wav")),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "1:a:0".into(),
        "-c".into(),
        "copy".into(),
        arg(&root.join("master.mkv")),
    ])?;
    ffmpeg(&[
        "-i".into(),
        arg(&root.join("master.mkv")),
        "-map".into(),
        "0:v:0".into(),
        "-map".into(),
        "0:a:0".into(),
        "-c:v".into(),
        "libx264".into(),
        "-crf".into(),
        "18".into(),
        "-pix_fmt".into(),
        "yuv420p".into(),
        "-c:a".into(),
        "aac".into(),
        "-b:a".into(),
        "192k".into(),
        "-movflags".into(),
        "+faststart".into(),
        arg(&root.join("review.mp4")),
    ])?;
    let names = [
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
    ];
    let mut outputs = BTreeMap::new();
    for name in names {
        let p = root.join(name);
        outputs.insert(
            name.into(),
            FileRef {
                path: name.into(),
                sha256: crate::sha256_file(&p)?,
                bytes: fs::metadata(p)?.len(),
            },
        );
    }
    let version = Command::new("ffmpeg").arg("-version").output()?;
    let receipt = Receipt {
        schema: "reel.scene-delivery-receipt.v0.1".into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        ffmpeg_version: String::from_utf8_lossy(&version.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .into(),
        content_samples: plan.duration_samples,
        delivery_frames: plan.frame_count,
        plan,
        outputs,
        publication: "not-authorized-by-tool".into(),
    };
    fs::write(
        root.join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    check(job_path, asset_root, root)?;
    // No result directory is published until all streams and hashes recheck.
    fs::rename(root, output)?;
    Ok(receipt)
}

pub fn check(job_path: &Path, asset_root: &Path, output: &Path) -> Result<Receipt> {
    let (job, plan) = plan(job_path, asset_root)?;
    let receipt: Receipt = serde_json::from_slice(&fs::read(output.join("receipt.json"))?)?;
    if receipt.schema != "reel.scene-delivery-receipt.v0.1"
        || serde_json::to_value(&receipt.plan)? != serde_json::to_value(&plan)?
        || receipt.content_samples != plan.duration_samples
        || receipt.delivery_frames != plan.frame_count
    {
        bail!("receipt does not match current compiled scene");
    }
    let expected = BTreeSet::from([
        "picture.mkv",
        "D.wav",
        "M.wav",
        "E.wav",
        "mix.wav",
        "master.mkv",
        "review.mp4",
    ]);
    if receipt
        .outputs
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>()
        != expected
    {
        bail!("receipt output set mismatch");
    }
    for (name, item) in &receipt.outputs {
        if item.path != Path::new(name) {
            bail!("output name mismatch");
        }
        checked_file(output, item)?;
    }
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav", "master.mkv"] {
        let info = probe(&output.join(name))?;
        let audio = info["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "audio"))
            .context("delivery audio stream missing")?;
        if audio["codec_name"] != "pcm_s24le"
            || audio["sample_rate"].as_str() != Some(&plan.sample_rate.to_string())
            || audio["channels"].as_u64() != Some(2)
        {
            bail!("delivery PCM format mismatch: {name}");
        }
        if pcm_samples(&output.join(name), plan.sample_rate)? != plan.duration_samples {
            bail!("decoded sample count mismatch: {name}");
        }
    }
    for name in ["picture.mkv", "master.mkv", "review.mp4"] {
        let o = Command::new("ffprobe")
            .args([
                "-v",
                "error",
                "-count_frames",
                "-show_streams",
                "-of",
                "json",
            ])
            .arg(output.join(name))
            .output()?;
        if !o.status.success() {
            bail!("frame decode failed");
        }
        let d: serde_json::Value = serde_json::from_slice(&o.stdout)?;
        let stream = d["streams"]
            .as_array()
            .and_then(|xs| xs.iter().find(|s| s["codec_type"] == "video"))
            .context("delivery video missing")?;
        let rate = stream["r_frame_rate"]
            .as_str()
            .context("video frame rate missing")?;
        let (rn, rd) = rate.split_once('/').context("invalid video frame rate")?;
        let (rn, rd) = (rn.parse::<u64>()?, rd.parse::<u64>()?);
        let codec = if name == "review.mp4" { "h264" } else { "ffv1" };
        if stream["codec_name"] != codec
            || rd == 0
            || u128::from(rn) * u128::from(plan.fps_denominator)
                != u128::from(rd) * u128::from(plan.fps_numerator)
        {
            bail!("delivery codec/frame rate mismatch: {name}");
        }
        if stream["nb_read_frames"]
            .as_str()
            .and_then(|s| s.parse::<u64>().ok())
            != Some(plan.frame_count)
            || stream["width"].as_u64() != Some(u64::from(job.width))
            || stream["height"].as_u64() != Some(u64::from(job.height))
        {
            bail!("delivery frame geometry mismatch: {name}");
        }
    }
    Ok(receipt)
}
