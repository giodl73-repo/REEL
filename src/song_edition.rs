//! Bounded short-edition planning and sample-preserving source-evidence projection.
//! This in-memory core grants no rights, lyric qualification or creative approval.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;
mod lyrics;
pub mod packet;
pub mod repair;
pub mod source;
mod transcripts;
mod wave;

const MAX_FRAMES: u64 = 600 * 96_000;
const MAX_PASSES: usize = 16;
const MAX_TOKENS: usize = 10_000;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub segments: Vec<[f64; 2]>,
    pub crossfade_seconds: f64,
    pub fade_in_seconds: f64,
    pub fade_out_seconds: f64,
    pub max_duration_seconds: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub source_start: u64,
    pub source_end: u64,
    pub output_start: u64,
    pub core_start: u64,
    pub core_end: u64,
    pub blend_frames: u64,
}

/// Plans can only be constructed by validation, never trusted from caller JSON.
#[derive(Clone, Debug, Serialize)]
pub struct Plan {
    sample_rate: u32,
    source_frames: u64,
    output_frames: u64,
    segments: Vec<Segment>,
    fade_in_frames: u64,
    fade_out_frames: u64,
}

impl Plan {
    pub fn output_frames(&self) -> u64 {
        self.output_frames
    }
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }
}

fn frames(seconds: f64, rate: u32) -> Result<u64> {
    if !seconds.is_finite() || !(0.0..=600.0).contains(&seconds) {
        bail!("edition seconds must be finite and between zero and 600");
    }
    // Matches the existing recipe contract's round(seconds * rate), including ties.
    Ok((seconds * f64::from(rate)).round_ties_even() as u64)
}

pub fn plan(recipe: &Recipe, rate: u32, source_frames: u64) -> Result<Plan> {
    if !(8_000..=96_000).contains(&rate)
        || source_frames == 0
        || source_frames > MAX_FRAMES
        || source_frames > u64::from(rate) * 600
        || !(1..=64).contains(&recipe.segments.len())
        || !recipe.crossfade_seconds.is_finite()
        || !(0.0..=1.0).contains(&recipe.crossfade_seconds)
        || !recipe.fade_in_seconds.is_finite()
        || !(0.0..=5.0).contains(&recipe.fade_in_seconds)
        || !recipe.fade_out_seconds.is_finite()
        || !(0.0..=5.0).contains(&recipe.fade_out_seconds)
        || !recipe.max_duration_seconds.is_finite()
        || recipe.max_duration_seconds <= 0.0
        || recipe.max_duration_seconds > 600.0
    {
        bail!("edition clock, segment or fade bounds");
    }
    let blend = frames(recipe.crossfade_seconds, rate)?;
    let fade_in_frames = frames(recipe.fade_in_seconds, rate)?;
    let fade_out_frames = frames(recipe.fade_out_seconds, rate)?;
    let mut segments = Vec::new();
    let mut previous_end = 0;
    let mut cursor = 0u64;
    for (index, &[start, end]) in recipe.segments.iter().enumerate() {
        let start = frames(start, rate)?;
        let end = frames(end, rate)?;
        if start < previous_end || start >= end || end > source_frames {
            bail!("edition segments must be ordered, nonoverlapping and in bounds");
        }
        let blend_frames = if index == 0 { 0 } else { blend };
        let left = blend_frames.max(if index == 0 { fade_in_frames } else { 0 });
        let right = (if index + 1 < recipe.segments.len() {
            blend
        } else {
            0
        })
        .max(if index + 1 == recipe.segments.len() {
            fade_out_frames
        } else {
            0
        });
        if end - start <= left + right {
            bail!("edition fades consume a segment");
        }
        let output_start = cursor
            .checked_sub(blend_frames)
            .ok_or_else(|| anyhow::anyhow!("edition blend exceeds previous segment"))?;
        segments.push(Segment {
            source_start: start,
            source_end: end,
            output_start,
            core_start: start + left,
            core_end: end - right,
            blend_frames,
        });
        cursor = output_start + end - start;
        previous_end = end;
    }
    if cursor == 0 || cursor as f64 / f64::from(rate) > recipe.max_duration_seconds {
        bail!("edition exceeds hard duration ceiling");
    }
    Ok(Plan {
        sample_rate: rate,
        source_frames,
        output_frames: cursor,
        segments,
        fade_in_frames,
        fade_out_frames,
    })
}

fn ramp(index: usize, count: usize) -> f32 {
    if count == 1 {
        0.0
    } else {
        (index as f64 / (count - 1) as f64) as f32
    }
}

