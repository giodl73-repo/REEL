use std::{fs, path::Path};

use reel_music::{
    hash::{sha256_bytes, sha256_path},
    interchange::{ArtifactPurpose, InterchangeArtifact, InterchangeFormat, NormalizedPcm},
    source::{NetworkPolicy, RawPcmFormat},
    time::AudioTimebase,
};
use tempfile::tempdir;

fn copy_fixture(root: &Path) -> std::path::PathBuf {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("manifests/fixtures");
    let intake = fixtures.join("music-interchange-intake");
    let source = fixtures.join("music-repair-foundation");
    fs::create_dir_all(&intake).unwrap();
    fs::create_dir_all(&source).unwrap();
    for name in [
        "intake.yaml",
        "note-events.csv",
        "note-events-alt.csv",
        "annotations.jams",
    ] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-interchange-intake")
                .join(name),
            intake.join(name),
        )
        .unwrap();
    }
    for name in ["source.yaml", "source.u8"] {
        fs::copy(
            repository
                .join("manifests/fixtures/music-repair-foundation")
                .join(name),
            source.join(name),
        )
        .unwrap();
    }
    intake.join("intake.yaml")
}

#[test]
fn validates_existing_tool_csv_and_jams_without_executing_them() {
    let temporary = tempdir().unwrap();
    let intake = copy_fixture(temporary.path());
    let report = reel_music::interchange::validate(&intake).unwrap();
    assert_eq!(report.producers, 3);
    assert_eq!(report.artifacts, 3);
    assert_eq!(report.formats, ["csv", "jams"]);
    assert_eq!(report.normalized_stems, 0);
    assert!(!report.shareable);
    assert!(report.verified);
}

#[test]
fn detects_changed_external_bytes_and_false_format_declarations() {
    let temporary = tempdir().unwrap();
    let intake = copy_fixture(temporary.path());
    let folder = intake.parent().unwrap();
    fs::write(folder.join("note-events.csv"), b"changed,payload\n").unwrap();
    assert!(reel_music::interchange::validate(&intake).is_err());

    let intake = copy_fixture(&temporary.path().join("second"));
    let mut manifest = reel_music::interchange::load(&intake).unwrap();
    manifest.artifacts[0].format = InterchangeFormat::Midi;
    fs::write(&intake, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    assert!(reel_music::interchange::validate(&intake).is_err());
}

#[test]
fn binds_a_container_stem_to_exact_normalized_pcm() {
    let temporary = tempdir().unwrap();
    let intake_path = copy_fixture(temporary.path());
    let folder = intake_path.parent().unwrap();
    let wav = b"RIFF\x04\x00\x00\x00WAVE";
    fs::write(folder.join("vocals.wav"), wav).unwrap();
    fs::copy(
        temporary
            .path()
            .join("manifests/fixtures/music-repair-foundation/source.u8"),
        folder.join("vocals.u8"),
    )
    .unwrap();
    let pcm_hash = sha256_path(&folder.join("vocals.u8")).unwrap();
    let mut manifest = reel_music::interchange::load(&intake_path).unwrap();
    manifest.artifacts.push(InterchangeArtifact {
        id: "synthetic-vocal-stem".into(),
        producer_id: "annotation-fixture".into(),
        purpose: ArtifactPurpose::Stem,
        format: InterchangeFormat::Wav,
        path: "vocals.wav".into(),
        sha256: sha256_bytes(wav),
        bytes: wav.len() as u64,
        semantic_roles: vec!["vocal-estimate".into()],
        uncertainty: "Synthetic bytes test normalization lineage only.".into(),
        normalized_pcm: Some(NormalizedPcm {
            path: "vocals.u8".into(),
            sha256: pcm_hash.clone(),
            decoded_pcm_sha256: pcm_hash,
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 62,
            },
            decoder_id: "fixture-decoder".into(),
            decoder_version: "1".into(),
            parameters_sha256: "7777777777777777777777777777777777777777777777777777777777777777"
                .into(),
            network_policy: NetworkPolicy::Denied,
        }),
    });
    fs::write(&intake_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let report = reel_music::interchange::validate(&intake_path).unwrap();
    assert_eq!(report.artifacts, 4);
    assert_eq!(report.normalized_stems, 1);
    assert!(report.formats.contains(&"wav".to_string()));
}
