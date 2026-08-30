use std::{collections::BTreeMap, fs, path::Path};

use reel::{
    production::{self, TimingStatus},
    series::{Episode, Season, SeriesDefaults, SeriesManifest, SeriesRange},
    showrunner::{
        self, AudienceContract, AuditPolicies, EndingInvitation, EpisodeControl, KnowledgeUse,
        ProductionLoad, RevelationStep, RevelationThread, SeasonControl, SeriesEngine,
        ShowrunnerPlan, ShowrunnerSeriesRef, ToneControl, Vocabularies,
    },
};
use tempfile::tempdir;

fn series_episode(id: &str, order: u32, unit: u64) -> Episode {
    Episode {
        id: id.to_string(),
        order,
        part: None,
        production_title: format!("Sanitized {id}"),
        manuscript_title: format!("Fixture source {unit}"),
        poem_ids: Vec::new(),
        source_ranges: vec![SeriesRange {
            start: unit,
            end: unit,
        }],
        omissions: Vec::new(),
        chronology_place: "sanitized place".to_string(),
        memory_mode: "fixture".to_string(),
        sensitivity: "fixture-only".to_string(),
        recurring_motifs: Vec::new(),
        continuity_entry: Vec::new(),
        continuity_exit: Vec::new(),
        runtime_plan: None,
        timing_status: TimingStatus::Untimed,
        human_review_status: "open".to_string(),
        raw_orientation_seconds: 0.0,
        measured_narration_seconds: 0.0,
        protected_pause_seconds: 0.0,
        scene_duration_seconds: 0.0,
        total_runtime_seconds: 0.0,
        release_ready: false,
        accepted_speakers: Vec::new(),
        findings: Vec::new(),
        dependencies: Vec::new(),
        children: Vec::new(),
        production_units: Vec::new(),
    }
}

fn write_series(root: &Path) -> (std::path::PathBuf, String) {
    let series = SeriesManifest {
        schema: reel::series::SERIES_SCHEMA.to_string(),
        series_id: "sanitized-six-episode-series".to_string(),
        title: "Sanitized six episode series".to_string(),
        canonical_source_start: 1,
        canonical_source_end: 6,
        defaults: SeriesDefaults::default(),
        seasons: vec![
            Season {
                id: "S1".to_string(),
                order: 1,
                title: "Opening".to_string(),
                runtime_plan: None,
                total_runtime_seconds: Some(0.0),
                episodes: (1..=3)
                    .map(|number| {
                        series_episode(&format!("S1E{number:02}"), number, u64::from(number))
                    })
                    .collect(),
            },
            Season {
                id: "S2".to_string(),
                order: 2,
                title: "Consequence".to_string(),
                runtime_plan: None,
                total_runtime_seconds: Some(0.0),
                episodes: (1..=3)
                    .map(|number| {
                        series_episode(&format!("S2E{number:02}"), number, 3 + u64::from(number))
                    })
                    .collect(),
            },
        ],
    };
    let path = root.join("series.yaml");
    fs::write(&path, serde_yaml::to_string(&series).unwrap()).unwrap();
    let hash = production::sha256_path(&path).unwrap();
    (path, hash)
}

fn episode_control(id: &str, family: &str, delivers: bool) -> EpisodeControl {
    EpisodeControl {
        id: id.to_string(),
        function: format!("{family} function"),
        function_family: family.to_string(),
        dramatic_question: "What changes?".to_string(),
        pressure: "A sanitized pressure acts.".to_string(),
        consequential_action: "A source-supported action changes the state.".to_string(),
        narrator_distance: "immediate".to_string(),
        audience_revelation: "The audience receives one bounded fact.".to_string(),
        revelations: Vec::new(),
        knowledge_uses: Vec::new(),
        internal_tone_beats: vec!["mystery".to_string(), "recognition".to_string()],
        tone: ToneControl {
            primary: "mystery".to_string(),
            ending: "recognition".to_string(),
            intensity: 3,
            bridge: String::new(),
        },
        ending_invitation: EndingInvitation {
            mode: "opened-world".to_string(),
            statement: "The changed state invites continued attention.".to_string(),
        },
        production_scale: "intimate".to_string(),
        production_load: None,
        engine_break: None,
        delivers_season_finale: delivers,
    }
}

