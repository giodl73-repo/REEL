use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::{Builder, TempDir};

use crate::{
    adapters::{
        ffmpeg::{FfmpegAdapter, RenderEnvironmentReport},
        still_animatic::{AnimaticInput, AnimaticRenderReport, check_animatic},
    },
    audio_mix::{self, CompiledDuckingPolicy, ResolvedGainAutomation},
    audio_quality,
    production::{self, AudioRole},
};

pub const AUDIO_PREVIEW_SCHEMA: &str = "reel.audio-preview-artifacts.v0.1";
pub const STEM_PACKAGE_SCHEMA: &str = "reel.audio-stem-package.v0.1";
pub const PICTURE_REMUX_SCHEMA: &str = "reel.picture-remux-artifacts.v0.1";

#[derive(Clone, Debug)]
pub struct AudioPreviewOptions {
    pub manifest: PathBuf,
    pub asset_root: PathBuf,
    pub output: PathBuf,
    pub dry_run: bool,
    pub stems_dir: Option<PathBuf>,
    pub sample_rate_hz: u32,
    pub channels: u8,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AudioPreviewReport {
    pub schema: String,
    pub work: String,
    pub manifest_sha256: String,
    pub audio_policy_sha256: String,
    pub output: String,
    pub artifact_report: String,
    pub duration_ms: u64,
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub render_environment: Option<RenderEnvironmentReport>,
    pub inputs: Vec<AnimaticInput>,
    pub command_arguments: Vec<String>,
    pub dry_run: bool,
    pub output_sha256: Option<String>,
    pub output_bytes: Option<u64>,
    pub output_duration_ms: Option<u64>,
    #[serde(default)]
    pub resolved_gain_automation: Vec<ResolvedGainAutomation>,
    #[serde(default)]
    pub ducking_policies: Vec<CompiledDuckingPolicy>,
    #[serde(default = "default_true")]
    pub dynamic_eq_render_supported: bool,
    #[serde(default)]
    pub stem_package: Option<StemPackageReference>,
    #[serde(default)]
    pub stem_command_arguments: Vec<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StemPackageReference {
    pub directory: String,
    pub receipt_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StemSourceHash {
    pub id: String,
    pub role: AudioRole,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StemArtifact {
    pub id: String,
    pub filename: String,
    pub stage: String,
    pub sha256: String,
    pub bytes: u64,
    pub sample_rate_hz: u32,
    pub channels: u8,
    pub sample_count: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecombinationEvidence {
    pub expression: String,
    pub tolerance_lsb: u32,
    pub maximum_error_lsb: u32,
    pub samples_checked: u64,
    pub passed: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StemPackageReceipt {
    pub schema: String,
    pub work: String,
    pub manifest_sha256: String,
    pub audio_policy_sha256: String,
    pub tool_version: String,
    pub ffmpeg_version: String,
    pub duration_ms: u64,
    pub sample_rate_hz: u32,
    pub bit_depth: u8,
    pub channels: u8,
    pub sample_count: u64,
    pub mastering_semantics: String,
    pub sources: Vec<StemSourceHash>,
    pub resolved_gain_automation: Vec<ResolvedGainAutomation>,
    pub ducking_policies: Vec<CompiledDuckingPolicy>,
    pub outputs: Vec<StemArtifact>,
    pub recombination: RecombinationEvidence,
    pub quality_evidence: Option<DialogueMixQualityEvidence>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DialogueMixQualityEvidence {
    pub policy: production::AudioReviewPolicy,
    pub dialogue_gated_loudness_lufs: f64,
    pub speech_active_windows: u64,
    pub window_ms: u64,
    pub minimum_speech_to_background_margin_db: Option<f64>,
    pub mono_loss_db: f64,
    pub mastered_peak_dbfs: f64,
    pub small_speaker_proxy_non_silent: bool,
    pub violations: Vec<String>,
    pub passed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AudioPreviewCheckReport {
    pub artifact_report: String,
    pub work: String,
    pub output_sha256: String,
    pub duration_ms: u64,
    pub audio_events: usize,
    pub verified: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PictureRemuxReport {
    pub schema: String,
    pub tool_version: String,
    pub work: String,
    pub picture_artifact: String,
    pub picture_artifact_sha256: String,
    pub picture_output_sha256: String,
    pub audio_artifact: String,
    pub audio_artifact_sha256: String,
    pub audio_output_sha256: String,
    pub output: String,
    pub output_sha256: String,
    pub output_bytes: u64,
    pub output_duration_ms: u64,
    pub video_codec_mode: String,
    pub verified: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct PictureRemuxCheckReport {
    pub artifact_report: String,
    pub work: String,
    pub output_sha256: String,
    pub output_duration_ms: u64,
    pub video_codec_mode: String,
    pub verified: bool,
}

pub fn render_audio_preview(options: &AudioPreviewOptions) -> Result<AudioPreviewReport> {
    let loaded = production::require_timing_ready(&options.manifest)?;
    if loaded.manifest.audio_events.is_empty() {
        bail!("audio-only rendering requires manifest audio_events");
    }
    if options.output.extension().and_then(|value| value.to_str()) != Some("m4a") {
        bail!("audio-only output must use the .m4a extension");
    }
    if !(8_000..=192_000).contains(&options.sample_rate_hz) {
        bail!("stem sample rate must be between 8000 and 192000 Hz");
    }
    if !matches!(options.channels, 1 | 2) {
        bail!("stem channel count must be 1 or 2");
    }
    if options.stems_dir.as_ref().is_some_and(|path| path.exists()) {
        bail!("refusing to overwrite existing stems directory");
    }
    let artifact_path = options.output.with_extension("audio-artifacts.json");
    if options.output.exists() || artifact_path.exists() {
        bail!(
            "refusing to overwrite existing audio preview or report: {}",
            options.output.display()
        );
    }
    let duration_ms = production::validate(&loaded)?
        .duration_ms
        .ok_or_else(|| anyhow!("audio-only rendering requires a timed manifest"))?;
    let timeline_seconds = duration_ms as f64 / 1000.0;
    let manifest_path = options.manifest.canonicalize()?;
    let asset_root = options.asset_root.canonicalize().with_context(|| {
        format!(
            "failed to resolve asset root {}",
            options.asset_root.display()
        )
    })?;
    let adapter = FfmpegAdapter;
    let mut inputs = vec![AnimaticInput {
        kind: "manifest".to_string(),
        id: loaded.manifest.work.clone(),
        path: manifest_path.display().to_string(),
        sha256: production::sha256_path(&manifest_path)?,
    }];
    let mut args = vec![
        "-y".to_string(),
        "-hide_banner".to_string(),
        "-loglevel".to_string(),
        "warning".to_string(),
    ];
    for event in &loaded.manifest.audio_events {
        let candidate = if Path::new(&event.source).is_absolute() {
            PathBuf::from(&event.source)
        } else {
            asset_root.join(&event.source)
        };
        let resolved = candidate.canonicalize().with_context(|| {
            format!(
                "missing source for audio event {}: {}",
                event.id,
                candidate.display()
            )
        })?;
        if !Path::new(&event.source).is_absolute() && !resolved.starts_with(&asset_root) {
            bail!("audio event {} source escapes asset root", event.id);
        }
        inputs.push(AnimaticInput {
            kind: format!("audio-{}", audio_role_name(event.role)),
            id: event.id.clone(),
            path: resolved.display().to_string(),
            sha256: production::sha256_path(&resolved)?,
        });
        if event.loop_source {
            args.extend(["-stream_loop".to_string(), "-1".to_string()]);
        }
        args.extend(["-i".to_string(), adapter.path_argument(&resolved)?]);
    }
    let input_args = args.clone();
    let compiled = audio_mix::compile(&loaded.manifest, timeline_seconds, 0, false, 48_000, 2)?;
    if !compiled.dynamic_eq_render_supported && !options.dry_run {
        bail!(
            "dynamic_eq is validated and compiled as policy, but portable rendering is not implemented"
        );
    }
    let filters = compiled.filters.clone();
    let filter_graph = filters.join(";");
    args.extend([
        "-filter_complex".to_string(),
        filter_graph.clone(),
        "-map".to_string(),
        "[finala]".to_string(),
        "-vn".to_string(),
        "-c:a".to_string(),
        "aac".to_string(),
        "-b:a".to_string(),
        "192k".to_string(),
        "-movflags".to_string(),
        "+faststart".to_string(),
        adapter.path_argument(&options.output)?,
    ]);
    let output_parent = options
        .output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let absolute_output = output_parent.canonicalize()?.join(
        options
            .output
            .file_name()
            .ok_or_else(|| anyhow!("output path has no filename"))?,
    );
    let render_environment = if options.dry_run {
        None
    } else {
        Some(adapter.render_environment()?)
    };
    let ffmpeg_version = render_environment
        .as_ref()
        .map(|value| value.ffmpeg_version.clone())
        .unwrap_or_else(|| "not-probed-dry-run".to_string());
    let mut rendered_temp = None;
    let (output_sha256, output_bytes, output_duration_ms) = if options.dry_run {
        (None, None, None)
    } else {
        let temp = Builder::new()
            .prefix(".reel-audio-")
            .suffix(".m4a")
            .tempfile_in(output_parent)?
            .into_temp_path();
        let mut render_args = args.clone();
        let mut script = Builder::new()
            .prefix(".reel-audio-filter-")
            .suffix(".txt")
            .tempfile_in(output_parent)?;
        script.write_all(filter_graph.as_bytes())?;
        script.flush()?;
        let filter_index = render_args
            .iter()
            .position(|argument| argument == "-filter_complex")
            .expect("filter graph exists");
        render_args[filter_index] = "-filter_complex_script".to_string();
        render_args[filter_index + 1] = adapter.path_argument(script.path())?;
        *render_args.last_mut().expect("output exists") = adapter.path_argument(&temp)?;
        adapter.run_ffmpeg(&render_args, &[])?;
        let measured_ms =
            (adapter.ffprobe_duration(&temp)?.parse::<f64>()? * 1000.0).round() as u64;
        if measured_ms.abs_diff(duration_ms) > 50 {
            bail!("audio preview duration differs from manifest timeline by more than 50ms");
        }
        let measured = (
            Some(production::sha256_path(&temp)?),
            Some(fs::metadata(&temp)?.len()),
            Some(measured_ms),
        );
        rendered_temp = Some(temp);
        measured
    };
    let legacy_policy_shape = loaded.manifest.audio_ducking.is_empty()
        && loaded.manifest.audio_review_policy.is_none()
        && loaded
            .manifest
            .audio_events
            .iter()
            .all(|event| event.gain_automation.is_empty() && event.role != AudioRole::Dialogue);
    let policy = if legacy_policy_shape {
        serde_json::json!({
            "duration_ms": duration_ms,
            "audio_events": loaded.manifest.audio_events,
            "narration_ducking": loaded.manifest.narration_ducking,
            "audio_mastering": loaded.manifest.audio_mastering,
        })
    } else {
        serde_json::json!({
            "duration_ms": duration_ms,
            "audio_events": loaded.manifest.audio_events,
            "narration_ducking": loaded.manifest.narration_ducking,
            "audio_ducking": loaded.manifest.audio_ducking,
            "audio_mastering": loaded.manifest.audio_mastering,
            "audio_review_policy": loaded.manifest.audio_review_policy,
            "resolved_gain_automation": &compiled.resolved_automation,
            "compiled_ducking": &compiled.ducking,
        })
    };
    let audio_policy_sha256 = sha256_bytes(&serde_json::to_vec(&policy)?);
    let (stem_temp, stem_package, stem_command_arguments) =
        if let Some(stems_dir) = &options.stems_dir {
            if options.dry_run {
                let (_, command) = stem_render_command(
                    &adapter,
                    &input_args,
                    &loaded.manifest,
                    timeline_seconds,
                    options.sample_rate_hz,
                    options.channels,
                    stems_dir,
                )?;
                (None, None, command)
            } else {
                let parent = stems_dir
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                fs::create_dir_all(parent)?;
                let temp = Builder::new().prefix(".reel-stems-").tempdir_in(parent)?;
                let (receipt, command) = render_stem_package(
                    &adapter,
                    &input_args,
                    &loaded.manifest,
                    timeline_seconds,
                    duration_ms,
                    options.sample_rate_hz,
                    options.channels,
                    &temp,
                    &inputs,
                    &production::sha256_path(&manifest_path)?,
                    &audio_policy_sha256,
                    &ffmpeg_version,
                )?;
                (Some(temp), Some(receipt), command)
            }
        } else {
            (None, None, Vec::new())
        };
    let stem_package_reference =
        if let (Some(stems_dir), Some(receipt)) = (&options.stems_dir, &stem_package) {
            Some(StemPackageReference {
                directory: absolute_path(stems_dir)?.display().to_string(),
                receipt_sha256: sha256_bytes(&serde_json::to_vec_pretty(receipt)?),
            })
        } else {
            None
        };
    let report = AudioPreviewReport {
        schema: AUDIO_PREVIEW_SCHEMA.to_string(),
        work: loaded.manifest.work,
        manifest_sha256: production::sha256_path(&manifest_path)?,
        audio_policy_sha256,
        output: absolute_output.display().to_string(),
        artifact_report: absolute_output
            .with_extension("audio-artifacts.json")
            .display()
            .to_string(),
        duration_ms,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        ffmpeg_version,
        render_environment,
        inputs,
        command_arguments: args,
        dry_run: options.dry_run,
        output_sha256,
        output_bytes,
        output_duration_ms,
        resolved_gain_automation: compiled.resolved_automation,
        ducking_policies: compiled.ducking,
        dynamic_eq_render_supported: compiled.dynamic_eq_render_supported,
        stem_package: stem_package_reference,
        stem_command_arguments,
    };
    let mut report_temp = Builder::new()
        .prefix(".reel-audio-artifacts-")
        .tempfile_in(output_parent)?;
    report_temp.write_all(&serde_json::to_vec_pretty(&report)?)?;
    report_temp.flush()?;
    if let Some(temp) = rendered_temp {
        temp.persist_noclobber(&options.output)?;
    }
    if let (Some(temp), Some(stems_dir)) = (stem_temp, &options.stems_dir) {
        let temp_path = temp.keep();
        if let Err(error) = fs::rename(&temp_path, stems_dir) {
            if !options.dry_run {
                let _ = fs::remove_file(&options.output);
            }
            let _ = fs::remove_dir_all(&temp_path);
            return Err(error).context("failed to publish stem package atomically");
        }
    }
    if let Err(error) = report_temp.persist_noclobber(&artifact_path) {
        if !options.dry_run {
            let _ = fs::remove_file(&options.output);
            if let Some(stems_dir) = &options.stems_dir {
                let _ = fs::remove_dir_all(stems_dir);
            }
        }
        return Err(error.error).context("failed to publish audio preview report atomically");
    }
    Ok(report)
}

pub fn check_audio_preview(report_path: impl AsRef<Path>) -> Result<AudioPreviewCheckReport> {
    let report_path = report_path.as_ref().canonicalize()?;
    let report: AudioPreviewReport = serde_json::from_slice(&fs::read(&report_path)?)
        .context("audio preview report is not valid JSON")?;
    if report.schema != AUDIO_PREVIEW_SCHEMA || report.dry_run {
        bail!("unsupported or dry-run audio preview report");
    }
    let output = PathBuf::from(&report.output).canonicalize()?;
    let output_sha256 = production::sha256_path(&output)?;
    if report.output_sha256.as_deref() != Some(&output_sha256)
        || report.output_bytes != Some(fs::metadata(&output)?.len())
    {
        bail!("audio preview output does not match its report");
    }
    for input in &report.inputs {
        if production::sha256_path(&input.path)? != input.sha256 {
            bail!("audio preview input {} does not match its report", input.id);
        }
    }
    let actual_ms =
        (FfmpegAdapter.ffprobe_duration(&output)?.parse::<f64>()? * 1000.0).round() as u64;
    if report.output_duration_ms != Some(actual_ms) || actual_ms.abs_diff(report.duration_ms) > 50 {
        bail!("audio preview duration does not match its report");
    }
    if let Some(package) = &report.stem_package {
        check_stem_package(&report, package)?;
    }
    Ok(AudioPreviewCheckReport {
        artifact_report: report_path.display().to_string(),
        work: report.work,
        output_sha256,
        duration_ms: actual_ms,
        audio_events: report
            .inputs
            .iter()
            .filter(|input| input.kind.starts_with("audio-"))
            .count(),
        verified: true,
    })
}

#[allow(clippy::too_many_arguments)]
fn render_stem_package(
    adapter: &FfmpegAdapter,
    input_args: &[String],
    manifest: &production::ProductionManifest,
    timeline_seconds: f64,
    duration_ms: u64,
    sample_rate_hz: u32,
    channels: u8,
    temp: &TempDir,
    inputs: &[AnimaticInput],
    manifest_sha256: &str,
    audio_policy_sha256: &str,
    ffmpeg_version: &str,
) -> Result<(StemPackageReceipt, Vec<String>)> {
    let (compiled, mut command) = stem_render_command(
        adapter,
        input_args,
        manifest,
        timeline_seconds,
        sample_rate_hz,
        channels,
        temp.path(),
    )?;
    if !compiled.dynamic_eq_render_supported {
        bail!(
            "dynamic_eq is validated and compiled as policy, but portable rendering is not implemented"
        );
    }
    let filter_index = command
        .iter()
        .position(|argument| argument == "-filter_complex")
        .expect("stem command has filter graph");
    let filter_graph = command[filter_index + 1].clone();
    let mut script = Builder::new()
        .prefix(".reel-stem-filter-")
        .suffix(".txt")
        .tempfile_in(temp.path())?;
    script.write_all(filter_graph.as_bytes())?;
    script.flush()?;
    command[filter_index] = "-filter_complex_script".into();
    command[filter_index + 1] = adapter.path_argument(script.path())?;
    adapter.run_ffmpeg(&command, &[])?;

    let names = [
        ("D", "dialogue.pre-master.wav", "pre-master", channels),
        ("M", "music.pre-master.wav", "pre-master", channels),
        ("E", "effects.pre-master.wav", "pre-master", channels),
        ("pre-master", "mix.pre-master.wav", "pre-master", channels),
        ("full-mix", "mix.mastered.wav", "mastered", channels),
        ("no-score", "review.no-score.wav", "review", channels),
        ("mono", "review.mono.wav", "review", 1),
        ("small-speaker", "review.small-speaker.wav", "review", 1),
    ];
    let mut outputs = Vec::new();
    let mut pcm = BTreeMap::new();
    for (id, filename, stage, expected_channels) in names {
        let path = temp.path().join(filename);
        let wav = read_pcm24_wav(&path)?;
        if wav.sample_rate_hz != sample_rate_hz || wav.channels != expected_channels {
            bail!("stem {id} has unexpected sample rate or channel layout");
        }
        pcm.insert(id, wav.samples);
        outputs.push(StemArtifact {
            id: id.into(),
            filename: filename.into(),
            stage: stage.into(),
            sha256: production::sha256_path(&path)?,
            bytes: fs::metadata(&path)?.len(),
            sample_rate_hz,
            channels: expected_channels,
            sample_count: wav.sample_count,
        });
    }
    let expected_sample_count = (timeline_seconds * sample_rate_hz as f64).round() as u64;
    if outputs
        .iter()
        .any(|output| output.sample_count != expected_sample_count)
    {
        bail!("stem/full-mix outputs do not share exact sample geometry");
    }
    let recombination = recombination_evidence(
        pcm["D"].as_slice(),
        pcm["M"].as_slice(),
        pcm["E"].as_slice(),
        pcm["pre-master"].as_slice(),
    )?;
    if !recombination.passed {
        bail!("D+M+E does not recombine to the declared pre-master within tolerance");
    }
    let quality_evidence = manifest
        .audio_review_policy
        .as_ref()
        .map(|policy| {
            let dialogue_lufs =
                audio_quality::analyze_audio(&temp.path().join("dialogue.pre-master.wav"))?
                    .0
                    .integrated_lufs;
            dialogue_mix_quality_evidence(
                policy,
                dialogue_lufs,
                &pcm["D"],
                &pcm["M"],
                &pcm["E"],
                &pcm["full-mix"],
                &pcm["mono"],
                &pcm["small-speaker"],
                sample_rate_hz,
                channels,
            )
        })
        .transpose()?;
    let receipt = StemPackageReceipt {
        schema: STEM_PACKAGE_SCHEMA.into(),
        work: manifest.work.clone(),
        manifest_sha256: manifest_sha256.into(),
        audio_policy_sha256: audio_policy_sha256.into(),
        tool_version: env!("CARGO_PKG_VERSION").into(),
        ffmpeg_version: ffmpeg_version.into(),
        duration_ms,
        sample_rate_hz,
        bit_depth: 24,
        channels,
        sample_count: expected_sample_count,
        mastering_semantics:
            "D/M/E and mix.pre-master are post-routing/post-ducking and pre-mastering; mix.mastered applies declared mastering"
                .into(),
        sources: inputs
            .iter()
            .skip(1)
            .map(|input| StemSourceHash {
                id: input.id.clone(),
                role: manifest
                    .audio_events
                    .iter()
                    .find(|event| event.id == input.id)
                    .expect("input came from manifest event")
                    .role,
                sha256: input.sha256.clone(),
            })
            .collect(),
        resolved_gain_automation: compiled.resolved_automation,
        ducking_policies: compiled.ducking,
        outputs,
        recombination,
        quality_evidence,
    };
    fs::write(
        temp.path().join("receipt.json"),
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    Ok((receipt, command))
}

fn stem_render_command(
    adapter: &FfmpegAdapter,
    input_args: &[String],
    manifest: &production::ProductionManifest,
    timeline_seconds: f64,
    sample_rate_hz: u32,
    channels: u8,
    output_dir: &Path,
) -> Result<(audio_mix::CompiledAudioMix, Vec<String>)> {
    stem_render_command_with_paths(
        input_args,
        manifest,
        timeline_seconds,
        sample_rate_hz,
        channels,
        output_dir,
        &|path| adapter.path_argument(path),
    )
}

fn stem_render_command_with_paths(
    input_args: &[String],
    manifest: &production::ProductionManifest,
    timeline_seconds: f64,
    sample_rate_hz: u32,
    channels: u8,
    output_dir: &Path,
    path_argument: &dyn Fn(&Path) -> Result<String>,
) -> Result<(audio_mix::CompiledAudioMix, Vec<String>)> {
    let compiled = audio_mix::compile(
        manifest,
        timeline_seconds,
        0,
        true,
        sample_rate_hz,
        channels,
    )?;
    let stems = compiled
        .stems
        .as_ref()
        .ok_or_else(|| anyhow!("stem compiler did not expose stem labels"))?;
    let mut command = input_args.to_vec();
    command.extend(["-filter_complex".into(), compiled.filters.join(";")]);
    let mappings = [
        (&stems.dialogue, "dialogue.pre-master.wav", channels),
        (&stems.music, "music.pre-master.wav", channels),
        (&stems.effects, "effects.pre-master.wav", channels),
        (&stems.pre_master, "mix.pre-master.wav", channels),
        (&compiled.final_label, "mix.mastered.wav", channels),
        (&stems.no_score, "review.no-score.wav", channels),
        (&stems.mono_review, "review.mono.wav", 1),
        (&stems.small_speaker_review, "review.small-speaker.wav", 1),
    ];
    for (label, filename, output_channels) in mappings {
        command.extend([
            "-map".into(),
            format!("[{label}]"),
            "-c:a".into(),
            "pcm_s24le".into(),
            "-ar".into(),
            sample_rate_hz.to_string(),
            "-ac".into(),
            output_channels.to_string(),
            "-t".into(),
            format!("{timeline_seconds:.9}"),
            path_argument(&output_dir.join(filename))?,
        ]);
    }
    Ok((compiled, command))
}

fn check_stem_package(report: &AudioPreviewReport, package: &StemPackageReference) -> Result<()> {
    let directory = PathBuf::from(&package.directory).canonicalize()?;
    let receipt_path = directory.join("receipt.json");
    let receipt_bytes = fs::read(&receipt_path)?;
    if sha256_bytes(&receipt_bytes) != package.receipt_sha256 {
        bail!("stem package receipt does not match the audio artifact report");
    }
    let receipt: StemPackageReceipt =
        serde_json::from_slice(&receipt_bytes).context("stem package receipt is not valid JSON")?;
    if receipt.schema != STEM_PACKAGE_SCHEMA
        || receipt.work != report.work
        || receipt.manifest_sha256 != report.manifest_sha256
        || receipt.audio_policy_sha256 != report.audio_policy_sha256
        || receipt.tool_version != report.tool_version
        || receipt.duration_ms != report.duration_ms
        || receipt.bit_depth != 24
    {
        bail!("stem package lineage does not match the audio artifact report");
    }
    let mut pcm = BTreeMap::new();
    for output in &receipt.outputs {
        if output.filename.contains('/') || output.filename.contains('\\') {
            bail!("stem receipt filenames must be path-free basenames");
        }
        let path = directory.join(&output.filename);
        if production::sha256_path(&path)? != output.sha256
            || fs::metadata(&path)?.len() != output.bytes
        {
            bail!("stem output {} does not match its receipt", output.id);
        }
        let wav = read_pcm24_wav(&path)?;
        if wav.sample_rate_hz != receipt.sample_rate_hz
            || wav.channels != output.channels
            || wav.sample_count != receipt.sample_count
            || output.sample_rate_hz != receipt.sample_rate_hz
            || output.sample_count != receipt.sample_count
        {
            bail!("stem output {} has stale sample geometry", output.id);
        }
        pcm.insert(output.id.as_str(), wav.samples);
    }
    for required in [
        "D",
        "M",
        "E",
        "pre-master",
        "full-mix",
        "no-score",
        "mono",
        "small-speaker",
    ] {
        if !pcm.contains_key(required) {
            bail!("stem package is missing {required}");
        }
    }
    let recombination =
        recombination_evidence(&pcm["D"], &pcm["M"], &pcm["E"], &pcm["pre-master"])?;
    if !recombination.passed
        || recombination.maximum_error_lsb != receipt.recombination.maximum_error_lsb
        || recombination.samples_checked != receipt.recombination.samples_checked
    {
        bail!("stem recombination evidence does not match current outputs");
    }
    if let Some(expected) = &receipt.quality_evidence {
        let dialogue_path = directory.join(
            &receipt
                .outputs
                .iter()
                .find(|item| item.id == "D")
                .expect("required D output")
                .filename,
        );
        let dialogue_lufs = audio_quality::analyze_audio(&dialogue_path)?
            .0
            .integrated_lufs;
        let actual = dialogue_mix_quality_evidence(
            &expected.policy,
            dialogue_lufs,
            &pcm["D"],
            &pcm["M"],
            &pcm["E"],
            &pcm["full-mix"],
            &pcm["mono"],
            &pcm["small-speaker"],
            receipt.sample_rate_hz,
            receipt.channels,
        )?;
        if serde_json::to_vec(&actual)? != serde_json::to_vec(expected)? {
            bail!("dialogue mix quality evidence does not match current outputs");
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Pcm24Wav {
    sample_rate_hz: u32,
    channels: u8,
    sample_count: u64,
    samples: Vec<i32>,
}

fn read_pcm24_wav(path: &Path) -> Result<Pcm24Wav> {
    let bytes = fs::read(path)?;
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("{} is not a RIFF/WAVE file", path.display());
    }
    let mut cursor = 12usize;
    let mut format = None;
    let mut data = None;
    while cursor + 8 <= bytes.len() {
        let id = &bytes[cursor..cursor + 4];
        let size = u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into()?) as usize;
        let start = cursor + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| anyhow!("WAV chunk size overflow"))?;
        if end > bytes.len() {
            bail!("WAV chunk exceeds file size");
        }
        if id == b"fmt " && size >= 16 {
            let raw_codec = u16::from_le_bytes(bytes[start..start + 2].try_into()?);
            let codec = if raw_codec == 0xfffe
                && size >= 40
                && u16::from_le_bytes(bytes[start + 24..start + 26].try_into()?) == 1
            {
                1
            } else {
                raw_codec
            };
            format = Some((
                codec,
                u16::from_le_bytes(bytes[start + 2..start + 4].try_into()?),
                u32::from_le_bytes(bytes[start + 4..start + 8].try_into()?),
                u16::from_le_bytes(bytes[start + 12..start + 14].try_into()?),
                u16::from_le_bytes(bytes[start + 14..start + 16].try_into()?),
            ));
        } else if id == b"data" {
            data = Some(&bytes[start..end]);
        }
        cursor = end + (size & 1);
    }
    let (codec, channels, sample_rate_hz, block_align, bits) =
        format.ok_or_else(|| anyhow!("WAV lacks fmt chunk"))?;
    if codec != 1 || bits != 24 || channels == 0 || channels > 2 || block_align != channels * 3 {
        bail!("WAV is not 24-bit mono/stereo PCM");
    }
    let data = data.ok_or_else(|| anyhow!("WAV lacks data chunk"))?;
    if data.len() % block_align as usize != 0 {
        bail!("WAV data is not sample-frame aligned");
    }
    let samples = data
        .chunks_exact(3)
        .map(|chunk| {
            let raw = (chunk[0] as i32) | ((chunk[1] as i32) << 8) | ((chunk[2] as i32) << 16);
            if raw & 0x80_0000 != 0 {
                raw | !0x00ff_ffff
            } else {
                raw
            }
        })
        .collect::<Vec<_>>();
    Ok(Pcm24Wav {
        sample_rate_hz,
        channels: channels as u8,
        sample_count: (data.len() / block_align as usize) as u64,
        samples,
    })
}

fn recombination_evidence(
    dialogue: &[i32],
    music: &[i32],
    effects: &[i32],
    premaster: &[i32],
) -> Result<RecombinationEvidence> {
    if dialogue.len() != music.len()
        || dialogue.len() != effects.len()
        || dialogue.len() != premaster.len()
    {
        bail!("D/M/E and pre-master sample buffers have different geometry");
    }
    const TOLERANCE_LSB: u32 = 3;
    let maximum_error_lsb = dialogue
        .iter()
        .zip(music)
        .zip(effects)
        .zip(premaster)
        .map(|(((d, m), e), expected)| {
            let sum = (*d as i64 + *m as i64 + *e as i64).clamp(-8_388_608, 8_388_607);
            (sum - *expected as i64).unsigned_abs() as u32
        })
        .max()
        .unwrap_or(0);
    Ok(RecombinationEvidence {
        expression: "clamp(PCM24(D) + PCM24(M) + PCM24(E)) == PCM24(pre-master)".into(),
        tolerance_lsb: TOLERANCE_LSB,
        maximum_error_lsb,
        samples_checked: premaster.len() as u64,
        passed: maximum_error_lsb <= TOLERANCE_LSB,
    })
}

#[allow(clippy::too_many_arguments)]
fn dialogue_mix_quality_evidence(
    policy: &production::AudioReviewPolicy,
    dialogue_lufs: f64,
    dialogue: &[i32],
    music: &[i32],
    effects: &[i32],
    mastered: &[i32],
    mono: &[i32],
    small_speaker: &[i32],
    sample_rate_hz: u32,
    channels: u8,
) -> Result<DialogueMixQualityEvidence> {
    if channels == 0
        || dialogue.len() != music.len()
        || dialogue.len() != effects.len()
        || dialogue.len() != mastered.len()
        || dialogue.len() / channels as usize != mono.len()
        || mono.len() != small_speaker.len()
    {
        bail!("quality evidence inputs do not share compatible sample geometry");
    }
    if !dialogue_lufs.is_finite() {
        bail!("dialogue-gated loudness measurement is not finite");
    }
    let frame_count = dialogue.len() / channels as usize;
    let window_frames = (sample_rate_hz / 10).max(1) as usize;
    let mut margins = Vec::new();
    for frame_start in (0..frame_count).step_by(window_frames) {
        let frame_end = (frame_start + window_frames).min(frame_count);
        let sample_start = frame_start * channels as usize;
        let sample_end = frame_end * channels as usize;
        let d_rms = normalized_rms(&dialogue[sample_start..sample_end]);
        let d_dbfs = amplitude_dbfs(d_rms);
        if d_dbfs >= policy.speech_activity_threshold_dbfs {
            let background = music[sample_start..sample_end]
                .iter()
                .zip(&effects[sample_start..sample_end])
                .map(|(m, e)| (*m as i64 + *e as i64).clamp(-8_388_608, 8_388_607) as i32)
                .collect::<Vec<_>>();
            let background_rms = normalized_rms(&background);
            margins.push(20.0 * (d_rms.max(1e-12) / background_rms.max(1e-12)).log10());
        }
    }
    let minimum_margin = margins.iter().copied().reduce(f64::min);
    let mastered_rms = normalized_rms(mastered);
    let mono_rms = normalized_rms(mono);
    let mono_loss_db = if mastered_rms <= 1e-12 {
        0.0
    } else {
        (20.0 * (mastered_rms / mono_rms.max(1e-12)).log10()).max(0.0)
    };
    let peak_sample = mastered
        .iter()
        .map(|sample| (*sample as i64).unsigned_abs())
        .max()
        .unwrap_or(0);
    let peak = peak_sample as f64 / 8_388_608.0;
    let mastered_peak_dbfs = amplitude_dbfs(peak);
    let mut violations = Vec::new();
    if (dialogue_lufs - policy.dialogue_loudness_target_lufs).abs()
        > policy.dialogue_loudness_tolerance_lu
    {
        violations.push("dialogue-gated-loudness".into());
    }
    match minimum_margin {
        Some(value) if value >= policy.minimum_speech_to_background_margin_db => {}
        Some(_) => violations.push("speech-to-background-margin".into()),
        None => violations.push("no-speech-active-windows".into()),
    }
    if mono_loss_db > policy.maximum_mono_loss_db {
        violations.push("mono-downmix-loss".into());
    }
    if peak_sample >= 8_388_607 {
        violations.push("clipping".into());
    }
    let small_speaker_proxy_non_silent = normalized_rms(small_speaker) > 1e-12;
    if !small_speaker_proxy_non_silent {
        violations.push("small-speaker-proxy-silent".into());
    }
    Ok(DialogueMixQualityEvidence {
        policy: policy.clone(),
        dialogue_gated_loudness_lufs: dialogue_lufs,
        speech_active_windows: margins.len() as u64,
        window_ms: 100,
        minimum_speech_to_background_margin_db: minimum_margin,
        mono_loss_db,
        mastered_peak_dbfs,
        small_speaker_proxy_non_silent,
        passed: violations.is_empty(),
        violations,
    })
}

fn normalized_rms(samples: &[i32]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let square_sum = samples
        .iter()
        .map(|sample| {
            let normalized = *sample as f64 / 8_388_608.0;
            normalized * normalized
        })
        .sum::<f64>();
    (square_sum / samples.len() as f64).sqrt()
}

fn amplitude_dbfs(amplitude: f64) -> f64 {
    20.0 * amplitude.max(1e-12).log10()
}

fn absolute_path(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.canonicalize()?.join(
        path.file_name()
            .ok_or_else(|| anyhow!("path has no final component"))?,
    ))
}

pub fn remux_picture(
    picture_artifact: impl AsRef<Path>,
    audio_artifact: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<PictureRemuxReport> {
    let picture_artifact = picture_artifact.as_ref().canonicalize()?;
    let audio_artifact = audio_artifact.as_ref().canonicalize()?;
    check_animatic(&picture_artifact)?;
    check_audio_preview(&audio_artifact)?;
    let picture: AnimaticRenderReport = serde_json::from_slice(&fs::read(&picture_artifact)?)?;
    let audio: AudioPreviewReport = serde_json::from_slice(&fs::read(&audio_artifact)?)?;
    if picture.work != audio.work {
        bail!("picture and audio artifacts belong to different works");
    }
    let picture_duration = picture
        .output_duration_ms
        .ok_or_else(|| anyhow!("picture artifact has no measured duration"))?;
    let audio_duration = audio
        .output_duration_ms
        .ok_or_else(|| anyhow!("audio artifact has no measured duration"))?;
    if picture_duration.abs_diff(audio_duration) > 50 {
        bail!("picture and audio durations differ by more than 50ms");
    }
    let output = output.as_ref();
    let report_path = output.with_extension("remux-artifacts.json");
    if output.exists() || report_path.exists() {
        bail!(
            "refusing to overwrite remux output or report: {}",
            output.display()
        );
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let picture_output = PathBuf::from(&picture.output).canonicalize()?;
    let audio_output = PathBuf::from(&audio.output).canonicalize()?;
    let adapter = FfmpegAdapter;
    let temp = Builder::new()
        .prefix(".reel-remux-")
        .suffix(".mp4")
        .tempfile_in(parent)?
        .into_temp_path();
    adapter.run_ffmpeg(
        &[
            "-y".to_string(),
            "-hide_banner".to_string(),
            "-loglevel".to_string(),
            "warning".to_string(),
            "-i".to_string(),
            adapter.path_argument(&picture_output)?,
            "-i".to_string(),
            adapter.path_argument(&audio_output)?,
            "-map".to_string(),
            "0:v:0".to_string(),
            "-map".to_string(),
            "1:a:0".to_string(),
            "-c:v".to_string(),
            "copy".to_string(),
            "-c:a".to_string(),
            "copy".to_string(),
            "-shortest".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
            adapter.path_argument(&temp)?,
        ],
        &[],
    )?;
    let actual_ms = (adapter.ffprobe_duration(&temp)?.parse::<f64>()? * 1000.0).round() as u64;
    if actual_ms.abs_diff(picture_duration.min(audio_duration)) > 50 {
        bail!("remux duration differs from its picture/audio sources");
    }
    let report = PictureRemuxReport {
        schema: PICTURE_REMUX_SCHEMA.to_string(),
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
        work: picture.work,
        picture_artifact: picture_artifact.display().to_string(),
        picture_artifact_sha256: production::sha256_path(&picture_artifact)?,
        picture_output_sha256: production::sha256_path(&picture_output)?,
        audio_artifact: audio_artifact.display().to_string(),
        audio_artifact_sha256: production::sha256_path(&audio_artifact)?,
        audio_output_sha256: production::sha256_path(&audio_output)?,
        output: parent
            .canonicalize()?
            .join(
                output
                    .file_name()
                    .ok_or_else(|| anyhow!("output path has no filename"))?,
            )
            .display()
            .to_string(),
        output_sha256: production::sha256_path(&temp)?,
        output_bytes: fs::metadata(&temp)?.len(),
        output_duration_ms: actual_ms,
        video_codec_mode: "stream-copy".to_string(),
        verified: true,
    };
    let mut report_temp = Builder::new()
        .prefix(".reel-remux-artifacts-")
        .tempfile_in(parent)?;
    report_temp.write_all(&serde_json::to_vec_pretty(&report)?)?;
    report_temp.flush()?;
    temp.persist_noclobber(output)?;
    if let Err(error) = report_temp.persist_noclobber(&report_path) {
        let _ = fs::remove_file(output);
        return Err(error.error).context("failed to publish remux report atomically");
    }
    Ok(report)
}

pub fn check_picture_remux(report_path: impl AsRef<Path>) -> Result<PictureRemuxCheckReport> {
    let report_path = report_path.as_ref().canonicalize()?;
    let report: PictureRemuxReport = serde_json::from_slice(&fs::read(&report_path)?)
        .context("picture remux report is not valid JSON")?;
    if report.schema != PICTURE_REMUX_SCHEMA
        || !report.verified
        || report.video_codec_mode != "stream-copy"
    {
        bail!("unsupported or unverified picture remux report");
    }
    let picture_artifact = PathBuf::from(&report.picture_artifact).canonicalize()?;
    let audio_artifact = PathBuf::from(&report.audio_artifact).canonicalize()?;
    if production::sha256_path(&picture_artifact)? != report.picture_artifact_sha256
        || production::sha256_path(&audio_artifact)? != report.audio_artifact_sha256
    {
        bail!("picture or audio artifact no longer matches the remux report");
    }
    check_animatic(&picture_artifact)?;
    check_audio_preview(&audio_artifact)?;
    let picture: AnimaticRenderReport = serde_json::from_slice(&fs::read(&picture_artifact)?)?;
    let audio: AudioPreviewReport = serde_json::from_slice(&fs::read(&audio_artifact)?)?;
    if picture.output_sha256.as_deref() != Some(&report.picture_output_sha256)
        || audio.output_sha256.as_deref() != Some(&report.audio_output_sha256)
    {
        bail!("remux source output lineage no longer matches");
    }
    let output = PathBuf::from(&report.output).canonicalize()?;
    let output_sha256 = production::sha256_path(&output)?;
    let actual_ms =
        (FfmpegAdapter.ffprobe_duration(&output)?.parse::<f64>()? * 1000.0).round() as u64;
    if output_sha256 != report.output_sha256
        || fs::metadata(&output)?.len() != report.output_bytes
        || actual_ms != report.output_duration_ms
    {
        bail!("remux output no longer matches its report");
    }
    Ok(PictureRemuxCheckReport {
        artifact_report: report_path.display().to_string(),
        work: report.work,
        output_sha256,
        output_duration_ms: actual_ms,
        video_codec_mode: report.video_codec_mode,
        verified: true,
    })
}

fn audio_role_name(role: AudioRole) -> &'static str {
    match role {
        AudioRole::Music => "music",
        AudioRole::Ambience => "ambience",
        AudioRole::Effect => "effect",
        AudioRole::Narration => "narration",
        AudioRole::Dialogue => "dialogue",
    }
}

fn sha256_bytes(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::production::{
        AudioDuckingPolicy, AudioEvent, AudioMastering, GainAutomationPoint, GainCurve,
        NarrationDucking,
    };
    use std::collections::BTreeSet;
    use tempfile::tempdir;

    #[test]
    fn dry_run_compiles_manifest_mix_without_rendering_picture() {
        let temp = tempdir().unwrap();
        let fixture = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        let mut manifest = production::load(fixture.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.audio_events = vec![
            AudioEvent {
                id: "room".to_string(),
                role: AudioRole::Ambience,
                source: "frame-hook.ppm".to_string(),
                start_seconds: 0.0,
                duration_seconds: Some(6.0),
                source_in_seconds: 0.0,
                gain_db: -12.0,
                loop_source: true,
                fade_in_ms: 100,
                fade_out_ms: 200,
                beat_marker_id: None,
                gain_automation: vec![],
            },
            AudioEvent {
                id: "voice".to_string(),
                role: AudioRole::Narration,
                source: "frame-landing.ppm".to_string(),
                start_seconds: 0.5,
                duration_seconds: Some(2.0),
                source_in_seconds: 0.0,
                gain_db: -3.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
                gain_automation: vec![],
            },
        ];
        manifest.narration_ducking = Some(NarrationDucking {
            threshold: 0.03,
            ratio: 8.0,
            attack_ms: 20,
            release_ms: 300,
        });
        let manifest_path = temp.path().join("manifest.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        let output = temp.path().join("mix.m4a");

        let report = render_audio_preview(&AudioPreviewOptions {
            manifest: manifest_path.clone(),
            asset_root: fixture,
            output: output.clone(),
            dry_run: true,
            stems_dir: None,
            sample_rate_hz: 48_000,
            channels: 2,
        })
        .unwrap();

        assert!(report.dry_run);
        assert_eq!(report.duration_ms, 6_000);
        assert_eq!(report.inputs.len(), 3);
        assert!(
            report
                .command_arguments
                .join(" ")
                .contains("sidechaincompress=")
        );
        assert!(report.command_arguments.join(" ").contains("-vn"));
        assert!(!output.exists());
        assert!(output.with_extension("audio-artifacts.json").exists());
    }

    #[test]
    fn audio_preview_requires_manifest_events_and_m4a_output() {
        let fixture = Path::new("manifests/fixtures/vertical-sound-off");
        let temp = tempdir().unwrap();
        let no_events = render_audio_preview(&AudioPreviewOptions {
            manifest: fixture.join("manifest.yaml"),
            asset_root: fixture.to_path_buf(),
            output: temp.path().join("mix.m4a"),
            dry_run: true,
            stems_dir: None,
            sample_rate_hz: 48_000,
            channels: 2,
        })
        .unwrap_err();
        assert!(
            no_events
                .to_string()
                .contains("requires manifest audio_events")
        );
    }

    #[test]
    fn dialogue_quality_evidence_has_synthetic_pass_and_fail_cases() {
        let policy = production::AudioReviewPolicy {
            id: "synthetic-delivery".into(),
            dialogue_loudness_target_lufs: -20.0,
            dialogue_loudness_tolerance_lu: 1.0,
            minimum_speech_to_background_margin_db: 6.0,
            speech_activity_threshold_dbfs: -50.0,
            maximum_mono_loss_db: 3.0,
        };
        let dialogue = vec![800_000; 960];
        let music = vec![80_000; 960];
        let effects = vec![20_000; 960];
        let mastered = vec![600_000; 960];
        let mono = vec![600_000; 480];
        let small = vec![300_000; 480];
        let passing = dialogue_mix_quality_evidence(
            &policy, -20.0, &dialogue, &music, &effects, &mastered, &mono, &small, 48_000, 2,
        )
        .unwrap();
        assert!(passing.passed);
        assert!(passing.speech_active_windows > 0);

        let clipped = vec![8_388_607; 960];
        let failing = dialogue_mix_quality_evidence(
            &policy, -30.0, &dialogue, &dialogue, &effects, &clipped, &mono, &small, 48_000, 2,
        )
        .unwrap();
        assert!(!failing.passed);
        assert!(
            failing
                .violations
                .contains(&"dialogue-gated-loudness".to_string())
        );
        assert!(
            failing
                .violations
                .contains(&"speech-to-background-margin".to_string())
        );
        assert!(failing.violations.contains(&"clipping".to_string()));
    }

    #[test]
    #[ignore = "requires external FFmpeg/ffprobe and renders synthetic audio"]
    fn real_dialogue_ducking_stems_recombine_and_recheck() {
        let temp = tempdir().unwrap();
        let fixture = Path::new("manifests/fixtures/vertical-sound-off")
            .canonicalize()
            .unwrap();
        for (name, frequency, duration) in [
            ("music.wav", 220, 6),
            ("dialogue.wav", 880, 2),
            ("effect.wav", 440, 1),
        ] {
            let status = std::process::Command::new("ffmpeg")
                .args([
                    "-y",
                    "-hide_banner",
                    "-loglevel",
                    "error",
                    "-f",
                    "lavfi",
                    "-i",
                    &format!("sine=frequency={frequency}:duration={duration}:sample_rate=48000"),
                    "-c:a",
                    "pcm_s24le",
                ])
                .arg(temp.path().join(name))
                .status()
                .unwrap();
            assert!(status.success());
        }
        let mut manifest = production::load(fixture.join("manifest.yaml"))
            .unwrap()
            .manifest;
        manifest.audio_events = vec![
            AudioEvent {
                id: "score".into(),
                role: AudioRole::Music,
                source: "music.wav".into(),
                start_seconds: 0.0,
                duration_seconds: Some(6.0),
                source_in_seconds: 0.0,
                gain_db: -9.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
                gain_automation: vec![
                    GainAutomationPoint {
                        time_seconds: Some(0.0),
                        beat_marker_id: None,
                        gain_db: -3.0,
                        curve: GainCurve::Smooth,
                    },
                    GainAutomationPoint {
                        time_seconds: Some(2.0),
                        beat_marker_id: None,
                        gain_db: 0.0,
                        curve: GainCurve::Linear,
                    },
                ],
            },
            AudioEvent {
                id: "line".into(),
                role: AudioRole::Dialogue,
                source: "dialogue.wav".into(),
                start_seconds: 1.0,
                duration_seconds: Some(2.0),
                source_in_seconds: 0.0,
                gain_db: -3.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
                gain_automation: vec![],
            },
            AudioEvent {
                id: "impact".into(),
                role: AudioRole::Effect,
                source: "effect.wav".into(),
                start_seconds: 4.0,
                duration_seconds: Some(1.0),
                source_in_seconds: 0.0,
                gain_db: -12.0,
                loop_source: false,
                fade_in_ms: 0,
                fade_out_ms: 0,
                beat_marker_id: None,
                gain_automation: vec![],
            },
        ];
        manifest.audio_ducking = vec![AudioDuckingPolicy {
            id: "speech-over-score".into(),
            detector_roles: vec![AudioRole::Dialogue],
            target_roles: vec![AudioRole::Music],
            threshold: 0.03,
            ratio: 3.0,
            max_reduction_db: 6.0,
            attack_ms: 25,
            release_ms: 350,
            dynamic_eq: None,
        }];
        manifest.audio_mastering = Some(AudioMastering {
            integrated_lufs: -18.0,
            loudness_range_lu: 11.0,
            true_peak_dbfs: -2.0,
            limiter: 0.88,
        });
        let manifest_path = temp.path().join("manifest.yaml");
        fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
        if cfg!(windows) {
            let stems = temp.path().join("native-stems");
            fs::create_dir(&stems).unwrap();
            let mut input_args = vec![
                "-y".into(),
                "-hide_banner".into(),
                "-loglevel".into(),
                "error".into(),
            ];
            for source in ["music.wav", "dialogue.wav", "effect.wav"] {
                input_args.extend(["-i".into(), temp.path().join(source).display().to_string()]);
            }
            let (_, mut command) = stem_render_command_with_paths(
                &input_args,
                &manifest,
                6.0,
                48_000,
                2,
                &stems,
                &|path| Ok(path.display().to_string()),
            )
            .unwrap();
            let filter_index = command
                .iter()
                .position(|argument| argument == "-filter_complex")
                .unwrap();
            let filter_path = temp.path().join("filter.txt");
            fs::write(&filter_path, &command[filter_index + 1]).unwrap();
            command[filter_index] = "-filter_complex_script".into();
            command[filter_index + 1] = filter_path.display().to_string();
            let output = std::process::Command::new("ffmpeg")
                .args(&command)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let d = read_pcm24_wav(&stems.join("dialogue.pre-master.wav")).unwrap();
            let m = read_pcm24_wav(&stems.join("music.pre-master.wav")).unwrap();
            let e = read_pcm24_wav(&stems.join("effects.pre-master.wav")).unwrap();
            let mix = read_pcm24_wav(&stems.join("mix.pre-master.wav")).unwrap();
            assert_eq!(d.sample_count, 288_000);
            assert!(
                recombination_evidence(&d.samples, &m.samples, &e.samples, &mix.samples)
                    .unwrap()
                    .passed
            );
            return;
        }
        let output = temp.path().join("review.m4a");
        let stems = temp.path().join("stems");
        let report = render_audio_preview(&AudioPreviewOptions {
            manifest: manifest_path.clone(),
            asset_root: temp.path().to_path_buf(),
            output: output.clone(),
            dry_run: false,
            stems_dir: Some(stems.clone()),
            sample_rate_hz: 48_000,
            channels: 2,
        })
        .unwrap();
        assert!(report.stem_package.is_some());
        assert!(stems.join("dialogue.pre-master.wav").exists());
        assert!(stems.join("music.pre-master.wav").exists());
        assert!(stems.join("effects.pre-master.wav").exists());
        assert!(stems.join("mix.pre-master.wav").exists());
        assert!(stems.join("mix.mastered.wav").exists());
        assert!(stems.join("review.no-score.wav").exists());
        assert!(stems.join("review.mono.wav").exists());
        assert!(stems.join("review.small-speaker.wav").exists());
        let receipt_bytes = fs::read(stems.join("receipt.json")).unwrap();
        let receipt_text = String::from_utf8(receipt_bytes.clone()).unwrap();
        assert!(!receipt_text.contains(&temp.path().display().to_string()));
        assert!(!receipt_text.contains("\"path\""));
        let receipt: StemPackageReceipt = serde_json::from_slice(&receipt_bytes).unwrap();
        assert!(receipt.recombination.passed);
        assert_eq!(receipt.sample_count, 288_000);
        assert_eq!(
            receipt
                .outputs
                .iter()
                .map(|item| item.sample_count)
                .collect::<BTreeSet<_>>()
                .len(),
            1
        );
        let artifact = output.with_extension("audio-artifacts.json");
        assert!(check_audio_preview(&artifact).is_ok());
        assert!(
            render_audio_preview(&AudioPreviewOptions {
                manifest: manifest_path.clone(),
                asset_root: temp.path().to_path_buf(),
                output: output.clone(),
                dry_run: false,
                stems_dir: Some(stems.clone()),
                sample_rate_hz: 48_000,
                channels: 2,
            })
            .unwrap_err()
            .to_string()
            .contains("overwrite")
        );

        let manifest_bytes = fs::read(&manifest_path).unwrap();
        fs::write(&manifest_path, [manifest_bytes.as_slice(), b"\n"].concat()).unwrap();
        assert!(check_audio_preview(&artifact).is_err());
        fs::write(&manifest_path, &manifest_bytes).unwrap();

        let source_path = temp.path().join("music.wav");
        let source_bytes = fs::read(&source_path).unwrap();
        fs::write(&source_path, [source_bytes.as_slice(), b"x"].concat()).unwrap();
        assert!(check_audio_preview(&artifact).is_err());
        fs::write(&source_path, &source_bytes).unwrap();

        let stem_path = stems.join("music.pre-master.wav");
        let stem_bytes = fs::read(&stem_path).unwrap();
        fs::write(&stem_path, [stem_bytes.as_slice(), b"x"].concat()).unwrap();
        assert!(check_audio_preview(&artifact).is_err());
        fs::write(&stem_path, &stem_bytes).unwrap();

        fs::write(
            stems.join("receipt.json"),
            [receipt_bytes.as_slice(), b"\n"].concat(),
        )
        .unwrap();
        assert!(check_audio_preview(&artifact).is_err());
    }
}
