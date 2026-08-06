use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use tempfile::Builder;

use crate::production;

pub const FINDING_SCHEMA: &str = "reel.review-finding.v0.1";
pub const DECISION_SCHEMA: &str = "reel.review-decision.v0.1";
pub const INDEX_SCHEMA: &str = "reel.review-index.v0.1";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewFindingInput {
    pub schema: String,
    pub record_id: String,
    pub reviewer_key: String,
    pub target_kind: String,
    pub kind: String,
    pub selected_option: Option<String>,
    pub reason: String,
    pub timestamp: String,
    pub scope: String,
    pub authority: String,
    #[serde(default)]
    pub cites: Vec<PathBuf>,
    pub claims: ReviewClaims,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewClaims {
    pub authenticated: bool,
    pub signed: bool,
    pub consent: bool,
    pub approval: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecisionRecord {
    pub schema: String,
    pub record_id: String,
    pub source_finding_sha256: String,
    pub target_kind: String,
    pub target_sha256: String,
    pub reviewer_key: String,
    pub kind: String,
    pub selected_option: Option<String>,
    pub reason: String,
    pub timestamp: String,
    pub scope: String,
    pub authority: String,
    pub cited_record_sha256: Vec<String>,
    pub claims: ReviewClaims,
    pub tool_version: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewRecordReport {
    pub schema: String,
    pub output: String,
    pub record_sha256: String,
    pub target_sha256: String,
    pub reviewer_key: String,
    pub kind: String,
    pub private_reason_retained: bool,
    pub approval_inferred: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewIndex {
    schema: String,
    series_sha256: String,
    episodes: Vec<ReviewIndexEpisode>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewIndexEpisode {
    episode_id: String,
    target_sha256: String,
    required_reviewers: Vec<String>,
    records: Vec<ReviewRecordRef>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRecordRef {
    path: PathBuf,
    sha256: String,
}

#[derive(Clone, Debug, Default)]
pub struct DecisionQueueSummary {
    pub status_by_episode: BTreeMap<String, String>,
    pub missing_reviewers_by_episode: BTreeMap<String, Vec<String>>,
    pub record_counts: BTreeMap<String, usize>,
    pub explicit_resolutions: Vec<String>,
    pub release_gates: Vec<String>,
}

pub fn write_record(
    target: impl AsRef<Path>,
    finding: impl AsRef<Path>,
    output: impl AsRef<Path>,
) -> Result<ReviewRecordReport> {
    let target = target.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve review target {}",
            target.as_ref().display()
        )
    })?;
    let finding = finding.as_ref().canonicalize().with_context(|| {
        format!(
            "failed to resolve review finding {}",
            finding.as_ref().display()
        )
    })?;
    let finding_bytes = fs::read(&finding)?;
    let input: ReviewFindingInput = serde_yaml::from_slice(&finding_bytes)
        .context("review finding is not valid strict YAML")?;
    validate_input(&input)?;
    validate_target_kind(&target, &input.target_kind)?;
    let base = finding.parent().unwrap_or_else(|| Path::new("."));
    let mut cited_record_sha256 = Vec::new();
    let mut cited_records = Vec::new();
    for citation in &input.cites {
        let path = resolve(base, citation)?;
        let record: ReviewDecisionRecord = serde_json::from_slice(&fs::read(&path)?)
            .context("cited review record is not valid strict JSON")?;
        validate_record(&record)?;
        let hash = production::sha256_path(&path)?;
        if cited_record_sha256.contains(&hash) {
            bail!("review finding contains a duplicate citation");
        }
        cited_record_sha256.push(hash);
        cited_records.push(record);
    }
    if input.kind == "resolution" && cited_record_sha256.len() < 2 {
        bail!("resolution requires at least two independent cited records");
    }
    if input.kind == "resolution"
        && (cited_records
            .iter()
            .map(|record| &record.reviewer_key)
            .collect::<BTreeSet<_>>()
            .len()
            < 2
            || cited_records
                .iter()
                .any(|record| record.kind == "resolution"))
    {
        bail!("resolution must cite independent non-resolution findings");
    }
    if input.kind == "resolution" && !records_disagree(cited_records.iter()) {
        bail!("resolution citations do not record a disagreement");
    }
    let target_sha256 = production::sha256_path(&target)?;
    for citation in &input.cites {
        let path = resolve(base, citation)?;
        let record: ReviewDecisionRecord = serde_json::from_slice(&fs::read(path)?)?;
        if record.target_sha256 != target_sha256 || record.scope != input.scope {
            bail!("resolution citation targets a different artifact or scope");
        }
    }
    let record = ReviewDecisionRecord {
        schema: DECISION_SCHEMA.to_string(),
        record_id: input.record_id,
        source_finding_sha256: production::sha256_path(&finding)?,
        target_kind: input.target_kind,
        target_sha256: target_sha256.clone(),
        reviewer_key: input.reviewer_key.clone(),
        kind: input.kind.clone(),
        selected_option: input.selected_option,
        reason: input.reason,
        timestamp: input.timestamp,
        scope: input.scope,
        authority: input.authority,
        cited_record_sha256,
        claims: input.claims,
        tool_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    validate_record(&record)?;
    let output = output.as_ref();
    if output.exists() {
        bail!(
            "refusing to overwrite append-only review record {}",
            output.display()
        );
    }
    let parent = output
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temporary = Builder::new().prefix(".reel-review-").tempfile_in(parent)?;
    temporary.write_all(&serde_json::to_vec_pretty(&record)?)?;
    temporary.write_all(b"\n")?;
    temporary.flush()?;
    temporary.persist_noclobber(output).with_context(|| {
        format!(
            "failed to publish review record atomically {}",
            output.display()
        )
    })?;
    Ok(ReviewRecordReport {
        schema: "reel.review-record.v0.1".to_string(),
        output: output.display().to_string(),
        record_sha256: production::sha256_path(output)?,
        target_sha256,
        reviewer_key: input.reviewer_key,
        kind: input.kind,
        private_reason_retained: true,
        approval_inferred: false,
    })
}

pub fn summarize_index(
    series_path: &Path,
    index_path: &Path,
    episode_ids: &[String],
) -> Result<DecisionQueueSummary> {
    let index_path = index_path.canonicalize()?;
    let index: ReviewIndex = serde_yaml::from_slice(&fs::read(&index_path)?)
        .context("review index is not valid strict YAML")?;
    if index.schema != INDEX_SCHEMA || index.series_sha256 != production::sha256_path(series_path)?
    {
        bail!("review index does not match the series manifest");
    }
    let base = index_path.parent().unwrap_or_else(|| Path::new("."));
    let mut entries = BTreeMap::new();
    for episode in index.episodes {
        require_token("review-index episode", &episode.episode_id)?;
        if !is_sha256(&episode.target_sha256)
            || entries
                .insert(episode.episode_id.clone(), episode)
                .is_some()
        {
            bail!("review index has an invalid or duplicate episode entry");
        }
    }
    let known = episode_ids.iter().collect::<BTreeSet<_>>();
    if entries.keys().any(|id| !known.contains(id)) {
        bail!("review index references an unknown series episode");
    }
    let mut summary = DecisionQueueSummary::default();
    for episode_id in episode_ids {
        let Some(entry) = entries.get(episode_id) else {
            summary
                .status_by_episode
                .insert(episode_id.clone(), "missing".to_string());
            summary.record_counts.insert(episode_id.clone(), 0);
            summary.release_gates.push(episode_id.clone());
            continue;
        };
        let required = entry.required_reviewers.iter().collect::<BTreeSet<_>>();
        if required.is_empty() || required.len() != entry.required_reviewers.len() {
            bail!("review index required reviewers must be nonempty and unique");
        }
        let mut records = Vec::new();
        let mut hashes = BTreeSet::new();
        let mut record_ids = BTreeSet::new();
        for reference in &entry.records {
            let path = resolve(base, &reference.path)?;
            let hash = production::sha256_path(&path)?;
            if hash != reference.sha256 || !hashes.insert(hash.clone()) {
                bail!("review index record hash is stale or duplicated");
            }
            let record: ReviewDecisionRecord = serde_json::from_slice(&fs::read(&path)?)
                .context("indexed review record is not valid strict JSON")?;
            validate_record(&record)?;
            if !record_ids.insert(record.record_id.clone()) {
                bail!("review index contains a duplicate record id");
            }
            if record.target_sha256 != entry.target_sha256 || record.scope != *episode_id {
                bail!("indexed review record targets a different artifact or episode");
            }
            records.push((hash, record));
        }
        summary
            .record_counts
            .insert(episode_id.clone(), records.len());
        let observed = records
            .iter()
            .filter(|(_, record)| record.kind != "resolution")
            .map(|(_, record)| &record.reviewer_key)
            .collect::<BTreeSet<_>>();
        let missing = required
            .difference(&observed)
            .map(|value| (*value).clone())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            summary
                .missing_reviewers_by_episode
                .insert(episode_id.clone(), missing);
            summary
                .status_by_episode
                .insert(episode_id.clone(), "missing".to_string());
            summary.release_gates.push(episode_id.clone());
            continue;
        }
        let resolution = records.iter().find(|(_, record)| {
            if record.kind != "resolution"
                || record.authority != "final"
                || record.cited_record_sha256.len() < 2
                || !record
                    .cited_record_sha256
                    .iter()
                    .all(|hash| hashes.contains(hash))
            {
                return false;
            }
            let cited = records
                .iter()
                .filter(|(hash, _)| record.cited_record_sha256.contains(hash))
                .map(|(_, cited)| cited);
            records_disagree(cited)
        });
        if resolution.is_some() {
            summary
                .status_by_episode
                .insert(episode_id.clone(), "resolved".to_string());
            summary.explicit_resolutions.push(episode_id.clone());
            continue;
        }
        let selections = records
            .iter()
            .filter(|(_, record)| record.kind == "selection")
            .filter_map(|(_, record)| record.selected_option.as_ref())
            .collect::<BTreeSet<_>>();
        let has_objection = records.iter().any(|(_, record)| record.kind == "objection");
        let status = if has_objection || selections.len() > 1 {
            "disagreement"
        } else {
            "agreement"
        };
        summary
            .status_by_episode
            .insert(episode_id.clone(), status.to_string());
        summary.release_gates.push(episode_id.clone());
    }
    Ok(summary)
}

fn validate_input(input: &ReviewFindingInput) -> Result<()> {
    if input.schema != FINDING_SCHEMA {
        bail!("unsupported review finding schema {}", input.schema);
    }
    require_token("record id", &input.record_id)?;
    require_token("reviewer key", &input.reviewer_key)?;
    require_token("scope", &input.scope)?;
    require_text("private reason", &input.reason, 5_000)?;
    if !valid_timestamp(&input.timestamp) {
        bail!("review timestamp must be explicit RFC3339 text");
    }
    if !matches!(
        input.target_kind.as_str(),
        "video" | "artifact-report" | "animatic-receipt" | "comparison-receipt"
    ) {
        bail!("unsupported review target kind {}", input.target_kind);
    }
    if !matches!(input.authority.as_str(), "advisory" | "final") {
        bail!("review authority must be advisory or final");
    }
    match input.kind.as_str() {
        "selection"
            if input
                .selected_option
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                && input.cites.is_empty() => {}
        "objection" if input.selected_option.is_none() && input.cites.is_empty() => {}
        "resolution"
            if input.authority == "final"
                && input
                    .selected_option
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()) => {}
        _ => bail!("review kind, selection, authority, and citations are inconsistent"),
    }
    if input.claims.authenticated
        || input.claims.signed
        || input.claims.consent
        || input.claims.approval
    {
        bail!("REEL review records cannot claim authentication, signature, consent, or approval");
    }
    Ok(())
}

fn validate_record(record: &ReviewDecisionRecord) -> Result<()> {
    if record.schema != DECISION_SCHEMA
        || !is_sha256(&record.source_finding_sha256)
        || !is_sha256(&record.target_sha256)
        || record
            .cited_record_sha256
            .iter()
            .any(|hash| !is_sha256(hash))
        || record.claims.authenticated
        || record.claims.signed
        || record.claims.consent
        || record.claims.approval
        || !matches!(
            record.target_kind.as_str(),
            "video" | "artifact-report" | "animatic-receipt" | "comparison-receipt"
        )
        || !matches!(record.authority.as_str(), "advisory" | "final")
    {
        bail!("review decision record is inconsistent");
    }
    require_token("record id", &record.record_id)?;
    require_token("reviewer key", &record.reviewer_key)?;
    require_token("scope", &record.scope)?;
    require_text("private reason", &record.reason, 5_000)?;
    if !valid_timestamp(&record.timestamp) {
        bail!("review record timestamp is invalid");
    }
    match record.kind.as_str() {
        "selection"
            if record
                .selected_option
                .as_ref()
                .is_some_and(|value| !value.trim().is_empty())
                && record.cited_record_sha256.is_empty() => {}
        "objection"
            if record.selected_option.is_none() && record.cited_record_sha256.is_empty() => {}
        "resolution"
            if record.authority == "final"
                && record
                    .selected_option
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty())
                && record.cited_record_sha256.len() >= 2 => {}
        _ => bail!("review decision record kind is inconsistent"),
    }
    Ok(())
}

fn validate_target_kind(path: &Path, kind: &str) -> Result<()> {
    if kind == "video" {
        return Ok(());
    }
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path)?).context("review target is not JSON")?;
    let schema = value["schema"]
        .as_str()
        .ok_or_else(|| anyhow!("review target omits schema"))?;
    let valid = match kind {
        "artifact-report" => matches!(
            schema,
            "reel.animatic-artifacts.v0.1" | "reel.comparison-artifacts.v0.1"
        ),
        "animatic-receipt" => schema == "reel.animatic-receipt.v0.1",
        "comparison-receipt" => schema == "reel.comparison-receipt.v0.1",
        _ => false,
    };
    if !valid {
        bail!("review target kind does not match target schema {schema}");
    }
    Ok(())
}