fn write_showrunner(root: &Path, hash: String) -> std::path::PathBuf {
    let mut episodes = Vec::new();
    for season in 1..=2 {
        episodes.push(episode_control(&format!("S{season}E01"), "premiere", false));
        episodes.push(episode_control(&format!("S{season}E02"), "pressure", false));
        episodes.push(episode_control(&format!("S{season}E03"), "finale", true));
    }
    episodes[0].revelations.push("thread-open".to_string());
    episodes[1].revelations.push("thread-deepens".to_string());
    episodes[3].revelations.push("thread-turns".to_string());
    let plan = ShowrunnerPlan {
        schema: showrunner::SHOWRUNNER_SCHEMA.to_string(),
        showrunner_id: "sanitized-showrunner-v1".to_string(),
        title: "Sanitized showrunner fixture".to_string(),
        coverage: "full".to_string(),
        series: ShowrunnerSeriesRef {
            path: "series.yaml".to_string(),
            sha256: hash,
            series_id: "sanitized-six-episode-series".to_string(),
        },
        engine: SeriesEngine {
            promise: "A repeated threshold changes meaning.".to_string(),
            default_movements: vec!["threshold".to_string(), "afterimage".to_string()],
            allowed_breaks: vec!["compact-afterpiece".to_string()],
        },
        audience_contract: AudienceContract {
            assumed_knowledge: "No prior knowledge.".to_string(),
            no_foreknowledge: true,
            memory_layers: vec!["immediate".to_string(), "adult-afterlight".to_string()],
            immediate_narrator_distances: vec!["immediate".to_string()],
            later_knowledge_layers: vec!["adult-afterlight".to_string()],
        },
        vocabularies: Vocabularies {
            function_families: vec![
                "premiere".to_string(),
                "pressure".to_string(),
                "finale".to_string(),
            ],
            narrator_distances: vec!["immediate".to_string()],
            primary_tones: vec!["mystery".to_string()],
            ending_tones: vec!["recognition".to_string()],
            ending_modes: vec!["opened-world".to_string()],
            production_scales: vec!["intimate".to_string()],
            knowledge_layers: vec!["immediate".to_string(), "adult-afterlight".to_string()],
        },
        policies: AuditPolicies {
            max_adjacent_same_function_family: Some(2),
            max_adjacent_same_primary_tone: Some(2),
            maximum_intensity: Some(5),
            max_adjacent_maximum_intensity: Some(2),
            max_adjacent_scales: BTreeMap::from([("intimate".to_string(), 4)]),
            high_production_load_threshold: None,
            max_adjacent_high_production_load: None,
            abrupt_tone_transitions: Vec::new(),
        },
        seasons: (1..=2)
            .map(|season| SeasonControl {
                id: format!("S{season}"),
                action: "change".to_string(),
                audience_job: "Track one changing relationship.".to_string(),
                thematic_proposition: "Attention can create recognition.".to_string(),
                thematic_counterforce: "Recognition does not prove everything.".to_string(),
                finale_delivery: "The relationship reaches a changed state.".to_string(),
                required_function_families: vec!["premiere".to_string(), "finale".to_string()],
                opening_function_family: Some("premiere".to_string()),
                closing_function_family: Some("finale".to_string()),
            })
            .collect(),
        episodes,
        revelation_threads: vec![RevelationThread {
            id: "relationship".to_string(),
            remain_open: true,
            allow_dormancy: true,
            max_dormant_episodes: None,
            steps: vec![
                RevelationStep {
                    id: "thread-open".to_string(),
                    episode_id: "S1E01".to_string(),
                    through_episode_id: None,
                    state: "The relationship appears.".to_string(),
                    prerequisites: Vec::new(),
                    closes_thread: false,
                },
                RevelationStep {
                    id: "thread-deepens".to_string(),
                    episode_id: "S1E02".to_string(),
                    through_episode_id: None,
                    state: "The relationship gains consequence.".to_string(),
                    prerequisites: vec!["thread-open".to_string()],
                    closes_thread: false,
                },
                RevelationStep {
                    id: "thread-turns".to_string(),
                    episode_id: "S2E01".to_string(),
                    through_episode_id: None,
                    state: "The relationship changes again.".to_string(),
                    prerequisites: vec!["thread-deepens".to_string()],
                    closes_thread: false,
                },
            ],
        }],
        reviewers: vec!["author".to_string(), "producer".to_string()],
        extra: BTreeMap::new(),
    };
    let path = root.join("showrunner.yaml");
    fs::write(&path, serde_yaml::to_string(&plan).unwrap()).unwrap();
    path
}

fn load_plan(path: &Path) -> ShowrunnerPlan {
    serde_yaml::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn save_plan(path: &Path, plan: &ShowrunnerPlan) {
    fs::write(path, serde_yaml::to_string(plan).unwrap()).unwrap();
}

#[test]
fn validates_and_audits_a_sanitized_two_season_six_episode_plan() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let validation = showrunner::validate(&path).unwrap();
    assert_eq!(validation.seasons, 2);
    assert_eq!(validation.episodes, 6);
    assert_eq!(validation.revelation_steps, 3);
    assert!(validation.full_coverage);

    let audit = showrunner::audit(&path).unwrap();
    assert_eq!(audit.rhythm.episodes, 6);
    assert!(
        audit
            .rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "repeated-primary-tone")
    );
    assert!(
        !audit
            .rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "undeclared-engine-break")
    );
    let first = serde_json::to_string(&audit).unwrap();
    let second = serde_json::to_string(&showrunner::audit(&path).unwrap()).unwrap();
    assert_eq!(first, second);
    assert!(!first.contains(&temp.path().to_string_lossy().to_string()));
}

