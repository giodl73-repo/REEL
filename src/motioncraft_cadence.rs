//! Existing motion-check metrics with native-frame camera expectations.
use crate::{
    adapters::still_animatic::{
        MAX_NEAR_STATIONARY_FRACTION, MIN_HOLD_STATIONARY_FRACTION, NEAR_STATIONARY_LUMA_THRESHOLD,
        cadence_values_for_frames,
    },
    scene_delivery::{Job, PictureKind, PictureMotion, Plan},
};
use anyhow::{Context, Result};
use serde_json::{Value, json};
use std::path::Path;

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
                        Ok((a - b).abs() < 1e-12)
                    }
                    None => Ok(true),
                    _ => unreachable!(),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        let values =
            cadence_values_for_frames(&root.join("picture.mkv"), span.start_frame, span.end_frame)?;
        let mut report = evaluate(&values, &holds);
        report["attachment_id"] = json!(picture.attachment_id);
        report["start_frame"] = json!(span.start_frame);
        report["end_frame_exclusive"] = json!(span.end_frame);
        report["status"] = json!("evaluated");
        rows.push(report);
    }
    let passed = !rows.is_empty() && rows.iter().all(|row| row["passed"] == true);
    Ok(
        json!({"schema":"reel.motioncraft-cadence.v1","job_sha256":plan.job_sha256,
        "picture_sha256":crate::sha256_file(&root.join("picture.mkv"))?,
        "fps_numerator":plan.fps_numerator,"fps_denominator":plan.fps_denominator,
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
