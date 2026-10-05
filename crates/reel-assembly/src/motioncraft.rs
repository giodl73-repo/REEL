//! Portable authored motion intent. Native audio clocks remain authoritative.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Shot overrides are scoped to language-local event IDs. No positional merging.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SceneDirection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<Direction>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub shots: BTreeMap<String, BTreeMap<String, Direction>>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Direction {
    pub purpose: String,
    pub dominant_element: String,
    pub working_fps: u32,
    pub duration_frames: u64,
    #[serde(default)]
    pub reduced_motion: bool,
    pub elements: Vec<Element>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Element {
    pub id: String,
    pub role: String,
    pub bounds: Rect,
    #[serde(default)]
    pub phases: Vec<Phase>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase {
    pub id: String,
    pub kind: PhaseKind,
    pub start_frame: u64,
    pub end_frame: u64,
    pub curve: Curve,
    pub zoom_from: f64,
    pub zoom_to: f64,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PhaseKind {
    Entrance,
    Settle,
    Hold,
    Exit,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Curve {
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FramePlan {
    pub direction: Direction,
    pub sample_rate: u32,
    pub duration_samples: u64,
    pub delivery_fps_numerator: u64,
    pub delivery_fps_denominator: u64,
    pub delivery_frames: u64,
    /// Signed error in units of 1/(sample_rate * working_fps) seconds.
    pub duration_residual_numerator: i64,
    pub samples: Vec<ReviewSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSample {
    pub frame: u64,
    pub reasons: Vec<String>,
}

fn portable(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub fn validate(direction: &Direction, safe: &Rect) -> Result<()> {
    if direction.purpose.trim().is_empty()
        || !(1..=120).contains(&direction.working_fps)
        || direction.duration_frames == 0
        || direction.duration_frames > u64::from(direction.working_fps) * 3600
        || direction.elements.is_empty()
        || direction.elements.len() > 256
    {
        bail!("motion direction needs purpose, bounded timebase and elements");
    }
    fn valid_rect(r: &Rect) -> bool {
        [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
            && r.x >= 0.0
            && r.y >= 0.0
            && r.width > 0.0
            && r.height > 0.0
            && r.x + r.width <= 1.0
            && r.y + r.height <= 1.0
    }
    if !valid_rect(safe) {
        bail!("invalid motion safe area");
    }
    let mut ids = BTreeSet::new();
    for element in &direction.elements {
        if !portable(&element.id) || !ids.insert(&element.id) || element.role.trim().is_empty() {
            bail!("invalid or duplicate motion element ID/role");
        }
        let r = &element.bounds;
        if !valid_rect(r)
            || r.x < safe.x
            || r.y < safe.y
            || r.x + r.width > safe.x + safe.width
            || r.y + r.height > safe.y + safe.height
        {
            bail!("motion element {} leaves safe bounds", element.id);
        }
        let mut phase_ids = BTreeSet::new();
        let mut previous_end = None;
        let mut previous_zoom = None;
        if element.phases.len() > 256 {
            bail!("too many motion phases");
        }
        for phase in &element.phases {
            if !portable(&phase.id)
                || !phase_ids.insert(&phase.id)
                || phase.start_frame > phase.end_frame
                || phase.end_frame >= direction.duration_frames
                || previous_end.is_some_and(|end| phase.start_frame <= end)
            {
                bail!(
                    "invalid, overlapping or unordered motion phase {}",
                    phase.id
                );
            }
            if !phase.zoom_from.is_finite()
                || !phase.zoom_to.is_finite()
                || !(1.0..=4.0).contains(&phase.zoom_from)
                || !(1.0..=4.0).contains(&phase.zoom_to)
                || (phase.kind == PhaseKind::Hold && phase.zoom_from != phase.zoom_to)
                || (phase.start_frame == phase.end_frame && phase.zoom_from != phase.zoom_to)
                || previous_zoom.is_some_and(|zoom| zoom != phase.zoom_from)
            {
                bail!("invalid or discontinuous motion zoom in {}", phase.id);
            }
            previous_end = Some(phase.end_frame);
            previous_zoom = Some(phase.zoom_to);
        }
    }
    if !ids.contains(&direction.dominant_element) {
        bail!("unknown dominant motion element");
    }
    Ok(())
}

pub fn compile(
    direction: &Direction,
    safe: &Rect,
    duration_samples: u64,
    sample_rate: u32,
    fps_numerator: u64,
    fps_denominator: u64,
) -> Result<FramePlan> {
    validate(direction, safe)?;
    if duration_samples == 0
        || sample_rate == 0
        || fps_numerator == 0
        || fps_denominator == 0
        || fps_denominator > 1_000_000
        || u128::from(fps_numerator) > 120 * u128::from(fps_denominator)
    {
        bail!("invalid native/delivery motion timebase");
    }
    let residual = i128::from(direction.duration_frames) * i128::from(sample_rate)
        - i128::from(duration_samples) * i128::from(direction.working_fps);
    if residual.abs() * 2 > i128::from(sample_rate) {
        bail!("motion duration differs from native span by more than half a working frame");
    }
    let divisor = u128::from(sample_rate) * u128::from(fps_denominator);
    let frames = u64::try_from(
        (u128::from(duration_samples) * u128::from(fps_numerator) + divisor / 2) / divisor,
    )?;
    if frames == 0 {
        bail!("motion span has no delivery frames");
    }
    let mut samples: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    let mut add = |working: u64, reason: String| -> Result<()> {
        let denominator = u128::from(direction.working_fps) * u128::from(fps_denominator);
        let frame = u64::try_from(
            (u128::from(working) * u128::from(fps_numerator) + denominator / 2) / denominator,
        )?
        .min(frames - 1);
        samples.entry(frame).or_default().insert(reason);
        Ok(())
    };
    for element in &direction.elements {
        for phase in &element.phases {
            let reason = format!("{}.{}", element.id, phase.id);
            for frame in [
                phase.start_frame.saturating_sub(1),
                phase.start_frame,
                phase.end_frame,
                phase
                    .end_frame
                    .saturating_add(1)
                    .min(direction.duration_frames - 1),
            ] {
                add(frame, reason.clone())?;
            }
            if phase.kind == PhaseKind::Hold {
                add(
                    phase.start_frame + (phase.end_frame - phase.start_frame) / 2,
                    reason,
                )?;
            }
        }
    }
    for i in 0..=8 {
        add(
            (direction.duration_frames - 1) * i / 8,
            "contact-sheet".into(),
        )?;
    }
    Ok(FramePlan {
        direction: direction.clone(),
        sample_rate,
        duration_samples,
        delivery_fps_numerator: fps_numerator,
        delivery_fps_denominator: fps_denominator,
        delivery_frames: frames,
        duration_residual_numerator: i64::try_from(residual)?,
        samples: samples
            .into_iter()
            .map(|(frame, reasons)| ReviewSample {
                frame,
                reasons: reasons.into_iter().collect(),
            })
            .collect(),
    })
}

/// Whole-direction precedence. Arrays are never merged by position.
pub fn resolve<'a>(
    episode: Option<&'a Direction>,
    scene: Option<&'a Direction>,
    shot: Option<&'a Direction>,
) -> Option<&'a Direction> {
    shot.or(scene).or(episode)
}

/// Cumulative scene boundaries can allocate one frame differently from a local
/// rounded duration. Keep the native sample clock and expose that allocation.
pub fn allocate_delivery_frames(plan: &mut FramePlan, frames: u64) -> Result<()> {
    if frames == 0 || frames.abs_diff(plan.delivery_frames) > 1 {
        bail!("motion frame allocation differs from native duration");
    }
    plan.delivery_frames = frames;
    let mut samples: BTreeMap<u64, BTreeSet<String>> = BTreeMap::new();
    for sample in &plan.samples {
        samples
            .entry(sample.frame.min(frames - 1))
            .or_default()
            .extend(sample.reasons.clone());
    }
    plan.samples = samples
        .into_iter()
        .map(|(frame, reasons)| ReviewSample {
            frame,
            reasons: reasons.into_iter().collect(),
        })
        .collect();
    Ok(())
}

/// The first consuming scene-engine treatment is a whole-picture camera.
/// Other elements require a future executable layer contract, never silent loss.
pub fn validate_camera(plan: &FramePlan) -> Result<()> {
    if plan.direction.elements.len() != 1
        || plan.direction.elements[0].role != "camera"
        || plan.direction.dominant_element != plan.direction.elements[0].id
    {
        bail!("phased scene camera supports exactly one dominant camera element");
    }
    Ok(())
}

/// FFmpeg perspective's `in` is the zero-based input frame counter. Sampling
/// from that counter matches zoom_at; expressions never depend on prior frames.
pub fn camera_expression(plan: &FramePlan) -> Result<String> {
    validate_camera(plan)?;
    if plan.direction.elements[0].phases.len() > 32 {
        bail!("phased camera exceeds bounded renderer phase budget");
    }
    if plan.direction.reduced_motion {
        return Ok("1".into());
    }
    let element = &plan.direction.elements[0];
    let frame = format!(
        "(in*{}*{}/{})",
        plan.direction.working_fps, plan.delivery_fps_denominator, plan.delivery_fps_numerator
    );
    let mut expression = element
        .phases
        .first()
        .map_or("1".into(), |p| p.zoom_from.to_string());
    for phase in &element.phases {
        let raw = if phase.end_frame == phase.start_frame {
            "1".into()
        } else {
            format!(
                "min(1,max(0,({frame}-{})/{}))",
                phase.start_frame,
                phase.end_frame - phase.start_frame
            )
        };
        let eased = match phase.curve {
            Curve::Linear => raw,
            Curve::EaseIn => format!("({raw})*({raw})"),
            Curve::EaseOut => format!("1-(1-({raw}))*(1-({raw}))"),
            Curve::EaseInOut => format!("(1-cos(PI*({raw})))/2"),
        };
        let value = format!(
            "({}+({})*({eased}))",
            phase.zoom_from,
            phase.zoom_to - phase.zoom_from
        );
        expression = format!(
            "if(gte({frame},{}),{value},{expression})",
            phase.start_frame
        );
        if expression.len() > 8192 {
            bail!("phased camera expression exceeds renderer argument budget");
        }
    }
    Ok(expression)
}

pub fn zoom_at(direction: &Direction, element_id: &str, working_frame: f64) -> Result<f64> {
    if !working_frame.is_finite() || working_frame < 0.0 {
        bail!("invalid sampled frame");
    }
    let element = direction
        .elements
        .iter()
        .find(|e| e.id == element_id)
        .ok_or_else(|| anyhow::anyhow!("unknown sampled motion element"))?;
    if direction.reduced_motion {
        return Ok(1.0);
    }
    let mut zoom = element.phases.first().map_or(1.0, |p| p.zoom_from);
    for phase in &element.phases {
        if working_frame < phase.start_frame as f64 {
            break;
        }
        let span = phase.end_frame - phase.start_frame;
        let t = if span == 0 {
            1.0
        } else {
            ((working_frame - phase.start_frame as f64) / span as f64).clamp(0.0, 1.0)
        };
        let t = match phase.curve {
            Curve::Linear => t,
            Curve::EaseIn => t * t,
            Curve::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Curve::EaseInOut => (1.0 - (std::f64::consts::PI * t).cos()) / 2.0,
        };
        zoom = phase.zoom_from + (phase.zoom_to - phase.zoom_from) * t;
    }
    Ok(zoom)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn safe() -> Rect {
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
    fn direction() -> Direction {
        Direction {
            purpose: "read then approach".into(),
            dominant_element: "picture".into(),
            working_fps: 24,
            duration_frames: 48,
            reduced_motion: false,
            elements: vec![Element {
                id: "picture".into(),
                role: "camera".into(),
                bounds: safe(),
                phases: vec![
                    Phase {
                        id: "read".into(),
                        kind: PhaseKind::Hold,
                        start_frame: 0,
                        end_frame: 23,
                        curve: Curve::Linear,
                        zoom_from: 1.0,
                        zoom_to: 1.0,
                    },
                    Phase {
                        id: "push".into(),
                        kind: PhaseKind::Settle,
                        start_frame: 24,
                        end_frame: 47,
                        curve: Curve::EaseOut,
                        zoom_from: 1.0,
                        zoom_to: 1.08,
                    },
                ],
            }],
        }
    }
    #[test]
    fn native_samples_survive_compilation_and_evidence_is_bounded() {
        let d = direction();
        let p = compile(&d, &safe(), 96000, 48000, 24, 1).unwrap();
        assert_eq!(p.duration_samples, 48001 + 47999);
        assert_eq!(p.delivery_frames, 48);
        assert_eq!(p.duration_residual_numerator, 0);
        assert!(p.samples.iter().all(|s| s.frame < 48));
        assert_eq!(p, compile(&d, &safe(), 96000, 48000, 24, 1).unwrap());
        assert_eq!(zoom_at(&d, "picture", 23.0).unwrap(), 1.0);
        assert_eq!(zoom_at(&d, "picture", 47.0).unwrap(), 1.08);
    }
    #[test]
    fn structural_errors_and_drift_fail() {
        let mut d = direction();
        d.elements[0].phases[1].start_frame = 23;
        assert!(validate(&d, &safe()).is_err());
        d = direction();
        d.dominant_element = "missing".into();
        assert!(validate(&d, &safe()).is_err());
        d = direction();
        d.elements[0].bounds.x = 0.1;
        assert!(validate(&d, &safe()).is_err());
        assert!(compile(&direction(), &safe(), 98001, 48000, 24, 1).is_err());
    }
    #[test]
    fn static_optional_phases_precedence_and_reduced_motion() {
        let episode = direction();
        let mut scene = direction();
        scene.reduced_motion = true;
        assert_eq!(resolve(Some(&episode), Some(&scene), None), Some(&scene));
        assert_eq!(
            resolve(Some(&episode), Some(&scene), Some(&episode)),
            Some(&episode)
        );
        assert_eq!(zoom_at(&scene, "picture", 47.0).unwrap(), 1.0);
        scene.elements[0].phases.clear();
        assert!(validate(&scene, &safe()).is_ok());
    }
    #[test]
    fn rejects_unrecognized_authoring_fields() {
        let mut value = serde_json::to_value(direction()).unwrap();
        value["spring"] = serde_json::json!(true);
        assert!(serde_json::from_value::<Direction>(value).is_err());
    }
    #[test]
    fn working_and_delivery_clocks_have_explicit_residuals() {
        let p = compile(&direction(), &safe(), 96001, 48000, 30, 1).unwrap();
        assert_eq!(p.duration_samples, 96001);
        assert_eq!(p.delivery_frames, 60);
        assert_eq!(p.duration_residual_numerator, -24);
        assert!(p.samples.iter().all(|s| s.frame < 60));
        assert!(compile(&direction(), &safe(), 96000, 48000, 0, 1).is_err());
    }
}
