//! Frozen creative direction and bounded density forecasts from measured takes.
//! Forecasts neither create drawings nor establish renderer support or approval.
use crate::*;
use reel_assembly::scene_authoring::{self as scene, NativeEventSpan, RenderedSpan, ScenePolicy};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    pub numerator: u32,
    pub denominator: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Cadence {
    HeldCels,
    EveryFrames { frames: u32 },
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CostRate {
    pub unit: String,
    pub microunits_per_candidate: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub drawing_slots: u64,
    pub stored_bytes: u64,
    pub raw_rgb_bytes: u64,
    pub estimated_bytes_per_candidate: u64,
    pub cost: Option<CostRate>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub schema: String,
    pub policy: ScenePolicy,
    pub desired_seconds_per_cel: Ratio,
    pub delivery_fps: Ratio,
    pub cadence: Cadence,
    pub raster_size: [u32; 2],
    /// Authored descriptions, deliberately separate from renderer identity.
    pub visual_school: String,
    pub performance_style: String,
    pub animation_technique: String,
    pub references: Vec<Artifact>,
    pub budget: Budget,
}
#[derive(Debug, Serialize)]
pub struct Run {
    pub composition_sha256: String,
    pub start_frame: u64,
    pub end_frame: u64,
    pub drawing_slots: u64,
    pub duration_hint: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Preview {
    pub schema: String,
    pub settings_sha256: String,
    pub settings: Settings,
    pub language: String,
    pub sample_rate: u32,
    pub duration_samples: u64,
    pub delivery_frames: u64,
    pub desired_composition_count: u64,
    pub actual_composition_count: u64,
    pub unique_selected_drawings: u64,
    pub actual_run_drawing_slots: u64,
    /// Upper bound allowing each planned composition to restart its cadence.
    pub target_drawing_slots_upper_bound: u64,
    pub additional_candidates_upper_bound: u64,
    pub estimated_stored_bytes: u64,
    pub estimated_generation_cost_microunits: Option<u64>,
    pub raw_rgb_bytes: u64,
    pub more_compositions_requested: bool,
    pub exposure_asset_binding_required: bool,
    pub runs: Vec<Run>,
    pub rendered: bool,
    pub approved: bool,
}
fn description(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn ratio(r: Ratio) -> bool {
    (1..=1_000_000).contains(&r.numerator) && (1..=1_000_000).contains(&r.denominator)
}
impl Settings {
    pub fn check(&self) -> Result<()> {
        canonical_digest(self)?;
        if self.schema != "reel.workflow-film-direction-settings.v1"
            || scene::validate_policy(&self.policy).is_err()
            || !description(&self.policy.policy_id)
            || !ratio(self.desired_seconds_per_cel)
            || !ratio(self.delivery_fps)
            || u64::from(self.delivery_fps.numerator)
                > 240 * u64::from(self.delivery_fps.denominator)
            || self.raster_size.iter().any(|n| !(1..=8192).contains(n))
            || [
                &self.visual_school,
                &self.performance_style,
                &self.animation_technique,
            ]
            .iter()
            .any(|s| !description(s))
            || self.references.len() > 16
            || self
                .references
                .iter()
                .map(|a| &a.logical_id)
                .collect::<BTreeSet<_>>()
                .len()
                != self.references.len()
            || !(1..=1_000_000).contains(&self.budget.drawing_slots)
            || !(1..=16 * 1024 * 1024 * 1024).contains(&self.budget.stored_bytes)
            || !(1..=16 * 1024 * 1024 * 1024).contains(&self.budget.raw_rgb_bytes)
            || !(1..=128 * 1024 * 1024).contains(&self.budget.estimated_bytes_per_candidate)
            || matches!(self.cadence,Cadence::EveryFrames {frames} if frames==0 || frames>1_000_000)
            || self
                .budget
                .cost
                .as_ref()
                .is_some_and(|c| !description(&c.unit))
        {
            return fail(ErrorKind::Invalid, "film-direction-settings");
        }
        let seconds = f64::from(self.desired_seconds_per_cel.numerator)
            / f64::from(self.desired_seconds_per_cel.denominator);
        if seconds < self.policy.target_composition_seconds_min
            || seconds > self.policy.target_composition_seconds_max
        {
            return fail(ErrorKind::Conflict, "film-direction-desired-policy");
        }
        for a in &self.references {
            artifact(a)?;
        }
        Ok(())
    }
}
pub fn preview(
    settings: &Settings,
    language: &str,
    spans: &[NativeEventSpan],
    pictures: &BTreeMap<String, Artifact>,
) -> Result<Preview> {
    settings.check()?;
    if !token(language)
        || spans.is_empty()
        || spans.len() > 4096
        || spans
            .iter()
            .any(|s| !description(&s.event_id) || !description(&s.cue_id))
        || spans
            .iter()
            .map(|s| &s.event_id)
            .collect::<BTreeSet<_>>()
            .len()
            != spans.len()
        || pictures.keys().collect::<BTreeSet<_>>() != spans.iter().map(|s| &s.event_id).collect()
    {
        return fail(ErrorKind::Invalid, "film-direction-span-set");
    }
    let mut rate = 1u32;
    for s in spans {
        if s.sample_rate == 0
            || s.sample_rate > 384_000
            || s.end_sample <= s.start_sample
            || s.end_sample > u64::from(s.sample_rate) * 3600
        {
            return fail(ErrorKind::Invalid, "film-direction-source-clock");
        }
        rate = (rate / gcd(rate, s.sample_rate))
            .checked_mul(s.sample_rate)
            .ok_or(Error {
                kind: ErrorKind::Budget,
                code: "film-direction-common-clock".into(),
            })?;
    }
    let mut offset = 0u64;
    let mut previous: Option<&NativeEventSpan> = None;
    let mut cues = BTreeSet::new();
    let mut rendered = Vec::new();
    let mut selected = BTreeMap::new();
    for s in spans {
        if let Some(p) = previous {
            if p.cue_id == s.cue_id {
                if p.sample_rate != s.sample_rate || p.end_sample != s.start_sample {
                    return fail(ErrorKind::Conflict, "film-direction-cue-clock");
                }
            } else {
                offset += p.end_sample * u64::from(rate / p.sample_rate);
            }
        }
        if previous.is_none_or(|p| p.cue_id != s.cue_id)
            && (s.start_sample != 0 || !cues.insert(&s.cue_id))
        {
            return fail(ErrorKind::Conflict, "film-direction-cue-order");
        }
        let picture = &pictures[&s.event_id];
        artifact(picture)?;
        if selected
            .insert(picture.sha256.clone(), picture.bytes)
            .is_some_and(|n| n != picture.bytes)
        {
            return fail(ErrorKind::Integrity, "film-direction-picture-identity");
        }
        rendered.push(RenderedSpan {
            composition_id: picture.sha256.clone(),
            start_sample: offset + s.start_sample * u64::from(rate / s.sample_rate),
            end_sample: offset + s.end_sample * u64::from(rate / s.sample_rate),
        });
        previous = Some(s);
    }
    let duration = rendered.last().unwrap().end_sample;
    if duration > u64::from(rate) * 3600 {
        return fail(ErrorKind::Budget, "film-direction-duration");
    }
    let audited =
        scene::audit_rendered_compositions(&settings.policy, rate, &rendered).map_err(|_| {
            Error {
                kind: ErrorKind::Conflict,
                code: "film-direction-composition-policy".into(),
            }
        })?;
    let frame = |sample: u64, last: bool| {
        let n = u128::from(sample) * u128::from(settings.delivery_fps.numerator);
        let d = u128::from(rate) * u128::from(settings.delivery_fps.denominator);
        u64::try_from(if last { n.div_ceil(d) } else { n / d }).map_err(|_| Error {
            kind: ErrorKind::Budget,
            code: "film-direction-frame-count".into(),
        })
    };
    let frames = frame(duration, true)?;
    let desired = u64::try_from(
        (u128::from(duration) * u128::from(settings.desired_seconds_per_cel.denominator))
            .div_ceil(u128::from(rate) * u128::from(settings.desired_seconds_per_cel.numerator)),
    )
    .map_err(|_| Error {
        kind: ErrorKind::Budget,
        code: "film-direction-target-count".into(),
    })?;
    if frames > 1_000_000 || desired > frames {
        return fail(ErrorKind::Budget, "film-direction-unrepresentable-density");
    }
    let mut runs = Vec::new();
    let mut drawing_slots = 0;
    for r in &audited {
        let start = frame(r.start_sample, false)?;
        let end = frame(r.end_sample, r.end_sample == duration)?;
        if start >= end {
            return fail(ErrorKind::Conflict, "film-direction-subframe-composition");
        }
        let count = match settings.cadence {
            Cadence::HeldCels => 1,
            Cadence::EveryFrames { frames } => (end - start).div_ceil(u64::from(frames)),
        };
        drawing_slots += count;
        runs.push(Run {
            composition_sha256: r.composition_id.clone(),
            start_frame: start,
            end_frame: end,
            drawing_slots: count,
            duration_hint: r.target_hint.clone(),
        });
    }
    let compositions = desired.max(runs.len() as u64);
    let target = match settings.cadence {
        Cadence::HeldCels => compositions,
        Cadence::EveryFrames { frames: hold } => frames
            .div_ceil(u64::from(hold))
            .saturating_add(compositions - 1)
            .min(frames),
    };
    let extra = target.saturating_sub(selected.len() as u64);
    let selected_bytes = selected
        .values()
        .try_fold(0u64, |n, v| n.checked_add(*v))
        .ok_or(Error {
            kind: ErrorKind::Budget,
            code: "film-direction-storage-overflow".into(),
        })?;
    let stored = selected_bytes
        .checked_add(
            extra
                .checked_mul(settings.budget.estimated_bytes_per_candidate)
                .ok_or(Error {
                    kind: ErrorKind::Budget,
                    code: "film-direction-storage-overflow".into(),
                })?,
        )
        .ok_or(Error {
            kind: ErrorKind::Budget,
            code: "film-direction-storage-overflow".into(),
        })?;
    let rgb = frames * u64::from(settings.raster_size[0]) * u64::from(settings.raster_size[1]) * 3;
    let cost = settings
        .budget
        .cost
        .as_ref()
        .map(|c| {
            extra.checked_mul(c.microunits_per_candidate).ok_or(Error {
                kind: ErrorKind::Budget,
                code: "film-direction-cost-overflow".into(),
            })
        })
        .transpose()?;
    if target > settings.budget.drawing_slots
        || stored > settings.budget.stored_bytes
        || rgb > settings.budget.raw_rgb_bytes
    {
        return fail(ErrorKind::Budget, "film-direction-demand-budget");
    }
    let result = Preview {
        schema: "reel.workflow-film-direction-preview.v1".into(),
        settings_sha256: canonical_digest(settings)?,
        settings: settings.clone(),
        language: language.into(),
        sample_rate: rate,
        duration_samples: duration,
        delivery_frames: frames,
        desired_composition_count: desired,
        actual_composition_count: runs.len() as u64,
        unique_selected_drawings: selected.len() as u64,
        actual_run_drawing_slots: drawing_slots,
        target_drawing_slots_upper_bound: target,
        additional_candidates_upper_bound: extra,
        estimated_stored_bytes: stored,
        estimated_generation_cost_microunits: cost,
        raw_rgb_bytes: rgb,
        more_compositions_requested: desired > runs.len() as u64,
        exposure_asset_binding_required: matches!(settings.cadence, Cadence::EveryFrames { .. }),
        runs,
        rendered: false,
        approved: false,
    };
    canonical_digest(&result)?;
    Ok(result)
}
