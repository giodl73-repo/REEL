use std::{collections::BTreeSet, path::Path};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    analysis::{self, AnalysisManifest, Analyzer, Observation, ObservationValue},
    hash::{canonical_sha256, sha256_bytes},
    nonempty, source,
    time::SampleRange,
    validate_sha256,
    validation_inputs::ValidationInputs,
};

pub const SCHEMA: &str = "reel.music-conformance-policy.v1";

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub track_id: String,
    pub label: String,
    pub artist: String,
    pub source_contract_sha256: String,
    pub intent_sha256: String,
    pub tempo: Option<TempoStandard>,
    pub duration: Option<DurationStandard>,
    #[serde(default)]
    pub instruments: Vec<InstrumentStandard>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceStandard {
    pub analyzer_sha256: Vec<String>,
    pub minimum_confidence_millionths: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TempoStandard {
    pub target_milli_bpm: u32,
    pub tolerance_milli_bpm: u32,
    #[serde(default)]
    pub allow_half_double_pulse: bool,
    pub evidence: EvidenceStandard,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DurationStandard {
    pub target_milliseconds: u64,
    pub tolerance_milliseconds: u64,
}

#[derive(Debug, Serialize)]
pub struct DurationFinding {
    pub observed_milliseconds: u64,
    pub target_milliseconds: u64,
    pub outcome: Outcome,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentStandard {
    pub instrument: String,
    #[serde(default)]
    pub equivalent_labels: Vec<String>,
    pub role: InstrumentRole,
    pub evidence: EvidenceStandard,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum InstrumentRole {
    RequiredEachTrack,
    RequestedOnTrack,
    EnsemblePalette,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Outcome {
    Supported,
    ConflictingEvidence,
    Unverified,
    ContextOnly,
}

#[derive(Debug, Serialize)]
pub struct Evidence {
    pub observation_id: String,
    pub analyzer_id: String,
    pub source: SampleRange,
    pub confidence_millionths: u32,
    pub value: ObservationValue,
    pub uncertainty: String,
}

#[derive(Debug, Serialize)]
pub struct Finding {
    pub check: String,
    pub outcome: Outcome,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub schema: String,
    pub policy_sha256: String,
    pub analysis_sha256: String,
    pub analysis_contract_sha256: String,
    pub source_contract_sha256: String,
    pub track_id: String,
    pub duration: Option<DurationFinding>,
    pub findings: Vec<Finding>,
}

/// Evaluate exact-source observations against explicitly supplied production intent.
pub fn evaluate(policy_path: &Path, analysis_path: &Path) -> Result<Report> {
    let policy_bytes = std::fs::read(policy_path)?;
    let policy: Policy = serde_yaml::from_slice(&policy_bytes)?;
    validate_policy(&policy)?;
    let mut inputs = ValidationInputs::filesystem();
    let manifest = analysis::load_with_inputs(analysis_path, &mut inputs)?;
    let verified = analysis::validate_loaded(analysis_path, &manifest, &mut inputs)?;
    if !policy
        .source_contract_sha256
        .eq_ignore_ascii_case(&verified.source_contract_sha256)
    {
        bail!("policy and analysis refer to different source contracts");
    }
    let duration = if let Some(standard) = &policy.duration {
        let source_path = source::resolve(analysis_path, &manifest.source.manifest);
        let geometry = source::load_with_inputs(&source_path, &mut inputs)?
            .media
            .timebase;
        let sample_rate = u128::from(geometry.sample_rate_hz);
        let milliseconds = u64::try_from(
            (u128::from(geometry.samples_per_channel) * 1_000 + sample_rate / 2) / sample_rate,
        )?;
        Some(DurationFinding {
            observed_milliseconds: milliseconds,
            target_milliseconds: standard.target_milliseconds,
            outcome: if milliseconds.abs_diff(standard.target_milliseconds)
                <= standard.tolerance_milliseconds
            {
                Outcome::Supported
            } else {
                Outcome::ConflictingEvidence
            },
        })
    } else {
        None
    };
    Ok(Report {
        schema: "reel.music-conformance-report.v1".into(),
        policy_sha256: sha256_bytes(&policy_bytes),
        analysis_sha256: verified.manifest_sha256,
        analysis_contract_sha256: verified.contract_sha256,
        source_contract_sha256: verified.source_contract_sha256,
        track_id: policy.track_id.clone(),
        duration,
        findings: findings(&policy, &manifest)?,
    })
}

/// Fingerprint a REEL analyzer's declared identity independently of its local ID.
pub fn analyzer_fingerprint(analyzer: &Analyzer) -> Result<String> {
    canonical_sha256(&(
        &analyzer.adapter,
        &analyzer.version,
        &analyzer.model_revision,
        analyzer.parameters_sha256.to_ascii_lowercase(),
    ))
}

fn validate_standard(standard: &EvidenceStandard) -> Result<()> {
    if !(1..=1_000_000).contains(&standard.minimum_confidence_millionths) {
        bail!("minimum confidence must be between 1 and 1000000");
    }
    for pin in &standard.analyzer_sha256 {
        validate_sha256("analyzer fingerprint", pin)?;
    }
    Ok(())
}

fn validate_policy(policy: &Policy) -> Result<()> {
    if policy.schema != SCHEMA {
        bail!("unsupported music conformance policy schema");
    }
    nonempty("track_id", &policy.track_id)?;
    nonempty("label", &policy.label)?;
    nonempty("artist", &policy.artist)?;
    validate_sha256("source contract", &policy.source_contract_sha256)?;
    validate_sha256("intent", &policy.intent_sha256)?;
    if let Some(tempo) = &policy.tempo {
        if !(20_000..=400_000).contains(&tempo.target_milli_bpm)
            || tempo.tolerance_milli_bpm > tempo.target_milli_bpm / 4
        {
            bail!("invalid tempo target or tolerance");
        }
        validate_standard(&tempo.evidence)?;
    }
    if let Some(duration) = &policy.duration {
        if duration.target_milliseconds == 0
            || duration.tolerance_milliseconds > duration.target_milliseconds / 4
        {
            bail!("invalid duration target or tolerance");
        }
    }
    let mut names = BTreeSet::new();
    for instrument in &policy.instruments {
        nonempty("instrument", &instrument.instrument)?;
        if !names.insert(normalize_label(&instrument.instrument)) {
            bail!("duplicate instrument standard: {}", instrument.instrument);
        }
        for label in &instrument.equivalent_labels {
            nonempty("equivalent label", label)?;
        }
        validate_standard(&instrument.evidence)?;
    }
    Ok(())
}

fn normalize_label(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn accepted<'a>(
    manifest: &'a AnalysisManifest,
    standard: &EvidenceStandard,
) -> Result<Vec<&'a Observation>> {
    let mut analyzer_ids = BTreeSet::new();
    for analyzer in &manifest.analyzers {
        let fingerprint = analyzer_fingerprint(analyzer)?;
        if standard
            .analyzer_sha256
            .iter()
            .any(|pin| pin.eq_ignore_ascii_case(&fingerprint))
        {
            analyzer_ids.insert(analyzer.id.as_str());
        }
    }
    Ok(manifest
        .observations
        .iter()
        .filter(|observation| {
            observation.confidence_millionths >= standard.minimum_confidence_millionths
                && analyzer_ids.contains(observation.analyzer_id.as_str())
        })
        .collect())
}

fn convert(observation: &Observation) -> Evidence {
    Evidence {
        observation_id: observation.id.clone(),
        analyzer_id: observation.analyzer_id.clone(),
        source: observation.source,
        confidence_millionths: observation.confidence_millionths,
        value: observation.value.clone(),
        uncertainty: observation.uncertainty.clone(),
    }
}

fn tempo_matches(observed: u32, tempo: &TempoStandard) -> bool {
    let target = u64::from(tempo.target_milli_bpm);
    let tolerance = u64::from(tempo.tolerance_milli_bpm);
    let observed = u64::from(observed);
    observed.abs_diff(target) <= tolerance
        || (tempo.allow_half_double_pulse
            && ((observed * 2).abs_diff(target) <= tolerance
                || observed.abs_diff(target * 2) <= tolerance * 2))
}

fn findings(policy: &Policy, manifest: &AnalysisManifest) -> Result<Vec<Finding>> {
    let mut findings = Vec::new();
    if let Some(tempo) = &policy.tempo {
        let mut in_range = false;
        let mut out_of_range = false;
        let mut evidence = Vec::new();
        for item in accepted(manifest, &tempo.evidence)? {
            if let ObservationValue::Tempo { milli_bpm } = item.value {
                if tempo_matches(milli_bpm, tempo) {
                    in_range = true;
                } else {
                    out_of_range = true;
                }
                evidence.push(convert(item));
            }
        }
        let outcome = match (in_range, out_of_range) {
            (true, false) => Outcome::Supported,
            (_, true) => Outcome::ConflictingEvidence,
            _ => Outcome::Unverified,
        };
        findings.push(Finding {
            check: "tempo".into(),
            outcome,
            evidence,
        });
    }

    for instrument in &policy.instruments {
        let labels: BTreeSet<_> = std::iter::once(&instrument.instrument)
            .chain(&instrument.equivalent_labels)
            .map(|label| normalize_label(label))
            .collect();
        let evidence: Vec<_> = accepted(manifest, &instrument.evidence)?
            .into_iter()
            .filter(|item| match &item.value {
                ObservationValue::Instrumentation { label } => {
                    labels.contains(&normalize_label(label))
                }
                _ => false,
            })
            .map(convert)
            .collect();
        let outcome = match (instrument.role, evidence.is_empty()) {
            (InstrumentRole::EnsemblePalette, _) => Outcome::ContextOnly,
            (_, false) => Outcome::Supported,
            (_, true) => Outcome::Unverified,
        };
        findings.push(Finding {
            check: instrument.instrument.clone(),
            outcome,
            evidence,
        });
    }
    Ok(findings)
}
