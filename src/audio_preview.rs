use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tempfile::Builder;

use crate::{
    adapters::{
        ffmpeg::{FfmpegAdapter, RenderEnvironmentReport},
        still_animatic::{AnimaticInput, AnimaticRenderReport, check_animatic},
    },
    production::{self, AudioRole},
};

pub const AUDIO_PREVIEW_SCHEMA: &str = "reel.audio-preview-artifacts.v0.1";
pub const PICTURE_REMUX_SCHEMA: &str = "reel.picture-remux-artifacts.v0.1";

#[derive(Clone, Debug)]
pub struct AudioPreviewOptions {
    pub manifest: PathBuf,
    pub asset_root: PathBuf,
    pub output: PathBuf,
    pub dry_run: bool,
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
    let mut filters = Vec::new();
    let mut narration = Vec::new();
    let mut background = Vec::new();
    for (index, event) in loaded.manifest.audio_events.iter().enumerate() {
        let duration = event
            .duration_seconds
            .unwrap_or(timeline_seconds - event.start_seconds);
        let mut chain = format!(
            "[{index}:a:0]atrim=start={:.3}:duration={duration:.3},asetpts=PTS-STARTPTS,volume={:.3}dB",
            event.source_in_seconds, event.gain_db
        );
        if event.fade_in_ms > 0 {
            chain.push_str(&format!(
                ",afade=t=in:st=0:d={:.3}",
                event.fade_in_ms as f64 / 1000.0
            ));
        }
        if event.fade_out_ms > 0 {
            let fade = event.fade_out_ms as f64 / 1000.0;
            chain.push_str(&format!(
                ",afade=t=out:st={:.3}:d={fade:.3}",
                (duration - fade).max(0.0)
            ));
        }
        chain.push_str(&format!(
            ",adelay={}:all=1[ae{index}]",
            (event.start_seconds * 1000.0).round() as u64
        ));
        filters.push(chain);
        if event.role == AudioRole::Narration {
            narration.push(format!("ae{index}"));
        } else {
            background.push(format!("ae{index}"));
        }
    }
    let mixed = match (background.is_empty(), narration.is_empty()) {
        (false, false) => {
            mix_labels(&mut filters, &background, "background");
            mix_labels(&mut filters, &narration, "narration");
            if let Some(ducking) = &loaded.manifest.narration_ducking {
                filters
                    .push("[narration]asplit=2[narration_detector][narration_program]".to_string());
                filters.push(format!(
                    "[background][narration_detector]sidechaincompress=threshold={:.6}:ratio={:.3}:attack={}:release={}[ducked]",
                    ducking.threshold, ducking.ratio, ducking.attack_ms, ducking.release_ms
                ));
                filters.push("[ducked][narration_program]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]".to_string());
            } else {
                filters.push("[background][narration]amix=inputs=2:normalize=0:dropout_transition=0[mixedaudio]".to_string());
            }
            "mixedaudio"
        }
        (false, true) => {
            mix_labels(&mut filters, &background, "mixedaudio");
            "mixedaudio"
        }
        (true, false) => {
            mix_labels(&mut filters, &narration, "mixedaudio");
            "mixedaudio"
        }
        (true, true) => unreachable!(),
    };
    let mastering =
        loaded
            .manifest
            .audio_mastering
            .as_ref()
            .map_or_else(String::new, |mastering| {
                format!(
                    ",loudnorm=I={:.3}:LRA={:.3}:TP={:.3},alimiter=limit={:.3}:level=false",
                    mastering.integrated_lufs,
                    mastering.loudness_range_lu,
                    mastering.true_peak_dbfs,
                    mastering.limiter
                )
            });
    filters.push(format!(
        "[{mixed}]aresample=async=1:first_pts=0,apad{mastering},atrim=duration={timeline_seconds:.3}[finala]"
    ));
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
    let policy = serde_json::json!({
        "duration_ms": duration_ms,
        "audio_events": loaded.manifest.audio_events,
        "narration_ducking": loaded.manifest.narration_ducking,
        "audio_mastering": loaded.manifest.audio_mastering,
    });
    let report = AudioPreviewReport {
        schema: AUDIO_PREVIEW_SCHEMA.to_string(),
        work: loaded.manifest.work,
        manifest_sha256: production::sha256_path(&manifest_path)?,
        audio_policy_sha256: sha256_bytes(&serde_json::to_vec(&policy)?),
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
    };
    let mut report_temp = Builder::new()
        .prefix(".reel-audio-artifacts-")
        .tempfile_in(output_parent)?;
    report_temp.write_all(&serde_json::to_vec_pretty(&report)?)?;
    report_temp.flush()?;
    if let Some(temp) = rendered_temp {
        temp.persist_noclobber(&options.output)?;
    }
    if let Err(error) = report_temp.persist_noclobber(&artifact_path) {
        if !options.dry_run {
            let _ = fs::remove_file(&options.output);
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

fn mix_labels(filters: &mut Vec<String>, labels: &[String], output: &str) {
    let inputs = labels
        .iter()
        .map(|label| format!("[{label}]"))
        .collect::<String>();
    if labels.len() == 1 {
        filters.push(format!("{inputs}anull[{output}]"));
    } else {
        filters.push(format!(
            "{inputs}amix=inputs={}:normalize=0:dropout_transition=0[{output}]",
            labels.len()
        ));
    }
}

fn audio_role_name(role: AudioRole) -> &'static str {
    match role {
        AudioRole::Music => "music",
        AudioRole::Ambience => "ambience",
        AudioRole::Effect => "effect",
        AudioRole::Narration => "narration",
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
    use crate::production::{AudioEvent, NarrationDucking};
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
            manifest: manifest_path,
            asset_root: fixture,
            output: output.clone(),
            dry_run: true,
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
        })
        .unwrap_err();
        assert!(
            no_events
                .to_string()
                .contains("requires manifest audio_events")
        );
    }
}
