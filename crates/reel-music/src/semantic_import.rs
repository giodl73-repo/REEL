use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;

use crate::{
    AuthorityRef, DecisionRef,
    analysis::{
        self, AnalysisManifest, Analyzer, ImportBinding, Observation, ObservationValue, Review,
        SourceBinding,
    },
    comparison,
    hash::{canonical_sha256, sha256_path},
    interchange::{self, ArtifactPurpose},
    nonempty,
    source::{self, NetworkPolicy},
    status_requires_decision,
    time::{RoundingMode, SampleRange},
    unique_nonempty, validate_authority, validate_sha256,
};

pub const SCHEMA: &str = "reel.music-semantic-import.v0.1";
const REQUIRED_ROLES: &[&str] = &[
    "music-reconstruction-engineer",
    "sound-designer",
    "editor",
    "rights-provenance-steward",
];

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticImport {
    pub schema: String,
    pub import_id: String,
    pub intake: ContractBinding,
    pub comparison: ContractBinding,
    pub comparison_set_id: String,
    pub selected_artifact_id: String,
    pub authority: AuthorityRef,
    pub adapter: SemanticAdapter,
    pub events: Vec<SemanticEvent>,
    pub limitations: Vec<String>,
    pub review: Review,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContractBinding {
    pub manifest: PathBuf,
    pub manifest_sha256: String,
    pub contract_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticAdapter {
    pub id: String,
    pub name: String,
    pub version: String,
    pub executable_sha256: String,
    pub parameters_sha256: String,
    pub model_revision: String,
    pub license: String,
    pub network_policy: NetworkPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SemanticEvent {
    pub id: String,
    pub source_locator: String,
    pub original_time: OriginalTime,
    pub mapped_source: SampleRange,
    pub confidence_millionths: u32,
    pub uncertainty: String,
    pub value: ObservationValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "unit", rename_all = "kebab-case", deny_unknown_fields)]
pub enum OriginalTime {
    Samples {
        start: u64,
        end: u64,
        sample_rate_hz: u32,
    },
    Microseconds {
        start: u64,
        end: u64,
    },
    MusicalTicks {
        start: u64,
        end: u64,
        pulses_per_quarter: u32,
        microseconds_per_quarter: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ValidationReport {
    pub schema: String,
    pub import_id: String,
    pub manifest_sha256: String,
    pub contract_sha256: String,
    pub source_contract_sha256: String,
    pub comparison_contract_sha256: String,
    pub selected_artifact_id: String,
    pub events: usize,
    pub reviewed: bool,
    pub shareable: bool,
    pub verified: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteReport {
    pub schema: String,
    pub import_id: String,
    pub import_contract_sha256: String,
    pub analysis_id: String,
    pub analysis_manifest_sha256: String,
    pub analysis_contract_sha256: String,
    pub observations: usize,
    pub shareable: bool,
    pub verified: bool,
}

pub fn load(path: &Path) -> Result<SemanticImport> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_yaml::from_slice(&bytes).with_context(|| {
        format!(
            "music semantic import is not valid YAML: {}",
            path.display()
        )
    })
}

pub fn validate(path: &Path) -> Result<ValidationReport> {
    let manifest = load(path)?;
    validate_loaded(path, &manifest)
}

pub fn write_analysis(import_path: &Path, output: &Path) -> Result<WriteReport> {
    if output.exists() {
        bail!("refusing to overwrite existing analysis output");
    }
    let import_report = validate(import_path)?;
    let import = load(import_path)?;
    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;

    let intake_path = resolve(import_path, &import.intake.manifest);
    let intake = interchange::load(&intake_path)?;
    let source_path = source::resolve(&intake_path, &intake.source.manifest);
    let analysis = AnalysisManifest {
        schema: analysis::SCHEMA.into(),
        analysis_id: format!("{}-analysis", import.import_id),
        source: SourceBinding {
            manifest: relative_path(output_parent, &source_path)?,
            manifest_sha256: intake.source.manifest_sha256,
            contract_sha256: intake.source.contract_sha256,
            decoded_pcm_sha256: intake.source.decoded_pcm_sha256,
        },
        imports: vec![ImportBinding {
            manifest: relative_path(output_parent, import_path)?,
            manifest_sha256: import_report.manifest_sha256.clone(),
            contract_sha256: import_report.contract_sha256.clone(),
            import_id: import.import_id.clone(),
        }],
        analyzers: vec![Analyzer {
            id: import.adapter.id.clone(),
            adapter: import.adapter.name.clone(),
            version: import.adapter.version.clone(),
            model_revision: import.adapter.model_revision.clone(),
            parameters_sha256: import.adapter.parameters_sha256.clone(),
            license: import.adapter.license.clone(),
            network_policy: NetworkPolicy::Denied,
            import_id: Some(import.import_id.clone()),
        }],
        stems: vec![],
        observations: import
            .events
            .iter()
            .map(|event| Observation {
                id: format!("imported-{}", event.id),
                analyzer_id: import.adapter.id.clone(),
                source: event.mapped_source,
                confidence_millionths: event.confidence_millionths,
                uncertainty: event.uncertainty.clone(),
                value: event.value.clone(),
                import_event_id: Some(event.id.clone()),
            })
            .collect(),
        limitations: import.limitations,
        review: import.review,
    };

    let mut temporary = NamedTempFile::new_in(output_parent)?;
    temporary.write_all(serde_yaml::to_string(&analysis)?.as_bytes())?;
    temporary.flush()?;
    analysis::validate(temporary.path())?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to publish {}", output.display()))?;
    let report = analysis::validate(output)?;

    Ok(WriteReport {
        schema: SCHEMA.into(),
        import_id: import_report.import_id,
        import_contract_sha256: import_report.contract_sha256,
        analysis_id: report.analysis_id,
        analysis_manifest_sha256: report.manifest_sha256,
        analysis_contract_sha256: report.contract_sha256,
        observations: report.observations,
        shareable: false,
        verified: true,
    })
}

fn validate_loaded(path: &Path, manifest: &SemanticImport) -> Result<ValidationReport> {
    if manifest.schema != SCHEMA {
        bail!("music semantic import schema must be {SCHEMA}");
    }
    nonempty("import_id", &manifest.import_id)?;
    validate_authority(&manifest.authority)?;
    validate_binding("intake", &manifest.intake)?;
    validate_binding("comparison", &manifest.comparison)?;

    let intake_path = resolve(path, &manifest.intake.manifest);
    if sha256_path(&intake_path)? != manifest.intake.manifest_sha256.to_lowercase() {
        bail!("intake manifest sha256 does not match semantic import binding");
    }
    let intake_report = interchange::validate(&intake_path)?;
    if intake_report.contract_sha256 != manifest.intake.contract_sha256.to_lowercase() {
        bail!("intake contract sha256 does not match semantic import binding");
    }
    let intake = interchange::load(&intake_path)?;
    let source_path = source::resolve(&intake_path, &intake.source.manifest);
    let source_manifest = source::load(&source_path)?;

    let comparison_path = resolve(path, &manifest.comparison.manifest);
    if sha256_path(&comparison_path)? != manifest.comparison.manifest_sha256.to_lowercase() {
        bail!("comparison manifest sha256 does not match semantic import binding");
    }
    let comparison_report = comparison::validate(&comparison_path)?;
    if comparison_report.contract_sha256 != manifest.comparison.contract_sha256.to_lowercase()
        || comparison_report.intake_contract_sha256 != intake_report.contract_sha256
    {
        bail!("comparison contract or intake lineage is stale");
    }
    let comparison = comparison::load(&comparison_path)?;
    let set = comparison
        .sets
        .iter()
        .find(|set| set.id == manifest.comparison_set_id)
        .ok_or_else(|| anyhow::anyhow!("semantic import references an unknown comparison set"))?;
    let selection = set.selection.as_ref().ok_or_else(|| {
        anyhow::anyhow!("semantic import requires an explicit candidate selection")
    })?;
    if selection.artifact_id != manifest.selected_artifact_id {
        bail!("semantic import selected_artifact_id does not match the comparison decision");
    }
    if !matches!(
        set.purpose,
        ArtifactPurpose::NoteEvents
            | ArtifactPurpose::FeatureAnnotations
            | ArtifactPurpose::ScoreCandidate
    ) {
        bail!("semantic import v0.1 supports event, annotation, and score candidates only");
    }

    validate_adapter(&manifest.adapter)?;
    if manifest.events.is_empty() {
        bail!("events must not be empty");
    }
    let mut event_ids = BTreeSet::new();
    for event in &manifest.events {
        nonempty("events[].id", &event.id)?;
        if !event_ids.insert(event.id.as_str()) {
            bail!("events[].id must be unique");
        }
        nonempty("events[].source_locator", &event.source_locator)?;
        let mapped = map_original_time(
            &event.original_time,
            source_manifest.media.timebase.sample_rate_hz,
            source_manifest.musical_timebase.rounding,
        )?;
        if mapped != event.mapped_source {
            bail!(
                "event {} mapped_source does not match exact time conversion",
                event.id
            );
        }
        event.mapped_source.validate(
            source_manifest.media.timebase.samples_per_channel,
            "events[].mapped_source",
        )?;
        if event.confidence_millionths > 1_000_000 {
            bail!("events[].confidence_millionths must be within 0..=1000000");
        }
        nonempty("events[].uncertainty", &event.uncertainty)?;
        analysis::validate_observation_value(&event.value)?;
    }
    unique_nonempty("limitations", &manifest.limitations)?;
    if manifest.limitations.is_empty() {
        bail!("limitations must disclose at least one semantic import limit");
    }
    validate_review(&manifest.review)?;

    Ok(ValidationReport {
        schema: SCHEMA.into(),
        import_id: manifest.import_id.clone(),
        manifest_sha256: sha256_path(path)?,
        contract_sha256: canonical_sha256(manifest)?,
        source_contract_sha256: intake_report.source_contract_sha256,
        comparison_contract_sha256: comparison_report.contract_sha256,
        selected_artifact_id: manifest.selected_artifact_id.clone(),
        events: manifest.events.len(),
        reviewed: status_requires_decision(&manifest.review.status),
        shareable: false,
        verified: true,
    })
}

fn map_original_time(
    time: &OriginalTime,
    output_rate: u32,
    rounding: RoundingMode,
) -> Result<SampleRange> {
    let (start, end, numerator, denominator) = match *time {
        OriginalTime::Samples {
            start,
            end,
            sample_rate_hz,
        } => {
            if sample_rate_hz == 0 {
                bail!("original sample rate must be positive");
            }
            (
                start,
                end,
                u128::from(output_rate),
                u128::from(sample_rate_hz),
            )
        }
        OriginalTime::Microseconds { start, end } => {
            (start, end, u128::from(output_rate), 1_000_000)
        }
        OriginalTime::MusicalTicks {
            start,
            end,
            pulses_per_quarter,
            microseconds_per_quarter,
        } => {
            if pulses_per_quarter == 0 || microseconds_per_quarter == 0 {
                bail!("musical tick mapping requires positive PPQ and tempo");
            }
            (
                start,
                end,
                u128::from(output_rate) * u128::from(microseconds_per_quarter),
                u128::from(pulses_per_quarter) * 1_000_000,
            )
        }
    };
    if start >= end {
        bail!("original time must be a non-empty half-open range");
    }
    Ok(SampleRange {
        start: rounded_ratio(start, numerator, denominator, rounding)?,
        end: rounded_ratio(end, numerator, denominator, rounding)?,
    })
}

fn rounded_ratio(
    value: u64,
    numerator: u128,
    denominator: u128,
    rounding: RoundingMode,
) -> Result<u64> {
    let scaled = u128::from(value)
        .checked_mul(numerator)
        .ok_or_else(|| anyhow::anyhow!("time conversion overflow"))?;
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let rounded = match rounding {
        RoundingMode::Floor => quotient,
        RoundingMode::Ceiling => quotient + u128::from(remainder != 0),
        RoundingMode::HalfAwayFromZero => quotient + u128::from(remainder * 2 >= denominator),
    };
    u64::try_from(rounded).map_err(|_| anyhow::anyhow!("mapped sample position overflow"))
}

fn validate_binding(name: &str, binding: &ContractBinding) -> Result<()> {
    validate_sha256(&format!("{name}.manifest_sha256"), &binding.manifest_sha256)?;
    validate_sha256(&format!("{name}.contract_sha256"), &binding.contract_sha256)
}

fn validate_adapter(adapter: &SemanticAdapter) -> Result<()> {
    nonempty("adapter.id", &adapter.id)?;
    nonempty("adapter.name", &adapter.name)?;
    nonempty("adapter.version", &adapter.version)?;
    validate_sha256("adapter.executable_sha256", &adapter.executable_sha256)?;
    validate_sha256("adapter.parameters_sha256", &adapter.parameters_sha256)?;
    nonempty("adapter.model_revision", &adapter.model_revision)?;
    nonempty("adapter.license", &adapter.license)?;
    if adapter.network_policy != NetworkPolicy::Denied {
        bail!("semantic import v0.1 requires adapter network_policy denied");
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
    let mut decision_ids = BTreeSet::new();
    for DecisionRef {
        artifact_id,
        sha256,
    } in &review.decision_refs
    {
        nonempty("review.decision_refs[].artifact_id", artifact_id)?;
        validate_sha256("review.decision_refs[].sha256", sha256)?;
        if !decision_ids.insert(artifact_id.as_str()) {
            bail!("review.decision_refs artifact ids must be unique");
        }
    }
    if status_requires_decision(&review.status) && review.decision_refs.is_empty() {
        bail!("review.status {} requires decision_refs", review.status);
    }
    Ok(())
}

fn resolve(manifest: &Path, child: &Path) -> PathBuf {
    if child.is_absolute() {
        child.to_path_buf()
    } else {
        manifest
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(child)
    }
}

fn relative_path(from: &Path, target: &Path) -> Result<PathBuf> {
    let from = fs::canonicalize(from)?;
    let target = fs::canonicalize(target)?;
    let from_parts = from.components().collect::<Vec<_>>();
    let target_parts = target.components().collect::<Vec<_>>();
    let common = from_parts
        .iter()
        .zip(&target_parts)
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0
        || matches!(from_parts.first(), Some(Component::Prefix(_)))
            != matches!(target_parts.first(), Some(Component::Prefix(_)))
    {
        bail!("analysis output and bound artifacts must share a filesystem root");
    }
    let mut path = PathBuf::new();
    for _ in common..from_parts.len() {
        path.push("..");
    }
    for component in &target_parts[common..] {
        path.push(component.as_os_str());
    }
    Ok(path)
}