#[test]
fn rejects_stale_hash_and_reveal_before_prerequisite() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let mut plan = load_plan(&path);
    plan.series.sha256 = "0".repeat(64);
    save_plan(&path, &plan);
    assert!(
        showrunner::validate(&path)
            .unwrap_err()
            .to_string()
            .contains("hash mismatch")
    );

    let actual_hash = production::sha256_path(temp.path().join("series.yaml")).unwrap();
    plan.series.sha256 = actual_hash;
    plan.revelation_threads[0].steps[1].prerequisites = vec!["thread-turns".to_string()];
    save_plan(&path, &plan);
    assert!(
        showrunner::validate(&path)
            .unwrap_err()
            .to_string()
            .contains("is not earlier")
    );
}

#[test]
fn detects_future_reveal_use_and_unmarked_later_knowledge() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let mut plan = load_plan(&path);
    plan.episodes[0].knowledge_uses.push(KnowledgeUse {
        step_id: "thread-turns".to_string(),
        knowledge_layer: "adult-afterlight".to_string(),
        handoff_declared: false,
        handoff: String::new(),
    });
    save_plan(&path, &plan);
    assert!(
        showrunner::validate(&path)
            .unwrap_err()
            .to_string()
            .contains("before it is opened")
    );

    plan.episodes[0].knowledge_uses[0].step_id = "thread-open".to_string();
    save_plan(&path, &plan);
    let map = showrunner::revelation_map(&path).unwrap();
    assert!(
        map.findings
            .iter()
            .any(|finding| finding.code == "unmarked-later-knowledge")
    );
}

#[test]
fn renders_human_readable_markdown_and_tracks_internal_turns() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let audit = showrunner::audit(&path).unwrap();
    assert_eq!(audit.rhythm.internal_tone_turns, 6);
    let pack = showrunner::review_pack(&path).unwrap();
    let markdown = showrunner::review_pack_markdown(&pack);
    assert!(markdown.starts_with("# Showrunner audit"));
    assert!(markdown.contains("# Showrunner human review queue"));
    assert!(markdown.contains("Internal tone movement: mystery → recognition"));
    assert!(!markdown.trim_start().starts_with('{'));
}

#[test]
fn reports_finale_mismatch_but_keeps_creative_plan_valid() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let mut plan = load_plan(&path);
    plan.episodes[2].delivers_season_finale = false;
    plan.episodes[1].engine_break = Some("compact-afterpiece".to_string());
    save_plan(&path, &plan);
    assert!(showrunner::validate(&path).is_ok());
    let rhythm = showrunner::rhythm_audit(&path).unwrap();
    assert!(
        rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "finale-delivery-unconfirmed")
    );
    assert!(
        !rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "undeclared-engine-break")
    );
}

#[test]
fn review_queue_preserves_distinct_open_human_reviewers() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let queue = showrunner::review_queue(&path).unwrap();
    assert_eq!(queue.open.len(), 6);
    assert!(queue.reviewed.is_empty());
    assert_eq!(queue.open[0].open_reviewers, vec!["author", "producer"]);
}

#[test]
fn audits_boundary_functions_and_optional_production_load_without_forcing_cost_data() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let mut plan = load_plan(&path);
    plan.seasons[0].opening_function_family = Some("pressure".to_string());
    plan.policies.high_production_load_threshold = Some(4);
    plan.policies.max_adjacent_high_production_load = Some(2);
    for episode in &mut plan.episodes[0..3] {
        episode.production_load = Some(ProductionLoad {
            complexity: 5,
            locations: 2,
            speaking_roles: 4,
            crowd: false,
            new_assets: vec!["sanitized-set".to_string()],
            reusable_assets: vec!["shared-prop".to_string()],
        });
    }
    save_plan(&path, &plan);
    assert!(showrunner::validate(&path).is_ok());
    let rhythm = showrunner::rhythm_audit(&path).unwrap();
    assert!(
        rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "opening-function-mismatch")
    );
    assert!(
        rhythm
            .findings
            .iter()
            .any(|finding| finding.code == "high-production-load-cluster")
    );
}

#[test]
fn preserves_revelation_spans_and_rejects_overlapping_next_steps() {
    let temp = tempdir().unwrap();
    let (_, hash) = write_series(temp.path());
    let path = write_showrunner(temp.path(), hash);
    let mut plan = load_plan(&path);
    plan.revelation_threads[0].steps[0].through_episode_id = Some("S1E02".to_string());
    save_plan(&path, &plan);
    assert!(
        showrunner::validate(&path)
            .unwrap_err()
            .to_string()
            .contains("overlap")
    );

    plan.revelation_threads[0].steps[0].through_episode_id = None;
    plan.revelation_threads[0].steps[1].through_episode_id = Some("S1E03".to_string());
    save_plan(&path, &plan);
    let map = showrunner::revelation_map(&path).unwrap();
    assert_eq!(
        map.threads[0].steps[1].through_episode_id.as_deref(),
        Some("S1E03")
    );
}
