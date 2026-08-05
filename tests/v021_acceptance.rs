use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use reel::{
    continuity, cue_import,
    production::{self, ContinuityEntity, SourceRange, TimingStatus},
    series::{
        self, ChildManifestRef, Episode, ProductionUnit, Season, SeriesDefaults, SeriesFinding,
        SeriesManifest, SeriesOmission, SeriesRange,
    },
};
use serde_yaml::Value;
use tempfile::tempdir;

const VERTICAL: &str = "manifests/fixtures/vertical-sound-off/manifest.yaml";
const VERTICAL_CAPTIONS: &str = "manifests/fixtures/vertical-sound-off/captions.srt";
const PLANNING: &str = "manifests/fixtures/two-speaker-untimed/planning.yaml";
const MEASUREMENTS: &str = "manifests/fixtures/two-speaker-untimed/cue-measurements.yaml";
const SERIES_TEMPLATE: &str = "manifests/templates/episodic-series.yaml";

fn narrator_tempo() -> BTreeMap<String, u32> {
    BTreeMap::from([("narrator".to_string(), 85)])
}

fn write_child(root: &Path, work: &str, start: u64, end: u64) -> (PathBuf, String) {
    let child_dir = root.join(work);
    fs::create_dir_all(&child_dir).unwrap();
    let path = child_dir.join("manifest.yaml");
    let mut manifest = production::load(VERTICAL).unwrap().manifest;
    manifest.work = work.to_string();
    manifest.title = format!("Sanitized child {work}");
    manifest.source_ranges = vec![SourceRange {
        id: "episode-source".to_string(),
        start,
        end,
        label: "sanitized episode range".to_string(),
    }];
    manifest.omissions.clear();
    for shot in &mut manifest.shots {
        shot.source_refs = vec!["episode-source".to_string()];
    }
    for cue in &mut manifest.narration_cues {
        cue.source_refs = vec!["episode-source".to_string()];
    }
    manifest.review.status = "panel-reviewed".to_string();
    manifest.lineage = None;
    fs::write(&path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    fs::copy(VERTICAL_CAPTIONS, child_dir.join("captions.srt")).unwrap();
    let hash = production::sha256_path(&path).unwrap();
    (path, hash)
}

fn child_ref(
    series_dir: &Path,
    path: &Path,
    work: &str,
    hash: String,
    duration: f64,
) -> ChildManifestRef {
    ChildManifestRef {
        path: path
            .strip_prefix(series_dir)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/"),
        work_id: work.to_string(),
        expected_sha256: hash,
        accepted_timing_states: vec![TimingStatus::Conformed],
        accepted_review_states: vec!["panel-reviewed".to_string()],
        required_platforms: vec!["vertical-sound-off".to_string()],
        source_complete: true,
        privacy_clear: true,
        duration_seconds: Some(duration),
    }
}

fn episode(id: String, order: u32, range: SeriesRange, child: ChildManifestRef) -> Episode {
    Episode {
        id,
        order,
        part: None,
        production_title: format!("Working episode {order}"),
        manuscript_title: format!("Original section {order}"),
        poem_ids: Vec::new(),
        source_ranges: vec![range],
        omissions: Vec::new(),
        chronology_place: "sanitized chronology / place".to_string(),
        memory_mode: "recollection".to_string(),
        sensitivity: "review-required".to_string(),
        recurring_motifs: vec!["road".to_string()],
        continuity_entry: vec!["road-distant".to_string()],
        continuity_exit: vec!["road-near".to_string()],
        timing_status: TimingStatus::Conformed,
        human_review_status: "open".to_string(),
        raw_orientation_seconds: 0.0,
        measured_narration_seconds: 6.0,
        protected_pause_seconds: 0.0,
        scene_duration_seconds: 6.0,
        total_runtime_seconds: 6.0,
        release_ready: false,
        accepted_speakers: vec!["on-screen-story".to_string()],
        findings: vec![
            SeriesFinding {
                reviewer: "bertica".to_string(),
                finding: "open".to_string(),
                decision_reference: String::new(),
                approved: false,
            },
            SeriesFinding {
                reviewer: "herman".to_string(),
                finding: "open".to_string(),
                decision_reference: String::new(),
                approved: false,
            },
        ],
        dependencies: Vec::new(),
        children: vec![child],
        production_units: Vec::new(),
    }
}

#[test]
fn validates_the_sanitized_five_season_fifty_episode_slate() {
    let temp = tempdir().unwrap();
    let series_dir = temp.path();
    let mut cursor = 34u64;
    let mut seasons = Vec::new();
    for season_number in 1..=5 {
        let mut episodes = Vec::new();
        for episode_number in 1..=10 {
            let global = (season_number - 1) * 10 + episode_number;
            let units = if global <= 36 { 88 } else { 87 };
            let end = cursor + units - 1;
            let id = format!("S{season_number}E{episode_number:02}");
            let work = format!("fixture-{id}");
            let (path, hash) = write_child(series_dir, &work, cursor, end);
            episodes.push(episode(
                id,
                episode_number,
                SeriesRange { start: cursor, end },
                child_ref(series_dir, &path, &work, hash, 6.0),
            ));
            cursor = end + 1;
        }
        seasons.push(Season {
            id: format!("S{season_number}"),
            order: season_number,
            title: format!("Season {season_number}"),
            total_runtime_seconds: Some(60.0),
            episodes,
        });
    }
    assert_eq!(cursor - 1, 4419);
    let series = SeriesManifest {
        schema: series::SERIES_SCHEMA.to_string(),
        series_id: "el-camino-sanitized".to_string(),
        title: "Sanitized five-season slate".to_string(),
        canonical_source_start: 34,
        canonical_source_end: 4419,
        defaults: SeriesDefaults::default(),
        seasons,
    };
    let path = series_dir.join("series.yaml");
    fs::write(&path, serde_yaml::to_string(&series).unwrap()).unwrap();
    let report = series::validate(&path).unwrap();
    assert_eq!(report.seasons, 5);
    assert_eq!(report.episodes, 50);
    assert_eq!(report.children, 50);
    assert!(report.continuous_coverage);
    assert_eq!(report.source_start, 34);
    assert_eq!(report.source_end, 4419);
    assert_eq!(report.human_approvals, 0);
    assert_eq!(report.release_ready_episodes, 0);
    assert_eq!(series::plan(&path).unwrap().seasons.len(), 5);
    assert!(series::coverage(&path).unwrap().continuous);
    assert_eq!(series::review_queue(&path).unwrap().open.len(), 50);
}

#[test]
fn rejects_unsafe_series_states_without_inferred_approval() {
    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0].release_ready = true;
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("without explicit human review approval"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0]
        .poem_ids
        .push("unstructured-poem".to_string());
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("lacks an approved poem-prose structure"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0].source_ranges[0].end = 5;
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("outside the canonical series range"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0].children[0].expected_sha256 = "wrong".to_string();
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("hash mismatch"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    let repeated = loaded.manifest.seasons[0].episodes[0].children[0].clone();
    loaded.manifest.seasons[0].episodes[0]
        .children
        .push(repeated);
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("repeated child manifest"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0].children[0].path = "missing.yaml".to_string();
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("child is missing"));

    let mut loaded = series::load(SERIES_TEMPLATE).unwrap();
    loaded.manifest.seasons[0].episodes[0].timing_status = TimingStatus::Locked;
    let error = series::validate_loaded(&loaded).unwrap_err().to_string();
    assert!(error.contains("unlocked child"));
}

