mod common;

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use common::music_language::{authority, build_adaptation, decision};
use reel_music::{
    hash::sha256_path,
    language_adaptation::TextUnit,
    language_performance::{
        AdaptationBinding, BilingualComparison, ComparisonDimension, ConsentBinding, ConsentStatus,
        CreationEgress, LanguagePerformance, PcmBinding, PerformanceMethod, PerformanceProvenance,
        PerformedText, UnitAudit, UnitAuditOutcome,
    },
    repair_candidate::{ListeningGate, ListeningStatus, SelectionGate, SelectionStatus},
    source::RawPcmFormat,
    time::AudioTimebase,
};
use tempfile::tempdir;

fn pcm(path: &Path, value: u8) -> PcmBinding {
    fs::write(path, vec![value; 32_000]).unwrap();
    let sha256 = sha256_path(path).unwrap();
    PcmBinding {
        path: PathBuf::from(path.file_name().unwrap()),
        sha256: sha256.clone(),
        decoded_pcm_sha256: sha256,
        format: RawPcmFormat::RawPcmU8,
        timebase: AudioTimebase {
            sample_rate_hz: 8_000,
            channels: 1,
            samples_per_channel: 32_000,
        },
    }
}

fn passed(id: &str, digit: char) -> ListeningGate {
    ListeningGate {
        status: ListeningStatus::Passed,
        decision: Some(decision(id, digit)),
    }
}

fn build_fixture(root: &Path) -> PathBuf {
    let adaptation_path = build_adaptation(&root.join("adaptation"));
    let adaptation_report = reel_music::language_adaptation::validate(&adaptation_path).unwrap();
    let performance_root = root.join("performance");
    fs::create_dir_all(&performance_root).unwrap();

    let performed_text = performance_root.join("performed.txt");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("manifests/fixtures/music-language-adaptation/target.txt"),
        &performed_text,
    )
    .unwrap();
    let vocal_take = pcm(&performance_root.join("vocal-take.u8"), 132);
    let source_reference = pcm(&performance_root.join("source-reference.u8"), 124);

    let manifest = LanguagePerformance {
        schema: reel_music::language_performance::SCHEMA.into(),
        performance_id: "synthetic-english-performance-1".into(),
        adaptation: AdaptationBinding {
            manifest: adaptation_path.clone(),
            manifest_sha256: adaptation_report.manifest_sha256,
            contract_sha256: adaptation_report.contract_sha256,
            adaptation_id: adaptation_report.adaptation_id,
        },
        vocal_take,
        performed_text: PerformedText {
            language: "en".into(),
            path: PathBuf::from("performed.txt"),
            sha256: sha256_path(&performed_text).unwrap(),
            authority: authority("synthetic-performed-text", '6', "reviewed", true),
            units: vec![
                TextUnit {
                    id: "p1".into(),
                    byte_start: 0,
                    byte_end: 3,
                },
                TextUnit {
                    id: "p2".into(),
                    byte_start: 4,
                    byte_end: 8,
                },
                TextUnit {
                    id: "p3".into(),
                    byte_start: 9,
                    byte_end: 11,
                },
                TextUnit {
                    id: "p4".into(),
                    byte_start: 12,
                    byte_end: 16,
                },
                TextUnit {
                    id: "p5".into(),
                    byte_start: 17,
                    byte_end: 20,
                },
            ],
        },
        unit_audit: (1..=5)
            .map(|index| UnitAudit {
                target_unit_id: format!("t{index}"),
                performed_unit_ids: vec![format!("p{index}")],
                outcome: UnitAuditOutcome::Matched,
                rationale: "Exact synthetic target/performed unit match.".into(),
                decision: None,
            })
            .collect(),
        lyric_listening: passed("synthetic-lyric-listening", '7'),
        provenance: PerformanceProvenance {
            method: PerformanceMethod::NonIdentifiableFixtureTone,
            adapter_id: "reel-test-tone".into(),
            adapter_version: "1".into(),
            model_checkpoint: None,
            seed: Some("fixed-tone-132".into()),
            creation_egress: CreationEgress::LocalPrivate,
            egress_decision: None,
        },
        consent: ConsentBinding {
            subject_id: "non-identifiable-fixture".into(),
            status: ConsentStatus::NotApplicableFixture,
            operation: "generated constant-value fixture tone; no human voice".into(),
            service_runtime: "local REEL integration test".into(),
            audience: "automated test only".into(),
            retention: "temporary-directory lifetime".into(),
            reuse_scope: "REEL contract tests".into(),
            decision: Some(decision("fixture-consent-not-applicable", '8')),
        },
        comparison: BilingualComparison {
            source_reference,
            source_authority: authority("synthetic-source-reference", 'e', "reviewed", true),
            source_language: "x-source".into(),
            target_language: "en".into(),
            model_contract_sha256: adaptation_report.model_contract_sha256,
            source_blind_label: "variant-k".into(),
            target_blind_label: "variant-r".into(),
            review_dimensions: vec![
                ComparisonDimension::LyricFidelity,
                ComparisonDimension::Prosody,
                ComparisonDimension::CompositionRecognition,
                ComparisonDimension::AccompanimentContinuity,
                ComparisonDimension::MixBalance,
            ],
            listening: passed("synthetic-bilingual-listening", '9'),
        },
        authority: authority("synthetic-performance", 'a', "selected", true),
        selection: SelectionGate {
            status: SelectionStatus::Selected,
            decision: Some(decision("synthetic-performance-selection", 'b')),
        },
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "lyrics-vocal-adaptation-editor".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
                "platform-audience".into(),
            ],
            decision_refs: vec![],
        },
    };
    let path = performance_root.join("performance.yaml");
    write(&path, &manifest);
    path
}

