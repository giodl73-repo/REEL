use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::{
    AuthorityRef, DecisionRef,
    analysis::{Review, SourceBinding},
    hash::{canonical_sha256, sha256_path},
    nonempty,
    source::{self, NetworkPolicy, RawPcmFormat},
    status_requires_decision,
    time::AudioTimebase,
    unique_nonempty, validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-interchange-intake.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InterchangeIntake {
    pub schema: String,
    pub intake_id: String,
    pub source: SourceBinding,
    pub authority: AuthorityRef,
    pub producers: Vec<Producer>,
    pub artifacts: Vec<InterchangeArtifact>,
    pub limitations: Vec<String>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub id: String,
    pub adapter: String,
    pub version: String,
    pub executable_sha256: String,
    pub model_revision: Option<String>,
    pub model_sha256: Option<String>,
    pub software_license: String,
    pub model_license: Option<String>,
    pub dataset_disclosure: String,
    pub parameters_sha256: String,
    pub network_policy: NetworkPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InterchangeArtifact {
    pub id: String,
    pub producer_id: String,
    pub purpose: ArtifactPurpose,
    pub format: InterchangeFormat,
    pub path: PathBuf,
    pub sha256: String,
    pub bytes: u64,
    pub semantic_roles: Vec<String>,
    pub uncertainty: String,
    pub normalized_pcm: Option<NormalizedPcm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ArtifactPurpose {
    Stem,
    NoteEvents,
    FeatureAnnotations,
    ScoreCandidate,
    ModelOutput,
    Sonification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum InterchangeFormat {
    Wav,
    Flac,
    Midi,
    MusicXml,
    Csv,
    Lab,
    Jams,
    Rdf,
    Npz,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedPcm {
    pub path: PathBuf,
    pub sha256: String,
    pub decoded_pcm_sha256: String,
    pub format: RawPcmFormat,
    pub timebase: AudioTimebase,
    pub decoder_id: String,
    pub decoder_version: String,
    pub parameters_sha256: String,
    pub network_policy: NetworkPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub intake_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub source_contract_sha256: String,
    pub producers: usize,
    pub artifacts: usize,
    pub formats: Vec<String>,
    pub normalized_stems: usize,
    pub reviewed: bool,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<InterchangeIntake> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music interchange intake is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

fn validate_loaded(path: &Path, manifest: &InterchangeIntake) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music interchange intake schema must be {SCHEMA}");
    }
    nonempty("intake_id", &manifest.intake_id)?;
    validate_authority(&manifest.authority)?;
    let source_report = validate_source(path, &manifest.source)?;
    let source_manifest = source::load(&source::resolve(path, &manifest.source.manifest))?;

    if manifest.producers.is_empty() {
        bail!("producers must not be empty");
    }
    let mut producer_ids = BTreeSet::new();
    for producer in &manifest.producers {
        nonempty("producers[].id", &producer.id)?;
        if !producer_ids.insert(producer.id.as_str()) {
            bail!("producers[].id must be unique");
        }
        nonempty("producers[].adapter", &producer.adapter)?;
        nonempty("producers[].version", &producer.version)?;
        validate_sha256("producers[].executable_sha256", &producer.executable_sha256)?;
        let model_fields = [
            producer.model_revision.is_some(),
            producer.model_sha256.is_some(),
            producer.model_license.is_some(),
        ];
        if model_fields.iter().any(|value| *value) && !model_fields.iter().all(|value| *value) {
            bail!(
                "producer {} model_revision, model_sha256, and model_license must be supplied together",
                producer.id
            );
        }
        if let Some(revision) = &producer.model_revision {
            nonempty("producers[].model_revision", revision)?;
        }
        if let Some(hash) = &producer.model_sha256 {
            validate_sha256("producers[].model_sha256", hash)?;
        }
        if let Some(license) = &producer.model_license {
            nonempty("producers[].model_license", license)?;
        }
        validate_sha256("producers[].parameters_sha256", &producer.parameters_sha256)?;
        nonempty("producers[].software_license", &producer.software_license)?;
        nonempty(
            "producers[].dataset_disclosure",
            &producer.dataset_disclosure,
        )?;
        if producer.network_policy != NetworkPolicy::Denied {
            bail!("interchange v0.1 requires producer network_policy denied");
        }
    }

    if manifest.artifacts.is_empty() {
        bail!("artifacts must not be empty");
    }
    let mut artifact_ids = BTreeSet::new();
    let mut formats = BTreeSet::new();
    let mut normalized_stems = 0;
    for artifact in &manifest.artifacts {
        nonempty("artifacts[].id", &artifact.id)?;
        if !artifact_ids.insert(artifact.id.as_str()) {
            bail!("artifacts[].id must be unique");
        }
        if !producer_ids.contains(artifact.producer_id.as_str()) {
            bail!("artifact {} references an unknown producer", artifact.id);
        }
        validate_sha256("artifacts[].sha256", &artifact.sha256)?;
        unique_nonempty("artifacts[].semantic_roles", &artifact.semantic_roles)?;
        if artifact.semantic_roles.is_empty() {
            bail!(
                "artifact {} must declare at least one semantic role",
                artifact.id
            );
        }
        nonempty("artifacts[].uncertainty", &artifact.uncertainty)?;
        validate_purpose_format(artifact.purpose, artifact.format)?;
        let artifact_path = source::resolve(path, &artifact.path);
        let bytes = fs::read(&artifact_path)
            .with_context(|| format!("failed to read artifact {}", artifact_path.display()))?;
        if artifact.bytes == 0 || artifact.bytes != bytes.len() as u64 {
            bail!("artifact {} byte count is stale or zero", artifact.id);
        }
        if sha256_path(&artifact_path)? != artifact.sha256.to_lowercase() {
            bail!("artifact {} sha256 does not match", artifact.id);
        }
        validate_signature(artifact.format, &bytes, &artifact.id)?;
        formats.insert(format_name(artifact.format).to_string());

        match (&artifact.purpose, &artifact.normalized_pcm) {
            (ArtifactPurpose::Stem, Some(normalized)) => {
                validate_normalized_pcm(path, normalized, source_manifest.media.timebase)?;
                normalized_stems += 1;
            }
            (ArtifactPurpose::Stem, None) => {
                bail!("stem artifact {} requires normalized_pcm", artifact.id)
            }
            (_, Some(_)) => bail!(
                "only stem artifacts may declare normalized_pcm; artifact {} is not a stem",
                artifact.id
            ),
            (_, None) => {}
        }
    }

    unique_nonempty("limitations", &manifest.limitations)?;
    if manifest.limitations.is_empty() {
        bail!("limitations must disclose at least one interchange limit");
    }
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        intake_id: manifest.intake_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        source_contract_sha256: source_report.contract_sha256,
        producers: manifest.producers.len(),
        artifacts: manifest.artifacts.len(),
        formats: formats.into_iter().collect(),
        normalized_stems,
        reviewed: status_requires_decision(&manifest.review.status),
        shareable: false,
        verified: true,
    })
}

fn validate_source(path: &Path, binding: &SourceBinding) -> Result<source::ValidationReport> {
    validate_sha256("source.manifest_sha256", &binding.manifest_sha256)?;
    validate_sha256("source.contract_sha256", &binding.contract_sha256)?;
    validate_sha256("source.decoded_pcm_sha256", &binding.decoded_pcm_sha256)?;
    let source_path = source::resolve(path, &binding.manifest);
    if sha256_path(&source_path)? != binding.manifest_sha256.to_lowercase() {
        bail!("source manifest sha256 does not match interchange binding");
    }
    let report = source::validate(&source_path)?;
    if report.contract_sha256 != binding.contract_sha256
        || report.decoded_pcm_sha256 != binding.decoded_pcm_sha256
    {
        bail!("interchange source contract or decoded PCM identity is stale");
    }
    Ok(report)
}

fn validate_purpose_format(purpose: ArtifactPurpose, format: InterchangeFormat) -> Result<()> {
    let valid = match purpose {
        ArtifactPurpose::Stem | ArtifactPurpose::Sonification => {
            matches!(format, InterchangeFormat::Wav | InterchangeFormat::Flac)
        }
        ArtifactPurpose::NoteEvents => {
            matches!(format, InterchangeFormat::Midi | InterchangeFormat::Csv)
        }
        ArtifactPurpose::FeatureAnnotations => matches!(
            format,
            InterchangeFormat::Csv
                | InterchangeFormat::Lab
                | InterchangeFormat::Jams
                | InterchangeFormat::Rdf
                | InterchangeFormat::Midi
        ),
        ArtifactPurpose::ScoreCandidate => {
            matches!(
                format,
                InterchangeFormat::Midi | InterchangeFormat::MusicXml
            )
        }
        ArtifactPurpose::ModelOutput => matches!(format, InterchangeFormat::Npz),
    };
    if !valid {
        bail!(
            "artifact purpose {} does not accept format {}",
            purpose_name(purpose),
            format_name(format)
        );
    }
    Ok(())
}

fn validate_signature(format: InterchangeFormat, bytes: &[u8], id: &str) -> Result<()> {
    let text = || {
        std::str::from_utf8(bytes)
            .with_context(|| format!("artifact {id} must contain valid UTF-8 text"))
    };
    let valid = match format {
        InterchangeFormat::Wav => {
            bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WAVE"
        }
        InterchangeFormat::Flac => bytes.starts_with(b"fLaC"),
        InterchangeFormat::Midi => bytes.len() >= 14 && bytes.starts_with(b"MThd"),
        InterchangeFormat::MusicXml => {
            let value = text()?;
            value.contains("<score-partwise") || value.contains("<score-timewise")
        }
        InterchangeFormat::Csv => {
            let value = text()?;
            !value.trim().is_empty() && value.lines().next().is_some_and(|line| line.contains(','))
        }
        InterchangeFormat::Lab => {
            let value = text()?;
            !value.trim().is_empty() && value.lines().next().is_some_and(|line| line.contains('\t'))
        }
        InterchangeFormat::Jams => {
            let value: serde_json::Value = serde_json::from_slice(bytes)
                .with_context(|| format!("artifact {id} is not valid JAMS JSON"))?;
            value.get("annotations").is_some_and(|item| item.is_array())
                && value
                    .get("file_metadata")
                    .is_some_and(|item| item.is_object())
        }
        InterchangeFormat::Rdf => {
            let value = text()?;
            !value.trim().is_empty()
                && (value.contains("@prefix")
                    || value.contains("http://")
                    || value.contains("https://"))
        }
        InterchangeFormat::Npz => bytes.starts_with(b"PK\x03\x04"),
    };
    if !valid {
        bail!(
            "artifact {id} does not match declared format {}",
            format_name(format)
        );
    }
    Ok(())
}

fn validate_normalized_pcm(
    manifest_path: &Path,
    normalized: &NormalizedPcm,
    source_timebase: AudioTimebase,
) -> Result<()> {
    validate_sha256("normalized_pcm.sha256", &normalized.sha256)?;
    validate_sha256(
        "normalized_pcm.decoded_pcm_sha256",
        &normalized.decoded_pcm_sha256,
    )?;
    validate_sha256(
        "normalized_pcm.parameters_sha256",
        &normalized.parameters_sha256,
    )?;
    normalized.timebase.validate()?;
    if normalized.timebase != source_timebase {
        bail!("normalized stem PCM timebase must match the immutable source timebase");
    }
    nonempty("normalized_pcm.decoder_id", &normalized.decoder_id)?;
    nonempty(
        "normalized_pcm.decoder_version",
        &normalized.decoder_version,
    )?;
    if normalized.network_policy != NetworkPolicy::Denied {
        bail!("normalized stem decoding requires network_policy denied");
    }
    let pcm_path = source::resolve(manifest_path, &normalized.path);
    let hash = sha256_path(&pcm_path)?;
    if hash != normalized.sha256.to_lowercase()
        || hash != normalized.decoded_pcm_sha256.to_lowercase()
    {
        bail!("normalized stem PCM hashes do not match exact raw bytes");
    }
    let expected = normalized
        .timebase
        .samples_per_channel
        .checked_mul(u64::from(normalized.timebase.channels))
        .and_then(|value| value.checked_mul(normalized.format.bytes_per_sample()))
        .ok_or_else(|| anyhow::anyhow!("normalized stem PCM byte count overflow"))?;
    if fs::metadata(&pcm_path)?.len() != expected {
        bail!("normalized stem PCM byte count does not match its timebase");
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
    for DecisionRef {
        artifact_id,
        sha256,
    } in &review.decision_refs
    {
        nonempty("review.decision_refs[].artifact_id", artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", sha256)?;
        if !ids.insert(artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}

fn purpose_name(value: ArtifactPurpose) -> &'static str {
    match value {
        ArtifactPurpose::Stem => "stem",
        ArtifactPurpose::NoteEvents => "note-events",
        ArtifactPurpose::FeatureAnnotations => "feature-annotations",
        ArtifactPurpose::ScoreCandidate => "score-candidate",
        ArtifactPurpose::ModelOutput => "model-output",
        ArtifactPurpose::Sonification => "sonification",
    }
}

fn format_name(value: InterchangeFormat) -> &'static str {
    match value {
        InterchangeFormat::Wav => "wav",
        InterchangeFormat::Flac => "flac",
        InterchangeFormat::Midi => "midi",
        InterchangeFormat::MusicXml => "musicxml",
        InterchangeFormat::Csv => "csv",
        InterchangeFormat::Lab => "lab",
        InterchangeFormat::Jams => "jams",
        InterchangeFormat::Rdf => "rdf",
        InterchangeFormat::Npz => "npz",
    }
}