#[test]
fn imports_two_speaker_srt_and_preserves_the_protected_pause_exactly() {
    let temp = tempdir().unwrap();
    let packet = temp.path().join("conformed");
    production::conform(PLANNING, MEASUREMENTS, &packet, &narrator_tempo()).unwrap();
    let output = temp.path().join("imported.yaml");
    let report = cue_import::import_srt(
        packet.join("manifest.yaml"),
        "manifests/fixtures/cue-import/captions.es.srt",
        None,
        &[],
        Some(Path::new("manifests/fixtures/cue-import/mapping.yaml")),
        &output,
    )
    .unwrap();
    assert_eq!(report.cues, 6);
    assert_eq!(report.speakers, vec!["narrator", "poet"]);
    assert_eq!(report.protected_pauses, 1);
    let loaded = production::load(&output).unwrap();
    assert_eq!(loaded.manifest.protected_pauses[0].duration_ms, 1500);
    assert_eq!(loaded.manifest.protected_pauses[0].after_cue_id, "cue-poem");
    assert!(
        loaded.manifest.narration_cues[..4]
            .iter()
            .all(|cue| cue.speaker_id == "poet" && cue.shot_ids == vec!["shot-01"])
    );
    assert!(
        loaded.manifest.narration_cues[4..]
            .iter()
            .all(|cue| cue.speaker_id == "narrator" && cue.shot_ids == vec!["shot-02"])
    );
    assert!(loaded.manifest.extra.contains_key("cue_import"));
    let exported = temp.path().join("captions.srt");
    production::caption_export(&output, &exported).unwrap();
    assert_eq!(
        fs::read_to_string(exported).unwrap(),
        fs::read_to_string("manifests/fixtures/cue-import/captions.es.srt").unwrap()
    );

    let bad_srt = temp.path().join("bad-pause.srt");
    fs::write(
        &bad_srt,
        fs::read_to_string("manifests/fixtures/cue-import/captions.es.srt")
            .unwrap()
            .replace("00:00:03,500 -->", "00:00:03,400 -->"),
    )
    .unwrap();
    let error = cue_import::import_srt(
        packet.join("manifest.yaml"),
        &bad_srt,
        None,
        &[],
        Some(Path::new("manifests/fixtures/cue-import/mapping.yaml")),
        temp.path().join("bad-import.yaml"),
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("must remain exactly 1500ms"));
}

