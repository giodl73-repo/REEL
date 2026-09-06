use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

const REQUEST_SCHEMA: &str = "reel.sung-lyric-alignment.request.v1";
const DOCUMENT_SCHEMA: &str = "reel.sung-lyric-alignment.document.v1";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlignmentRequest {
    pub schema: String,
    pub recording_sha256: String,
    pub score_sha256: String,
    pub clock: ClockTransform,
    pub canonical: Vec<CanonicalSyllable>,
    pub score_events: Vec<ScoreEvent>,
    pub evidence: Vec<PerformedEvidence>,
    #[serde(default)]
    pub evidence_streams: Vec<EvidenceStream>,
    #[serde(default)]
    pub anchors: Vec<HumanAnchor>,
    #[serde(default)]
    pub resolve: Option<ResolveScope>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceStream {
    pub adapter: String,
    pub stream_sha256: String,
    pub clock: ClockTransform,
    pub observations: Vec<AdapterObservation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterObservation {
    pub observation_id: String,
    pub evidence_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence_micros: u32,
    #[serde(default)]
    pub normalized: Option<String>,
    #[serde(default)]
    pub phones: Vec<String>,
    #[serde(default)]
    pub vowel_nucleus: Option<String>,
    #[serde(default)]
    pub candidates: Vec<EvidenceCandidate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveScope {
    pub first_evidence_id: String,
    pub last_evidence_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ClockTransform {
    pub offset_ms: i64,
    pub rate_num: u64,
    pub rate_den: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalSyllable {
    pub index: u32,
    pub line_id: String,
    pub word: String,
    pub printed: String,
    pub normalized: String,
    pub phones: Vec<String>,
    pub vowel_nucleus: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreEvent {
    pub id: String,
    pub order: u32,
    pub measure: String,
    pub voice: String,
    pub kind: String,
    #[serde(default)]
    pub start_ms: Option<u64>,
    #[serde(default)]
    pub end_ms: Option<u64>,
    #[serde(default)]
    pub canonical_indices: Vec<u32>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PerformedEvidence {
    pub id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub class: String,
    #[serde(default)]
    pub normalized: Option<String>,
    #[serde(default)]
    pub phones: Vec<String>,
    #[serde(default)]
    pub vowel_nucleus: Option<String>,
    #[serde(default)]
    pub candidates: Vec<EvidenceCandidate>,
    pub sources: Vec<EvidenceSource>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceCandidate {
    pub canonical_index: u32,
    pub confidence_micros: u32,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSource {
    pub adapter: String,
    pub observation_id: String,
    pub confidence_micros: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanAnchor {
    pub correction_id: String,
    pub evidence_id: String,
    pub canonical_index: u32,
    pub reviewer: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignmentDocument {
    pub schema: String,
    pub recording_sha256: String,
    pub score_sha256: String,
    pub clock: ClockTransform,
    pub recommended: Vec<AlignmentEvent>,
    pub ranked_recommendations: Vec<SongRecommendation>,
    pub timing_segments: Vec<TimingSegment>,
    pub alternatives: Vec<Alternative>,
    pub coverage: Coverage,
    pub validation: Validation,
    pub review_state: String,
    pub resolved_scope: Option<ResolveScopeResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResolveScopeResult {
    pub first_evidence_id: String,
    pub last_evidence_id: String,
    pub first_position: usize,
    pub last_position: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SongRecommendation {
    pub rank: u32,
    pub score_micros: i64,
    pub canonical_path: Vec<Option<u32>>,
    pub differs_at_evidence_ids: Vec<String>,
    pub timing_segments: Vec<TimingSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TimingSegment {
    pub segment: u32,
    pub first_evidence_id: String,
    pub last_evidence_id: String,
    pub first_position: usize,
    pub last_position: usize,
    pub score_origin_ms: u64,
    pub recording_origin_ms: u64,
    pub cumulative_repeat_offset_ms: u64,
    pub comparison_offset_ms: i64,
    pub opened_by_repeat: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AlignmentEvent {
    pub event_id: String,
    pub evidence_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub class: String,
    pub canonical_index: Option<u32>,
    pub canonical_line_id: Option<String>,
    pub score_event_id: Option<String>,
    pub score_start_ms: Option<u64>,
    pub recording_score_offset_ms: Option<i64>,
    pub printed: Option<String>,
    pub normalized: Option<String>,
    pub word: Option<String>,
    pub phones: Vec<String>,
    pub vowel_nucleus: Option<String>,
    pub occurrence: Option<u32>,
    pub evidence: Vec<EvidenceSource>,
    pub confidence_micros: u32,
    pub local_margin_micros: u32,
    pub locked_human: bool,
    pub review_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Alternative {
    pub evidence_id: String,
    pub canonical_index: u32,
    pub score_micros: u32,
    pub rank: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Coverage {
    pub canonical_total: usize,
    pub covered_indices: Vec<u32>,
    pub omitted_indices: Vec<u32>,
    pub performed_lyric_events: usize,
    pub non_lyric_events: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Validation {
    pub chronological: bool,
    pub every_event_classified: bool,
    pub all_lyric_events_linked: bool,
    pub hashes_valid: bool,
    pub timing_checked_events: usize,
    pub timing_within_500ms: bool,
}

pub fn align_file(input: &Path, output: &Path) -> Result<AlignmentDocument> {
    let raw = fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let request: AlignmentRequest =
        serde_json::from_slice(&raw).context("parse alignment request")?;
    let document = align(&request)?;
    let bytes = serde_json::to_vec_pretty(&document)?;
    fs::write(output, [bytes.as_slice(), b"\n"].concat())
        .with_context(|| format!("write {}", output.display()))?;
    Ok(document)
}

pub fn align(request: &AlignmentRequest) -> Result<AlignmentDocument> {
    validate_request(request)?;
    let evidence = fuse_adapter_streams(request)?;
    let canonical: BTreeMap<u32, &CanonicalSyllable> =
        request.canonical.iter().map(|s| (s.index, s)).collect();
    let score_by_canonical: BTreeMap<u32, &ScoreEvent> = request
        .score_events
        .iter()
        .flat_map(|s| s.canonical_indices.iter().map(move |i| (*i, s)))
        .collect();
    let anchors: BTreeMap<&str, &HumanAnchor> = request
        .anchors
        .iter()
        .map(|a| (a.evidence_id.as_str(), a))
        .collect();
    let mut occurrences = BTreeMap::<u32, u32>::new();
    let mut recommended = Vec::new();
    let mut alternatives = Vec::new();

    let (ranked_recommendations, selected) = solve_ranked_paths(
        &evidence,
        &canonical,
        &score_by_canonical,
        &anchors,
        request.resolve.as_ref(),
    );
    let timing_segments = ranked_recommendations
        .first()
        .map(|r| r.timing_segments.clone())
        .unwrap_or_default();
    for (event_position, evidence) in evidence.iter().enumerate() {
        if evidence.class != "lyric" {
            recommended.push(non_lyric_event(evidence));
            continue;
        }
        let locked = anchors.get(evidence.id.as_str()).copied();
        let mut candidates = evidence.candidates.clone();
        if let Some(anchor) = locked {
            candidates.retain(|c| c.canonical_index == anchor.canonical_index);
            if candidates.is_empty() {
                candidates.push(EvidenceCandidate {
                    canonical_index: anchor.canonical_index,
                    confidence_micros: 1_000_000,
                });
            }
        }
        candidates.retain(|c| canonical.contains_key(&c.canonical_index));
        candidates.sort_by_key(|c| {
            (
                std::cmp::Reverse(score_candidate(evidence, canonical[&c.canonical_index], c)),
                c.canonical_index,
            )
        });
        let selected_index = selected.get(evidence.id.as_str()).copied().flatten();
        if let Some(index) = selected_index {
            candidates.sort_by_key(|c| {
                (
                    c.canonical_index != index,
                    std::cmp::Reverse(score_candidate(evidence, canonical[&c.canonical_index], c)),
                )
            });
        }
        let best = candidates.first().context(format!(
            "lyric evidence {} has no valid canonical candidate",
            evidence.id
        ))?;
        let best_score = score_candidate(evidence, canonical[&best.canonical_index], best);
        let second_score = candidates
            .get(1)
            .map(|c| score_candidate(evidence, canonical[&c.canonical_index], c))
            .unwrap_or(0);
        for (rank, candidate) in candidates.iter().skip(1).take(2).enumerate() {
            alternatives.push(Alternative {
                evidence_id: evidence.id.clone(),
                canonical_index: candidate.canonical_index,
                score_micros: score_candidate(
                    evidence,
                    canonical[&candidate.canonical_index],
                    candidate,
                ),
                rank: rank as u32 + 2,
            });
        }
        let syllable = canonical[&best.canonical_index];
        let occurrence = occurrences.entry(best.canonical_index).or_insert(0);
        *occurrence += 1;
        let mut review_reasons = Vec::new();
        if best_score.saturating_sub(second_score) < 100_000 {
            review_reasons.push("low-local-margin".into());
        }
        if *occurrence > 1 {
            review_reasons.push("performed-repeat".into());
        }
        recommended.push(AlignmentEvent {
            event_id: format!("performed:{}", evidence.id),
            evidence_id: evidence.id.clone(),
            start_ms: evidence.start_ms,
            end_ms: evidence.end_ms,
            class: "lyric".into(),
            canonical_index: Some(syllable.index),
            canonical_line_id: Some(syllable.line_id.clone()),
            score_event_id: score_by_canonical
                .get(&syllable.index)
                .map(|s| s.id.clone()),
            score_start_ms: score_by_canonical
                .get(&syllable.index)
                .and_then(|s| s.start_ms),
            recording_score_offset_ms: event_timing_offset(event_position, &timing_segments),
            printed: Some(syllable.printed.clone()),
            normalized: Some(syllable.normalized.clone()),
            word: Some(syllable.word.clone()),
            phones: syllable.phones.clone(),
            vowel_nucleus: Some(syllable.vowel_nucleus.clone()),
            occurrence: Some(*occurrence),
            evidence: evidence.sources.clone(),
            confidence_micros: best_score,
            local_margin_micros: best_score.saturating_sub(second_score),
            locked_human: locked.is_some(),
            review_reasons,
        });
    }
    let covered: BTreeSet<u32> = recommended
        .iter()
        .filter_map(|e| e.canonical_index)
        .collect();
    let omitted = canonical
        .keys()
        .filter(|i| !covered.contains(i))
        .copied()
        .collect();
    let coverage = Coverage {
        canonical_total: canonical.len(),
        covered_indices: covered.into_iter().collect(),
        omitted_indices: omitted,
        performed_lyric_events: recommended.iter().filter(|e| e.class == "lyric").count(),
        non_lyric_events: recommended.iter().filter(|e| e.class != "lyric").count(),
    };
    let validation = Validation {
        chronological: true,
        every_event_classified: true,
        all_lyric_events_linked: recommended
            .iter()
            .filter(|e| e.class == "lyric")
            .all(|e| e.canonical_index.is_some() && e.score_event_id.is_some()),
        hashes_valid: true,
        timing_checked_events: recommended
            .iter()
            .filter(|event| {
                event.score_start_ms.is_some() && event.recording_score_offset_ms.is_some()
            })
            .count(),
        timing_within_500ms: recommended.iter().all(|event| {
            match (event.score_start_ms, event.recording_score_offset_ms) {
                (Some(score), Some(offset)) => {
                    event
                        .start_ms
                        .abs_diff((score as i64 + offset).max(0) as u64)
                        <= 500
                }
                _ => true,
            }
        }),
    };
    if !validation.all_lyric_events_linked {
        bail!("every lyric event must link canonical and score identity");
    }
    Ok(AlignmentDocument {
        schema: DOCUMENT_SCHEMA.into(),
        recording_sha256: request.recording_sha256.clone(),
        score_sha256: request.score_sha256.clone(),
        clock: request.clock.clone(),
        recommended,
        ranked_recommendations,
        timing_segments,
        alternatives,
        coverage,
        validation,
        review_state: "machine-candidate-human-listening-required".into(),
        resolved_scope: resolve_scope_result(request, &evidence)?,
    })
}

fn transform_ms(value: u64, clock: &ClockTransform) -> Result<u64> {
    let scaled = (value as i128) * (clock.rate_num as i128) / (clock.rate_den as i128);
    let transformed = scaled + clock.offset_ms as i128;
    if transformed < 0 || transformed > u64::MAX as i128 {
        bail!("adapter clock transform is outside the recording clock");
    }
    Ok(transformed as u64)
}

fn fuse_adapter_streams(request: &AlignmentRequest) -> Result<Vec<PerformedEvidence>> {
    let mut fused = request.evidence.clone();
    let positions: BTreeMap<String, usize> = fused
        .iter()
        .enumerate()
        .map(|(n, e)| (e.id.clone(), n))
        .collect();
    for stream in &request.evidence_streams {
        if stream.stream_sha256.len() != 64
            || !stream.stream_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            bail!("adapter stream {} lacks a valid SHA-256", stream.adapter);
        }
        if stream.clock.rate_num == 0 || stream.clock.rate_den == 0 {
            bail!("adapter stream {} has an invalid clock", stream.adapter);
        }
        for observation in &stream.observations {
            let position = positions.get(&observation.evidence_id).with_context(|| {
                format!(
                    "adapter observation {} references unknown evidence {}",
                    observation.observation_id, observation.evidence_id
                )
            })?;
            let start = transform_ms(observation.start_ms, &stream.clock)?;
            let end = transform_ms(observation.end_ms, &stream.clock)?;
            if end <= start {
                bail!(
                    "adapter observation {} has an invalid transformed span",
                    observation.observation_id
                );
            }
            let event = &mut fused[*position];
            if end < event.start_ms.saturating_sub(500) || start > event.end_ms.saturating_add(500)
            {
                bail!(
                    "adapter observation {} does not support evidence {} in time",
                    observation.observation_id,
                    event.id
                );
            }
            if event.normalized.is_none() {
                event.normalized = observation.normalized.clone();
            }
            for phone in &observation.phones {
                if !event.phones.contains(phone) {
                    event.phones.push(phone.clone());
                }
            }
            if event.vowel_nucleus.is_none() {
                event.vowel_nucleus = observation.vowel_nucleus.clone();
            }
            for candidate in &observation.candidates {
                if let Some(existing) = event
                    .candidates
                    .iter_mut()
                    .find(|c| c.canonical_index == candidate.canonical_index)
                {
                    existing.confidence_micros =
                        existing.confidence_micros.max(candidate.confidence_micros);
                } else {
                    event.candidates.push(candidate.clone());
                }
            }
            event.sources.push(EvidenceSource {
                adapter: stream.adapter.clone(),
                observation_id: observation.observation_id.clone(),
                confidence_micros: observation.confidence_micros,
            });
        }
    }
    Ok(fused)
}

#[derive(Clone)]
struct BeamPath {
    score: i64,
    indices: Vec<Option<u32>>,
    baseline_offset_ms: Option<i64>,
    cumulative_repeat_offset_ms: u64,
    active_repeat: Option<ActiveRepeat>,
}

#[derive(Clone)]
struct ActiveRepeat {
    return_index: u32,
    recording_origin_ms: u64,
    segment_offset_ms: i64,
}

fn solve_ranked_paths(
    evidence: &[PerformedEvidence],
    canonical: &BTreeMap<u32, &CanonicalSyllable>,
    score_by_canonical: &BTreeMap<u32, &ScoreEvent>,
    anchors: &BTreeMap<&str, &HumanAnchor>,
    resolve: Option<&ResolveScope>,
) -> (Vec<SongRecommendation>, BTreeMap<String, Option<u32>>) {
    let mut beam = vec![BeamPath {
        score: 0,
        indices: Vec::new(),
        baseline_offset_ms: None,
        cumulative_repeat_offset_ms: 0,
        active_repeat: None,
    }];
    let bounds = resolve.and_then(|scope| {
        Some((
            evidence
                .iter()
                .position(|e| e.id == scope.first_evidence_id)?,
            evidence
                .iter()
                .position(|e| e.id == scope.last_evidence_id)?,
        ))
    });
    for (event_position, event) in evidence.iter().enumerate() {
        let choices: Vec<Option<u32>> = if event.class != "lyric" {
            vec![None]
        } else if let Some(anchor) = anchors.get(event.id.as_str()) {
            vec![Some(anchor.canonical_index)]
        } else if bounds
            .is_some_and(|(first, last)| event_position < first || event_position > last)
        {
            event
                .candidates
                .iter()
                .filter(|c| canonical.contains_key(&c.canonical_index))
                .max_by_key(|c| score_candidate(event, canonical[&c.canonical_index], c))
                .map(|c| vec![Some(c.canonical_index)])
                .unwrap_or_default()
        } else {
            event
                .candidates
                .iter()
                .filter(|c| canonical.contains_key(&c.canonical_index))
                .map(|c| Some(c.canonical_index))
                .collect()
        };
        let mut next = Vec::new();
        for path in &beam {
            let prior = path.indices.iter().rev().flatten().next().copied();
            for choice in &choices {
                let mut state = path.clone();
                if let (Some(active), Some(index)) = (&state.active_repeat, *choice)
                    && index > active.return_index
                {
                    state.cumulative_repeat_offset_ms = state
                        .cumulative_repeat_offset_ms
                        .saturating_add(event.start_ms.saturating_sub(active.recording_origin_ms));
                    state.active_repeat = None;
                }
                if let (Some(prior_index), Some(index)) = (prior, *choice)
                    && index < prior_index
                    && state.active_repeat.is_none()
                    && let Some(score_start) =
                        score_by_canonical.get(&index).and_then(|s| s.start_ms)
                {
                    state.active_repeat = Some(ActiveRepeat {
                        return_index: prior_index,
                        recording_origin_ms: event.start_ms,
                        segment_offset_ms: event.start_ms as i64 - score_start as i64,
                    });
                }
                let local = choice
                    .and_then(|index| {
                        event
                            .candidates
                            .iter()
                            .find(|c| c.canonical_index == index)
                            .map(|c| score_candidate(event, canonical[&index], c) as i64)
                    })
                    .unwrap_or(0);
                let timing = choice
                    .and_then(|index| score_by_canonical.get(&index).and_then(|s| s.start_ms))
                    .map(|score_start| {
                        let observed_offset = event.start_ms as i64 - score_start as i64;
                        let expected_offset = if let Some(active) = &state.active_repeat {
                            active.segment_offset_ms
                        } else if let Some(baseline) = state.baseline_offset_ms {
                            baseline + state.cumulative_repeat_offset_ms as i64
                        } else {
                            state.baseline_offset_ms = Some(observed_offset);
                            observed_offset
                        };
                        160_000i64.saturating_sub(
                            observed_offset.abs_diff(expected_offset).min(800) as i64 * 200,
                        )
                    })
                    .unwrap_or(0);
                let transition = match (prior, *choice) {
                    (_, None) => 0,
                    (None, Some(_)) => 0,
                    (Some(a), Some(b)) if b == a + 1 => 180_000,
                    (Some(a), Some(b)) if b == a => -120_000,
                    (Some(a), Some(b)) if b < a && a - b <= 32 => {
                        -220_000 - ((a - b) as i64 * 2_000)
                    }
                    (Some(a), Some(b)) if b > a + 1 => -80_000 - ((b - a - 1) as i64 * 12_000),
                    _ => -700_000,
                };
                state.indices.push(*choice);
                next.push(BeamPath {
                    score: path.score + local + transition + timing,
                    ..state
                });
            }
        }
        next.sort_by_key(|p| std::cmp::Reverse(p.score));
        next.dedup_by(|a, b| a.indices == b.indices);
        next.truncate(64);
        beam = next;
    }
    beam.sort_by_key(|p| std::cmp::Reverse(p.score));
    let best = beam.first().cloned().unwrap_or(BeamPath {
        score: 0,
        indices: vec![],
        baseline_offset_ms: None,
        cumulative_repeat_offset_ms: 0,
        active_repeat: None,
    });
    let ranked = beam
        .iter()
        .take(3)
        .enumerate()
        .map(|(rank, path)| SongRecommendation {
            rank: rank as u32 + 1,
            score_micros: path.score,
            canonical_path: path.indices.clone(),
            differs_at_evidence_ids: path
                .indices
                .iter()
                .zip(&best.indices)
                .enumerate()
                .filter_map(|(n, (a, b))| {
                    if a != b {
                        Some(evidence[n].id.clone())
                    } else {
                        None
                    }
                })
                .collect(),
            timing_segments: build_timing_segments(&path.indices, evidence, score_by_canonical),
        })
        .collect();
    let selected = evidence
        .iter()
        .zip(best.indices)
        .map(|(e, i)| (e.id.clone(), i))
        .collect();
    (ranked, selected)
}

fn resolve_scope_result(
    request: &AlignmentRequest,
    evidence: &[PerformedEvidence],
) -> Result<Option<ResolveScopeResult>> {
    let Some(scope) = &request.resolve else {
        return Ok(None);
    };
    let first = evidence
        .iter()
        .position(|e| e.id == scope.first_evidence_id)
        .context("resolve scope first evidence is unknown")?;
    let last = evidence
        .iter()
        .position(|e| e.id == scope.last_evidence_id)
        .context("resolve scope last evidence is unknown")?;
    if first > last {
        bail!("resolve scope is reversed");
    }
    let anchors: BTreeSet<&str> = request
        .anchors
        .iter()
        .map(|a| a.evidence_id.as_str())
        .collect();
    if first > 0 && !anchors.contains(evidence[first - 1].id.as_str()) {
        bail!("bounded re-solve requires a locked left boundary");
    }
    if last + 1 < evidence.len() && !anchors.contains(evidence[last + 1].id.as_str()) {
        bail!("bounded re-solve requires a locked right boundary");
    }
    Ok(Some(ResolveScopeResult {
        first_evidence_id: scope.first_evidence_id.clone(),
        last_evidence_id: scope.last_evidence_id.clone(),
        first_position: first,
        last_position: last,
    }))
}

fn build_timing_segments(
    indices: &[Option<u32>],
    evidence: &[PerformedEvidence],
    score_by_canonical: &BTreeMap<u32, &ScoreEvent>,
) -> Vec<TimingSegment> {
    let Some((first_position, first_index, first_score)) =
        indices.iter().enumerate().find_map(|(position, index)| {
            let index = (*index)?;
            let score = score_by_canonical.get(&index)?.start_ms?;
            Some((position, index, score))
        })
    else {
        return vec![];
    };
    let mut segments = Vec::new();
    let mut segment_start = first_position;
    let mut segment_score_origin = first_score;
    let mut segment_recording_origin = evidence[first_position].start_ms;
    let mut segment_is_repeat = false;
    let mut cumulative = 0u64;
    let baseline_offset = segment_recording_origin as i64 - segment_score_origin as i64;
    let mut active_repeat: Option<(u32, u64)> = None;
    let mut prior = first_index;

    for position in first_position + 1..indices.len() {
        let Some(index) = indices[position] else {
            continue;
        };
        let Some(score_start) = score_by_canonical.get(&index).and_then(|s| s.start_ms) else {
            continue;
        };
        if let Some((return_index, repeat_origin)) = active_repeat
            && index > return_index
        {
            push_timing_segment(
                &mut segments,
                evidence,
                segment_start,
                position - 1,
                segment_score_origin,
                segment_recording_origin,
                cumulative,
                if segment_is_repeat {
                    segment_recording_origin as i64 - segment_score_origin as i64
                } else {
                    baseline_offset + cumulative as i64
                },
                segment_is_repeat,
            );
            cumulative = cumulative
                .saturating_add(evidence[position].start_ms.saturating_sub(repeat_origin));
            segment_start = position;
            segment_score_origin = score_start;
            segment_recording_origin = evidence[position].start_ms;
            segment_is_repeat = false;
            active_repeat = None;
        }
        if index < prior && active_repeat.is_none() {
            push_timing_segment(
                &mut segments,
                evidence,
                segment_start,
                position - 1,
                segment_score_origin,
                segment_recording_origin,
                cumulative,
                if segment_is_repeat {
                    segment_recording_origin as i64 - segment_score_origin as i64
                } else {
                    baseline_offset + cumulative as i64
                },
                segment_is_repeat,
            );
            active_repeat = Some((prior, evidence[position].start_ms));
            segment_start = position;
            segment_score_origin = score_start;
            segment_recording_origin = evidence[position].start_ms;
            segment_is_repeat = true;
        }
        prior = index;
    }
    push_timing_segment(
        &mut segments,
        evidence,
        segment_start,
        indices.len().saturating_sub(1),
        segment_score_origin,
        segment_recording_origin,
        cumulative,
        if segment_is_repeat {
            segment_recording_origin as i64 - segment_score_origin as i64
        } else {
            baseline_offset + cumulative as i64
        },
        segment_is_repeat,
    );
    segments
}

#[allow(clippy::too_many_arguments)]
fn push_timing_segment(
    segments: &mut Vec<TimingSegment>,
    evidence: &[PerformedEvidence],
    first: usize,
    last: usize,
    score_origin_ms: u64,
    recording_origin_ms: u64,
    cumulative_repeat_offset_ms: u64,
    comparison_offset_ms: i64,
    opened_by_repeat: bool,
) {
    if first > last || first >= evidence.len() || last >= evidence.len() {
        return;
    }
    segments.push(TimingSegment {
        segment: segments.len() as u32 + 1,
        first_evidence_id: evidence[first].id.clone(),
        last_evidence_id: evidence[last].id.clone(),
        first_position: first,
        last_position: last,
        score_origin_ms,
        recording_origin_ms,
        cumulative_repeat_offset_ms,
        comparison_offset_ms,
        opened_by_repeat,
    });
}

fn event_timing_offset(position: usize, segments: &[TimingSegment]) -> Option<i64> {
    segments
        .iter()
        .find(|segment| position >= segment.first_position && position <= segment.last_position)
        .map(|segment| segment.comparison_offset_ms)
}

fn validate_request(request: &AlignmentRequest) -> Result<()> {
    if request.schema != REQUEST_SCHEMA {
        bail!("unsupported schema: {}", request.schema);
    }
    for (name, value) in [
        ("recording_sha256", &request.recording_sha256),
        ("score_sha256", &request.score_sha256),
    ] {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("{name} must be a 64-character SHA-256");
        }
    }
    if request.clock.rate_num == 0 || request.clock.rate_den == 0 {
        bail!("clock rate must be non-zero");
    }
    let mut indices = BTreeSet::new();
    for syllable in &request.canonical {
        if !indices.insert(syllable.index) {
            bail!("duplicate canonical index {}", syllable.index);
        }
        if syllable.normalized.is_empty()
            || syllable.vowel_nucleus.is_empty()
            || syllable.phones.is_empty()
        {
            bail!(
                "canonical syllable {} lacks phonetic identity",
                syllable.index
            );
        }
    }
    let mut prior_end = 0;
    let mut evidence_ids = BTreeSet::new();
    for evidence in &request.evidence {
        if !evidence_ids.insert(&evidence.id) {
            bail!("duplicate evidence id {}", evidence.id);
        }
        if evidence.end_ms <= evidence.start_ms || evidence.start_ms < prior_end {
            bail!("evidence timeline is not chronological at {}", evidence.id);
        }
        prior_end = evidence.end_ms;
        if evidence.sources.is_empty() {
            bail!("evidence {} has no provenance", evidence.id);
        }
        if evidence.class == "lyric" && evidence.candidates.is_empty() {
            bail!("lyric evidence {} has no candidates", evidence.id);
        }
        if evidence.class == "silence" && !evidence.candidates.is_empty() {
            bail!(
                "confirmed silence {} cannot carry lyric candidates",
                evidence.id
            );
        }
    }
    for anchor in &request.anchors {
        if !evidence_ids.contains(&anchor.evidence_id) || !indices.contains(&anchor.canonical_index)
        {
            bail!(
                "anchor {} references unknown identity",
                anchor.correction_id
            );
        }
    }
    Ok(())
}

fn score_candidate(e: &PerformedEvidence, s: &CanonicalSyllable, c: &EvidenceCandidate) -> u32 {
    let mut score = c.confidence_micros.min(1_000_000) / 2;
    if e.normalized.as_deref() == Some(s.normalized.as_str()) {
        score += 200_000;
    }
    if e.vowel_nucleus.as_deref() == Some(s.vowel_nucleus.as_str()) {
        score += 175_000;
    }
    if !e.phones.is_empty() && e.phones == s.phones {
        score += 125_000;
    }
    score.min(1_000_000)
}

fn non_lyric_event(e: &PerformedEvidence) -> AlignmentEvent {
    AlignmentEvent {
        event_id: format!("performed:{}", e.id),
        evidence_id: e.id.clone(),
        start_ms: e.start_ms,
        end_ms: e.end_ms,
        class: e.class.clone(),
        canonical_index: None,
        canonical_line_id: None,
        score_event_id: None,
        score_start_ms: None,
        recording_score_offset_ms: None,
        printed: None,
        normalized: None,
        word: None,
        phones: e.phones.clone(),
        vowel_nucleus: e.vowel_nucleus.clone(),
        occurrence: None,
        evidence: e.sources.clone(),
        confidence_micros: e
            .sources
            .iter()
            .map(|s| s.confidence_micros)
            .max()
            .unwrap_or(0),
        local_margin_micros: 0,
        locked_human: false,
        review_reasons: vec![],
    }
}
