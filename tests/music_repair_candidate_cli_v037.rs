use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use reel_music::{
    AuthorityRef, DecisionRef,
    hash::sha256_path,
    repair::Operation,
    repair_candidate::{
        CandidateBinding, EvidenceBinding, IntentBinding, ListeningGate, ListeningStatus,
        RepairCandidateManifest, SelectionGate, SelectionStatus,
    },
    time::SampleRange,
};
use tempfile::tempdir;

fn copy_base(root: &Path) -> PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = root.join("manifests/fixtures");
    for (directory, names) in [
        ("music-repair-intent", &["intent.yaml"][..]),
        (
            "music-model-corrected",
            &["draft.yaml", "model.yaml", "analysis.yaml"][..],
        ),
        (
            "music-repair-foundation",
            &["repair.yaml", "source.yaml", "source.u8"][..],
        ),
    ] {
        let destination = fixtures.join(directory);
        fs::create_dir_all(&destination).unwrap();
        for name in names {
            fs::copy(
                repository
                    .join("manifests/fixtures")
                    .join(directory)
                    .join(name),
                destination.join(name),
            )
            .unwrap();
        }
    }
    fixtures
}

fn decision(id: &str, digit: char) -> DecisionRef {
    DecisionRef {
        artifact_id: id.into(),
        sha256: digit.to_string().repeat(64),
    }
}