#[test]
fn composes_children_and_an_attributed_end_card_atomically() {
    let temp = tempdir().unwrap();
    let conformed = temp.path().join("source");
    production::conform(PLANNING, MEASUREMENTS, &conformed, &narrator_tempo()).unwrap();
    let source = production::load(conformed.join("manifest.yaml"))
        .unwrap()
        .manifest;
    let series_dir = temp.path().join("series");
    fs::create_dir_all(&series_dir).unwrap();
    let mut poem = source.clone();
    poem.work = "poem-threshold".to_string();
    poem.scenes[0].duration_seconds = Some(3.5);
    poem.shots = vec![poem.shots[0].clone()];
    poem.speakers = vec![poem.speakers[0].clone()];
    poem.narration_cues = vec![poem.narration_cues[0].clone()];
    poem.source_ranges = vec![poem.source_ranges[0].clone()];
    poem.review.status = "panel-reviewed".to_string();
    poem.platforms[0].target_duration_seconds = Some(3.5);
    poem.exports[0].duration_seconds = Some(3.5);
    let poem_dir = series_dir.join("poem");
    fs::create_dir_all(&poem_dir).unwrap();
    let poem_path = poem_dir.join("manifest.yaml");
    fs::write(&poem_path, serde_yaml::to_string(&poem).unwrap()).unwrap();
    production::caption_export(&poem_path, poem_dir.join("captions.srt")).unwrap();

    let mut prose = source;
    prose.work = "prose-scene".to_string();
    prose.scenes[0].duration_seconds = Some(4.0);
    prose.shots = vec![prose.shots[1].clone()];
    prose.shots[0].start_seconds = Some(0.0);
    prose.speakers = vec![prose.speakers[1].clone()];
    prose.narration_cues = vec![prose.narration_cues[1].clone()];
    prose.narration_cues[0].start_seconds = Some(0.0);
    prose.protected_pauses.clear();
    prose.source_ranges = vec![prose.source_ranges[1].clone()];
    prose.omissions.clear();
    prose.review.status = "panel-reviewed".to_string();
    prose.platforms[0].target_duration_seconds = Some(4.0);
    prose.exports[0].duration_seconds = Some(4.0);
    let prose_dir = series_dir.join("prose");
    fs::create_dir_all(&prose_dir).unwrap();
    let prose_path = prose_dir.join("manifest.yaml");
    fs::write(&prose_path, serde_yaml::to_string(&prose).unwrap()).unwrap();
    production::caption_export(&prose_path, prose_dir.join("captions.srt")).unwrap();

    let episode = Episode {
        id: "S2E02".to_string(),
        order: 1,
        part: None,
        production_title: "Poem, prose, landing".to_string(),
        manuscript_title: "Sanitized threshold".to_string(),
        poem_ids: vec!["andresito-poem-fixture".to_string()],
        source_ranges: vec![
            SeriesRange { start: 1, end: 3 },
            SeriesRange { start: 6, end: 8 },
        ],
        omissions: vec![SeriesOmission {
            start: 4,
            end: 5,
            bridge: "silence".to_string(),
            reason: "protected threshold".to_string(),
        }],
        chronology_place: "fixture".to_string(),
        memory_mode: "threshold".to_string(),
        sensitivity: "none-fixture".to_string(),
        recurring_motifs: vec!["doorway".to_string()],
        continuity_entry: vec!["closed".to_string()],
        continuity_exit: vec!["open".to_string()],
        timing_status: TimingStatus::Conformed,
        human_review_status: "open".to_string(),
        raw_orientation_seconds: 0.0,
        measured_narration_seconds: 6.0,
        protected_pause_seconds: 1.5,
        scene_duration_seconds: 7.5,
        total_runtime_seconds: 8.5,
        release_ready: false,
        accepted_speakers: vec!["poet".to_string(), "narrator".to_string()],
        findings: Vec::new(),
        dependencies: vec![reel::series::SeriesDependency {
            kind: "poem-prose".to_string(),
            episode_id: "S2E02".to_string(),
            detail: "protected poem-to-prose threshold".to_string(),
            approved_structure: true,
        }],
        children: vec![
            {
                let mut child = child_ref(
                    &series_dir,
                    &poem_path,
                    "poem-threshold",
                    production::sha256_path(&poem_path).unwrap(),
                    3.5,
                );
                child.required_platforms.clear();
                child
            },
            {
                let mut child = child_ref(
                    &series_dir,
                    &prose_path,
                    "prose-scene",
                    production::sha256_path(&prose_path).unwrap(),
                    4.0,
                );
                child.required_platforms.clear();
                child
            },
        ],
        production_units: vec![ProductionUnit {
            id: "end-card".to_string(),
            kind: "end-card".to_string(),
            source_kind: "production-authored".to_string(),
            duration_seconds: 1.0,
            caption_text: "Fin".to_string(),
        }],
    };
    let series = SeriesManifest {
        schema: series::SERIES_SCHEMA.to_string(),
        series_id: "composition-fixture".to_string(),
        title: "Composition fixture".to_string(),
        canonical_source_start: 1,
        canonical_source_end: 8,
        defaults: SeriesDefaults::default(),
        seasons: vec![Season {
            id: "S2".to_string(),
            order: 1,
            title: "Season 2".to_string(),
            total_runtime_seconds: Some(8.5),
            episodes: vec![episode],
        }],
    };
    let series_path = series_dir.join("series.yaml");
    fs::write(&series_path, serde_yaml::to_string(&series).unwrap()).unwrap();
    let output = temp.path().join("composed");
    let report = series::compose_episode(&series_path, "S2E02", &output).unwrap();
    assert_eq!(report.duration_ms, 8500);
    let manifest = fs::read_to_string(output.join("manifest.yaml")).unwrap();
    assert!(manifest.contains("production-authored"));
    assert!(manifest.contains("defaults:"));
    let lineage = fs::read_to_string(output.join("lineage.json")).unwrap();
    assert!(lineage.contains("poem-to-prose-pause"));
    assert!(lineage.contains(&production::sha256_path(&poem_path).unwrap()));
    assert!(lineage.contains(&production::sha256_path(&prose_path).unwrap()));
    let coverage = fs::read_to_string(output.join("coverage.json")).unwrap();
    assert!(coverage.contains("production-authored"));
    let captions = fs::read_to_string(output.join("captions.srt")).unwrap();
    assert!(captions.contains("00:00:07,500 --> 00:00:08,500"));
    assert!(captions.contains("Fin"));
}

