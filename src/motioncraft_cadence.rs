//! Existing motion-check metrics with native-frame camera expectations.
use crate::{
    adapters::still_animatic::{
        MAX_NEAR_STATIONARY_FRACTION, MIN_HOLD_STATIONARY_FRACTION, NEAR_STATIONARY_LUMA_THRESHOLD,
    },
    scene_delivery::{Job, PictureKind, PictureMotion, Plan},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// Resolve once, then execute that exact native binary for every measurement.
/// An explicit override never falls back to PATH or WSL.
fn resolve_analyzer() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("REEL_CADENCE_FFMPEG") {
        return checked_executable(Path::new(&path));
    }
    let name = if cfg!(windows) {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    };
    for dir in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        let path = dir.join(name);
        if path.is_file() {
            return checked_executable(&path);
        }
    }
    anyhow::bail!("native cadence FFmpeg not found; set REEL_CADENCE_FFMPEG to an executable path")
}

fn checked_executable(path: &Path) -> Result<PathBuf> {
    let resolved = path
        .canonicalize()
        .context("cannot resolve native cadence FFmpeg executable")?;
    if !resolved.is_file() {
        anyhow::bail!("native cadence FFmpeg path is not a file");
    }
    Ok(resolved)
}

fn native_output(executable: &Path, args: &[String]) -> Result<String> {
    let output = Command::new(executable)
        .args(args)
        .output()
        .context("cannot execute native cadence FFmpeg")?;
    if !output.status.success() {
        anyhow::bail!(
            "native cadence FFmpeg failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    String::from_utf8(output.stdout).context("native cadence FFmpeg wrote non-UTF8 metrics")
}

fn native_frame_values(executable: &Path, video: &Path, start: u64, end: u64) -> Result<Vec<f64>> {
    if end
        .checked_sub(start)
        .is_none_or(|n| !(2..=432_000).contains(&n))
    {
        anyhow::bail!("indexed cadence requires 2..432000 frames");
    }
    let output = native_output(
        executable,
        &[
            "-hide_banner".into(),
            "-loglevel".into(),
            "error".into(),
            "-i".into(),
            video.to_string_lossy().into_owned(),
            "-vf".into(),
            format!(
                "trim=start_frame={start}:end_frame={end},setpts=PTS-STARTPTS,tblend=all_mode=difference,signalstats,metadata=print:key=lavfi.signalstats.YAVG:file=-"
            ),
            "-f".into(),
            "null".into(),
            "-".into(),
        ],
    )?;
    let values = output
        .lines()
        .filter_map(|line| line.strip_prefix("lavfi.signalstats.YAVG="))
        .map(|value| {
            value
                .parse::<f64>()
                .context("invalid native FFmpeg YAVG metric")
        })
        .collect::<Result<Vec<_>>>()?;
    if values.len() as u64 != end - start - 1 || values.iter().any(|v| !v.is_finite() || *v < 0.0) {
        anyhow::bail!("invalid indexed cadence metrics or transition count");
    }
    Ok(values)
}

fn evaluate(values: &[f64], holds: &[bool]) -> Value {
    let held = holds.iter().filter(|v| **v).count();
    let moving = holds.len() - held;
    let held_stationary = values
        .iter()
        .zip(holds)
        .filter(|(v, h)| **h && **v < NEAR_STATIONARY_LUMA_THRESHOLD)
        .count();
    let unexpected_stationary = values
        .iter()
        .zip(holds)
        .filter(|(v, h)| !**h && **v < NEAR_STATIONARY_LUMA_THRESHOLD)
        .count();
    let moving_fraction = if moving == 0 {
        0.0
    } else {
        unexpected_stationary as f64 / moving as f64
    };
    let hold_fraction = if held == 0 {
        1.0
    } else {
        held_stationary as f64 / held as f64
    };
    json!({"transitions":values.len(), "declared_camera_hold_transitions":held,
        "expected_moving_transitions":moving,"unexpected_stationary_transitions":unexpected_stationary,
        "unexpected_stationary_fraction":moving_fraction,"held_stationary_fraction":hold_fraction,
        "passed":moving_fraction<=MAX_NEAR_STATIONARY_FRACTION && hold_fraction>=MIN_HOLD_STATIONARY_FRACTION})
}

pub fn analyze(job: &Job, plan: &Plan, root: &Path) -> Result<Value> {
    let analyzer = resolve_analyzer()?;
    let analyzer_sha256 = crate::sha256_file(&analyzer)?;
    let analyzer_version = native_output(&analyzer, &["-version".into()])?;
    let analyzer_version = analyzer_version
        .lines()
        .next()
        .filter(|line| !line.trim().is_empty())
        .context("cadence analyzer FFmpeg version unavailable")?;
    let layered = !job.external_layers.is_empty() || job.post_compose_camera.is_some();
    let mut rows = Vec::new();
    for (picture, span) in job.pictures.iter().zip(&plan.pictures) {
        let unsupported = layered
            || picture.kind != PictureKind::Still
            || matches!(&picture.motion, Some(motion) if !matches!(motion, PictureMotion::PhasedCamera{..}));
        if unsupported || span.end_frame - span.start_frame < 2 {
            rows.push(json!({"attachment_id":picture.attachment_id,"status":"needs-separate-cadence-analysis",
                "whole_frame_hold_allowance":false,"passed":null}));
            continue;
        }
        let holds = (1..span.end_frame - span.start_frame)
            .map(|local| -> Result<bool> {
                match &picture.motion {
                    Some(PictureMotion::PhasedCamera { plan: motion }) => {
                        let direction = motion
                            .execution_direction
                            .as_ref()
                            .unwrap_or(&motion.direction);
                        let working = |frame: u64| {
                            frame as f64
                                * f64::from(direction.working_fps)
                                * plan.fps_denominator as f64
                                / plan.fps_numerator as f64
                        };
                        let a = reel_assembly::motioncraft::zoom_at(
                            direction,
                            &direction.dominant_element,
                            working(local - 1),
                        )?;
                        let b = reel_assembly::motioncraft::zoom_at(
                            direction,
                            &direction.dominant_element,
                            working(local),
                        )?;
                        let pa = reel_assembly::motioncraft::pan_at(
                            direction,
                            &direction.dominant_element,
                            working(local - 1),
                        )?;
                        let pb = reel_assembly::motioncraft::pan_at(
                            direction,
                            &direction.dominant_element,
                            working(local),
                        )?;
                        Ok((a - b).abs() < 1e-12
                            && (pa.x - pb.x).abs() < 1e-12
                            && (pa.y - pb.y).abs() < 1e-12)
                    }
                    None => Ok(true),
                    _ => unreachable!(),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let values = native_frame_values(
            &analyzer,
            &root.join("picture.mkv"),
            span.start_frame,
            span.end_frame,
        )?;
        let mut report = evaluate(&values, &holds);
        report["attachment_id"] = json!(picture.attachment_id);
        report["start_frame"] = json!(span.start_frame);
        report["end_frame_exclusive"] = json!(span.end_frame);
        report["status"] = json!("evaluated");
        rows.push(report);
    }
    if crate::sha256_file(&analyzer)? != analyzer_sha256 {
        anyhow::bail!("native cadence executable changed during analysis");
    }
    let passed = !rows.is_empty() && rows.iter().all(|row| row["passed"] == true);
    Ok(
        json!({"schema":"reel.motioncraft-cadence.v1","job_sha256":plan.job_sha256,
        "picture_sha256":crate::sha256_file(&root.join("picture.mkv"))?,
        "fps_numerator":plan.fps_numerator,"fps_denominator":plan.fps_denominator,
        "analyzer_ffmpeg_version":analyzer_version,
        "analyzer_backend":"native",
        "analyzer_executable":analyzer,
        "analyzer_executable_sha256":analyzer_sha256,
        "near_stationary_luma_threshold":NEAR_STATIONARY_LUMA_THRESHOLD,
        "maximum_moving_near_stationary_fraction":MAX_NEAR_STATIONARY_FRACTION,
        "minimum_hold_stationary_fraction":MIN_HOLD_STATIONARY_FRACTION,
        "shots":rows,"passed":passed,
        "scope":"Perceptual cadence evidence using existing motion-check thresholds; not creative approval. Camera holds never excuse layered composition freezes."}),
    )
}

pub fn checked_report(job_path: &Path, asset_root: &Path, render_root: &Path) -> Result<Value> {
    let (job, plan) = crate::scene_delivery::plan(job_path, asset_root)?;
    crate::scene_delivery::check(job_path, asset_root, render_root)
        .context("cadence analysis requires a verified existing scene render")?;
    analyze(&job, &plan, render_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_or_directory_analyzer_rejects_without_fallback() {
        let dir = tempfile::tempdir().unwrap();
        assert!(checked_executable(&dir.path().join("missing.exe")).is_err());
        assert!(checked_executable(dir.path()).is_err());
    }

    #[test]
    #[ignore = "requires native FFmpeg"]
    fn native_metrics_reject_a_frozen_moving_interval() {
        let dir = tempfile::tempdir().unwrap();
        let executable = resolve_analyzer().unwrap();
        let video = dir.path().join("frozen.mkv");
        native_output(
            &executable,
            &[
                "-v".into(),
                "error".into(),
                "-f".into(),
                "lavfi".into(),
                "-i".into(),
                "color=c=blue:s=64x64:r=24".into(),
                "-frames:v".into(),
                "24".into(),
                "-c:v".into(),
                "ffv1".into(),
                video.to_string_lossy().into_owned(),
            ],
        )
        .unwrap();
        let values = native_frame_values(&executable, &video, 0, 24).unwrap();
        assert_eq!(evaluate(&values, &[false; 23])["passed"], false);
        assert_eq!(evaluate(&values, &[true; 23])["passed"], true);
    }
    #[test]
    fn camera_hold_does_not_hide_an_unexpected_moving_phase_freeze() {
        assert_eq!(
            evaluate(&[0.0, 0.0, 1.0, 1.0], &[true, true, false, false])["passed"],
            true
        );
        let frozen = evaluate(&[0.0, 0.0, 0.0, 0.0], &[true, true, false, false]);
        assert_eq!(frozen["unexpected_stationary_transitions"], 2);
        assert_eq!(frozen["passed"], false);
        assert_eq!(evaluate(&[1.0, 1.0], &[true, true])["passed"], false);
    }
}
