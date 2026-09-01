use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::{production, series};

pub const SHOWRUNNER_SCHEMA: &str = "reel.showrunner.v0.1";

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShowrunnerPlan {
    pub schema: String,
    pub showrunner_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default = "full_coverage")]
    pub coverage: String,
    pub series: ShowrunnerSeriesRef,
    pub engine: SeriesEngine,
    pub audience_contract: AudienceContract,
    pub vocabularies: Vocabularies,
    #[serde(default)]
    pub policies: AuditPolicies,
    pub seasons: Vec<SeasonControl>,
    pub episodes: Vec<EpisodeControl>,
    #[serde(default)]
    pub revelation_threads: Vec<RevelationThread>,
    #[serde(default)]
    pub reviewers: Vec<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn full_coverage() -> String {
    "full".to_string()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ShowrunnerSeriesRef {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub series_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct SeriesEngine {
    pub promise: String,
    #[serde(default)]
    pub default_movements: Vec<String>,
    #[serde(default)]
    pub allowed_breaks: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AudienceContract {
    pub assumed_knowledge: String,
    #[serde(default)]
    pub no_foreknowledge: bool,
    #[serde(default)]
    pub memory_layers: Vec<String>,
    #[serde(default)]
    pub immediate_narrator_distances: Vec<String>,
    #[serde(default)]
    pub later_knowledge_layers: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Vocabularies {
    pub function_families: Vec<String>,
    pub narrator_distances: Vec<String>,
    pub primary_tones: Vec<String>,
    pub ending_tones: Vec<String>,
    pub ending_modes: Vec<String>,
    pub production_scales: Vec<String>,
    #[serde(default)]
    pub knowledge_layers: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct AuditPolicies {
    #[serde(default)]
    pub max_adjacent_same_function_family: Option<usize>,
    #[serde(default)]
    pub max_adjacent_same_primary_tone: Option<usize>,
    #[serde(default)]
    pub maximum_intensity: Option<u8>,
    #[serde(default)]
    pub max_adjacent_maximum_intensity: Option<usize>,
    #[serde(default)]
    pub max_adjacent_scales: BTreeMap<String, usize>,
    #[serde(default)]
    pub high_production_load_threshold: Option<u8>,
    #[serde(default)]
    pub max_adjacent_high_production_load: Option<usize>,
    #[serde(default)]
    pub abrupt_tone_transitions: Vec<ToneTransition>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToneTransition {
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SeasonControl {
    pub id: String,
    pub action: String,
    pub audience_job: String,
    pub thematic_proposition: String,
    pub thematic_counterforce: String,
    pub finale_delivery: String,
    #[serde(default)]
    pub required_function_families: Vec<String>,
    #[serde(default)]
    pub opening_function_family: Option<String>,
    #[serde(default)]
    pub closing_function_family: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EpisodeControl {
    pub id: String,
    pub function: String,
    pub function_family: String,
    pub dramatic_question: String,
    pub pressure: String,
    pub consequential_action: String,
    pub narrator_distance: String,
    pub audience_revelation: String,
    #[serde(default)]
    pub revelations: Vec<String>,
    #[serde(default)]
    pub knowledge_uses: Vec<KnowledgeUse>,
    #[serde(default)]
    pub internal_tone_beats: Vec<String>,
    pub tone: ToneControl,
    pub ending_invitation: EndingInvitation,
    pub production_scale: String,
    #[serde(default)]
    pub production_load: Option<ProductionLoad>,
    #[serde(default)]
    pub engine_break: Option<String>,
    #[serde(default)]
    pub delivers_season_finale: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProductionLoad {
    pub complexity: u8,
    #[serde(default)]
    pub locations: u32,
    #[serde(default)]
    pub speaking_roles: u32,
    #[serde(default)]
    pub crowd: bool,
    #[serde(default)]
    pub new_assets: Vec<String>,
    #[serde(default)]
    pub reusable_assets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ToneControl {
    pub primary: String,
    pub ending: String,
    pub intensity: u8,
    #[serde(default)]
    pub bridge: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EndingInvitation {
    pub mode: String,
    pub statement: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct KnowledgeUse {
    pub step_id: String,
    pub knowledge_layer: String,
    #[serde(default)]
    pub handoff_declared: bool,
    #[serde(default)]
    pub handoff: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RevelationThread {
    pub id: String,
    #[serde(default)]
    pub remain_open: bool,
    #[serde(default)]
    pub allow_dormancy: bool,
    #[serde(default)]
    pub max_dormant_episodes: Option<usize>,
    pub steps: Vec<RevelationStep>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RevelationStep {
    pub id: String,
    pub episode_id: String,
    #[serde(default)]
    pub through_episode_id: Option<String>,
    pub state: String,
    #[serde(default)]
    pub prerequisites: Vec<String>,
    #[serde(default)]
    pub closes_thread: bool,
}

#[derive(Clone, Debug)]
pub struct LoadedShowrunner {
    pub path: PathBuf,
    pub plan: ShowrunnerPlan,
    pub series_path: PathBuf,
    pub series: series::SeriesManifest,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShowrunnerValidationReport {
    pub schema: String,
    pub showrunner_id: String,
    pub series_id: String,
    pub series_sha256: String,
    pub seasons: usize,
    pub episodes: usize,
    pub revelation_threads: usize,
    pub revelation_steps: usize,
    pub full_coverage: bool,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AuditFinding {
    pub code: String,
    pub severity: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season_id: Option<String>,
    pub episode_ids: Vec<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RhythmAuditReport {
    pub schema: String,
    pub showrunner_id: String,
    pub series_id: String,
    pub episodes: usize,
    pub function_family_counts: BTreeMap<String, usize>,
    pub primary_tone_counts: BTreeMap<String, usize>,
    pub production_scale_counts: BTreeMap<String, usize>,
    pub internal_tone_turns: usize,
    pub production_load_estimated_episodes: usize,
    pub findings: Vec<AuditFinding>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RevelationMapReport {
    pub schema: String,
    pub showrunner_id: String,
    pub series_id: String,
    pub threads: Vec<RevelationThreadReport>,
    pub findings: Vec<AuditFinding>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RevelationThreadReport {
    pub id: String,
    pub remain_open: bool,
    pub steps: Vec<RevelationStep>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShowrunnerAuditReport {
    pub schema: String,
    pub validation: ShowrunnerValidationReport,
    pub rhythm: RhythmAuditReport,
    pub revelation: RevelationMapReport,
    pub finding_count: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShowrunnerReviewQueueReport {
    pub schema: String,
    pub showrunner_id: String,
    pub series_id: String,
    pub required_reviewers: Vec<String>,
    pub open: Vec<ShowrunnerReviewEpisode>,
    pub reviewed: Vec<ShowrunnerReviewEpisode>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShowrunnerReviewPackReport {
    pub schema: String,
    pub audit: ShowrunnerAuditReport,
    pub review_queue: ShowrunnerReviewQueueReport,
}

#[derive(Clone, Debug, Serialize)]
pub struct ShowrunnerReviewEpisode {
    pub id: String,
    pub function: String,
    pub dramatic_question: String,
    pub narrator_distance: String,
    pub audience_revelation: String,
    pub knowledge_uses: Vec<KnowledgeUse>,
    pub internal_tone_beats: Vec<String>,
    pub transition_bridge: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub production_load: Option<ProductionLoad>,
    pub declares_season_finale_delivery: bool,
    pub ending_invitation: String,
    pub human_review_status: String,
    pub open_reviewers: Vec<String>,
    pub approved_reviewers: Vec<String>,
}

pub fn load(path: impl AsRef<Path>) -> Result<LoadedShowrunner> {
    let path = path.as_ref();
    let bytes = fs::read(path)
        .with_context(|| format!("failed to read showrunner plan {}", path.display()))?;
    let plan: ShowrunnerPlan = serde_yaml::from_slice(&bytes)
        .with_context(|| format!("failed to parse showrunner plan {}", path.display()))?;
    let base = path.parent().unwrap_or_else(|| Path::new("."));
    let series_path = base.join(&plan.series.path);
    let loaded_series = series::load(&series_path).with_context(|| {
        format!(
            "failed to load showrunner-bound series {}",
            series_path.display()
        )
    })?;
    Ok(LoadedShowrunner {
        path: path.to_path_buf(),
        plan,
        series_path,
        series: loaded_series.manifest,
    })
}

pub fn validate(path: impl AsRef<Path>) -> Result<ShowrunnerValidationReport> {
    let loaded = load(path)?;
    validate_loaded(&loaded)
}

pub fn validate_loaded(loaded: &LoadedShowrunner) -> Result<ShowrunnerValidationReport> {
    let plan = &loaded.plan;
    require_eq("showrunner schema", &plan.schema, SHOWRUNNER_SCHEMA)?;
    require("showrunner_id", &plan.showrunner_id)?;
    require("engine.promise", &plan.engine.promise)?;
    require(
        "audience_contract.assumed_knowledge",
        &plan.audience_contract.assumed_knowledge,
    )?;
    if plan.coverage != "full" && plan.coverage != "partial" {
        bail!("showrunner coverage must be full or partial");
    }
    if loaded.series.schema != series::SERIES_SCHEMA {
        bail!(
            "bound series schema must be {}, got {}",
            series::SERIES_SCHEMA,
            loaded.series.schema
        );
    }
    let actual_hash = production::sha256_path(&loaded.series_path)?;
    if actual_hash != plan.series.sha256 {
        bail!("showrunner bound series hash mismatch");
    }
    if !plan.series.series_id.is_empty() && plan.series.series_id != loaded.series.series_id {
        bail!("showrunner bound series id mismatch");
    }

    validate_vocabularies(&plan.vocabularies)?;
    validate_policies(&plan.policies, &plan.vocabularies)?;

    let series_seasons = loaded
        .series
        .seasons
        .iter()
        .map(|season| season.id.clone())
        .collect::<Vec<_>>();
    let series_episodes = loaded
        .series
        .seasons
        .iter()
        .flat_map(|season| season.episodes.iter().map(|episode| episode.id.clone()))
        .collect::<Vec<_>>();
    let episode_positions = series_episodes
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect::<HashMap<_, _>>();
    let episode_seasons = loaded
        .series
        .seasons
        .iter()
        .flat_map(|season| {
            season
                .episodes
                .iter()
                .map(move |episode| (episode.id.clone(), season.id.clone()))
        })
        .collect::<HashMap<_, _>>();

    let control_seasons = plan
        .seasons
        .iter()
        .map(|season| season.id.clone())
        .collect::<Vec<_>>();
    let control_episodes = plan
        .episodes
        .iter()
        .map(|episode| episode.id.clone())
        .collect::<Vec<_>>();
    require_unique("showrunner season", &control_seasons)?;
    require_unique("showrunner episode", &control_episodes)?;
    if plan.coverage == "full" {
        if control_seasons != series_seasons {
            bail!("full showrunner season coverage/order differs from bound series");
        }
        if control_episodes != series_episodes {
            bail!("full showrunner episode coverage/order differs from bound series");
        }
    } else {
        require_ordered_subset("season", &control_seasons, &series_seasons)?;
        require_ordered_subset("episode", &control_episodes, &series_episodes)?;
    }

    let function_families = values(&plan.vocabularies.function_families);
    let narrator_distances = values(&plan.vocabularies.narrator_distances);
    let primary_tones = values(&plan.vocabularies.primary_tones);
    let ending_tones = values(&plan.vocabularies.ending_tones);
    let ending_modes = values(&plan.vocabularies.ending_modes);
    let production_scales = values(&plan.vocabularies.production_scales);
    let knowledge_layers = values(&plan.vocabularies.knowledge_layers);
    let allowed_breaks = values(&plan.engine.allowed_breaks);

    for season in &plan.seasons {
        for (label, text) in [
            ("action", &season.action),
            ("audience_job", &season.audience_job),
            ("thematic_proposition", &season.thematic_proposition),
            ("thematic_counterforce", &season.thematic_counterforce),
            ("finale_delivery", &season.finale_delivery),
        ] {
            require(&format!("season {} {label}", season.id), text)?;
        }
        for family in &season.required_function_families {
            require_declared("function family", family, &function_families)?;
        }
        for family in [
            season.opening_function_family.as_ref(),
            season.closing_function_family.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            require_declared(
                "season boundary function family",
                family,
                &function_families,
            )?;
        }
    }
    for episode in &plan.episodes {
        if !episode_positions.contains_key(&episode.id) {
            bail!(
                "showrunner episode {} is absent from bound series",
                episode.id
            );
        }
        for (label, text) in [
            ("function", &episode.function),
            ("dramatic_question", &episode.dramatic_question),
            ("pressure", &episode.pressure),
            ("consequential_action", &episode.consequential_action),
            ("audience_revelation", &episode.audience_revelation),
            (
                "ending_invitation.statement",
                &episode.ending_invitation.statement,
            ),
        ] {
            require(&format!("episode {} {label}", episode.id), text)?;
        }
        require_declared(
            "function family",
            &episode.function_family,
            &function_families,
        )?;
        require_declared(
            "narrator distance",
            &episode.narrator_distance,
            &narrator_distances,
        )?;
        require_declared("primary tone", &episode.tone.primary, &primary_tones)?;
        require_declared("ending tone", &episode.tone.ending, &ending_tones)?;
        require_declared(
            "ending mode",
            &episode.ending_invitation.mode,
            &ending_modes,
        )?;
        require_declared(
            "production scale",
            &episode.production_scale,
            &production_scales,
        )?;
        if !(1..=5).contains(&episode.tone.intensity) {
            bail!(
                "episode {} tone intensity must be between 1 and 5",
                episode.id
            );
        }
        if let Some(load) = &episode.production_load
            && !(1..=5).contains(&load.complexity)
        {
            bail!(
                "episode {} production load complexity must be between 1 and 5",
                episode.id
            );
        }
        if let Some(engine_break) = &episode.engine_break
            && !allowed_breaks.contains(engine_break)
        {
            // This remains an audit finding rather than an invalid creative plan.
        }
        for knowledge in &episode.knowledge_uses {
            require_declared(
                "knowledge layer",
                &knowledge.knowledge_layer,
                &knowledge_layers,
            )?;
            if knowledge.handoff_declared && knowledge.handoff.trim().is_empty() {
                bail!(
                    "episode {} knowledge use {} declares a handoff without describing it",
                    episode.id,
                    knowledge.step_id
                );
            }
        }
        for beat in &episode.internal_tone_beats {
            require(&format!("episode {} internal tone beat", episode.id), beat)?;
        }
    }

    let mut thread_ids = HashSet::new();
    let mut step_index: HashMap<String, (&str, usize, usize, bool)> = HashMap::new();
    let mut revelation_steps = 0usize;
    for thread in &plan.revelation_threads {
        if !thread_ids.insert(thread.id.clone()) {
            bail!("duplicate revelation thread id {}", thread.id);
        }
        require("revelation thread id", &thread.id)?;
        if thread.steps.is_empty() {
            bail!("revelation thread {} has no steps", thread.id);
        }
        let mut prior_end_position = None;
        for step in &thread.steps {
            require("revelation step id", &step.id)?;
            require("revelation step state", &step.state)?;
            let position = *episode_positions.get(&step.episode_id).with_context(|| {
                format!(
                    "revelation step {} references absent episode {}",
                    step.id, step.episode_id
                )
            })?;
            let end_position = match &step.through_episode_id {
                Some(through_episode_id) => {
                    *episode_positions.get(through_episode_id).with_context(|| {
                        format!(
                            "revelation step {} references absent through episode {}",
                            step.id, through_episode_id
                        )
                    })?
                }
                None => position,
            };
            if end_position < position {
                bail!(
                    "revelation step {} through episode precedes its opening episode",
                    step.id
                );
            }
            if prior_end_position.is_some_and(|prior| position <= prior) {
                bail!(
                    "revelation thread {} steps overlap or are out of episode order",
                    thread.id
                );
            }
            prior_end_position = Some(end_position);
            if step_index
                .insert(
                    step.id.clone(),
                    (&thread.id, position, end_position, step.closes_thread),
                )
                .is_some()
            {
                bail!("duplicate revelation step id {}", step.id);
            }
            revelation_steps += 1;
        }
    }
    for thread in &plan.revelation_threads {
        for step in &thread.steps {
            let (_, position, _, _) = step_index[&step.id];
            for prerequisite in &step.prerequisites {
                let (_, _, prerequisite_end_position, _) =
                    step_index.get(prerequisite).with_context(|| {
                        format!(
                            "revelation step {} has unknown prerequisite {}",
                            step.id, prerequisite
                        )
                    })?;
                if *prerequisite_end_position >= position {
                    bail!(
                        "revelation step {} prerequisite {} is not earlier",
                        step.id,
                        prerequisite
                    );
                }
            }
        }
    }
    for episode in &plan.episodes {
        let position = episode_positions[&episode.id];
        for step_id in &episode.revelations {
            let (_, step_position, _, _) = step_index.get(step_id).with_context(|| {
                format!(
                    "episode {} opens unknown revelation step {step_id}",
                    episode.id
                )
            })?;
            if *step_position != position {
                bail!(
                    "episode {} revelation step {} belongs to a different episode",
                    episode.id,
                    step_id
                );
            }
        }
        for knowledge in &episode.knowledge_uses {
            let (_, step_position, _, _) =
                step_index.get(&knowledge.step_id).with_context(|| {
                    format!(
                        "episode {} uses unknown revelation step {}",
                        episode.id, knowledge.step_id
                    )
                })?;
            if *step_position > position {
                bail!(
                    "episode {} uses revelation step {} before it is opened",
                    episode.id,
                    knowledge.step_id
                );
            }
        }
        if !episode_seasons.contains_key(&episode.id) {
            bail!("episode {} has no bound season", episode.id);
        }
    }

    let mut warnings = Vec::new();
    if required_reviewers(loaded).is_empty() {
        warnings.push("showrunner plan declares no required human reviewers".to_string());
    }
    if plan.revelation_threads.is_empty() {
        warnings.push("showrunner plan declares no revelation threads".to_string());
    }
    Ok(ShowrunnerValidationReport {
        schema: SHOWRUNNER_SCHEMA.to_string(),
        showrunner_id: plan.showrunner_id.clone(),
        series_id: loaded.series.series_id.clone(),
        series_sha256: actual_hash,
        seasons: plan.seasons.len(),
        episodes: plan.episodes.len(),
        revelation_threads: plan.revelation_threads.len(),
        revelation_steps,
        full_coverage: plan.coverage == "full",
        warnings,
    })
}

pub fn rhythm_audit(path: impl AsRef<Path>) -> Result<RhythmAuditReport> {
    let loaded = load(path)?;
    rhythm_audit_loaded(&loaded)
}

pub fn rhythm_audit_loaded(loaded: &LoadedShowrunner) -> Result<RhythmAuditReport> {
    validate_loaded(loaded)?;
    let plan = &loaded.plan;
    let mut findings = Vec::new();
    let mut function_family_counts = BTreeMap::new();
    let mut primary_tone_counts = BTreeMap::new();
    let mut production_scale_counts = BTreeMap::new();
    let mut internal_tone_turns = 0usize;
    let mut production_load_estimated_episodes = 0usize;
    for episode in &plan.episodes {
        increment(&mut function_family_counts, &episode.function_family);
        increment(&mut primary_tone_counts, &episode.tone.primary);
        increment(&mut production_scale_counts, &episode.production_scale);
        internal_tone_turns += episode.internal_tone_beats.len().saturating_sub(1);
        if episode.production_load.is_some() {
            production_load_estimated_episodes += 1;
        }
        for pair in episode.internal_tone_beats.windows(2) {
            if pair[0] == pair[1] {
                findings.push(AuditFinding {
                    code: "repeated-internal-tone-beat".to_string(),
                    severity: "advisory".to_string(),
                    season_id: None,
                    episode_ids: vec![episode.id.clone()],
                    message: format!(
                        "episode {} repeats internal tone beat {} without a declared change",
                        episode.id, pair[0]
                    ),
                });
            }
        }
    }

    let controls = plan
        .episodes
        .iter()
        .map(|episode| (episode.id.as_str(), episode))
        .collect::<HashMap<_, _>>();
    for season in &loaded.series.seasons {
        let episodes = season
            .episodes
            .iter()
            .filter_map(|episode| controls.get(episode.id.as_str()).copied())
            .collect::<Vec<_>>();
        if episodes.is_empty() {
            continue;
        }
        if let Some(maximum) = plan.policies.max_adjacent_same_function_family {
            findings.extend(run_findings(
                &season.id,
                &episodes,
                maximum,
                "repeated-function-family",
                "function family",
                |episode| episode.function_family.clone(),
            ));
        }
        if let Some(maximum) = plan.policies.max_adjacent_same_primary_tone {
            findings.extend(run_findings(
                &season.id,
                &episodes,
                maximum,
                "repeated-primary-tone",
                "primary tone",
                |episode| episode.tone.primary.clone(),
            ));
        }
        if let (Some(maximum_intensity), Some(maximum_run)) = (
            plan.policies.maximum_intensity,
            plan.policies.max_adjacent_maximum_intensity,
        ) {
            findings.extend(predicate_run_findings(
                &season.id,
                &episodes,
                maximum_run,
                "maximum-intensity-cluster",
                &format!("intensity {maximum_intensity}"),
                |episode| episode.tone.intensity == maximum_intensity,
            ));
        }
        if let (Some(threshold), Some(maximum_run)) = (
            plan.policies.high_production_load_threshold,
            plan.policies.max_adjacent_high_production_load,
        ) {
            findings.extend(predicate_run_findings(
                &season.id,
                &episodes,
                maximum_run,
                "high-production-load-cluster",
                &format!("production load complexity >= {threshold}"),
                |episode| {
                    episode
                        .production_load
                        .as_ref()
                        .is_some_and(|load| load.complexity >= threshold)
                },
            ));
        }
        for (scale, maximum) in &plan.policies.max_adjacent_scales {
            findings.extend(predicate_run_findings(
                &season.id,
                &episodes,
                *maximum,
                "production-scale-cluster",
                &format!("production scale {scale}"),
                |episode| episode.production_scale == *scale,
            ));
        }
        if let Some(season_control) = plan.seasons.iter().find(|value| value.id == season.id) {
            let present = episodes
                .iter()
                .map(|episode| episode.function_family.as_str())
                .collect::<HashSet<_>>();
            for required in &season_control.required_function_families {
                if !present.contains(required.as_str()) {
                    findings.push(AuditFinding {
                        code: "missing-required-function".to_string(),
                        severity: "advisory".to_string(),
                        season_id: Some(season.id.clone()),
                        episode_ids: Vec::new(),
                        message: format!(
                            "season {} lacks declared required function family {}",
                            season.id, required
                        ),
                    });
                }
            }
            if let Some(expected) = &season_control.opening_function_family
                && episodes[0].function_family != *expected
            {
                findings.push(AuditFinding {
                    code: "opening-function-mismatch".to_string(),
                    severity: "advisory".to_string(),
                    season_id: Some(season.id.clone()),
                    episode_ids: vec![episodes[0].id.clone()],
                    message: format!(
                        "season {} opens with {} rather than declared {}",
                        season.id, episodes[0].function_family, expected
                    ),
                });
            }
            let finale = episodes.last().expect("non-empty season controls");
            if let Some(expected) = &season_control.closing_function_family
                && finale.function_family != *expected
            {
                findings.push(AuditFinding {
                    code: "closing-function-mismatch".to_string(),
                    severity: "advisory".to_string(),
                    season_id: Some(season.id.clone()),
                    episode_ids: vec![finale.id.clone()],
                    message: format!(
                        "season {} closes with {} rather than declared {}",
                        season.id, finale.function_family, expected
                    ),
                });
            }
            if !finale.delivers_season_finale {
                findings.push(AuditFinding {
                    code: "finale-delivery-unconfirmed".to_string(),
                    severity: "advisory".to_string(),
                    season_id: Some(season.id.clone()),
                    episode_ids: vec![finale.id.clone()],
                    message: format!(
                        "{} does not declare delivery of the season audience job: {}",
                        finale.id, season_control.finale_delivery
                    ),
                });
            }
        }
        for episode in &episodes {
            if let Some(engine_break) = &episode.engine_break
                && !plan.engine.allowed_breaks.contains(engine_break)
            {
                findings.push(AuditFinding {
                    code: "undeclared-engine-break".to_string(),
                    severity: "advisory".to_string(),
                    season_id: Some(season.id.clone()),
                    episode_ids: vec![episode.id.clone()],
                    message: format!(
                        "episode {} uses undeclared engine break {}",
                        episode.id, engine_break
                    ),
                });
            }
        }
        for pair in episodes.windows(2) {
            let from = &pair[0].tone.ending;
            let to = &pair[1].tone.primary;
            let abrupt = plan
                .policies
                .abrupt_tone_transitions
                .iter()
                .any(|transition| transition.from == *from && transition.to == *to);
            if abrupt && pair[1].tone.bridge.trim().is_empty() {
                findings.push(AuditFinding {
                    code: "unbridged-tone-transition".to_string(),
                    severity: "advisory".to_string(),
                    season_id: Some(season.id.clone()),
                    episode_ids: vec![pair[0].id.clone(), pair[1].id.clone()],
                    message: format!(
                        "declared abrupt transition {from} -> {to} has no authored bridge"
                    ),
                });
            }
        }
    }

    Ok(RhythmAuditReport {
        schema: "reel.showrunner-rhythm-audit.v0.1".to_string(),
        showrunner_id: plan.showrunner_id.clone(),
        series_id: loaded.series.series_id.clone(),
        episodes: plan.episodes.len(),
        function_family_counts,
        primary_tone_counts,
        production_scale_counts,
        internal_tone_turns,
        production_load_estimated_episodes,
        findings,
    })
}

pub fn revelation_map(path: impl AsRef<Path>) -> Result<RevelationMapReport> {
    let loaded = load(path)?;
    revelation_map_loaded(&loaded)
}

pub fn revelation_map_loaded(loaded: &LoadedShowrunner) -> Result<RevelationMapReport> {
    validate_loaded(loaded)?;
    let plan = &loaded.plan;
    let episode_positions = loaded
        .series
        .seasons
        .iter()
        .flat_map(|season| season.episodes.iter())
        .enumerate()
        .map(|(index, episode)| (episode.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let immediate = values(&plan.audience_contract.immediate_narrator_distances);
    let later_layers = values(&plan.audience_contract.later_knowledge_layers);
    let step_lookup = plan
        .revelation_threads
        .iter()
        .flat_map(|thread| {
            thread
                .steps
                .iter()
                .map(move |step| (step.id.as_str(), step))
        })
        .collect::<HashMap<_, _>>();
    let mut findings = Vec::new();

    for thread in &plan.revelation_threads {
        for pair in thread.steps.windows(2) {
            if pair[0].state == pair[1].state {
                findings.push(AuditFinding {
                    code: "repeated-revelation-state".to_string(),
                    severity: "advisory".to_string(),
                    season_id: None,
                    episode_ids: vec![pair[0].episode_id.clone(), pair[1].episode_id.clone()],
                    message: format!(
                        "thread {} repeats the same audience state without declared change",
                        thread.id
                    ),
                });
            }
        }
        if thread.remain_open && thread.steps.iter().any(|step| step.closes_thread) {
            findings.push(AuditFinding {
                code: "open-thread-closed".to_string(),
                severity: "advisory".to_string(),
                season_id: None,
                episode_ids: thread
                    .steps
                    .iter()
                    .filter(|step| step.closes_thread)
                    .map(|step| step.episode_id.clone())
                    .collect(),
                message: format!(
                    "thread {} is marked remain-open but contains closure",
                    thread.id
                ),
            });
        }
        if !thread.remain_open && !thread.steps.last().is_some_and(|step| step.closes_thread) {
            findings.push(AuditFinding {
                code: "thread-without-closure".to_string(),
                severity: "advisory".to_string(),
                season_id: None,
                episode_ids: thread
                    .steps
                    .last()
                    .map(|step| vec![step.episode_id.clone()])
                    .unwrap_or_default(),
                message: format!(
                    "thread {} neither remains open nor declares closure",
                    thread.id
                ),
            });
        }
        if !thread.allow_dormancy
            && let Some(maximum) = thread.max_dormant_episodes
        {
            for pair in thread.steps.windows(2) {
                let prior_end_id = pair[0]
                    .through_episode_id
                    .as_deref()
                    .unwrap_or(pair[0].episode_id.as_str());
                let gap = episode_positions[pair[1].episode_id.as_str()]
                    .saturating_sub(episode_positions[prior_end_id])
                    .saturating_sub(1);
                if gap > maximum {
                    findings.push(AuditFinding {
                        code: "revelation-thread-dormancy".to_string(),
                        severity: "advisory".to_string(),
                        season_id: None,
                        episode_ids: vec![pair[0].episode_id.clone(), pair[1].episode_id.clone()],
                        message: format!(
                            "thread {} is dormant for {gap} episodes; declared maximum is {maximum}",
                            thread.id
                        ),
                    });
                }
            }
        }
    }
    if plan.audience_contract.no_foreknowledge {
        for episode in &plan.episodes {
            if !immediate.contains(&episode.narrator_distance) {
                continue;
            }
            for knowledge in &episode.knowledge_uses {
                if later_layers.contains(&knowledge.knowledge_layer)
                    && !knowledge.handoff_declared
                    && knowledge.handoff.trim().is_empty()
                {
                    let step = step_lookup[knowledge.step_id.as_str()];
                    findings.push(AuditFinding {
                        code: "unmarked-later-knowledge".to_string(),
                        severity: "advisory".to_string(),
                        season_id: None,
                        episode_ids: vec![episode.id.clone()],
                        message: format!(
                            "{} uses later layer {} for step {} without a declared handoff",
                            episode.id, knowledge.knowledge_layer, step.id
                        ),
                    });
                }
            }
        }
    }

    Ok(RevelationMapReport {
        schema: "reel.showrunner-revelation-map.v0.1".to_string(),
        showrunner_id: plan.showrunner_id.clone(),
        series_id: loaded.series.series_id.clone(),
        threads: plan
            .revelation_threads
            .iter()
            .map(|thread| RevelationThreadReport {
                id: thread.id.clone(),
                remain_open: thread.remain_open,
                steps: thread.steps.clone(),
            })
            .collect(),
        findings,
    })
}

pub fn audit(path: impl AsRef<Path>) -> Result<ShowrunnerAuditReport> {
    let loaded = load(path)?;
    let validation = validate_loaded(&loaded)?;
    let rhythm = rhythm_audit_loaded(&loaded)?;
    let revelation = revelation_map_loaded(&loaded)?;
    let finding_count = rhythm.findings.len() + revelation.findings.len();
    Ok(ShowrunnerAuditReport {
        schema: "reel.showrunner-audit.v0.1".to_string(),
        validation,
        rhythm,
        revelation,
        finding_count,
    })
}

pub fn review_pack(path: impl AsRef<Path>) -> Result<ShowrunnerReviewPackReport> {
    let path = path.as_ref();
    Ok(ShowrunnerReviewPackReport {
        schema: "reel.showrunner-review-pack.v0.1".to_string(),
        audit: audit(path)?,
        review_queue: review_queue(path)?,
    })
}

pub fn validation_markdown(report: &ShowrunnerValidationReport) -> String {
    let mut text = String::new();
    writeln!(text, "# Showrunner validation\n").unwrap();
    writeln!(text, "- Showrunner: `{}`", md(&report.showrunner_id)).unwrap();
    writeln!(text, "- Series: `{}`", md(&report.series_id)).unwrap();
    writeln!(text, "- Bound SHA-256: `{}`", report.series_sha256).unwrap();
    writeln!(
        text,
        "- Coverage: {}",
        if report.full_coverage {
            "full"
        } else {
            "partial"
        }
    )
    .unwrap();
    writeln!(
        text,
        "- Seasons / episodes: {} / {}",
        report.seasons, report.episodes
    )
    .unwrap();
    writeln!(
        text,
        "- Revelation threads / steps: {} / {}",
        report.revelation_threads, report.revelation_steps
    )
    .unwrap();
    writeln!(text, "- Warnings: {}", report.warnings.len()).unwrap();
    for warning in &report.warnings {
        writeln!(text, "  - {}", md(warning)).unwrap();
    }
    text
}

pub fn rhythm_markdown(report: &RhythmAuditReport) -> String {
    let mut text = String::new();
    writeln!(text, "# Showrunner rhythm audit\n").unwrap();
    writeln!(text, "- Episodes: {}", report.episodes).unwrap();
    writeln!(
        text,
        "- Authored internal tone turns: {}",
        report.internal_tone_turns
    )
    .unwrap();
    writeln!(
        text,
        "- Episodes with production-load estimates: {}",
        report.production_load_estimated_episodes
    )
    .unwrap();
    writeln!(text, "- Findings: {}\n", report.findings.len()).unwrap();
    append_counts(
        &mut text,
        "Function families",
        &report.function_family_counts,
    );
    append_counts(&mut text, "Primary tones", &report.primary_tone_counts);
    append_counts(
        &mut text,
        "Production scales",
        &report.production_scale_counts,
    );
    append_findings(&mut text, "Findings", &report.findings);
    text
}

pub fn revelation_markdown(report: &RevelationMapReport) -> String {
    let mut text = String::new();
    writeln!(text, "# Showrunner revelation map\n").unwrap();
    writeln!(text, "- Threads: {}", report.threads.len()).unwrap();
    writeln!(text, "- Findings: {}\n", report.findings.len()).unwrap();
    for thread in &report.threads {
        writeln!(text, "## {}\n", md(&thread.id)).unwrap();
        writeln!(text, "Remain open: {}\n", thread.remain_open).unwrap();
        writeln!(text, "| Episode span | Step | Audience state |").unwrap();
        writeln!(text, "|---|---|---|").unwrap();
        for step in &thread.steps {
            let span = step.through_episode_id.as_ref().map_or_else(
                || step.episode_id.clone(),
                |through| format!("{}–{through}", step.episode_id),
            );
            writeln!(
                text,
                "| {} | {} | {} |",
                md(&span),
                md(&step.id),
                md(&step.state)
            )
            .unwrap();
        }
        writeln!(text).unwrap();
    }
    append_findings(&mut text, "Findings", &report.findings);
    text
}

pub fn audit_markdown(report: &ShowrunnerAuditReport) -> String {
    let mut text = String::new();
    writeln!(text, "# Showrunner audit\n").unwrap();
    writeln!(
        text,
        "- Showrunner: `{}`",
        md(&report.validation.showrunner_id)
    )
    .unwrap();
    writeln!(text, "- Series: `{}`", md(&report.validation.series_id)).unwrap();
    writeln!(
        text,
        "- Seasons / episodes: {} / {}",
        report.validation.seasons, report.validation.episodes
    )
    .unwrap();
    writeln!(
        text,
        "- Revelation threads / steps: {} / {}",
        report.validation.revelation_threads, report.validation.revelation_steps
    )
    .unwrap();
    writeln!(
        text,
        "- Authored internal tone turns: {}",
        report.rhythm.internal_tone_turns
    )
    .unwrap();
    writeln!(
        text,
        "- Production-load estimates: {} of {} episodes",
        report.rhythm.production_load_estimated_episodes, report.rhythm.episodes
    )
    .unwrap();
    writeln!(text, "- Machine findings: {}\n", report.finding_count).unwrap();
    append_findings(&mut text, "Rhythm findings", &report.rhythm.findings);
    append_findings(
        &mut text,
        "Revelation and viewpoint findings",
        &report.revelation.findings,
    );
    text
}

pub fn review_queue_markdown(report: &ShowrunnerReviewQueueReport) -> String {
    let mut text = String::new();
    writeln!(text, "# Showrunner human review queue\n").unwrap();
    writeln!(
        text,
        "- Required reviewers: {}",
        report
            .required_reviewers
            .iter()
            .map(|value| md(value))
            .collect::<Vec<_>>()
            .join(", ")
    )
    .unwrap();
    writeln!(text, "- Open episodes: {}", report.open.len()).unwrap();
    writeln!(text, "- Reviewed episodes: {}\n", report.reviewed.len()).unwrap();
    writeln!(
        text,
        "| Episode | Function | Status | Open reviewers | Dramatic question |"
    )
    .unwrap();
    writeln!(text, "|---|---|---|---|---|").unwrap();
    for item in report.open.iter().chain(report.reviewed.iter()) {
        writeln!(
            text,
            "| {} | {} | {} | {} | {} |",
            md(&item.id),
            md(&item.function),
            md(&item.human_review_status),
            item.open_reviewers
                .iter()
                .map(|value| md(value))
                .collect::<Vec<_>>()
                .join(", "),
            md(&item.dramatic_question)
        )
        .unwrap();
    }
    writeln!(text, "\n## Episode controls\n").unwrap();
    for item in report.open.iter().chain(report.reviewed.iter()) {
        writeln!(text, "### {} — {}\n", md(&item.id), md(&item.function)).unwrap();
        writeln!(text, "- Dramatic question: {}", md(&item.dramatic_question)).unwrap();
        writeln!(
            text,
            "- Narrator distance: `{}`",
            md(&item.narrator_distance)
        )
        .unwrap();
        writeln!(
            text,
            "- Audience revelation: {}",
            md(&item.audience_revelation)
        )
        .unwrap();
        if !item.internal_tone_beats.is_empty() {
            writeln!(
                text,
                "- Internal tone movement: {}",
                item.internal_tone_beats
                    .iter()
                    .map(|value| md(value))
                    .collect::<Vec<_>>()
                    .join(" → ")
            )
            .unwrap();
        }
        if !item.transition_bridge.trim().is_empty() {
            writeln!(
                text,
                "- Incoming transition bridge: {}",
                md(&item.transition_bridge)
            )
            .unwrap();
        }
        for knowledge in &item.knowledge_uses {
            writeln!(
                text,
                "- Later-knowledge handoff (`{}` / `{}`): {}",
                md(&knowledge.step_id),
                md(&knowledge.knowledge_layer),
                md(&knowledge.handoff)
            )
            .unwrap();
        }
        if let Some(load) = &item.production_load {
            writeln!(
                text,
                "- Production estimate: complexity {}/5; {} locations; {} speaking roles; crowd {}; {} new / {} reusable assets",
                load.complexity,
                load.locations,
                load.speaking_roles,
                if load.crowd { "yes" } else { "no" },
                load.new_assets.len(),
                load.reusable_assets.len()
            )
            .unwrap();
        }
        if item.declares_season_finale_delivery {
            writeln!(
                text,
                "- Season-finale delivery: declared; human script/animatic proof still required."
            )
            .unwrap();
        }
        writeln!(
            text,
            "- Ending invitation: {}\n",
            md(&item.ending_invitation)
        )
        .unwrap();
    }
    text
}

pub fn review_pack_markdown(report: &ShowrunnerReviewPackReport) -> String {
    let mut text = audit_markdown(&report.audit);
    writeln!(text, "\n---\n").unwrap();
    text.push_str(&review_queue_markdown(&report.review_queue));
    text
}

fn append_counts(text: &mut String, title: &str, counts: &BTreeMap<String, usize>) {
    writeln!(text, "## {title}\n").unwrap();
    writeln!(text, "| Value | Count |").unwrap();
    writeln!(text, "|---|---:|").unwrap();
    for (value, count) in counts {
        writeln!(text, "| {} | {} |", md(value), count).unwrap();
    }
    writeln!(text).unwrap();
}

fn append_findings(text: &mut String, title: &str, findings: &[AuditFinding]) {
    if findings.is_empty() {
        writeln!(text, "## {title}\n\nNone.\n").unwrap();
        return;
    }
    writeln!(text, "## {title}\n").unwrap();
    writeln!(text, "| Code | Severity | Episodes | Finding |").unwrap();
    writeln!(text, "|---|---|---|---|").unwrap();
    for finding in findings {
        writeln!(
            text,
            "| {} | {} | {} | {} |",
            md(&finding.code),
            md(&finding.severity),
            finding
                .episode_ids
                .iter()
                .map(|value| md(value))
                .collect::<Vec<_>>()
                .join(", "),
            md(&finding.message)
        )
        .unwrap();
    }
    writeln!(text).unwrap();
}

fn md(value: &str) -> String {
    value.replace('|', "\\|").replace(['\r', '\n'], " ")
}

pub fn review_queue(path: impl AsRef<Path>) -> Result<ShowrunnerReviewQueueReport> {
    let loaded = load(path)?;
    validate_loaded(&loaded)?;
    let required_reviewers = required_reviewers(&loaded);
    let series_episodes = loaded
        .series
        .seasons
        .iter()
        .flat_map(|season| {
            season
                .episodes
                .iter()
                .map(|episode| (episode.id.as_str(), episode))
        })
        .collect::<HashMap<_, _>>();
    let mut open = Vec::new();
    let mut reviewed = Vec::new();
    for control in &loaded.plan.episodes {
        let episode = series_episodes[control.id.as_str()];
        let approved_reviewers = required_reviewers
            .iter()
            .filter(|reviewer| {
                episode
                    .findings
                    .iter()
                    .any(|finding| finding.reviewer == reviewer.as_str() && finding.approved)
            })
            .cloned()
            .collect::<Vec<_>>();
        let open_reviewers = required_reviewers
            .iter()
            .filter(|reviewer| !approved_reviewers.contains(reviewer))
            .cloned()
            .collect::<Vec<_>>();
        let item = ShowrunnerReviewEpisode {
            id: control.id.clone(),
            function: control.function.clone(),
            dramatic_question: control.dramatic_question.clone(),
            narrator_distance: control.narrator_distance.clone(),
            audience_revelation: control.audience_revelation.clone(),
            knowledge_uses: control.knowledge_uses.clone(),
            internal_tone_beats: control.internal_tone_beats.clone(),
            transition_bridge: control.tone.bridge.clone(),
            production_load: control.production_load.clone(),
            declares_season_finale_delivery: control.delivers_season_finale,
            ending_invitation: control.ending_invitation.statement.clone(),
            human_review_status: episode.human_review_status.clone(),
            open_reviewers: open_reviewers.clone(),
            approved_reviewers,
        };
        if open_reviewers.is_empty()
            && matches!(
                episode.human_review_status.as_str(),
                "approved" | "accepted"
            )
        {
            reviewed.push(item);
        } else {
            open.push(item);
        }
    }
    Ok(ShowrunnerReviewQueueReport {
        schema: "reel.showrunner-review-queue.v0.1".to_string(),
        showrunner_id: loaded.plan.showrunner_id,
        series_id: loaded.series.series_id,
        required_reviewers,
        open,
        reviewed,
    })
}

fn validate_vocabularies(vocabularies: &Vocabularies) -> Result<()> {
    for (label, entries) in [
        ("function_families", &vocabularies.function_families),
        ("narrator_distances", &vocabularies.narrator_distances),
        ("primary_tones", &vocabularies.primary_tones),
        ("ending_tones", &vocabularies.ending_tones),
        ("ending_modes", &vocabularies.ending_modes),
        ("production_scales", &vocabularies.production_scales),
        ("knowledge_layers", &vocabularies.knowledge_layers),
    ] {
        if entries.is_empty() {
            bail!("showrunner vocabulary {label} cannot be empty");
        }
        require_unique(label, entries)?;
        for entry in entries {
            require(&format!("{label} entry"), entry)?;
        }
    }
    Ok(())
}

fn validate_policies(policies: &AuditPolicies, vocabularies: &Vocabularies) -> Result<()> {
    for (label, value) in [
        (
            "max_adjacent_same_function_family",
            policies.max_adjacent_same_function_family,
        ),
        (
            "max_adjacent_same_primary_tone",
            policies.max_adjacent_same_primary_tone,
        ),
        (
            "max_adjacent_maximum_intensity",
            policies.max_adjacent_maximum_intensity,
        ),
        (
            "max_adjacent_high_production_load",
            policies.max_adjacent_high_production_load,
        ),
    ] {
        if value == Some(0) {
            bail!("showrunner policy {label} must be positive");
        }
    }
    if policies
        .maximum_intensity
        .is_some_and(|value| !(1..=5).contains(&value))
    {
        bail!("showrunner maximum_intensity must be between 1 and 5");
    }
    if policies
        .high_production_load_threshold
        .is_some_and(|value| !(1..=5).contains(&value))
    {
        bail!("showrunner high_production_load_threshold must be between 1 and 5");
    }
    if policies.high_production_load_threshold.is_some()
        != policies.max_adjacent_high_production_load.is_some()
    {
        bail!(
            "showrunner high production load threshold and adjacent maximum must be declared together"
        );
    }
    let scales = values(&vocabularies.production_scales);
    for (scale, maximum) in &policies.max_adjacent_scales {
        require_declared("production scale policy", scale, &scales)?;
        if *maximum == 0 {
            bail!("showrunner production scale maximum must be positive");
        }
    }
    let ending_tones = values(&vocabularies.ending_tones);
    let primary_tones = values(&vocabularies.primary_tones);
    for transition in &policies.abrupt_tone_transitions {
        require_declared(
            "abrupt transition ending tone",
            &transition.from,
            &ending_tones,
        )?;
        require_declared(
            "abrupt transition primary tone",
            &transition.to,
            &primary_tones,
        )?;
    }
    Ok(())
}

fn run_findings<F>(
    season_id: &str,
    episodes: &[&EpisodeControl],
    maximum: usize,
    code: &str,
    label: &str,
    value: F,
) -> Vec<AuditFinding>
where
    F: Fn(&EpisodeControl) -> String,
{
    let mut findings = Vec::new();
    let mut index = 0usize;
    while index < episodes.len() {
        let current = value(episodes[index]);
        let mut end = index + 1;
        while end < episodes.len() && value(episodes[end]) == current {
            end += 1;
        }
        if end - index > maximum {
            findings.push(AuditFinding {
                code: code.to_string(),
                severity: "advisory".to_string(),
                season_id: Some(season_id.to_string()),
                episode_ids: episodes[index..end]
                    .iter()
                    .map(|episode| episode.id.clone())
                    .collect(),
                message: format!(
                    "{label} {current} runs for {} episodes; declared maximum is {maximum}",
                    end - index
                ),
            });
        }
        index = end;
    }
    findings
}

fn predicate_run_findings<F>(
    season_id: &str,
    episodes: &[&EpisodeControl],
    maximum: usize,
    code: &str,
    label: &str,
    predicate: F,
) -> Vec<AuditFinding>
where
    F: Fn(&EpisodeControl) -> bool,
{
    let mut findings = Vec::new();
    let mut index = 0usize;
    while index < episodes.len() {
        if !predicate(episodes[index]) {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < episodes.len() && predicate(episodes[end]) {
            end += 1;
        }
        if end - index > maximum {
            findings.push(AuditFinding {
                code: code.to_string(),
                severity: "advisory".to_string(),
                season_id: Some(season_id.to_string()),
                episode_ids: episodes[index..end]
                    .iter()
                    .map(|episode| episode.id.clone())
                    .collect(),
                message: format!(
                    "{label} runs for {} episodes; declared maximum is {maximum}",
                    end - index
                ),
            });
        }
        index = end;
    }
    findings
}

fn require(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("showrunner {label} must not be empty");
    }
    Ok(())
}

fn require_eq(label: &str, actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("{label} must be {expected}, got {actual}");
    }
    Ok(())
}

fn values(entries: &[String]) -> BTreeSet<String> {
    entries.iter().cloned().collect()
}

fn require_declared(label: &str, value: &str, entries: &BTreeSet<String>) -> Result<()> {
    if !entries.contains(value) {
        bail!("showrunner {label} {value} is not declared in vocabularies");
    }
    Ok(())
}

fn require_unique(label: &str, values: &[String]) -> Result<()> {
    let mut seen = HashSet::new();
    for value in values {
        if !seen.insert(value) {
            bail!("duplicate {label} id/value {value}");
        }
    }
    Ok(())
}

fn require_ordered_subset(label: &str, subset: &[String], complete: &[String]) -> Result<()> {
    let positions = complete
        .iter()
        .enumerate()
        .map(|(index, value)| (value, index))
        .collect::<HashMap<_, _>>();
    let mut prior = None;
    for value in subset {
        let position = positions
            .get(value)
            .with_context(|| format!("partial showrunner {label} {value} is absent from series"))?;
        if prior.is_some_and(|prior| *position <= prior) {
            bail!("partial showrunner {label} order differs from series");
        }
        prior = Some(*position);
    }
    Ok(())
}

fn increment(values: &mut BTreeMap<String, usize>, key: &str) {
    *values.entry(key.to_string()).or_default() += 1;
}

fn required_reviewers(loaded: &LoadedShowrunner) -> Vec<String> {
    if !loaded.plan.reviewers.is_empty() {
        return loaded.plan.reviewers.clone();
    }
    loaded
        .series
        .seasons
        .iter()
        .flat_map(|season| season.episodes.iter())
        .flat_map(|episode| episode.findings.iter())
        .map(|finding| finding.reviewer.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
