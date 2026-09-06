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
    pub anchors: Vec<HumanAnchor>,
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
    pub alternatives: Vec<Alternative>,
    pub coverage: Coverage,
    pub validation: Validation,
    pub review_state: String,
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

    for evidence in &request.evidence {
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
        alternatives,
        coverage,
        validation,
        review_state: "machine-candidate-human-listening-required".into(),
    })
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