#[test]
fn resolves_shared_continuity_without_egressing_private_paths() {
    let temp = tempdir().unwrap();
    let registry_source = Path::new("manifests/fixtures/shared-continuity/registry.yaml");
    let registry_path = temp.path().join("registry.yaml");
    fs::copy(registry_source, &registry_path).unwrap();
    let registry_report = continuity::validate(&registry_path).unwrap();
    assert_eq!(registry_report.entities, 7);
    let unapproved_registry = temp.path().join("unapproved-registry.yaml");
    fs::write(
        &unapproved_registry,
        fs::read_to_string(&registry_path).unwrap().replacen(
            "observations_approved: true",
            "observations_approved: false",
            1,
        ),
    )
    .unwrap();
    assert!(
        continuity::validate(&unapproved_registry)
            .unwrap_err()
            .to_string()
            .contains("require explicit approval")
    );
    let mut manifest = production::load(VERTICAL).unwrap().manifest;
    manifest.work = "shared-continuity-fixture".to_string();
    manifest.continuity.entities = vec![ContinuityEntity {
        id: "herrera".to_string(),
        age_at_scene: "young-adult".to_string(),
        observations: vec!["Wears a pale travel shirt.".to_string()],
        confidence: "scene-confirmed".to_string(),
        provenance: "scene-fixture".to_string(),
        human_confirmation_status: "approved-fixture".to_string(),
        reference_assets: Vec::new(),
        extra: BTreeMap::from([
            (
                "clothing".to_string(),
                Value::String("pale travel shirt".to_string()),
            ),
            (
                "condition".to_string(),
                Value::String("arriving from the road".to_string()),
            ),
            ("observations_approved".to_string(), Value::Bool(true)),
            (
                "observation_approval_reference".to_string(),
                Value::String("fixture/young-herrera".to_string()),
            ),
        ]),
    }];
    manifest.continuity.extra.insert(
        "external_registry".to_string(),
        serde_yaml::to_value(serde_json::json!({
            "path": "registry.yaml",
            "version": "1",
            "sha256": registry_report.sha256,
            "entity_ids": ["herrera", "bertha-maria", "herminio", "moro", "amado-rosa", "caimito-road", "riverita-house"]
        })).unwrap(),
    );
    let manifest_path = temp.path().join("manifest.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let package = production::provider_package(&manifest_path).unwrap();
    assert_eq!(package.approved_text_observations.len(), 7);
    assert!(
        package.approved_text_observations["herrera"]
            .iter()
            .any(|observation| observation.contains("pale travel shirt"))
    );
    let serialized = serde_json::to_string(&package).unwrap();
    assert!(!serialized.contains("C:/private"));
    assert!(!serialized.contains("local_path"));

    manifest.provider_handoff.asset_ids = vec!["herrera-private-photo".to_string()];
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let blocked = production::provider_package(&manifest_path).unwrap();
    assert!(blocked.blocked);
    assert!(
        !serde_json::to_string(&blocked)
            .unwrap()
            .contains("C:/private")
    );

    manifest.provider_handoff.asset_ids.clear();
    manifest.continuity.entities[0].age_at_scene = "later-adult".to_string();
    manifest.continuity.entities[0].observations = vec!["Wears a dark formal coat.".to_string()];
    manifest.continuity.entities[0].extra.insert(
        "observation_approval_reference".to_string(),
        Value::String("fixture/later-herrera".to_string()),
    );
    let later_path = temp.path().join("later-herrera.yaml");
    fs::write(&later_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let later = continuity::resolve_for_manifest(&later_path, &manifest).unwrap();
    let later_herrera = later.iter().find(|entity| entity.id == "herrera").unwrap();
    assert_eq!(later_herrera.age_at_scene, "later-adult");
    assert!(
        later_herrera
            .observations
            .iter()
            .any(|observation| observation.contains("dark formal coat"))
    );
}
