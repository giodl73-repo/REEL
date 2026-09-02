use std::{fs, path::PathBuf};

use reel_music::{
    AuthorityRef, DecisionRef,
    external_repair::{
        Adapter, CandidateDisposition, CandidateManifest, CandidatePolicy, FileBinding,
        LyricEvidence, PerformanceMode, Permissions, RepairBinding, RequestManifest,
        TargetPerformance,
    },
    hash::sha256_path,
    repair::{Operation, RepairManifest, Review, SourceRef},
    source::{Egress, Media, NetworkPolicy, RawPcmFormat, SourceManifest},
    time::{AudioTimebase, MusicalTimebase, RoundingMode, SampleRange},
};

#[test]
fn bounded_external_candidate_requires_independent_lyrics_and_exact_outside_region() {
    let temp = tempfile::tempdir().unwrap();
    let pcm = temp.path().join("source.raw");
    fs::write(&pcm, (120_u8..136).collect::<Vec<_>>()).unwrap();
    let pcm_sha = sha256_path(&pcm).unwrap();
    let timebase = AudioTimebase {
        sample_rate_hz: 8_000,
        channels: 1,
        samples_per_channel: 16,
    };
    let source = temp.path().join("source.yaml");
    let source_manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "synthetic-external-repair".into(),
        media: Media {
            path: PathBuf::from("source.raw"),
            sha256: pcm_sha.clone(),
            format: RawPcmFormat::RawPcmU8,
            timebase,
            decoded_pcm_sha256: pcm_sha.clone(),
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 960,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        authority: AuthorityRef {
            namespace: "fixture".into(),
            artifact_id: "synthetic".into(),
            content_sha256: pcm_sha.clone(),
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
    fs::write(&source, serde_yaml::to_string(&source_manifest).unwrap()).unwrap();
    let source_report = reel_music::source::validate(&source).unwrap();
    let repair = temp.path().join("repair.yaml");
    let repair_manifest = RepairManifest {
        schema: "reel.music-repair.v0.1".into(),
        repair_id: "bounded-vocal".into(),
        source: SourceRef {
            manifest: PathBuf::from("source.yaml"),
            sha256: source_report.manifest_sha256,
        },
        source_id: source_manifest.source_id,
        decoded_pcm_sha256: pcm_sha.clone(),
        timebase,
        beat_grid: None,
        operations: vec![Operation::MatchGain {
            id: "vocal-region".into(),
            range: SampleRange { start: 4, end: 8 },
            target_millilufs: -18_000,
        }],
        changed_envelopes: vec![SampleRange { start: 4, end: 8 }],
        locks: vec![
            SampleRange { start: 0, end: 4 },
            SampleRange { start: 8, end: 16 },
        ],
        review: Review {
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
    fs::write(&repair, serde_yaml::to_string(&repair_manifest).unwrap()).unwrap();
    let repair_report = reel_music::repair::validate(&repair).unwrap();
    let text = temp.path().join("target.txt");
    fs::write(&text, "synthetic target phrase\n").unwrap();
    let text_sha = sha256_path(&text).unwrap();
    let request = temp.path().join("request.yaml");
    let request_manifest = RequestManifest {
        schema: reel_music::external_repair::REQUEST_SCHEMA.into(),
        request_id: "synthetic-request".into(),
        repair: RepairBinding {
            manifest: PathBuf::from("repair.yaml"),
            manifest_sha256: sha256_path(&repair).unwrap(),
            contract_sha256: repair_report.contract_sha256,
            repair_id: "bounded-vocal".into(),
        },
        operation_id: "vocal-region".into(),
        region: SampleRange { start: 4, end: 8 },
        target: TargetPerformance {
            mode: PerformanceMode::ReSing,
            language: "und".into(),
            text: FileBinding {
                path: PathBuf::from("target.txt"),
                sha256: text_sha.clone(),
            },
            exact_text_authority_sha256: "1".repeat(64),
        },
        retained_music: FileBinding {
            path: PathBuf::from("source.raw"),
            sha256: pcm_sha.clone(),
        },
        adapter: Adapter {
            kind: "synthetic-generator".into(),
            version: "1".into(),
            executable: "not-executed-by-reel".into(),
            model_id: "fixture-model".into(),
            checkpoint_sha256: "2".repeat(64),
            model_license: "fixture-only".into(),
            seed: 42,
            local_only: true,
            network_policy: NetworkPolicy::Denied,
            auto_download: false,
            parameters: Default::default(),
        },
        permissions: Permissions {
            voice_consent_status: "recorded".into(),
            voice_consent_evidence: vec![decision("consent")],
            third_party_upload: false,
            public_release: false,
        },
        candidate_policy: CandidatePolicy {
            maximum_boundary_delta_millionths: 100_000,
            maximum_region_loudness_delta_millidb: 1_000,
            minimum_lyric_coverage_millionths: 1_000_000,
        },
    };
    fs::write(&request, serde_yaml::to_string(&request_manifest).unwrap()).unwrap();
    let receipt = temp.path().join("request.receipt.json");
    reel_music::external_repair::write_plan(&request, &receipt, "test-v1").unwrap();
    let receipt_text = fs::read_to_string(&receipt).unwrap();
    assert!(!receipt_text.contains(temp.path().to_str().unwrap()));
    assert!(!receipt_text.contains("synthetic target phrase"));

    let candidate_pcm = temp.path().join("candidate.raw");
    fs::copy(&pcm, &candidate_pcm).unwrap();
    let candidate_sha = sha256_path(&candidate_pcm).unwrap();
    let lyric = temp.path().join("lyric.yaml");
    fs::write(
        &lyric,
        serde_yaml::to_string(&LyricEvidence {
            schema: reel_music::external_repair::LYRIC_EVIDENCE_SCHEMA.into(),
            candidate_pcm_sha256: candidate_sha.clone(),
            target_text_sha256: text_sha,
            analyzer_id: "independent-synthetic-lyric-audit".into(),
            analyzer_version: "1".into(),
            coverage_millionths: 1_000_000,
            exact_text_matched: true,
            passed: true,
        })
        .unwrap(),
    )
    .unwrap();
    let candidate = temp.path().join("candidate.yaml");
    let manifest = CandidateManifest {
        schema: reel_music::external_repair::CANDIDATE_SCHEMA.into(),
        candidate_id: "candidate-one".into(),
        request: binding("request.yaml", &request),
        plan_receipt: binding("request.receipt.json", &receipt),
        candidate_pcm: binding("candidate.raw", &candidate_pcm),
        format: RawPcmFormat::RawPcmU8,
        timebase,
        lyric_evidence: binding("lyric.yaml", &lyric),
        disposition: CandidateDisposition::AuditionReady,
        disposition_decision: Some(decision("technical-disposition")),
    };
    fs::write(&candidate, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let report = reel_music::external_repair::validate_candidate(&candidate).unwrap();
    assert!(report.technical_passed);
    assert!(report.outside_region_exact);
    assert!(report.audition_ready);
    assert!(!report.selected);
    assert!(!report.released);

    let mut damaged = fs::read(&candidate_pcm).unwrap();
    damaged[0] ^= 1;
    fs::write(&candidate_pcm, damaged).unwrap();
    let damaged_sha = sha256_path(&candidate_pcm).unwrap();
    let mut lyric_evidence: LyricEvidence =
        serde_yaml::from_slice(&fs::read(&lyric).unwrap()).unwrap();
    lyric_evidence.candidate_pcm_sha256 = damaged_sha;
    fs::write(&lyric, serde_yaml::to_string(&lyric_evidence).unwrap()).unwrap();
    let mut rejected = manifest;
    rejected.candidate_pcm.sha256 = sha256_path(&candidate_pcm).unwrap();
    rejected.lyric_evidence.sha256 = sha256_path(&lyric).unwrap();
    rejected.disposition = CandidateDisposition::Rejected;
    rejected.disposition_decision = Some(decision("rejected-after-independent-check"));
    fs::write(&candidate, serde_yaml::to_string(&rejected).unwrap()).unwrap();
    let report = reel_music::external_repair::validate_candidate(&candidate).unwrap();
    assert!(!report.technical_passed);
    assert!(!report.outside_region_exact);
    assert!(report.rejected);
    assert!(!report.audition_ready);
}

fn binding(name: &str, path: &std::path::Path) -> FileBinding {
    FileBinding {
        path: PathBuf::from(name),
        sha256: sha256_path(path).unwrap(),
    }
}

fn decision(id: &str) -> DecisionRef {
    DecisionRef {
        artifact_id: id.into(),
        sha256: "3".repeat(64),
    }
}
