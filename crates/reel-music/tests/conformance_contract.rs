use std::path::{Path, PathBuf};

use reel_music::{
    analysis::{self, Observation, ObservationValue},
    conformance::{
        self, EvidenceStandard, InstrumentRole, InstrumentStandard, Outcome, Policy, TempoStandard,
    },
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests/fixtures/music-model-corrected/analysis.yaml")
        .canonicalize()
        .unwrap()
}

#[test]
fn evaluates_exact_source_and_requires_calibrated_positive_instrument_evidence() {
    let original = fixture();
    let mut manifest = analysis::load(&original).unwrap();
    manifest.source.manifest = original
        .parent()
        .unwrap()
        .join(&manifest.source.manifest)
        .canonicalize()
        .unwrap();
    let analyzer = conformance::analyzer_fingerprint(&manifest.analyzers[0]).unwrap();
    let standard = || EvidenceStandard {
        analyzer_sha256: vec![analyzer.clone()],
        minimum_confidence_millionths: 800_000,
    };
    let mut policy = Policy {
        schema: conformance::SCHEMA.into(),
        track_id: "synthetic-track".into(),
        label: "Fixture".into(),
        artist: "Fixture".into(),
        source_contract_sha256: manifest.source.contract_sha256.clone(),
        intent_sha256: "1".repeat(64),
        tempo: Some(TempoStandard {
            target_milli_bpm: 120_000,
            tolerance_milli_bpm: 3_000,
            allow_half_double_pulse: false,
            evidence: standard(),
        }),
        duration: Some(conformance::DurationStandard {
            target_milliseconds: 8,
            tolerance_milliseconds: 0,
        }),
        instruments: vec![InstrumentStandard {
            instrument: "saxophone".into(),
            equivalent_labels: vec!["tenor saxophone".into()],
            role: InstrumentRole::RequiredEachTrack,
            evidence: standard(),
        }],
    };
    let folder = tempfile::tempdir().unwrap();
    let policy_file = folder.path().join("policy.json");
    let analysis_file = folder.path().join("analysis.json");
    let store = |path: &Path, value: Vec<u8>| std::fs::write(path, value).unwrap();
    store(&policy_file, serde_json::to_vec(&policy).unwrap());
    store(&analysis_file, serde_json::to_vec(&manifest).unwrap());
    let report = conformance::evaluate(&policy_file, &analysis_file).unwrap();
    assert_eq!(report.findings[0].outcome, Outcome::Supported);
    let duration = report.duration.unwrap();
    assert_eq!(duration.observed_milliseconds, 8);
    assert_eq!(duration.outcome, Outcome::Supported);
    assert_eq!(report.findings[1].outcome, Outcome::Unverified);

    manifest.observations.push(Observation {
        id: "test-tenor".into(),
        analyzer_id: manifest.analyzers[0].id.clone(),
        source: manifest.observations[0].source,
        confidence_millionths: 800_000,
        uncertainty: "Synthetic observation for testing only".into(),
        value: ObservationValue::Instrumentation {
            label: " Tenor  Saxophone ".into(),
        },
        import_event_id: None,
    });
    store(&analysis_file, serde_json::to_vec(&manifest).unwrap());
    let report = conformance::evaluate(&policy_file, &analysis_file).unwrap();
    assert_eq!(report.findings[1].outcome, Outcome::Supported);
    assert_eq!(report.findings[1].evidence[0].observation_id, "test-tenor");
    policy.instruments[0].evidence.analyzer_sha256.clear();
    let tempo = policy.tempo.as_mut().unwrap();
    tempo.target_milli_bpm = 60_000;
    tempo.allow_half_double_pulse = true;
    store(&policy_file, serde_json::to_vec(&policy).unwrap());
    let report = conformance::evaluate(&policy_file, &analysis_file).unwrap();
    assert_eq!(report.findings[0].outcome, Outcome::Supported);
    policy.tempo.as_mut().unwrap().target_milli_bpm = 90_000;
    store(&policy_file, serde_json::to_vec(&policy).unwrap());
    let report = conformance::evaluate(&policy_file, &analysis_file).unwrap();
    assert_eq!(report.findings[0].outcome, Outcome::ConflictingEvidence);
    assert_eq!(report.findings[1].outcome, Outcome::Unverified);
    policy.source_contract_sha256 = "2".repeat(64);
    store(&policy_file, serde_json::to_vec(&policy).unwrap());
    assert!(conformance::evaluate(&policy_file, &analysis_file).is_err());
}