/// Interleaved float32 PCM; callers own immutable decoding, WAVE IO and custody.
/// All evidence-bearing cores are independently compared by exact sample bits.
pub fn render(plan: &Plan, samples: &[f32], channels: u16) -> Result<Vec<f32>> {
    if !(1..=2).contains(&channels)
        || samples.len() as u64 != plan.source_frames * u64::from(channels)
        || samples.iter().any(|s| !s.is_finite())
    {
        bail!("edition source shape or finite-sample check");
    }
    let channels = usize::from(channels);
    let mut output = Vec::with_capacity(plan.output_frames as usize * channels);
    for row in &plan.segments {
        let source =
            &samples[row.source_start as usize * channels..row.source_end as usize * channels];
        let blend = row.blend_frames as usize;
        if blend != 0 {
            let offset = output.len() - blend * channels;
            for frame in 0..blend {
                let weight = ramp(frame, blend);
                for channel in 0..channels {
                    let at = frame * channels + channel;
                    // Keep separate float32 multiply/adds; never modify the core.
                    output[offset + at] =
                        output[offset + at] * (1.0 - weight) + source[at] * weight;
                }
            }
        }
        output.extend_from_slice(&source[blend * channels..]);
    }
    for (count, at_end) in [
        (plan.fade_in_frames as usize, false),
        (plan.fade_out_frames as usize, true),
    ] {
        let offset = if at_end {
            output.len() / channels - count
        } else {
            0
        };
        for frame in 0..count {
            let weight = ramp(if at_end { count - frame - 1 } else { frame }, count);
            for channel in 0..channels {
                output[(offset + frame) * channels + channel] *= weight;
            }
        }
    }
    verify_cores(plan, samples, &output, channels as u16)?;
    Ok(output)
}

pub fn verify_cores(plan: &Plan, source: &[f32], output: &[f32], channels: u16) -> Result<()> {
    if !(1..=2).contains(&channels)
        || source.len() as u64 != plan.source_frames * u64::from(channels)
        || output.len() as u64 != plan.output_frames * u64::from(channels)
        || source.iter().chain(output).any(|s| !s.is_finite())
    {
        bail!("edition verification shape or nonfinite samples");
    }
    let c = usize::from(channels);
    for row in &plan.segments {
        let out = row.output_start + row.core_start - row.source_start;
        let size = (row.core_end - row.core_start) as usize * c;
        let before = &source[row.core_start as usize * c..row.core_start as usize * c + size];
        let after = &output[out as usize * c..out as usize * c + size];
        if before
            .iter()
            .zip(after)
            .any(|(a, b)| a.to_bits() != b.to_bits())
        {
            bail!("evidence-bearing source samples changed");
        }
    }
    Ok(())
}

/// Preserve complete original token/pass metadata under `source_pass_metadata`;
/// only spans fully contained in an unchanged core become derived evidence.
/// Prompt provenance and source hash validation belong to the admitting adapter.
pub fn remap_passes(plan: &Plan, passes: &[Value]) -> Result<Vec<Value>> {
    if passes.is_empty() || passes.len() > MAX_PASSES {
        bail!("edition pass budget");
    }
    let rate = f64::from(plan.sample_rate);
    let mut names = std::collections::BTreeSet::new();
    let mut result = Vec::new();
    for pass in passes {
        let name = pass
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or_else(|| anyhow::anyhow!("source pass name missing"))?;
        if !names.insert(name) || pass.get("canonical_prompt").is_some_and(|p| p != false) {
            bail!("duplicate or prompt-conditioned source pass");
        }
        let tokens = pass
            .get("tokens")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("source tokens missing"))?;
        if tokens.len() > MAX_TOKENS {
            bail!("edition token budget");
        }
        let mut retained = Vec::new();
        for row in &plan.segments {
            for token in tokens {
                token
                    .get("word")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty() && s.len() <= 1024)
                    .ok_or_else(|| anyhow::anyhow!("invalid original word"))?;
                let Some(span) = token.get("span").and_then(Value::as_array) else {
                    continue;
                };
                if span.len() != 2 {
                    continue;
                }
                let (Some(start), Some(end)) = (span[0].as_f64(), span[1].as_f64()) else {
                    continue;
                };
                if !start.is_finite()
                    || !end.is_finite()
                    || start > end
                    || start < row.core_start as f64 / rate
                    || end > row.core_end as f64 / rate
                {
                    continue;
                }
                let mut token = token.clone();
                let offset = (row.output_start as f64 - row.source_start as f64) / rate;
                token["span"] = serde_json::json!([start + offset, end + offset]);
                retained.push(token);
            }
        }
        let mut metadata = pass.clone();
        metadata
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("pass object missing"))?
            .remove("tokens");
        let text = retained
            .iter()
            .map(|t| t["word"].as_str().unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        result.push(
            serde_json::json!({"name":format!("source-{name}-unchanged-samples"),
            "tokens":retained,"text":text,"source_pass":name,"source_pass_metadata":metadata,
            "new_recognition":false,"new_performance":false,"new_human_approval":false}),
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
