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
    /// Explicit template fitting changes motion phase allocation, never audio.
    #[serde(default, skip_serializing_if = "is_false")]
    pub fit_native_duration: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub safe_area: Option<Rect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_regions: Vec<Rect>,
    /// Owner's visual brief, carried into review; it never recolors selected art.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_intent: Option<VisualIntent>,
    #[serde(default)]
    pub reduced_motion: bool,
    pub elements: Vec<Element>,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VisualIntent {
    pub palette_roles: BTreeMap<String, String>,
    pub typography_roles: BTreeMap<String, TypographyRole>,
    #[serde(default)]
    pub reference_traits: Vec<String>,
    /// Current scene assembly uses hard cuts; unsupported transitions fail.
    pub transition: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TypographyRole {
    pub font_family: String,
    pub relative_height: f64,
    pub weight: u16,
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
    #[serde(default = "full_canvas")]
    pub safe_area: Rect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_direction: Option<Direction>,
    pub sample_rate: u32,
    pub duration_samples: u64,
    pub delivery_fps_numerator: u64,
    pub delivery_fps_denominator: u64,
    pub delivery_frames: u64,
    /// Signed error in units of 1/(sample_rate * working_fps) seconds.
    pub duration_residual_numerator: i64,
    pub samples: Vec<ReviewSample>,
}

fn is_false(value: &bool) -> bool {
    !value
}
pub fn full_canvas() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: 1.0,
        height: 1.0,
    }
}
pub fn valid_rect(r: &Rect) -> bool {
    [r.x, r.y, r.width, r.height].iter().all(|v| v.is_finite())
        && r.x >= 0.0
        && r.y >= 0.0
        && r.width > 0.0
        && r.height > 0.0
        && r.x + r.width <= 1.0
        && r.y + r.height <= 1.0
}
fn contains(outer: &Rect, inner: &Rect) -> bool {
    const EPS: f64 = 1e-9;
    inner.x + EPS >= outer.x
        && inner.y + EPS >= outer.y
        && inner.x + inner.width <= outer.x + outer.width + EPS
        && inner.y + inner.height <= outer.y + outer.height + EPS
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
    if let Some(intent) = &direction.visual_intent {
        if intent.palette_roles.is_empty()
            || intent.palette_roles.len() > 16
            || intent.typography_roles.is_empty()
            || intent.typography_roles.len() > 16
            || intent.reference_traits.len() > 32
            || intent.transition != "hard-cut"
        {
            bail!(
                "visual intent requires bounded palette/type roles and supported hard-cut transition"
            );
        }
        for (role, color) in &intent.palette_roles {
            if !portable(role)
                || color.len() != 7
                || !color.starts_with('#')
                || !color.as_bytes()[1..].iter().all(u8::is_ascii_hexdigit)
            {
                bail!("invalid visual palette role {role}");
            }
        }
        for (role, typo) in &intent.typography_roles {
            if !portable(role)
                || typo.font_family.trim().is_empty()
                || typo.font_family.len() > 128
                || !typo.relative_height.is_finite()
                || typo.relative_height <= 0.0
                || typo.relative_height > 1.0
                || !(100..=900).contains(&typo.weight)
            {
                bail!("invalid visual typography role {role}");
            }
        }
        if intent
            .reference_traits
            .iter()
            .any(|s| s.trim().is_empty() || s.len() > 256)
        {
            bail!("invalid visual reference traits");
        }
    }
    if !valid_rect(safe) {
        bail!("invalid motion safe area");
    }
    let effective = direction.safe_area.as_ref().unwrap_or(safe);
    if !valid_rect(effective) || !contains(safe, effective) {
        bail!("authored motion safe area exceeds delivery safe area");
    }
    if direction.protected_regions.len() > 64
        || direction
            .protected_regions
            .iter()
            .any(|r| !valid_rect(r) || !contains(effective, r))
    {
        bail!("invalid or unsafe protected motion region");
    }
    let safe = effective;
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
    let authored = direction;
    let mut execution_direction = None;
    if authored.fit_native_duration {
        let frames = u64::try_from(
            (u128::from(duration_samples) * u128::from(authored.working_fps)
                + u128::from(sample_rate) / 2)
                / u128::from(sample_rate),
        )?;
        if frames == 0 || frames > u64::from(authored.working_fps) * 3600 {
            bail!("native span cannot fit bounded working motion frames");
        }
        let mut resolved = authored.clone();
        resolved.fit_native_duration = false;
        resolved.duration_frames = frames;
        for element in &mut resolved.elements {
            for phase in &mut element.phases {
                let scale = |frame: u64| -> Result<u64> {
                    Ok(u64::try_from(
                        u128::from(frame) * u128::from(frames)
                            / u128::from(authored.duration_frames),
                    )?)
                };
                let start = scale(phase.start_frame)?;
                let end = scale(phase.end_frame + 1)?;
                if end <= start {
                    bail!("native fitting collapses motion phase {}", phase.id);
                }
                phase.start_frame = start;
                phase.end_frame = end - 1;
            }
        }
        validate(&resolved, safe)?;
        execution_direction = Some(resolved);
    }
    let direction = execution_direction.as_ref().unwrap_or(authored);
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
    // Concurrent elements are legal planning intent. Inspect the interior of
    // moving-phase intersections, not just the individual phase boundaries.
    // Bound the pairwise work independently of the delivery frame count.
    let moving = direction
        .elements
        .iter()
        .flat_map(|element| {
            element
                .phases
                .iter()
                .filter(|phase| phase.zoom_from != phase.zoom_to)
                .map(move |phase| (element.id.as_str(), phase))
        })
        .collect::<Vec<_>>();
    if moving.len() > 512 {
        bail!("motion overlap review exceeds 512 moving phase budget");
    }
    for (index, (element, phase)) in moving.iter().enumerate() {
        for (other_element, other) in &moving[index + 1..] {
            if element == other_element {
                continue;
            }
            let start = phase.start_frame.max(other.start_frame);
            let end = phase.end_frame.min(other.end_frame);
            if start <= end {
                let reason = format!(
                    "overlap:{element}.{}+{other_element}.{}",
                    phase.id, other.id
                );
                for frame in [start, start + (end - start) / 2, end] {
                    add(frame, reason.clone())?;
                }
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
        direction: authored.clone(),
        safe_area: safe.clone(),
        execution_direction,
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
    let direction = plan.execution_direction.as_ref().unwrap_or(&plan.direction);
    validate(direction, &plan.safe_area)?;
    if direction.elements.len() != 1
        || direction.elements[0].role != "camera"
        || direction.dominant_element != direction.elements[0].id
    {
        bail!("resolved phased scene camera has unsupported elements");
    }
    let safe = direction.safe_area.as_ref().unwrap_or(&plan.safe_area);
    // These curves are bounded and monotone; every intermediate zoom lies
    // between phase endpoints. Endpoint containment proves the whole interval.
    let mut zooms = vec![1.0];
    if !direction.reduced_motion {
        for phase in &direction.elements[0].phases {
            zooms.extend([phase.zoom_from, phase.zoom_to]);
        }
    }
    for zoom in zooms {
        for region in &direction.protected_regions {
            let transformed = Rect {
                x: 0.5 + (region.x - 0.5) * zoom,
                y: 0.5 + (region.y - 0.5) * zoom,
                width: region.width * zoom,
                height: region.height * zoom,
            };
            if !contains(safe, &transformed) {
                bail!("phased camera crops protected region at zoom {zoom}");
            }
        }
    }
    Ok(())
}

/// FFmpeg perspective's `in` is the zero-based input frame counter. Sampling
/// from that counter matches zoom_at; expressions never depend on prior frames.
pub fn camera_expression(plan: &FramePlan) -> Result<String> {
    validate_camera(plan)?;
    let direction = plan.execution_direction.as_ref().unwrap_or(&plan.direction);
    if direction.elements[0].phases.len() > 32 {
        bail!("phased camera exceeds bounded renderer phase budget");
    }
    if direction.reduced_motion {
        return Ok("1".into());
    }
    let element = &direction.elements[0];
    let frame = format!(
        "(in*{}*{}/{})",
        direction.working_fps, plan.delivery_fps_denominator, plan.delivery_fps_numerator
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
            fit_native_duration: false,
            safe_area: None,
            protected_regions: vec![],
            visual_intent: None,
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
    fn concurrent_element_motion_has_deduplicated_overlap_interior_evidence() {
        let mut d = direction();
        let mut secondary = d.elements[0].clone();
        secondary.id = "secondary".into();
        secondary.role = "planned-layer".into();
        secondary.phases = vec![Phase {
            id: "reveal".into(),
            kind: PhaseKind::Entrance,
            start_frame: 12,
            end_frame: 35,
            curve: Curve::EaseOut,
            zoom_from: 1.0,
            zoom_to: 1.08,
        }];
        d.elements.push(secondary);
        let plan = compile(&d, &safe(), 96000, 48000, 24, 1).unwrap();
        let reason = "overlap:picture.push+secondary.reveal";
        for frame in [24, 29, 35] {
            assert!(
                plan.samples
                    .iter()
                    .any(|sample| sample.frame == frame
                        && sample.reasons.iter().any(|r| r == reason))
            );
        }
        assert_eq!(
            plan.samples
                .iter()
                .filter(|sample| sample.frame == 29)
                .count(),
            1
        );
        assert!(
            plan.samples
                .iter()
                .any(|sample| sample.frame == 11
                    && sample.reasons.iter().any(|r| r == "picture.read"))
        );
        assert_eq!(plan.duration_samples, 96000);
        // Planned concurrency must not be silently lost by the first renderer.
        assert!(
            validate_camera(&plan)
                .unwrap_err()
                .to_string()
                .contains("exactly one")
        );
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

    #[test]
    fn explicit_fitting_preserves_native_samples_and_exposes_resolved_phases() {
        let mut d = direction();
        d.fit_native_duration = true;
        let p = compile(&d, &safe(), 72001, 48000, 30, 1).unwrap();
        assert_eq!(p.direction.duration_frames, 48);
        assert!(p.direction.fit_native_duration);
        let execution = p.execution_direction.as_ref().unwrap();
        assert_eq!(execution.duration_frames, 36);
        assert_eq!(execution.elements[0].phases[0].end_frame, 17);
        assert_eq!(execution.elements[0].phases[1].start_frame, 18);
        assert_eq!(execution.elements[0].phases[1].end_frame, 35);
        assert_eq!(p.duration_samples, 72001);
        assert_eq!(p.duration_residual_numerator, -24);
        assert_eq!(p.delivery_frames, 45);
        assert_eq!(zoom_at(execution, "picture", 17.0).unwrap(), 1.0);
        assert!(camera_expression(&p).unwrap().contains("-18"));
        d.fit_native_duration = false;
        assert!(compile(&d, &safe(), 72001, 48000, 30, 1).is_err());
        d.fit_native_duration = true;
        assert!(
            compile(&d, &safe(), 1000, 48000, 30, 1).is_err(),
            "collapsed moving phases must fail"
        );
    }

    #[test]
    fn protected_geometry_is_checked_through_the_camera_transform() {
        let mut d = direction();
        d.protected_regions = vec![Rect {
            x: 0.91,
            y: 0.4,
            width: 0.08,
            height: 0.1,
        }];
        let plan = compile(&d, &safe(), 96000, 48000, 24, 1).unwrap();
        assert!(
            validate_camera(&plan)
                .unwrap_err()
                .to_string()
                .contains("crops protected")
        );
        d.reduced_motion = true;
        assert!(validate_camera(&compile(&d, &safe(), 96000, 48000, 24, 1).unwrap()).is_ok());
        d.reduced_motion = false;
        d.elements[0].bounds = Rect {
            x: 0.15,
            y: 0.15,
            width: 0.7,
            height: 0.7,
        };
        d.protected_regions = vec![Rect {
            x: 0.4,
            y: 0.4,
            width: 0.2,
            height: 0.2,
        }];
        let profile = Rect {
            x: 0.1,
            y: 0.1,
            width: 0.8,
            height: 0.8,
        };
        let plan = compile(&d, &profile, 96000, 48000, 24, 1).unwrap();
        assert_eq!(plan.safe_area, profile);
        assert!(validate_camera(&plan).is_ok());
        d.safe_area = Some(safe());
        assert!(
            compile(&d, &profile, 96000, 48000, 24, 1).is_err(),
            "scene cannot widen profile safe area"
        );
    }

    #[test]
    fn visual_brief_is_retained_and_unsupported_claims_fail() {
        let mut d = direction();
        d.visual_intent = Some(VisualIntent {
            palette_roles: BTreeMap::from([("hero".into(), "#D97757".into())]),
            typography_roles: BTreeMap::from([(
                "headline".into(),
                TypographyRole {
                    font_family: "Local Sans".into(),
                    relative_height: 0.08,
                    weight: 600,
                },
            )]),
            reference_traits: vec!["Restrained editorial pacing".into()],
            transition: "hard-cut".into(),
        });
        let plan = compile(&d, &safe(), 96000, 48000, 24, 1).unwrap();
        assert_eq!(plan.direction.visual_intent, d.visual_intent);
        d.visual_intent.as_mut().unwrap().transition = "wipe".into();
        assert!(compile(&d, &safe(), 96000, 48000, 24, 1).is_err());
        d.visual_intent.as_mut().unwrap().transition = "hard-cut".into();
        d.visual_intent
            .as_mut()
            .unwrap()
            .palette_roles
            .insert("hero".into(), "red".into());
        assert!(validate(&d, &safe()).is_err());
    }
}
