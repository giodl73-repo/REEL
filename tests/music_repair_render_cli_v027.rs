use std::{fs, path::PathBuf, process::Command};

use reel_music::{
    AuthorityRef,
    hash::sha256_path,
    repair::{Operation, RepairManifest, Review, SourceRef},
    source::{Egress, Media, NetworkPolicy, RawPcmFormat, SourceManifest},
    time::{AudioTimebase, MusicalTimebase, RoundingMode, SampleRange},
};

#[test]
#[ignore = "requires external FFmpeg and renders a generated raw-PCM fixture"]
fn real_ffmpeg_cut_render_is_exact_and_rechecks_retained_evidence() {
    let reel = env!("CARGO_BIN_EXE_reel");
    let temp = tempfile::tempdir().expect("tempdir");
    let pcm = temp.path().join("synthetic.raw");
    let pattern = [
        128_u8, 150, 169, 181, 185, 181, 169, 150, 128, 106, 87, 75, 71, 75, 87, 112,
    ];
    let source_bytes = pattern.repeat(12);
    fs::write(&pcm, &source_bytes).expect("write synthetic PCM");
    let pcm_hash = sha256_path(&pcm).expect("hash PCM");
    let timebase = AudioTimebase {
        sample_rate_hz: 8_000,
        channels: 1,
        samples_per_channel: 192,
    };
    let source = temp.path().join("source.yaml");
    let source_manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "render-synthetic-phrase".into(),
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
            artifact_id: "render-generated-pcm".into(),
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
    let repair = temp.path().join("repair.yaml");
    let repair_manifest = RepairManifest {
        schema: "reel.music-repair.v0.1".into(),
        repair_id: "render-single-cut".into(),
        source: SourceRef {
            manifest: source.clone(),
            sha256: sha256_path(&source).expect("hash source"),
        },
        source_id: source_manifest.source_id,
        decoded_pcm_sha256: source_manifest.media.decoded_pcm_sha256,
        timebase,
        operations: vec![Operation::Cut {
            id: "remove-repeat".into(),
            range: SampleRange {
                start: 64,
                end: 128,
            },
        }],
        changed_envelopes: vec![SampleRange {
            start: 64,
            end: 128,
        }],
        locks: vec![
            SampleRange { start: 0, end: 64 },
            SampleRange {
                start: 128,
                end: 192,
            },
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

    let edl = temp.path().join("repair-edl.json");
    assert_success(
        Command::new(reel)
            .arg("music-repair-compile")
            .arg(&repair)
            .arg("--output-path")
            .arg(&edl)
            .args(["--output", "json"]),
        "EDL compilation",
    );
    let candidate = temp.path().join("candidate.raw");
    let evidence = temp.path().join("evidence.json");
    assert_success(
        Command::new(reel)
            .arg("music-repair-render")
            .arg(&edl)
            .arg(&repair)
            .arg("--output-pcm")
            .arg(&candidate)
            .arg("--evidence-path")
            .arg(&evidence)
            .args(["--output", "json"]),
        "FFmpeg render",
    );
    assert_eq!(
        fs::read(&candidate).expect("read candidate"),
        [&source_bytes[..64], &source_bytes[128..]].concat()
    );
    assert_success(
        Command::new(reel)
            .arg("music-repair-evidence-check")
            .arg(&evidence)
            .arg(&edl)
            .arg(&repair)
            .arg(&candidate)
            .args(["--output", "json"]),
        "evidence recheck",
    );

    let before = fs::read(&candidate).expect("read retained candidate");
    let rerender = Command::new(reel)
        .arg("music-repair-render")
        .arg(&edl)
        .arg(&repair)
        .arg("--output-pcm")
        .arg(&candidate)
        .arg("--evidence-path")
        .arg(&evidence)
        .args(["--output", "json"])
        .output()
        .expect("rerender command runs");
    assert!(!rerender.status.success());
    assert_eq!(
        fs::read(&candidate).expect("read candidate after refusal"),
        before
    );
}

fn assert_success(command: &mut Command, label: &str) {
    let output = command.output().expect("command runs");
    assert!(
        output.status.success(),
        "{label}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"verified\": true"), "{label}: {stdout}");
}