fn records_disagree<'a>(records: impl Iterator<Item = &'a ReviewDecisionRecord>) -> bool {
    let records = records.collect::<Vec<_>>();
    let independent = records
        .iter()
        .map(|record| &record.reviewer_key)
        .collect::<BTreeSet<_>>()
        .len()
        >= 2;
    let selections = records
        .iter()
        .filter(|record| record.kind == "selection")
        .filter_map(|record| record.selected_option.as_ref())
        .collect::<BTreeSet<_>>();
    independent && (selections.len() > 1 || records.iter().any(|record| record.kind == "objection"))
}

fn resolve(base: &Path, path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        path.canonicalize()
    } else {
        base.join(path).canonicalize()
    }
    .with_context(|| format!("failed to resolve review input {}", path.display()))
}
fn require_token(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 200 || value.chars().any(char::is_whitespace) {
        bail!("{name} must be a nonempty token of at most 200 characters");
    }
    Ok(())
}
fn require_text(name: &str, value: &str, maximum: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > maximum {
        bail!("{name} must contain 1..={maximum} characters");
    }
    Ok(())
}
fn valid_timestamp(value: &str) -> bool {
    value.len() >= 20
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(10) == Some(&b'T')
        && (value.ends_with('Z') || value.rfind(['+', '-']).is_some_and(|index| index > 18))
}
fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
