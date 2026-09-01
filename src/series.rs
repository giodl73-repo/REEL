use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::production::{self, TimingStatus};

pub const SERIES_SCHEMA: &str = "reel.series.v0.1";
pub const EPISODE_PACKET_SCHEMA: &str = "reel.episode-packet.v0.1";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SeriesDefaults {
    #[serde(default)]
    pub platforms: Vec<String>,
    #[serde(default)]
    pub disclosure: String,
    #[serde(default)]
    pub captions: Value,
    #[serde(default)]
    pub privacy: Value,
    #[serde(default)]
    pub continuity_registry: Option<ContinuityRegistryRef>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct ContinuityRegistryRef {
    pub path: String,
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub entity_ids: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeriesManifest {
    pub schema: String,
    pub series_id: String,
    pub title: String,
    pub canonical_source_start: u64,
    pub canonical_source_end: u64,
    #[serde(default)]
    pub defaults: SeriesDefaults,
    pub seasons: Vec<Season>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Season {
    pub id: String,
    pub order: u32,
    pub title: String,
    #[serde(default)]
    pub runtime_plan: Option<RuntimePlan>,
    #[serde(default)]
    pub total_runtime_seconds: Option<f64>,
    pub episodes: Vec<Episode>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Episode {
    pub id: String,
    pub order: u32,
    #[serde(default)]
    pub part: Option<String>,
    pub production_title: String,
    pub manuscript_title: String,
    #[serde(default)]
    pub poem_ids: Vec<String>,
    pub source_ranges: Vec<SeriesRange>,
    #[serde(default)]
    pub omissions: Vec<SeriesOmission>,
    #[serde(default)]
    pub chronology_place: String,
    #[serde(default)]
    pub memory_mode: String,
    #[serde(default)]
    pub sensitivity: String,
    #[serde(default)]
    pub recurring_motifs: Vec<String>,
    #[serde(default)]
    pub continuity_entry: Vec<String>,
    #[serde(default)]
    pub continuity_exit: Vec<String>,
    #[serde(default)]
    pub runtime_plan: Option<RuntimePlan>,
    pub timing_status: TimingStatus,
    pub human_review_status: String,
    #[serde(default)]
    pub raw_orientation_seconds: f64,
    #[serde(default)]
    pub measured_narration_seconds: f64,
    #[serde(default)]
    pub protected_pause_seconds: f64,
    #[serde(default)]
    pub scene_duration_seconds: f64,
    #[serde(default)]
    pub total_runtime_seconds: f64,
    #[serde(default)]
    pub release_ready: bool,
    #[serde(default)]
    pub accepted_speakers: Vec<String>,
    #[serde(default)]
    pub findings: Vec<SeriesFinding>,
    #[serde(default)]
    pub dependencies: Vec<SeriesDependency>,
    pub children: Vec<ChildManifestRef>,
    #[serde(default)]
    pub production_units: Vec<ProductionUnit>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct RuntimePlan {
    #[serde(default)]
    pub class: String,
    pub minimum_seconds: f64,
    pub target_seconds: f64,
    pub maximum_seconds: f64,
    #[serde(default)]
    pub components_seconds: BTreeMap<String, f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeriesRange {
    pub start: u64,
    pub end: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeriesOmission {
    pub start: u64,
    pub end: u64,
    pub bridge: String,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeriesFinding {
    pub reviewer: String,
    pub finding: String,
    #[serde(default)]
    pub decision_reference: String,
    #[serde(default)]
    pub approved: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeriesDependency {
    pub kind: String,
    pub episode_id: String,
    pub detail: String,
    #[serde(default)]
    pub approved_structure: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ChildManifestRef {
    pub path: String,
    pub work_id: String,
    pub expected_sha256: String,
    #[serde(default)]
    pub accepted_timing_states: Vec<TimingStatus>,
    #[serde(default)]
    pub accepted_review_states: Vec<String>,
    #[serde(default)]
    pub required_platforms: Vec<String>,
    #[serde(default)]
    pub source_complete: bool,
    #[serde(default)]
    pub privacy_clear: bool,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProductionUnit {
    pub id: String,
    pub kind: String,
    pub source_kind: String,
    pub duration_seconds: f64,
    #[serde(default)]
    pub caption_text: String,
}

#[derive(Clone, Debug)]
pub struct LoadedSeries {
    pub path: PathBuf,
    pub manifest: SeriesManifest,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesValidationReport {
    pub schema: String,
    pub series_id: String,
    pub seasons: usize,
    pub episodes: usize,
    pub children: usize,
    pub source_start: u64,
    pub source_end: u64,
    pub continuous_coverage: bool,
    pub total_runtime_ms: u64,
    pub release_ready_episodes: usize,
    pub human_approvals: usize,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesPlanReport {
    pub series_id: String,
    pub defaults: SeriesDefaults,
    pub seasons: Vec<SeasonPlan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeasonPlan {
    pub id: String,
    pub order: u32,
    pub runtime_plan: Option<RuntimePlan>,
    pub runtime_ms: u64,
    pub episodes: Vec<EpisodePlan>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EpisodePlan {
    pub id: String,
    pub order: u32,
    pub part: Option<String>,
    pub production_title: String,
    pub manuscript_title: String,
    pub poem_ids: Vec<String>,
    pub source_ranges: Vec<SeriesRange>,
    pub omissions: Vec<SeriesOmission>,
    pub chronology_place: String,
    pub memory_mode: String,
    pub sensitivity: String,
    pub recurring_motifs: Vec<String>,
    pub runtime_plan: Option<RuntimePlan>,
    pub timing_status: String,
    pub human_review_status: String,
    pub raw_orientation_ms: u64,
    pub measured_narration_ms: u64,
    pub protected_pause_ms: u64,
    pub scene_duration_ms: u64,
    pub runtime_ms: u64,
    pub children: Vec<String>,
    pub dependencies: Vec<SeriesDependency>,
    pub release_ready: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesTimingAuditReport {
    pub schema: String,
    pub series_id: String,
    pub neighbor_drift_warning_percent: f64,
    pub planned_episodes: usize,
    pub unplanned_episodes: Vec<String>,
    pub evaluated_episodes: usize,
    pub within_range_episodes: usize,
    pub under_range_episodes: Vec<String>,
    pub over_range_episodes: Vec<String>,
    pub planned_target_runtime_ms: u64,
    pub projected_runtime_ms: u64,
    pub seasons: Vec<SeasonTimingAudit>,
    pub neighbor_drift_warnings: Vec<NeighborTimingDrift>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeasonTimingAudit {
    pub id: String,
    pub runtime_plan: Option<RuntimePlan>,
    pub derived_episode_budget_ms: Option<RuntimeBudgetMs>,
    pub episode_target_delta_ms: Option<i64>,
    pub budget_alignment: String,
    pub projected_runtime_ms: u64,
    pub runtime_basis: String,
    pub status: String,
    pub episodes: Vec<EpisodeTimingAudit>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EpisodeTimingAudit {
    pub id: String,
    pub runtime_class: String,
    pub timing_status: String,
    pub runtime_basis: String,
    pub effective_runtime_ms: Option<u64>,
    pub budget_ms: Option<RuntimeBudgetMs>,
    pub delta_from_target_ms: Option<i64>,
    pub delta_from_target_percent: Option<f64>,
    pub measured_narration_ms: u64,
    pub protected_pause_ms: u64,
    pub narration_share_percent: Option<f64>,
    pub protected_pause_share_percent: Option<f64>,
    pub range_status: String,
    pub planned_components_ms: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBudgetMs {
    pub minimum_ms: u64,
    pub target_ms: u64,
    pub maximum_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct NeighborTimingDrift {
    pub from_episode: String,
    pub to_episode: String,
    pub from_runtime_basis: String,
    pub to_runtime_basis: String,
    pub from_runtime_ms: u64,
    pub to_runtime_ms: u64,
    pub change_percent: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesCoverageReport {
    pub series_id: String,
    pub selected_units: usize,
    pub omitted_units: usize,
    pub missing_units: Vec<u64>,
    pub overlapping_units: Vec<u64>,
    pub episodes: Vec<EpisodeCoverage>,
    pub continuous: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct EpisodeCoverage {
    pub id: String,
    pub source_ranges: Vec<SeriesRange>,
    pub omissions: Vec<SeriesOmission>,
    pub children: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SeriesReviewQueueReport {
    pub series_id: String,
    pub open: Vec<String>,
    pub reviewed: Vec<String>,
    pub release_blocked: Vec<String>,
    pub findings_by_reviewer: BTreeMap<String, Vec<String>>,
    pub decision_status_by_episode: BTreeMap<String, String>,
    pub missing_decision_reviewers: BTreeMap<String, Vec<String>>,
    pub decision_record_counts: BTreeMap<String, usize>,
    pub explicit_resolutions: Vec<String>,
    pub decision_release_gates: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct EpisodePacketReport {
    pub schema: String,
    pub series_id: String,
    pub episode_id: String,
    pub output_dir: String,
    pub manifest: String,
    pub captions: String,
    pub lineage: String,
    pub coverage: String,
    pub duration: String,
    pub duration_ms: u64,
    pub child_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct EpisodePacketManifest {
    schema: String,
    series_id: String,
    episode_id: String,
    production_title: String,
    manuscript_title: String,
    timing_status: String,
    human_review_status: String,
    defaults: SeriesDefaults,
    children: Vec<ComposedChild>,
    production_units: Vec<ComposedUnit>,
    duration_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
struct ComposedChild {
    path: String,
    work_id: String,
    sha256: String,
    offset_ms: u64,
    duration_ms: u64,
    protected_pauses: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct ComposedUnit {
    id: String,
    kind: String,
    source_kind: String,
    offset_ms: u64,
    duration_ms: u64,
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedSeries> {
    let path = path.as_ref();
    let manifest = serde_yaml::from_slice(&fs::read(path)?)
        .with_context(|| format!("failed to parse series manifest {}", path.display()))?;
    Ok(LoadedSeries {
        path: path.to_path_buf(),
        manifest,
    })
}

pub fn validate(path: impl AsRef<Path>) -> Result<SeriesValidationReport> {
    let loaded = load(path)?;
    validate_loaded(&loaded)
}

pub fn validate_loaded(loaded: &LoadedSeries) -> Result<SeriesValidationReport> {
    let series = &loaded.manifest;
    if series.schema != SERIES_SCHEMA {
        bail!(
            "series schema must be {SERIES_SCHEMA}, got {}",
            series.schema
        );
    }
    require("series_id", &series.series_id)?;
    require("title", &series.title)?;
    if series.canonical_source_start == 0
        || series.canonical_source_end < series.canonical_source_start
    {
        bail!("series canonical source range is invalid");
    }
    if series.seasons.is_empty() {
        bail!("series requires at least one season");
    }
    let base = loaded.path.parent().unwrap_or_else(|| Path::new("."));
    let mut season_ids = HashSet::new();
    let mut episode_ids = HashSet::new();
    let mut child_paths = HashSet::new();
    let mut episode_count = 0;
    let mut child_count = 0;
    let mut total_runtime_ms = 0;
    let mut release_ready_episodes = 0;
    let mut human_approvals = 0;
    let mut warnings = Vec::new();
    let mut poem_owner: BTreeMap<String, String> = BTreeMap::new();

    if let Some(reference) = &series.defaults.continuity_registry {
        let registry_path = base.join(&reference.path);
        if production::sha256_path(&registry_path)? != reference.sha256 {
            bail!("series continuity registry hash mismatch");
        }
        let registry = crate::continuity::load(&registry_path)?;
        if registry.version != reference.version {
            bail!("series continuity registry version mismatch");
        }
        for id in &reference.entity_ids {
            if !registry.entities.iter().any(|entity| entity.id == *id) {
                bail!("series continuity registry has no entity {id}");
            }
        }
    }
    let series_coverage = coverage_loaded(series)?;
    if !series_coverage.continuous {
        bail!("series source coverage is not continuous");
    }

    for (season_index, season) in series.seasons.iter().enumerate() {
        if !season_ids.insert(season.id.clone()) {
            bail!("duplicate season id {}", season.id);
        }
        let expected_season_order = season_index as u32 + 1;
        if season.order != expected_season_order {
            bail!(
                "season {} is out of order: expected {}",
                season.id,
                expected_season_order
            );
        }
        if season.episodes.is_empty() {
            bail!("season {} has no episodes", season.id);
        }
        if let Some(runtime_plan) = &season.runtime_plan {
            validate_runtime_plan(&format!("season {}", season.id), runtime_plan)?;
        }
        let mut season_runtime_ms = 0;
        for (episode_index, episode) in season.episodes.iter().enumerate() {
            episode_count += 1;
            if !episode_ids.insert(episode.id.clone()) {
                bail!("duplicate episode id {}", episode.id);
            }
            let expected_episode_order = episode_index as u32 + 1;
            if episode.order != expected_episode_order {
                bail!(
                    "episode {} is out of order in {}: expected {}",
                    episode.id,
                    season.id,
                    expected_episode_order
                );
            }
            require("production_title", &episode.production_title)?;
            require("manuscript_title", &episode.manuscript_title)?;
            require("human_review_status", &episode.human_review_status)?;
            if let Some(runtime_plan) = &episode.runtime_plan {
                validate_runtime_plan(&format!("episode {}", episode.id), runtime_plan)?;
            }
            for (label, value) in [
                ("raw_orientation_seconds", episode.raw_orientation_seconds),
                (
                    "measured_narration_seconds",
                    episode.measured_narration_seconds,
                ),
                ("protected_pause_seconds", episode.protected_pause_seconds),
                ("scene_duration_seconds", episode.scene_duration_seconds),
                ("total_runtime_seconds", episode.total_runtime_seconds),
            ] {
                if !value.is_finite() || value < 0.0 {
                    bail!("episode {} has invalid {label}", episode.id);
                }
            }
            for unit in &episode.production_units {
                if unit.source_kind != "production-authored" {
                    bail!(
                        "episode {} production unit {} must be production-authored",
                        episode.id,
                        unit.id
                    );
                }
                if !unit.duration_seconds.is_finite() || unit.duration_seconds <= 0.0 {
                    bail!(
                        "episode {} production unit {} has invalid duration",
                        episode.id,
                        unit.id
                    );
                }
            }
            if episode.source_ranges.is_empty() {
                bail!("episode {} has no canonical source ranges", episode.id);
            }
            validate_ranges(&episode.id, &episode.source_ranges, &episode.omissions)?;
            if episode.children.is_empty() {
                bail!("episode {} has no child manifests", episode.id);
            }
            for poem in &episode.poem_ids {
                if !episode.dependencies.iter().any(|dependency| {
                    dependency.kind == "poem-prose" && dependency.approved_structure
                }) {
                    bail!(
                        "poem {poem} in {} lacks an approved poem-prose structure",
                        episode.id
                    );
                }
                if let Some(previous) = poem_owner.get(poem) {
                    let approved = episode.dependencies.iter().any(|dependency| {
                        dependency.kind == "poem-continuation"
                            && dependency.episode_id == *previous
                            && dependency.approved_structure
                    });
                    if !approved {
                        bail!(
                            "poem {poem} continues from {previous} into {} without an approved structure",
                            episode.id
                        );
                    }
                }
                poem_owner.insert(poem.clone(), episode.id.clone());
            }
            for finding in &episode.findings {
                if finding.approved {
                    human_approvals += 1;
                }
            }
            let mut resolved_duration_ms = 0;
            let mut release_blockers = Vec::new();
            let mut child_source_units = BTreeSet::new();
            let mut actual_child_timing_states = Vec::new();
            for child in &episode.children {
                child_count += 1;
                let resolved = base.join(&child.path);
                let canonical = resolved.canonicalize().with_context(|| {
                    format!(
                        "episode {} child is missing: {}",
                        episode.id,
                        resolved.display()
                    )
                })?;
                let key = canonical.to_string_lossy().to_string();
                if !child_paths.insert(key) {
                    bail!("repeated child manifest reference {}", child.path);
                }
                let actual_hash = production::sha256_path(&canonical)?;
                if actual_hash != child.expected_sha256 {
                    bail!("child {} hash mismatch", child.path);
                }
                let loaded_child = production::load(&canonical)?;
                let child_report = production::validate(&loaded_child)?;
                if let Some(default_registry) = &series.defaults.continuity_registry {
                    if let Some(raw_child_registry) = loaded_child
                        .manifest
                        .continuity
                        .extra
                        .get("external_registry")
                    {
                        let child_registry: ContinuityRegistryRef =
                            serde_yaml::from_value(raw_child_registry.clone())?;
                        if child_registry.version != default_registry.version
                            || child_registry.sha256 != default_registry.sha256
                        {
                            bail!("child {} has incompatible continuity registry", child.path);
                        }
                    }
                }
                if loaded_child.manifest.work != child.work_id {
                    bail!("child {} work id mismatch", child.path);
                }
                if !child.accepted_timing_states.is_empty()
                    && !child
                        .accepted_timing_states
                        .contains(&loaded_child.manifest.timing_status)
                {
                    bail!("child {} has incompatible timing state", child.path);
                }
                actual_child_timing_states.push(loaded_child.manifest.timing_status);
                if !child.accepted_review_states.is_empty()
                    && !child
                        .accepted_review_states
                        .contains(&loaded_child.manifest.review.status)
                {
                    bail!("child {} has incompatible review state", child.path);
                }
                let required_platforms = series
                    .defaults
                    .platforms
                    .iter()
                    .chain(&child.required_platforms)
                    .collect::<BTreeSet<_>>();
                for platform in required_platforms {
                    if !loaded_child
                        .manifest
                        .platforms
                        .iter()
                        .any(|candidate| candidate.name == *platform)
                    {
                        bail!("child {} lacks required platform {platform}", child.path);
                    }
                }
                if !episode.accepted_speakers.is_empty() {
                    for speaker in &loaded_child.manifest.speakers {
                        if !episode.accepted_speakers.contains(&speaker.id) {
                            bail!(
                                "child {} has incompatible speaker {}",
                                child.path,
                                speaker.id
                            );
                        }
                    }
                }
                let coverage = production::source_coverage(&canonical)?;
                for range in &coverage.selected_ranges {
                    for unit in range.start..=range.end {
                        if !child_source_units.insert(unit) {
                            bail!(
                                "episode {} child source ranges overlap at {unit}",
                                episode.id
                            );
                        }
                    }
                }
                for omission in &coverage.omissions {
                    for unit in omission.start..=omission.end {
                        if !child_source_units.insert(unit) {
                            bail!(
                                "episode {} child source ranges overlap at {unit}",
                                episode.id
                            );
                        }
                    }
                }
                if child.source_complete && !coverage.complete {
                    bail!("child {} source coverage is incomplete", child.path);
                }
                let privacy_clear = !production::provider_package(&canonical)?.blocked;
                if child.privacy_clear && !privacy_clear {
                    bail!("child {} is privacy-blocked", child.path);
                }
                if !loaded_child.manifest.timing_status.allows_delivery() {
                    release_blockers.push(format!("{} timing", child.path));
                }
                if loaded_child.manifest.review.status != "accepted"
                    && loaded_child.manifest.review.status != "panel-reviewed"
                {
                    release_blockers.push(format!("{} review", child.path));
                }
                if !privacy_clear {
                    release_blockers.push(format!("{} privacy", child.path));
                }
                if !coverage.complete {
                    release_blockers.push(format!("{} coverage", child.path));
                }
                let duration = child_report.duration_ms.unwrap_or(0);
                if let Some(expected) = child.duration_seconds {
                    if duration.abs_diff(ms(expected)) > 1 {
                        bail!("child {} duration mismatch", child.path);
                    }
                }
                resolved_duration_ms += duration;
            }
            let mut declared_source_units = BTreeSet::new();
            for range in &episode.source_ranges {
                declared_source_units.extend(range.start..=range.end);
            }
            for omission in &episode.omissions {
                declared_source_units.extend(omission.start..=omission.end);
            }
            if child_source_units != declared_source_units {
                bail!(
                    "episode {} canonical source coverage does not match its child manifests",
                    episode.id
                );
            }
            resolved_duration_ms += episode
                .production_units
                .iter()
                .map(|unit| ms(unit.duration_seconds))
                .sum::<u64>();
            if resolved_duration_ms.abs_diff(ms(episode.total_runtime_seconds)) > 1 {
                bail!(
                    "episode {} total runtime does not match its children and production units",
                    episode.id
                );
            }
            if ms(episode.scene_duration_seconds)
                != episode
                    .children
                    .iter()
                    .map(|child| ms(child.duration_seconds.unwrap_or(0.0)))
                    .sum::<u64>()
            {
                bail!(
                    "episode {} scene duration does not match declared child durations",
                    episode.id
                );
            }
            if episode.release_ready && !release_blockers.is_empty() {
                bail!(
                    "episode {} is release-ready with blocked children: {}",
                    episode.id,
                    release_blockers.join(", ")
                );
            }
            if episode.release_ready
                && episode.human_review_status != "approved"
                && episode.human_review_status != "accepted"
            {
                bail!(
                    "episode {} is release-ready without explicit human review approval",
                    episode.id
                );
            }
            if episode.timing_status.allows_delivery()
                && actual_child_timing_states
                    .iter()
                    .any(|state| !state.allows_delivery())
            {
                bail!(
                    "episode {} has incompatible child timing states",
                    episode.id
                );
            }
            if episode.timing_status == TimingStatus::Locked
                && actual_child_timing_states
                    .iter()
                    .any(|state| *state != TimingStatus::Locked)
            {
                bail!("locked episode {} has an unlocked child", episode.id);
            }
            if episode.release_ready {
                release_ready_episodes += 1;
            }
            if !release_blockers.is_empty() {
                warnings.push(format!(
                    "episode {} release blockers: {}",
                    episode.id,
                    release_blockers.join(", ")
                ));
            }
            season_runtime_ms += resolved_duration_ms;
        }
        if let Some(expected) = season.total_runtime_seconds {
            if season_runtime_ms.abs_diff(ms(expected)) > 1 {
                bail!("season {} total runtime mismatch", season.id);
            }
        }
        total_runtime_ms += season_runtime_ms;
    }
    Ok(SeriesValidationReport {
        schema: series.schema.clone(),
        series_id: series.series_id.clone(),
        seasons: series.seasons.len(),
        episodes: episode_count,
        children: child_count,
        source_start: series.canonical_source_start,
        source_end: series.canonical_source_end,
        continuous_coverage: true,
        total_runtime_ms,
        release_ready_episodes,
        human_approvals,
        warnings,
    })
}

pub fn plan(path: impl AsRef<Path>) -> Result<SeriesPlanReport> {
    let loaded = load(path)?;
    validate_loaded(&loaded)?;
    Ok(SeriesPlanReport {
        series_id: loaded.manifest.series_id,
        defaults: loaded.manifest.defaults,
        seasons: loaded
            .manifest
            .seasons
            .into_iter()
            .map(|season| SeasonPlan {
                id: season.id,
                order: season.order,
                runtime_plan: season.runtime_plan,
                runtime_ms: season
                    .episodes
                    .iter()
                    .map(|episode| ms(episode.total_runtime_seconds))
                    .sum(),
                episodes: season
                    .episodes
                    .into_iter()
                    .map(|episode| EpisodePlan {
                        id: episode.id,
                        order: episode.order,
                        part: episode.part,
                        production_title: episode.production_title,
                        manuscript_title: episode.manuscript_title,
                        poem_ids: episode.poem_ids,
                        source_ranges: episode.source_ranges,
                        omissions: episode.omissions,
                        chronology_place: episode.chronology_place,
                        memory_mode: episode.memory_mode,
                        sensitivity: episode.sensitivity,
                        recurring_motifs: episode.recurring_motifs,
                        runtime_plan: episode.runtime_plan,
                        timing_status: episode.timing_status.as_str().to_string(),
                        human_review_status: episode.human_review_status,
                        raw_orientation_ms: ms(episode.raw_orientation_seconds),
                        measured_narration_ms: ms(episode.measured_narration_seconds),
                        protected_pause_ms: ms(episode.protected_pause_seconds),
                        scene_duration_ms: ms(episode.scene_duration_seconds),
                        runtime_ms: ms(episode.total_runtime_seconds),
                        children: episode
                            .children
                            .into_iter()
                            .map(|child| child.path)
                            .collect(),
                        dependencies: episode.dependencies,
                        release_ready: episode.release_ready,
                    })
                    .collect(),
            })
            .collect(),
    })
}

pub fn timing_audit(
    path: impl AsRef<Path>,
    neighbor_drift_warning_percent: f64,
) -> Result<SeriesTimingAuditReport> {
    let loaded = load(path)?;
    timing_audit_loaded(&loaded, neighbor_drift_warning_percent)
}

pub fn timing_audit_loaded(
    loaded: &LoadedSeries,
    neighbor_drift_warning_percent: f64,
) -> Result<SeriesTimingAuditReport> {
    if !neighbor_drift_warning_percent.is_finite() || neighbor_drift_warning_percent < 0.0 {
        bail!("neighbor drift warning percent must be finite and non-negative");
    }
    validate_loaded(loaded)?;

    let mut planned_episodes = 0;
    let mut unplanned_episodes = Vec::new();
    let mut evaluated_episodes = 0;
    let mut within_range_episodes = 0;
    let mut under_range_episodes = Vec::new();
    let mut over_range_episodes = Vec::new();
    let mut planned_target_runtime_ms = 0;
    let mut projected_runtime_ms = 0;
    let mut seasons = Vec::new();
    let mut neighbor_drift_warnings = Vec::new();

    for season in &loaded.manifest.seasons {
        let mut episode_audits = Vec::new();
        let mut season_projected_ms = 0;
        let mut previous: Option<(&str, u64, &str)> = None;
        let mut derived_minimum_ms = 0;
        let mut derived_target_ms = 0;
        let mut derived_maximum_ms = 0;
        let mut all_episodes_planned = true;
        let mut runtime_bases = BTreeSet::new();

        for episode in &season.episodes {
            let (runtime_basis, effective_runtime_ms) = if episode.total_runtime_seconds > 0.0 {
                ("declared-runtime", Some(ms(episode.total_runtime_seconds)))
            } else if episode.raw_orientation_seconds > 0.0 {
                ("raw-orientation", Some(ms(episode.raw_orientation_seconds)))
            } else if let Some(plan) = &episode.runtime_plan {
                ("planned-target", Some(ms(plan.target_seconds)))
            } else {
                ("unavailable", None)
            };

            let mut range_status = "unplanned".to_string();
            let mut delta_from_target_ms = None;
            let mut delta_from_target_percent = None;
            let mut budget_ms = None;
            let mut planned_components_ms = BTreeMap::new();
            let runtime_class = episode
                .runtime_plan
                .as_ref()
                .map(|plan| plan.class.clone())
                .unwrap_or_default();

            if let Some(plan) = &episode.runtime_plan {
                planned_episodes += 1;
                let budget = runtime_budget_ms(plan);
                planned_target_runtime_ms += budget.target_ms;
                derived_minimum_ms += budget.minimum_ms;
                derived_target_ms += budget.target_ms;
                derived_maximum_ms += budget.maximum_ms;
                planned_components_ms = plan
                    .components_seconds
                    .iter()
                    .map(|(name, seconds)| (name.clone(), ms(*seconds)))
                    .collect();
                if let Some(runtime) = effective_runtime_ms
                    && runtime_basis != "planned-target"
                {
                    evaluated_episodes += 1;
                    let delta = runtime as i128 - budget.target_ms as i128;
                    delta_from_target_ms =
                        Some(delta.clamp(i64::MIN as i128, i64::MAX as i128) as i64);
                    delta_from_target_percent = Some(
                        (runtime as f64 - budget.target_ms as f64) * 100.0
                            / budget.target_ms as f64,
                    );
                    range_status = if runtime < budget.minimum_ms {
                        under_range_episodes.push(episode.id.clone());
                        "under"
                    } else if runtime > budget.maximum_ms {
                        over_range_episodes.push(episode.id.clone());
                        "over"
                    } else {
                        within_range_episodes += 1;
                        "within"
                    }
                    .to_string();
                } else {
                    range_status = if runtime_basis == "planned-target" {
                        "planned"
                    } else {
                        "not-evaluated"
                    }
                    .to_string();
                }
                budget_ms = Some(budget);
            } else {
                all_episodes_planned = false;
                unplanned_episodes.push(episode.id.clone());
            }

            if let Some(runtime) = effective_runtime_ms {
                runtime_bases.insert(runtime_basis);
                season_projected_ms += runtime;
                projected_runtime_ms += runtime;
                if let Some((previous_id, previous_runtime, previous_basis)) = previous
                    && previous_runtime > 0
                {
                    let change_percent = (runtime as f64 - previous_runtime as f64) * 100.0
                        / previous_runtime as f64;
                    if change_percent.abs() > neighbor_drift_warning_percent {
                        neighbor_drift_warnings.push(NeighborTimingDrift {
                            from_episode: previous_id.to_string(),
                            to_episode: episode.id.clone(),
                            from_runtime_basis: previous_basis.to_string(),
                            to_runtime_basis: runtime_basis.to_string(),
                            from_runtime_ms: previous_runtime,
                            to_runtime_ms: runtime,
                            change_percent,
                        });
                    }
                }
                previous = Some((&episode.id, runtime, runtime_basis));
            } else {
                previous = None;
            }

            episode_audits.push(EpisodeTimingAudit {
                id: episode.id.clone(),
                runtime_class,
                timing_status: episode.timing_status.as_str().to_string(),
                runtime_basis: runtime_basis.to_string(),
                effective_runtime_ms,
                budget_ms,
                delta_from_target_ms,
                delta_from_target_percent,
                measured_narration_ms: ms(episode.measured_narration_seconds),
                protected_pause_ms: ms(episode.protected_pause_seconds),
                narration_share_percent: share_percent(
                    episode.measured_narration_seconds,
                    effective_runtime_ms,
                ),
                protected_pause_share_percent: share_percent(
                    episode.protected_pause_seconds,
                    effective_runtime_ms,
                ),
                range_status,
                planned_components_ms,
            });
        }

        let derived_episode_budget_ms = all_episodes_planned.then_some(RuntimeBudgetMs {
            minimum_ms: derived_minimum_ms,
            target_ms: derived_target_ms,
            maximum_ms: derived_maximum_ms,
        });
        let (episode_target_delta_ms, budget_alignment) =
            match (&season.runtime_plan, &derived_episode_budget_ms) {
                (Some(plan), Some(derived)) => {
                    let delta =
                        derived.target_ms as i128 - runtime_budget_ms(plan).target_ms as i128;
                    let delta = delta.clamp(i64::MIN as i128, i64::MAX as i128) as i64;
                    (Some(delta), if delta == 0 { "aligned" } else { "mismatch" })
                }
                (Some(_), None) => (None, "partial"),
                (None, Some(_)) => (None, "derived-only"),
                (None, None) => (None, "unplanned"),
            };
        let runtime_basis = if runtime_bases.is_empty() {
            "unavailable".to_string()
        } else if runtime_bases.len() == 1 {
            runtime_bases.iter().next().unwrap().to_string()
        } else {
            "mixed".to_string()
        };
        let status = match &season.runtime_plan {
            None => "unplanned",
            Some(_) if runtime_basis == "unavailable" => "not-evaluated",
            Some(_) if runtime_basis == "planned-target" => "planned",
            Some(plan) => range_status(season_projected_ms, &runtime_budget_ms(plan)),
        }
        .to_string();
        seasons.push(SeasonTimingAudit {
            id: season.id.clone(),
            runtime_plan: season.runtime_plan.clone(),
            derived_episode_budget_ms,
            episode_target_delta_ms,
            budget_alignment: budget_alignment.to_string(),
            projected_runtime_ms: season_projected_ms,
            runtime_basis,
            status,
            episodes: episode_audits,
        });
    }

    Ok(SeriesTimingAuditReport {
        schema: "reel.series-timing-audit.v0.1".to_string(),
        series_id: loaded.manifest.series_id.clone(),
        neighbor_drift_warning_percent,
        planned_episodes,
        unplanned_episodes,
        evaluated_episodes,
        within_range_episodes,
        under_range_episodes,
        over_range_episodes,
        planned_target_runtime_ms,
        projected_runtime_ms,
        seasons,
        neighbor_drift_warnings,
    })
}

pub fn coverage(path: impl AsRef<Path>) -> Result<SeriesCoverageReport> {
    let loaded = load(path)?;
    coverage_loaded(&loaded.manifest)
}

fn coverage_loaded(series: &SeriesManifest) -> Result<SeriesCoverageReport> {
    let mut selected = BTreeSet::new();
    let mut omitted = BTreeSet::new();
    let mut overlaps = BTreeSet::new();
    let mut previous_start = 0;
    for episode in series.seasons.iter().flat_map(|season| &season.episodes) {
        for range in &episode.source_ranges {
            if range.start < series.canonical_source_start
                || range.end > series.canonical_source_end
            {
                bail!(
                    "episode {} source range is outside the canonical series range",
                    episode.id
                );
            }
            if range.start < previous_start {
                bail!(
                    "episode {} source ranges are out of series order",
                    episode.id
                );
            }
            previous_start = range.start;
            for unit in range.start..=range.end {
                if !selected.insert(unit) {
                    overlaps.insert(unit);
                }
            }
        }
        for range in &episode.omissions {
            if range.start < series.canonical_source_start
                || range.end > series.canonical_source_end
            {
                bail!(
                    "episode {} omission is outside the canonical series range",
                    episode.id
                );
            }
            for unit in range.start..=range.end {
                if selected.contains(&unit) || !omitted.insert(unit) {
                    overlaps.insert(unit);
                }
            }
        }
    }
    let missing = (series.canonical_source_start..=series.canonical_source_end)
        .filter(|unit| !selected.contains(unit) && !omitted.contains(unit))
        .collect::<Vec<_>>();
    Ok(SeriesCoverageReport {
        series_id: series.series_id.clone(),
        selected_units: selected.len(),
        omitted_units: omitted.len(),
        continuous: missing.is_empty() && overlaps.is_empty(),
        missing_units: missing,
        overlapping_units: overlaps.into_iter().collect(),
        episodes: series
            .seasons
            .iter()
            .flat_map(|season| &season.episodes)
            .map(|episode| EpisodeCoverage {
                id: episode.id.clone(),
                source_ranges: episode.source_ranges.clone(),
                omissions: episode.omissions.clone(),
                children: episode
                    .children
                    .iter()
                    .map(|child| child.path.clone())
                    .collect(),
            })
            .collect(),
    })
}

pub fn review_queue(path: impl AsRef<Path>) -> Result<SeriesReviewQueueReport> {
    review_queue_with_decisions(path, None::<&Path>)
}

pub fn review_queue_with_decisions(
    path: impl AsRef<Path>,
    decision_index: Option<impl AsRef<Path>>,
) -> Result<SeriesReviewQueueReport> {
    let series_path = path.as_ref();
    let loaded = load(series_path)?;
    validate_loaded(&loaded)?;
    let mut report = SeriesReviewQueueReport {
        series_id: loaded.manifest.series_id,
        open: Vec::new(),
        reviewed: Vec::new(),
        release_blocked: Vec::new(),
        findings_by_reviewer: BTreeMap::new(),
        decision_status_by_episode: BTreeMap::new(),
        missing_decision_reviewers: BTreeMap::new(),
        decision_record_counts: BTreeMap::new(),
        explicit_resolutions: Vec::new(),
        decision_release_gates: Vec::new(),
    };
    let episode_ids = loaded
        .manifest
        .seasons
        .iter()
        .flat_map(|season| &season.episodes)
        .map(|episode| episode.id.clone())
        .collect::<Vec<_>>();
    for episode in loaded
        .manifest
        .seasons
        .into_iter()
        .flat_map(|season| season.episodes)
    {
        if episode.human_review_status == "open" {
            report.open.push(episode.id.clone());
        } else {
            report.reviewed.push(episode.id.clone());
        }
        if !episode.release_ready {
            report.release_blocked.push(episode.id.clone());
        }
        for finding in episode.findings {
            report
                .findings_by_reviewer
                .entry(finding.reviewer)
                .or_default()
                .push(format!("{}: {}", episode.id, finding.finding));
        }
    }
    if let Some(index) = decision_index {
        let decisions =
            crate::review_decision::summarize_index(series_path, index.as_ref(), &episode_ids)?;
        report.decision_status_by_episode = decisions.status_by_episode;
        report.missing_decision_reviewers = decisions.missing_reviewers_by_episode;
        report.decision_record_counts = decisions.record_counts;
        report.explicit_resolutions = decisions.explicit_resolutions;
        report.decision_release_gates = decisions.release_gates;
    }
    Ok(report)
}

pub fn compose_episode(
    series_path: impl AsRef<Path>,
    episode_id: &str,
    output_dir: impl AsRef<Path>,
) -> Result<EpisodePacketReport> {
    let loaded = load(series_path.as_ref())?;
    validate_loaded(&loaded)?;
    let episode = loaded
        .manifest
        .seasons
        .iter()
        .flat_map(|season| &season.episodes)
        .find(|episode| episode.id == episode_id)
        .ok_or_else(|| anyhow!("unknown episode {episode_id}"))?;
    if !episode.timing_status.allows_delivery() {
        bail!("timing not conformed: episode composition is gated");
    }
    let output_dir = output_dir.as_ref();
    if output_dir.exists() && fs::read_dir(output_dir)?.next().is_some() {
        bail!("episode packet output must not exist or must be empty");
    }
    let parent = output_dir.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".reel-episode-{}-{}",
        episode.id,
        std::process::id()
    ));
    if staging.exists() {
        bail!("episode staging path already exists: {}", staging.display());
    }
    fs::create_dir(&staging)?;
    let base = loaded.path.parent().unwrap_or_else(|| Path::new("."));
    let mut offset_ms = 0;
    let mut children = Vec::new();
    let mut caption_entries = Vec::new();
    for child in &episode.children {
        let child_path = base.join(&child.path).canonicalize()?;
        let loaded_child = production::load(&child_path)?;
        let report = production::validate(&loaded_child)?;
        let duration_ms = report
            .duration_ms
            .ok_or_else(|| anyhow!("timing not conformed: child {}", child.path))?;
        let captions_path = child_path.parent().unwrap_or(base).join("captions.srt");
        if !captions_path.is_file() {
            bail!("child {} has no sibling captions.srt", child.path);
        }
        let mut parsed = parse_srt(&fs::read_to_string(&captions_path)?)?;
        for entry in &mut parsed {
            entry.start_ms += offset_ms;
            entry.end_ms += offset_ms;
        }
        caption_entries.extend(parsed);
        children.push(ComposedChild {
            path: child.path.clone(),
            work_id: child.work_id.clone(),
            sha256: production::sha256_path(&child_path)?,
            offset_ms,
            duration_ms,
            protected_pauses: loaded_child
                .manifest
                .protected_pauses
                .iter()
                .map(|pause| pause.id.clone())
                .collect(),
        });
        offset_ms += duration_ms;
    }
    let mut units = Vec::new();
    for unit in &episode.production_units {
        let duration_ms = ms(unit.duration_seconds);
        if !unit.caption_text.is_empty() {
            caption_entries.push(SrtEntry {
                index: caption_entries.len() + 1,
                start_ms: offset_ms,
                end_ms: offset_ms + duration_ms,
                text: unit.caption_text.clone(),
            });
        }
        units.push(ComposedUnit {
            id: unit.id.clone(),
            kind: unit.kind.clone(),
            source_kind: unit.source_kind.clone(),
            offset_ms,
            duration_ms,
        });
        offset_ms += duration_ms;
    }
    if offset_ms.abs_diff(ms(episode.total_runtime_seconds)) > 1 {
        bail!("composed duration does not match declared episode runtime");
    }
    let packet_manifest = EpisodePacketManifest {
        schema: EPISODE_PACKET_SCHEMA.to_string(),
        series_id: loaded.manifest.series_id.clone(),
        episode_id: episode.id.clone(),
        production_title: episode.production_title.clone(),
        manuscript_title: episode.manuscript_title.clone(),
        timing_status: episode.timing_status.as_str().to_string(),
        human_review_status: episode.human_review_status.clone(),
        defaults: loaded.manifest.defaults.clone(),
        children,
        production_units: units,
        duration_ms: offset_ms,
    };
    let manifest_out = staging.join("manifest.yaml");
    let captions_out = staging.join("captions.srt");
    let lineage_out = staging.join("lineage.json");
    let coverage_out = staging.join("coverage.json");
    let duration_out = staging.join("duration.json");
    fs::write(&manifest_out, serde_yaml::to_string(&packet_manifest)?)?;
    fs::write(&captions_out, render_srt(&caption_entries))?;
    fs::write(
        &lineage_out,
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "reel.episode-lineage.v0.1",
            "series_manifest": series_path.as_ref(),
            "series_manifest_sha256": production::sha256_path(series_path.as_ref())?,
            "episode_id": episode.id,
            "children": packet_manifest.children,
            "production_units": packet_manifest.production_units,
            "generated_unix": unix_now()?,
        }))?,
    )?;
    fs::write(
        &coverage_out,
        serde_json::to_vec_pretty(&serde_json::json!({
            "episode_id": episode.id,
            "source_ranges": episode.source_ranges,
            "omissions": episode.omissions,
            "production_units": episode.production_units,
        }))?,
    )?;
    fs::write(
        &duration_out,
        serde_json::to_vec_pretty(&serde_json::json!({
            "episode_id": episode.id,
            "duration_ms": offset_ms,
            "child_duration_ms": packet_manifest.children.iter().map(|child| child.duration_ms).sum::<u64>(),
            "production_unit_duration_ms": packet_manifest.production_units.iter().map(|unit| unit.duration_ms).sum::<u64>(),
        }))?,
    )?;
    if output_dir.exists() {
        fs::remove_dir(output_dir)?;
    }
    fs::rename(&staging, output_dir).context("failed to atomically publish episode packet")?;
    Ok(EpisodePacketReport {
        schema: EPISODE_PACKET_SCHEMA.to_string(),
        series_id: loaded.manifest.series_id,
        episode_id: episode.id.clone(),
        output_dir: output_dir.display().to_string(),
        manifest: output_dir.join("manifest.yaml").display().to_string(),
        captions: output_dir.join("captions.srt").display().to_string(),
        lineage: output_dir.join("lineage.json").display().to_string(),
        coverage: output_dir.join("coverage.json").display().to_string(),
        duration: output_dir.join("duration.json").display().to_string(),
        duration_ms: offset_ms,
        child_count: episode.children.len(),
    })
}

#[derive(Clone, Debug)]
pub struct SrtEntry {
    pub index: usize,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

pub fn parse_srt(text: &str) -> Result<Vec<SrtEntry>> {
    let normalized = text.replace("\r\n", "\n");
    let mut entries = Vec::new();
    for block in normalized
        .split("\n\n")
        .filter(|block| !block.trim().is_empty())
    {
        let mut lines = block.lines();
        let index = lines
            .next()
            .ok_or_else(|| anyhow!("SRT cue is missing an index"))?
            .trim()
            .parse::<usize>()
            .context("SRT cue index is invalid")?;
        let timing = lines
            .next()
            .ok_or_else(|| anyhow!("SRT cue is missing timing"))?;
        let (start, end) = timing
            .split_once(" --> ")
            .ok_or_else(|| anyhow!("SRT cue {index} timing is invalid"))?;
        let start_ms = parse_timestamp(start)?;
        let end_ms = parse_timestamp(end)?;
        if end_ms <= start_ms {
            bail!("SRT cue {index} must have positive duration");
        }
        let text = lines.collect::<Vec<_>>().join("\n");
        if text.trim().is_empty() {
            bail!("SRT cue {index} has no text");
        }
        entries.push(SrtEntry {
            index,
            start_ms,
            end_ms,
            text,
        });
    }
    for pair in entries.windows(2) {
        if pair[1].index != pair[0].index + 1 {
            bail!("SRT cue indexes must be contiguous");
        }
        if pair[1].start_ms < pair[0].end_ms {
            bail!("SRT cues {} and {} overlap", pair[0].index, pair[1].index);
        }
    }
    Ok(entries)
}

pub fn render_srt(entries: &[SrtEntry]) -> String {
    let mut output = String::new();
    for (position, entry) in entries.iter().enumerate() {
        output.push_str(&format!(
            "{}\n{} --> {}\n{}\n",
            position + 1,
            format_timestamp(entry.start_ms),
            format_timestamp(entry.end_ms),
            entry.text
        ));
        if position + 1 < entries.len() {
            output.push('\n');
        }
    }
    output
}

fn parse_timestamp(value: &str) -> Result<u64> {
    let (hms, millis) = value
        .trim()
        .split_once(',')
        .ok_or_else(|| anyhow!("invalid SRT timestamp {value}"))?;
    let parts = hms
        .split(':')
        .map(str::parse::<u64>)
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if parts.len() != 3 || millis.len() != 3 || parts[1] >= 60 || parts[2] >= 60 {
        bail!("invalid SRT timestamp {value}");
    }
    let millis = millis.parse::<u64>()?;
    parts[0]
        .checked_mul(3_600_000)
        .and_then(|total| {
            parts[1]
                .checked_mul(60_000)
                .and_then(|v| total.checked_add(v))
        })
        .and_then(|total| {
            parts[2]
                .checked_mul(1_000)
                .and_then(|v| total.checked_add(v))
        })
        .and_then(|total| total.checked_add(millis))
        .ok_or_else(|| anyhow!("SRT timestamp exceeds supported range: {value}"))
}

fn format_timestamp(ms: u64) -> String {
    let hours = ms / 3_600_000;
    let minutes = (ms / 60_000) % 60;
    let seconds = (ms / 1000) % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02},{:03}", ms % 1000)
}

fn validate_ranges(id: &str, ranges: &[SeriesRange], omissions: &[SeriesOmission]) -> Result<()> {
    let mut previous_end = 0;
    for range in ranges {
        if range.start == 0 || range.end < range.start {
            bail!("episode {id} has invalid source range");
        }
        if range.start <= previous_end {
            bail!("episode {id} has overlapping or out-of-order source ranges");
        }
        previous_end = range.end;
    }
    for omission in omissions {
        if omission.start == 0 || omission.end < omission.start {
            bail!("episode {id} has invalid omission");
        }
        if !matches!(
            omission.bridge.as_str(),
            "silence" | "title-card" | "archival-image" | "approved-adaptation"
        ) {
            bail!(
                "episode {id} has unsupported omission bridge {}",
                omission.bridge
            );
        }
    }
    Ok(())
}

fn validate_runtime_plan(label: &str, plan: &RuntimePlan) -> Result<()> {
    for (field, value) in [
        ("minimum_seconds", plan.minimum_seconds),
        ("target_seconds", plan.target_seconds),
        ("maximum_seconds", plan.maximum_seconds),
    ] {
        if !value.is_finite() || value <= 0.0 {
            bail!("{label} runtime plan has invalid {field}");
        }
    }
    if plan.minimum_seconds > plan.target_seconds || plan.target_seconds > plan.maximum_seconds {
        bail!("{label} runtime plan must satisfy minimum <= target <= maximum");
    }
    let mut component_total_ms = 0;
    for (name, seconds) in &plan.components_seconds {
        require("runtime component name", name)?;
        if !seconds.is_finite() || *seconds < 0.0 {
            bail!("{label} runtime component {name} has an invalid duration");
        }
        component_total_ms += ms(*seconds);
    }
    if !plan.components_seconds.is_empty()
        && component_total_ms.abs_diff(ms(plan.target_seconds)) > 1
    {
        bail!("{label} runtime components must sum to target_seconds");
    }
    Ok(())
}

fn runtime_budget_ms(plan: &RuntimePlan) -> RuntimeBudgetMs {
    RuntimeBudgetMs {
        minimum_ms: ms(plan.minimum_seconds),
        target_ms: ms(plan.target_seconds),
        maximum_ms: ms(plan.maximum_seconds),
    }
}

fn range_status(runtime_ms: u64, budget: &RuntimeBudgetMs) -> &'static str {
    if runtime_ms < budget.minimum_ms {
        "under"
    } else if runtime_ms > budget.maximum_ms {
        "over"
    } else {
        "within"
    }
}

fn share_percent(seconds: f64, runtime_ms: Option<u64>) -> Option<f64> {
    (seconds > 0.0)
        .then_some(runtime_ms)
        .flatten()
        .filter(|runtime| *runtime > 0)
        .map(|runtime| ms(seconds) as f64 * 100.0 / runtime as f64)
}

fn require(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field} must not be empty");
    }
    Ok(())
}

fn ms(seconds: f64) -> u64 {
    (seconds * 1000.0).round() as u64
}

fn unix_now() -> Result<u64> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}