fn build_selected_candidate(root: &Path) -> PathBuf {
    let fixtures = copy_base(root);
    let source_dir = fixtures.join("music-repair-foundation");
    let source_path = source_dir.join("source.yaml");
    let pcm_path = source_dir.join("source.u8");
    let pattern = [
        128_u8, 150, 169, 181, 185, 181, 169, 150, 128, 106, 87, 75, 71, 75, 87, 112,
    ];
    let source_bytes = pattern.repeat(12);
    fs::write(&pcm_path, &source_bytes).unwrap();
    let pcm_sha = sha256_path(&pcm_path).unwrap();
    let mut source = reel_music::source::load(&source_path).unwrap();
    source.media.sha256 = pcm_sha.clone();
    source.media.decoded_pcm_sha256 = pcm_sha.clone();
    source.media.timebase.samples_per_channel = 192;
    source.authority.content_sha256 = pcm_sha;
    fs::write(&source_path, serde_yaml::to_string(&source).unwrap()).unwrap();
    let source_report = reel_music::source::validate(&source_path).unwrap();

    let repair_path = source_dir.join("repair.yaml");
    let mut repair = reel_music::repair::load(&repair_path).unwrap();
    repair.source.sha256 = source_report.manifest_sha256.clone();
    repair.decoded_pcm_sha256 = source_report.decoded_pcm_sha256.clone();
    repair.timebase.samples_per_channel = 192;
    repair.operations = vec![Operation::Cut {
        id: "remove-synthetic-repeat".into(),
        range: SampleRange {
            start: 64,
            end: 128,
        },
    }];
    repair.changed_envelopes = vec![SampleRange {
        start: 64,
        end: 128,
    }];
    repair.locks = vec![
        SampleRange { start: 0, end: 64 },
        SampleRange {
            start: 128,
            end: 192,
        },
    ];
    fs::write(&repair_path, serde_yaml::to_string(&repair).unwrap()).unwrap();
    let repair_report = reel_music::repair::validate(&repair_path).unwrap();

    let model_dir = fixtures.join("music-model-corrected");
    let analysis_path = model_dir.join("analysis.yaml");
    let mut analysis = reel_music::analysis::load(&analysis_path).unwrap();
    analysis.source.manifest_sha256 = source_report.manifest_sha256.clone();
    analysis.source.contract_sha256 = source_report.contract_sha256.clone();
    analysis.source.decoded_pcm_sha256 = source_report.decoded_pcm_sha256.clone();
    for observation in &mut analysis.observations {
        if observation.source.end == 62 {
            observation.source.end = 192;
        }
    }
    fs::write(&analysis_path, serde_yaml::to_string(&analysis).unwrap()).unwrap();
    let analysis_report = reel_music::analysis::validate(&analysis_path).unwrap();

    let model_path = model_dir.join("model.yaml");
    let mut model = reel_music::model::load(&model_path).unwrap();
    model.source.manifest_sha256 = source_report.manifest_sha256.clone();
    model.source.contract_sha256 = source_report.contract_sha256.clone();
    model.source.decoded_pcm_sha256 = source_report.decoded_pcm_sha256.clone();
    model.analyses[0].manifest_sha256 = analysis_report.manifest_sha256;
    model.analyses[0].contract_sha256 = analysis_report.contract_sha256;
    fs::write(&model_path, serde_yaml::to_string(&model).unwrap()).unwrap();
    let model_report = reel_music::model::validate(&model_path).unwrap();

    let draft_path = model_dir.join("draft.yaml");
    let mut draft = reel_music::model_draft::load(&draft_path).unwrap();
    draft.model.manifest_sha256 = model_report.manifest_sha256;
    draft.model.contract_sha256 = model_report.contract_sha256;
    fs::write(&draft_path, serde_yaml::to_string(&draft).unwrap()).unwrap();
    let draft_report = reel_music::model_draft::validate(&draft_path).unwrap();

    let intent_path = fixtures.join("music-repair-intent/intent.yaml");
    let mut intent = reel_music::repair_intent::load(&intent_path).unwrap();
    intent.model_draft.manifest_sha256 = draft_report.manifest_sha256;
    intent.model_draft.contract_sha256 = draft_report.contract_sha256;
    intent.repair.manifest_sha256 = repair_report.manifest_sha256.clone();
    intent.repair.contract_sha256 = repair_report.contract_sha256.clone();
    fs::write(&intent_path, serde_yaml::to_string(&intent).unwrap()).unwrap();
    let intent_report = reel_music::repair_intent::validate(&intent_path).unwrap();

    let candidate_dir = fixtures.join("music-repair-candidate");
    fs::create_dir_all(&candidate_dir).unwrap();
    let edl_path = candidate_dir.join("repair-edl.json");
    reel_music::edl::write(&repair_path, &edl_path).unwrap();
    let candidate_path = candidate_dir.join("candidate.u8");
    fs::write(
        &candidate_path,
        [&source_bytes[..64], &source_bytes[128..]].concat(),
    )
    .unwrap();
    let evidence_path = candidate_dir.join("evidence.json");
    let evidence = reel_music::evidence::analyze(
        &edl_path,
        &repair_path,
        &candidate_path,
        "test-exact-concatenation",
        "v1",
    )
    .unwrap();
    assert!(evidence.passed, "{:?}", evidence.violations);
    let evidence_report = reel_music::evidence::write(&evidence_path, &evidence).unwrap();

    let candidate_manifest = RepairCandidateManifest {
        schema: reel_music::repair_candidate::SCHEMA.into(),
        candidate_id: "synthetic-selected-repair-candidate".into(),
        intent: IntentBinding {
            manifest: PathBuf::from("../music-repair-intent/intent.yaml"),
            manifest_sha256: intent_report.manifest_sha256,
            contract_sha256: intent_report.contract_sha256,
            intent_id: intent_report.intent_id,
        },
        candidate_pcm: CandidateBinding {
            path: PathBuf::from("candidate.u8"),
            sha256: evidence_report.candidate_pcm_sha256,
        },
        evidence: EvidenceBinding {
            manifest: PathBuf::from("evidence.json"),
            manifest_sha256: evidence_report.evidence_sha256,
            contract_sha256: evidence_report.evidence_contract_sha256,
            edl: PathBuf::from("repair-edl.json"),
            repair: PathBuf::from("../music-repair-foundation/repair.yaml"),
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "synthetic-candidate-authority".into(),
            content_sha256: "7".repeat(64),
            status: "fixture-only".into(),
            required_roles: vec!["music-reconstruction-engineer".into(), "editor".into()],
            decision_refs: vec![],
        },
        listening: ListeningGate {
            status: ListeningStatus::Passed,
            decision: Some(decision("synthetic-listening-pass", '8')),
        },
        selection: SelectionGate {
            status: SelectionStatus::Selected,
            decision: Some(decision("synthetic-candidate-selection", '9')),
        },
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    let manifest_path = candidate_dir.join("candidate.yaml");
    fs::write(
        &manifest_path,
        serde_yaml::to_string(&candidate_manifest).unwrap(),
    )
    .unwrap();
    manifest_path
}

fn build_rejected_candidate(root: &Path) -> PathBuf {
    let fixtures = copy_base(root);
    let repair_path = fixtures.join("music-repair-foundation/repair.yaml");
    let intent_path = fixtures.join("music-repair-intent/intent.yaml");
    let intent_report = reel_music::repair_intent::validate(&intent_path).unwrap();
    let candidate_dir = fixtures.join("music-repair-candidate");
    fs::create_dir_all(&candidate_dir).unwrap();
    let edl_path = candidate_dir.join("repair-edl.json");
    reel_music::edl::write(&repair_path, &edl_path).unwrap();
    let source = fs::read(fixtures.join("music-repair-foundation/source.u8")).unwrap();
    let candidate_path = candidate_dir.join("candidate.u8");
    fs::write(&candidate_path, [&source[..16], &source[32..]].concat()).unwrap();
    let evidence_path = candidate_dir.join("evidence.json");
    let evidence = reel_music::evidence::analyze(
        &edl_path,
        &repair_path,
        &candidate_path,
        "test-exact-concatenation",
        "v1",
    )
    .unwrap();
    assert!(!evidence.passed);
    let evidence_report = reel_music::evidence::write(&evidence_path, &evidence).unwrap();
    let manifest = RepairCandidateManifest {
        schema: reel_music::repair_candidate::SCHEMA.into(),
        candidate_id: "synthetic-rejected-repair-candidate".into(),
        intent: IntentBinding {
            manifest: PathBuf::from("../music-repair-intent/intent.yaml"),
            manifest_sha256: intent_report.manifest_sha256,
            contract_sha256: intent_report.contract_sha256,
            intent_id: intent_report.intent_id,
        },
        candidate_pcm: CandidateBinding {
            path: PathBuf::from("candidate.u8"),
            sha256: evidence_report.candidate_pcm_sha256,
        },
        evidence: EvidenceBinding {
            manifest: PathBuf::from("evidence.json"),
            manifest_sha256: evidence_report.evidence_sha256,
            contract_sha256: evidence_report.evidence_contract_sha256,
            edl: PathBuf::from("repair-edl.json"),
            repair: PathBuf::from("../music-repair-foundation/repair.yaml"),
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "synthetic-rejected-candidate-authority".into(),
            content_sha256: "a".repeat(64),
            status: "fixture-only".into(),
            required_roles: vec!["music-reconstruction-engineer".into(), "editor".into()],
            decision_refs: vec![],
        },
        listening: ListeningGate {
            status: ListeningStatus::Failed,
            decision: Some(decision("synthetic-listening-failure", 'b')),
        },
        selection: SelectionGate {
            status: SelectionStatus::Rejected,
            decision: Some(decision("synthetic-candidate-rejection", 'c')),
        },
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    let manifest_path = candidate_dir.join("candidate.yaml");
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    manifest_path
}

#[test]
fn cli_rechecks_and_selects_exact_model_bound_candidate() {
    let temporary = tempdir().unwrap();
    let manifest = build_selected_candidate(temporary.path());
    let output = Command::new(env!("CARGO_BIN_EXE_reel"))
        .arg("music-repair-candidate-check")
        .arg(&manifest)
        .args(["--output", "json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["technical_passed"], true);
    assert_eq!(report["listening_passed"], true);
    assert_eq!(report["eligible_for_selection"], true);
    assert_eq!(report["selected"], true);
    assert_eq!(report["shareable"], false);
}

#[test]
fn rejects_tampered_candidate_evidence_and_gate_shortcuts() {
    let temporary = tempdir().unwrap();
    let manifest_path = build_selected_candidate(temporary.path());
    let candidate_path = manifest_path.parent().unwrap().join("candidate.u8");
    let mut bytes = fs::read(&candidate_path).unwrap();
    bytes[0] ^= 1;
    fs::write(&candidate_path, bytes).unwrap();
    assert!(reel_music::repair_candidate::validate(&manifest_path).is_err());

    let manifest_path = build_selected_candidate(&temporary.path().join("gate"));
    let mut manifest = reel_music::repair_candidate::load(&manifest_path).unwrap();
    manifest.listening.status = ListeningStatus::Pending;
    manifest.listening.decision = None;
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::repair_candidate::validate(&manifest_path).is_err());

    let manifest_path = build_selected_candidate(&temporary.path().join("binding"));
    let mut manifest = reel_music::repair_candidate::load(&manifest_path).unwrap();
    manifest.evidence.contract_sha256 = "0".repeat(64);
    fs::write(&manifest_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::repair_candidate::validate(&manifest_path).is_err());
}

#[test]
fn retains_failed_candidate_only_as_an_explicit_rejection() {
    let temporary = tempdir().unwrap();
    let manifest = build_rejected_candidate(temporary.path());
    let report = reel_music::repair_candidate::validate(&manifest).unwrap();
    assert!(!report.technical_passed);
    assert!(report.listening_complete);
    assert!(!report.eligible_for_selection);
    assert!(report.rejected);
    assert!(!report.selected);

    let mut changed = reel_music::repair_candidate::load(&manifest).unwrap();
    changed.selection.status = SelectionStatus::Selected;
    fs::write(&manifest, serde_yaml::to_string(&changed).unwrap()).unwrap();
    assert!(reel_music::repair_candidate::validate(&manifest).is_err());
}