fn write(path: &Path, manifest: &LanguagePerformance) {
    fs::write(path, serde_yaml::to_string(manifest).unwrap()).unwrap();
}

#[test]
fn cli_validates_selected_bilingual_performance_candidate() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-language-performance-check")
        .arg(&path)
        .arg("--output")
        .arg("json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["matched_units"], 5);
    assert_eq!(report["exception_units"], 0);
    assert_eq!(report["technical_passed"], true);
    assert_eq!(report["lyric_listening_passed"], true);
    assert_eq!(report["comparison_listening_passed"], true);
    assert_eq!(report["consent_satisfied"], true);
    assert_eq!(report["eligible_for_selection"], true);
    assert_eq!(report["selected"], true);
    assert_eq!(report["shareable"], false);
}

#[test]
fn rejects_audio_text_adaptation_and_comparison_tampering() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let vocal_path = path.parent().unwrap().join("vocal-take.u8");
    fs::write(&vocal_path, vec![133_u8; 32_000]).unwrap();
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("text"));
    fs::write(
        path.parent().unwrap().join("performed.txt"),
        "changed text\n",
    )
    .unwrap();
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("adaptation"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.adaptation.contract_sha256 = "f".repeat(64);
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("comparison"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.comparison.model_contract_sha256 = "e".repeat(64);
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("duration"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.vocal_take.timebase.samples_per_channel -= 1;
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());
}

#[test]
fn rejects_lyric_audit_consent_provenance_and_selection_shortcuts() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.unit_audit[0].outcome = UnitAuditOutcome::Changed;
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("coverage"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.unit_audit.pop();
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("lyrics"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.lyric_listening = ListeningGate {
        status: ListeningStatus::Pending,
        decision: None,
    };
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("consent"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.provenance.method = PerformanceMethod::SyntheticVoice;
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("egress"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.provenance.creation_egress = CreationEgress::ApprovedExternal;
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("comparison-listening"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.comparison.listening = ListeningGate {
        status: ListeningStatus::Pending,
        decision: None,
    };
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("labels"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.comparison.target_blind_label = manifest.comparison.source_blind_label.clone();
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("dimensions"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.comparison.review_dimensions.pop();
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());

    let path = build_fixture(&temporary.path().join("review"));
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest
        .review
        .required_roles
        .retain(|role| role != "platform-audience");
    write(&path, &manifest);
    assert!(reel_music::language_performance::validate(&path).is_err());
}

#[test]
fn retains_failed_lyric_candidate_as_explicit_rejection() {
    let temporary = tempdir().unwrap();
    let path = build_fixture(temporary.path());
    let mut manifest = reel_music::language_performance::load(&path).unwrap();
    manifest.lyric_listening = ListeningGate {
        status: ListeningStatus::Failed,
        decision: Some(decision("synthetic-lyric-failure", 'c')),
    };
    manifest.selection = SelectionGate {
        status: SelectionStatus::Rejected,
        decision: Some(decision("synthetic-performance-rejection", 'd')),
    };
    manifest.authority.status = "rejected".into();
    write(&path, &manifest);
    let report = reel_music::language_performance::validate(&path).unwrap();
    assert!(!report.lyric_listening_passed);
    assert!(!report.eligible_for_selection);
    assert!(!report.selected);
    assert!(report.rejected);
}
