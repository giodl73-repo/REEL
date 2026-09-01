use std::{fs, path::PathBuf, process::Command};

use reel_music::{
    AuthorityRef,
    hash::sha256_path,
    repair::{Operation, RepairManifest, Review, SourceRef},
    source::{Egress, Media, NetworkPolicy, RawPcmFormat, SourceManifest},
    time::{AudioTimebase, MusicalTimebase, RoundingMode, SampleRange},
};

#[test]
fn music_cli_validates_source_neutral_identity_and_repair_plan() {
    let reel = env!("CARGO_BIN_EXE_reel");
    let temp = tempfile::tempdir().expect("tempdir");
    let pcm = temp.path().join("synthetic.raw");
    fs::write(&pcm, (0_u8..24).collect::<Vec<_>>()).expect("write synthetic PCM");
    let pcm_hash = sha256_path(&pcm).expect("hash PCM");
    let timebase = AudioTimebase {
        sample_rate_hz: 8_000,
        channels: 1,
        samples_per_channel: 24,
    };
    let source = temp.path().join("source.yaml");
    let source_manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "cli-synthetic-phrase".into(),
        media: Media {
            path: PathBuf::from("synthetic.raw"),
            sha256: pcm_hash.clone(),
            format: RawPcmFormat::RawPcmU8,
            timebase,
            decoded_pcm_sha256: pcm_hash.clone(),
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 960,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "cli-generated-pcm".into(),
            content_sha256: pcm_hash,
            status: "fixture-only".into(),
            required_roles: vec!["music-reconstruction-engineer".into()],
            decision_refs: Vec::new(),
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

    assert_success(
        Command::new(reel)
            .args(["music-source-validate"])
            .arg(&source)
            .args(["--output", "json"]),
        "source validates",
    );

    let neutral = temp.path().join("neutral.json");
    assert_success(
        Command::new(reel)
            .args(["music-neutral-plan"])
            .arg(&source)
            .arg("--output-path")
            .arg(&neutral)
            .args(["--output", "json"]),
        "neutral plan writes",
    );
    assert_success(
        Command::new(reel)
            .args(["music-neutral-check"])
            .arg(&neutral)
            .arg(&source)
            .arg(&pcm)
            .args(["--output", "json"]),
        "neutral candidate verifies",
    );

    let source_hash = sha256_path(&source).expect("hash source manifest");
    let repair = temp.path().join("repair.yaml");
    let repair_manifest = RepairManifest {
        schema: "reel.music-repair.v0.1".into(),
        repair_id: "cli-single-cut".into(),
        source: SourceRef {
            manifest: source.clone(),
            sha256: source_hash,
        },
        source_id: source_manifest.source_id,
        decoded_pcm_sha256: source_manifest.media.decoded_pcm_sha256,
        timebase,
        operations: vec![Operation::Cut {
            id: "remove-repeat".into(),
            range: SampleRange { start: 8, end: 12 },
        }],
        changed_envelopes: vec![SampleRange { start: 8, end: 12 }],
        locks: vec![
            SampleRange { start: 0, end: 8 },
            SampleRange { start: 12, end: 24 },
        ],
        review: Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: Vec::new(),
        },
    };
    fs::write(
        &repair,
        serde_yaml::to_string(&repair_manifest).expect("serialize repair"),
    )
    .expect("write repair");
    assert_success(
        Command::new(reel)
            .args(["music-repair-plan"])
            .arg(&repair)
            .args(["--output", "json"]),
        "repair plan validates",
    );
}

#[test]
fn checked_in_music_fixture_has_portable_contract_hash() {
    let reel = env!("CARGO_BIN_EXE_reel");
    let output = Command::new(reel)
        .args([
            "music-source-validate",
            "manifests/fixtures/music-repair-foundation/source.yaml",
            "--output",
            "json",
        ])
        .output()
        .expect("fixture validation runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report = String::from_utf8_lossy(&output.stdout);
    assert!(report.contains(
        "\"contract_sha256\": \"2cc8f8b0de36676bb278133d05414e68ca4baa273ddcf67b36308b9019d6fb41\""
    ));
    assert!(report.contains("\"verified\": true"));
}

fn assert_success(command: &mut Command, label: &str) {
    let output = command.output().expect("command runs");
    assert!(
        output.status.success(),
        "{label}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"verified\": true"));
}
