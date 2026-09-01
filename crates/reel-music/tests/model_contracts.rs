use std::{fs, path::PathBuf};

use reel_music::{
    AuthorityRef, DecisionRef,
    analysis::{
        AnalysisManifest, Analyzer, Observation, ObservationValue, Review as AnalysisReview,
        SourceBinding as AnalysisSourceBinding,
    },
    hash::sha256_path,
    model::{
        AnalysisBinding, EvidenceRef, FormSection, HarmonyEvent, Hook, MeterEvent, MusicModel,
        Note, Part, PartRole, Provenance, ProvenanceState, Review as ModelReview, RhythmCell,
        SourceBinding as ModelSourceBinding, TempoEvent, TickRange,
    },
    source::{Egress, Media, NetworkPolicy, RawPcmFormat, SourceManifest},
    time::{AudioTimebase, MusicalTimebase, RoundingMode, SampleRange},
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    source: PathBuf,
    analysis: PathBuf,
    model: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let pcm = temp.path().join("source.raw");
    fs::write(&pcm, [128_u8; 64]).expect("write PCM");
    let pcm_hash = sha256_path(&pcm).expect("hash PCM");
    let source = temp.path().join("source.yaml");
    let source_manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "synthetic-model-source".into(),
        media: Media {
            path: PathBuf::from("source.raw"),
            sha256: pcm_hash.clone(),
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 64,
            },
            decoded_pcm_sha256: pcm_hash.clone(),
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 480,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "generated-model-source".into(),
            content_sha256: pcm_hash,
            status: "fixture-only".into(),
            required_roles: vec!["music-reconstruction-engineer".into()],
            decision_refs: vec![],
        },
        egress: Egress {
            private: true,
            network_policy: NetworkPolicy::Denied,
            third_party_upload: false,
        },
    };
    fs::write(
        &source,
        serde_yaml::to_string(&source_manifest).expect("serialize source"),
    )
    .expect("write source");
    let source_report = reel_music::source::validate(&source).expect("source validates");
    let analysis = temp.path().join("analysis.yaml");
    let analyzer_id = "fixture-analyzer".to_string();
    let observation = |id: &str, value: ObservationValue| Observation {
        id: id.into(),
        analyzer_id: analyzer_id.clone(),
        source: SampleRange { start: 0, end: 64 },
        confidence_millionths: 850_000,
        uncertainty: "Synthetic estimate for contract testing only.".into(),
        value,
        import_event_id: None,
    };
    let analysis_manifest = AnalysisManifest {
        schema: "reel.music-analysis.v0.1".into(),
        analysis_id: "synthetic-analysis".into(),
        source: AnalysisSourceBinding {
            manifest: source.clone(),
            manifest_sha256: source_report.manifest_sha256.clone(),
            contract_sha256: source_report.contract_sha256.clone(),
            decoded_pcm_sha256: source_report.decoded_pcm_sha256.clone(),
        },
        imports: vec![],
        analyzers: vec![Analyzer {
            id: analyzer_id.clone(),
            adapter: "generated-fixture".into(),
            version: "1".into(),
            model_revision: "none".into(),
            parameters_sha256: "1".repeat(64),
            license: "fixture-only".into(),
            network_policy: NetworkPolicy::Denied,
            import_id: None,
        }],
        stems: vec![],
        observations: vec![
            observation("tempo", ObservationValue::Tempo { milli_bpm: 120_000 }),
            observation(
                "meter",
                ObservationValue::Meter {
                    numerator: 4,
                    denominator: 4,
                },
            ),
            observation(
                "form",
                ObservationValue::Form {
                    label: "two-phrase form".into(),
                },
            ),
            observation(
                "pitch",
                ObservationValue::Pitch {
                    midi_note: 60,
                    cents: 0,
                },
            ),
            observation("harmony", ObservationValue::Harmony { symbol: "I".into() }),
            observation(
                "rhythm",
                ObservationValue::Rhythm {
                    label: "quarter pulse".into(),
                },
            ),
            observation(
                "hook",
                ObservationValue::Hook {
                    label: "opening four notes".into(),
                },
            ),
        ],
        limitations: vec!["No performance dynamics or timbre were analyzed.".into()],
        review: AnalysisReview {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "sound-designer".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    fs::write(
        &analysis,
        serde_yaml::to_string(&analysis_manifest).expect("serialize analysis"),
    )
    .expect("write analysis");
    let analysis_report = reel_music::analysis::validate(&analysis).expect("analysis validates");
    let model = temp.path().join("model.yaml");
    let evidence = |observation_id: &str| EvidenceRef {
        analysis_id: "synthetic-analysis".into(),
        observation_id: observation_id.into(),
    };
    let observed = |observation_id: &str| Provenance {
        state: ProvenanceState::Observed,
        evidence_refs: vec![evidence(observation_id)],
        rationale: "Directly retained from synthetic analysis evidence.".into(),
        correction_ref: None,
    };
    let corrected = |observation_id: &str| Provenance {
        state: ProvenanceState::HumanCorrected,
        evidence_refs: vec![evidence(observation_id)],
        rationale: "Fixture author corrected the synthetic pitch.".into(),
        correction_ref: Some(DecisionRef {
            artifact_id: "synthetic-correction-fixture".into(),
            sha256: "2".repeat(64),
        }),
    };
    let model_manifest = MusicModel {
        schema: "reel.music-model.v0.1".into(),
        model_id: "synthetic-corrected-model".into(),
        source: ModelSourceBinding {
            manifest: source,
            manifest_sha256: source_report.manifest_sha256,
            contract_sha256: source_report.contract_sha256,
            decoded_pcm_sha256: source_report.decoded_pcm_sha256,
        },
        analyses: vec![AnalysisBinding {
            manifest: analysis.clone(),
            manifest_sha256: analysis_report.manifest_sha256,
            contract_sha256: analysis_report.contract_sha256,
            analysis_id: analysis_report.analysis_id,
        }],
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "synthetic-corrected-model-authority".into(),
            content_sha256: "3".repeat(64),
            status: "fixture-only".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
            ],
            decision_refs: vec![],
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 480,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        duration_ticks: 3_840,
        tempo_map: vec![TempoEvent {
            tick: 0,
            microseconds_per_quarter: 500_000,
            provenance: observed("tempo"),
        }],
        meter_map: vec![MeterEvent {
            tick: 0,
            numerator: 4,
            denominator: 4,
            provenance: observed("meter"),
        }],
        form: vec![
            FormSection {
                id: "form-a".into(),
                label: "A".into(),
                range: TickRange {
                    start: 0,
                    end: 1_920,
                },
                provenance: observed("form"),
            },
            FormSection {
                id: "form-a2".into(),
                label: "A'".into(),
                range: TickRange {
                    start: 1_920,
                    end: 3_840,
                },
                provenance: observed("form"),
            },
        ],
        parts: vec![Part {
            id: "melody".into(),
            role: PartRole::Melody,
            name: "Synthetic melody".into(),
            notes: vec![
                Note {
                    id: "note-1".into(),
                    voice: 1,
                    start_tick: 0,
                    duration_ticks: 480,
                    midi_note: 60,
                    velocity: 80,
                    provenance: corrected("pitch"),
                },
                Note {
                    id: "note-2".into(),
                    voice: 1,
                    start_tick: 480,
                    duration_ticks: 480,
                    midi_note: 62,
                    velocity: 80,
                    provenance: observed("pitch"),
                },
                Note {
                    id: "note-3".into(),
                    voice: 1,
                    start_tick: 960,
                    duration_ticks: 480,
                    midi_note: 64,
                    velocity: 80,
                    provenance: observed("pitch"),
                },
                Note {
                    id: "note-4".into(),
                    voice: 1,
                    start_tick: 1_440,
                    duration_ticks: 480,
                    midi_note: 65,
                    velocity: 80,
                    provenance: observed("pitch"),
                },
            ],
        }],
        harmony: vec![HarmonyEvent {
            id: "harmony-i".into(),
            range: TickRange {
                start: 0,
                end: 3_840,
            },
            symbol: "I".into(),
            provenance: observed("harmony"),
        }],
        rhythm_cells: vec![RhythmCell {
            id: "quarter-cell".into(),
            range: TickRange {
                start: 0,
                end: 1_920,
            },
            onset_offsets_ticks: vec![0, 480, 960, 1_440],
            provenance: observed("rhythm"),
        }],
        hooks: vec![Hook {
            id: "opening-hook".into(),
            label: "Opening four-note hook".into(),
            range: TickRange {
                start: 0,
                end: 1_920,
            },
            element_refs: vec![
                "note-1".into(),
                "note-2".into(),
                "note-3".into(),
                "note-4".into(),
            ],
            provenance: observed("hook"),
        }],
        lyric_layers: vec![],
        expressive_timing: vec![],
        unknowns: vec!["Dynamics and articulation remain unknown.".into()],
        review: ModelReview {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    fs::write(
        &model,
        serde_yaml::to_string(&model_manifest).expect("serialize model"),
    )
    .expect("write model");
    Fixture {
        _temp: temp,
        source: model_manifest.source.manifest,
        analysis,
        model,
    }
}

#[test]
fn validates_external_analysis_without_promoting_it_to_ground_truth() {
    let fixture = fixture();
    let report = reel_music::analysis::validate(&fixture.analysis).expect("analysis validates");
    assert_eq!(report.analyzers, 1);
    assert_eq!(report.observations, 7);
    assert_eq!(report.minimum_confidence_millionths, 850_000);
    assert!(!report.reviewed);
    assert!(!report.shareable);
}

#[test]
fn validates_a_separate_corrected_editable_model() {
    let fixture = fixture();
    let report = reel_music::model::validate(&fixture.model).expect("model validates");
    assert_eq!(report.duration_ticks, 3_840);
    assert_eq!(report.notes, 4);
    assert_eq!(report.human_corrected_events, 1);
    assert_eq!(report.hooks, 1);
    assert!(!report.shareable);
}

#[test]
fn rejects_human_correction_without_immutable_decision_reference() {
    let fixture = fixture();
    let mut model = reel_music::model::load(&fixture.model).expect("load model");
    model.parts[0].notes[0].provenance.correction_ref = None;
    fs::write(
        &fixture.model,
        serde_yaml::to_string(&model).expect("serialize model"),
    )
    .expect("rewrite model");
    let error = reel_music::model::validate(&fixture.model).expect_err("correction ref required");
    assert!(error.to_string().contains("requires correction_ref"));
}

#[test]
fn rejects_unknown_or_stale_analysis_evidence() {
    {
        let fixture = fixture();
        let mut model = reel_music::model::load(&fixture.model).expect("load model");
        model.parts[0].notes[1].provenance.evidence_refs[0].observation_id = "missing".into();
        fs::write(
            &fixture.model,
            serde_yaml::to_string(&model).expect("serialize model"),
        )
        .expect("rewrite model");
        let error =
            reel_music::model::validate(&fixture.model).expect_err("unknown evidence rejected");
        assert!(error.to_string().contains("unknown analysis observation"));
    }

    let fixture = fixture();
    let mut analysis = reel_music::analysis::load(&fixture.analysis).expect("load analysis");
    analysis
        .limitations
        .push("Changed after model binding.".into());
    fs::write(
        &fixture.analysis,
        serde_yaml::to_string(&analysis).expect("serialize analysis"),
    )
    .expect("rewrite analysis");
    let error = reel_music::model::validate(&fixture.model).expect_err("stale binding rejected");
    assert!(error.to_string().contains("manifest sha256 is stale"));
}

#[test]
fn rejects_analysis_confidence_outside_the_integer_scale() {
    let fixture = fixture();
    let mut analysis = reel_music::analysis::load(&fixture.analysis).expect("load analysis");
    analysis.observations[0].confidence_millionths = 1_000_001;
    fs::write(
        &fixture.analysis,
        serde_yaml::to_string(&analysis).expect("serialize analysis"),
    )
    .expect("rewrite analysis");
    let error = reel_music::analysis::validate(&fixture.analysis).expect_err("confidence rejected");
    assert!(error.to_string().contains("confidence_millionths"));
}

#[test]
fn rejects_vocal_model_without_exact_lyric_layer() {
    let fixture = fixture();
    let mut model = reel_music::model::load(&fixture.model).expect("load model");
    model.parts[0].role = PartRole::Vocal;
    fs::write(
        &fixture.model,
        serde_yaml::to_string(&model).expect("serialize model"),
    )
    .expect("rewrite model");
    let error = reel_music::model::validate(&fixture.model).expect_err("lyrics required");
    assert!(error.to_string().contains("vocal part requires"));
}

#[test]
fn fixture_source_remains_valid_and_immutable() {
    let fixture = fixture();
    let report = reel_music::source::validate(&fixture.source).expect("source validates");
    assert_eq!(report.bytes, 64);
}

#[test]
fn rejects_unknown_observation_fields() {
    let fixture = fixture();
    let mut value: serde_yaml::Value =
        serde_yaml::from_str(&fs::read_to_string(&fixture.analysis).expect("read analysis"))
            .expect("parse analysis");
    value["observations"][0]["value"]
        .as_mapping_mut()
        .expect("observation value mapping")
        .insert(
            serde_yaml::Value::String("unexpected".into()),
            serde_yaml::Value::Bool(true),
        );
    fs::write(
        &fixture.analysis,
        serde_yaml::to_string(&value).expect("serialize analysis value"),
    )
    .expect("rewrite analysis");
    let error = reel_music::analysis::validate(&fixture.analysis).expect_err("unknown rejected");
    assert!(error.to_string().contains("not valid YAML"));
}

#[test]
fn rejects_expressive_timing_outside_model_duration() {
    let fixture = fixture();
    let mut model = reel_music::model::load(&fixture.model).expect("load model");
    model
        .expressive_timing
        .push(reel_music::model::ExpressiveTiming {
            note_id: "note-1".into(),
            onset_offset_ticks: -1,
            duration_offset_ticks: 0,
            provenance: model.parts[0].notes[0].provenance.clone(),
        });
    fs::write(
        &fixture.model,
        serde_yaml::to_string(&model).expect("serialize model"),
    )
    .expect("rewrite model");
    let error = reel_music::model::validate(&fixture.model).expect_err("negative onset rejected");
    assert!(error.to_string().contains("within model duration"));
}
