use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    DecisionRef,
    hash::{canonical_sha256, sha256_path},
    nonempty,
    source::{self, NetworkPolicy, RawPcmFormat},
    status_requires_decision,
    time::{AudioTimebase, SampleRange},
    unique_nonempty, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-analysis.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AnalysisManifest {
    pub schema: String,
    pub analysis_id: String,
    pub source: SourceBinding,
    pub analyzers: Vec<Analyzer>,
    #[serde(default)]
    pub stems: Vec<StemEvidence>,
    pub observations: Vec<Observation>,
    pub limitations: Vec<String>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub decoded_pcm_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Analyzer {
    pub id: String,
    pub adapter: String,
    pub version: String,
    pub model_revision: String,
    pub parameters_sha256: String,
    pub license: String,
    pub network_policy: NetworkPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StemEvidence {
    pub id: String,
    pub role: StemRole,
    pub path: PathBuf,
    pub sha256: String,
    pub decoded_pcm_sha256: String,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
    pub mixture_consistency_millionths: u32,
    pub bleed_millionths: u32,
    pub uncertainty: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum StemRole {
    Vocals,
    Accompaniment,
    Drums,
    Bass,
    Harmony,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub id: String,
    pub analyzer_id: String,
    pub source: SampleRange,
    pub confidence_millionths: u32,
    pub uncertainty: String,
    pub value: ObservationValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ObservationValue {
    Tempo {
        milli_bpm: u32,
    },
    Meter {
        numerator: u8,
        denominator: u8,
    },
    Beat {
        index: u32,
    },
    Bar {
        index: u32,
    },
    Form {
        label: String,
    },
    Pitch {
        midi_note: u8,
        cents: i16,
    },
    Harmony {
        symbol: String,
    },
    Bass {
        midi_note: u8,
        cents: i16,
    },
    Rhythm {
        label: String,
    },
    Hook {
        label: String,
    },
    Instrumentation {
        label: String,
    },
    VocalAlignment {
        text_layer_sha256: String,
        token_index: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Review {
    pub status: String,
    pub required_roles: Vec<String>,
    #[serde(default)]
    pub decision_refs: Vec<DecisionRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub analysis_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub source_contract_sha256: String,
    pub analyzers: usize,
    pub stems: usize,
    pub observations: usize,
    pub minimum_confidence_millionths: u32,
    pub reviewed: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<AnalysisManifest> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes)
        .with_context(|| format!("music analysis is not valid YAML: {}", path.display()))
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

fn validate_loaded(path: &Path, manifest: &AnalysisManifest) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music analysis schema must be {SCHEMA}");
    }
    nonempty("analysis_id", &manifest.analysis_id)?;
    validate_sha256("source.manifest_sha256", &manifest.source.manifest_sha256)?;
    validate_sha256("source.contract_sha256", &manifest.source.contract_sha256)?;
    validate_sha256(
        "source.decoded_pcm_sha256",
        &manifest.source.decoded_pcm_sha256,
    )?;
    let source_path = source::resolve(path, &manifest.source.manifest);
    if sha256_path(&source_path)? != manifest.source.manifest_sha256.to_lowercase() {
        bail!("source manifest sha256 does not match analysis binding");
    }
    let source_report = source::validate(&source_path)?;
    let source_manifest = source::load(&source_path)?;
    if source_report.contract_sha256 != manifest.source.contract_sha256
        || source_report.decoded_pcm_sha256 != manifest.source.decoded_pcm_sha256
    {
        bail!("analysis source contract or decoded PCM identity is stale");
    }

    if manifest.analyzers.is_empty() {
        bail!("analyzers must not be empty");
    }
    let mut analyzer_ids = BTreeSet::new();
    for analyzer in &manifest.analyzers {
        nonempty("analyzers[].id", &analyzer.id)?;
        if !analyzer_ids.insert(analyzer.id.as_str()) {
            bail!("analyzers[].id must be unique");
        }
        nonempty("analyzers[].adapter", &analyzer.adapter)?;
        nonempty("analyzers[].version", &analyzer.version)?;
        nonempty("analyzers[].model_revision", &analyzer.model_revision)?;
        validate_sha256("analyzers[].parameters_sha256", &analyzer.parameters_sha256)?;
        nonempty("analyzers[].license", &analyzer.license)?;
        if analyzer.network_policy != NetworkPolicy::Denied {
            bail!("analysis v0.1 requires network_policy denied");
        }
    }

    let mut stem_ids = BTreeSet::new();
    for stem in &manifest.stems {
        nonempty("stems[].id", &stem.id)?;
        if !stem_ids.insert(stem.id.as_str()) {
            bail!("stems[].id must be unique");
        }
        validate_sha256("stems[].sha256", &stem.sha256)?;
        validate_sha256("stems[].decoded_pcm_sha256", &stem.decoded_pcm_sha256)?;
        stem.timebase.validate()?;
        if stem.timebase != source_manifest.media.timebase {
            bail!("stems[].timebase must match source timebase");
        }
        if stem.mixture_consistency_millionths > 1_000_000 || stem.bleed_millionths > 1_000_000 {
            bail!("stem mixture consistency and bleed must be within 0..=1000000");
        }
        nonempty("stems[].uncertainty", &stem.uncertainty)?;
        let stem_path = source::resolve(path, &stem.path);
        if sha256_path(&stem_path)? != stem.sha256.to_lowercase() {
            bail!("stem {} sha256 does not match", stem.id);
        }
        let bytes = fs::metadata(&stem_path)?.len();
        let expected = stem
            .timebase
            .samples_per_channel
            .checked_mul(u64::from(stem.timebase.channels))
            .and_then(|value| value.checked_mul(stem.format.bytes_per_sample()))
            .ok_or_else(|| anyhow::anyhow!("stem byte count overflow"))?;
        if bytes != expected || stem.sha256 != stem.decoded_pcm_sha256 {
            bail!(
                "raw PCM stem {} byte count or decoded identity does not match",
                stem.id
            );
        }
    }

    if manifest.observations.is_empty() {
        bail!("observations must not be empty");
    }
    let mut observation_ids = BTreeSet::new();
    let mut minimum_confidence = 1_000_000;
    for observation in &manifest.observations {
        nonempty("observations[].id", &observation.id)?;
        if !observation_ids.insert(observation.id.as_str()) {
            bail!("observations[].id must be unique");
        }
        if !analyzer_ids.contains(observation.analyzer_id.as_str()) {
            bail!(
                "observation {} references an unknown analyzer",
                observation.id
            );
        }
        observation.source.validate(
            source_manifest.media.timebase.samples_per_channel,
            "observations[].source",
        )?;
        if observation.confidence_millionths > 1_000_000 {
            bail!("observations[].confidence_millionths must be within 0..=1000000");
        }
        minimum_confidence = minimum_confidence.min(observation.confidence_millionths);
        nonempty("observations[].uncertainty", &observation.uncertainty)?;
        validate_observation_value(&observation.value)?;
    }
    unique_nonempty("limitations", &manifest.limitations)?;
    if manifest.limitations.is_empty() {
        bail!("limitations must explicitly disclose at least one analysis limit");
    }
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        analysis_id: manifest.analysis_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        source_contract_sha256: source_report.contract_sha256,
        analyzers: manifest.analyzers.len(),
        stems: manifest.stems.len(),
        observations: manifest.observations.len(),
        minimum_confidence_millionths: minimum_confidence,
        reviewed: status_requires_decision(&manifest.review.status),
        shareable: false,
        verified: true,
    })
}

fn validate_observation_value(value: &ObservationValue) -> Result<()> {
    match value {
        ObservationValue::Tempo { milli_bpm } => {
            if !(10_000..=600_000).contains(milli_bpm) {
                bail!("tempo milli_bpm must be between 10000 and 600000");
            }
        }
        ObservationValue::Meter {
            numerator,
            denominator,
        } => {
            if *numerator == 0
                || *numerator > 32
                || !denominator.is_power_of_two()
                || *denominator > 64
            {
                bail!("meter must have numerator 1..=32 and power-of-two denominator <= 64");
            }
        }
        ObservationValue::Pitch { midi_note, cents }
        | ObservationValue::Bass { midi_note, cents } => {
            if *midi_note > 127 || !(-100..=100).contains(cents) {
                bail!("pitch must use MIDI 0..=127 and cents -100..=100");
            }
        }
        ObservationValue::Form { label }
        | ObservationValue::Harmony { symbol: label }
        | ObservationValue::Rhythm { label }
        | ObservationValue::Hook { label }
        | ObservationValue::Instrumentation { label } => nonempty("observation label", label)?,
        ObservationValue::VocalAlignment {
            text_layer_sha256, ..
        } => validate_sha256("text_layer_sha256", text_layer_sha256)?,
        ObservationValue::Beat { .. } | ObservationValue::Bar { .. } => {}
    }
    Ok(())
}

fn validate_review(review: &Review) -> Result<()> {
    nonempty("review.status", &review.status)?;
    unique_nonempty("review.required_roles", &review.required_roles)?;
    for role in REQUIRED_ROLES {
        if !review.required_roles.iter().any(|value| value == role) {
            bail!("review.required_roles must include {role}");
        }
    }
    let mut ids = BTreeSet::new();
    for decision in &review.decision_refs {
        nonempty("review.decision_refs[].artifact_id", &decision.artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", &decision.sha256)?;
        if !ids.insert(decision.artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}
