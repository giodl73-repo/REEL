use std::{
    fs,
    path::{Path, PathBuf},
};

use reel_music::{
    AuthorityRef,
    hash::sha256_path,
    neutral,
    repair::{Operation, RepairManifest, Review, SourceRef},
    source::{Egress, Media, NetworkPolicy, RawPcmFormat, SourceManifest},
    time::{AudioTimebase, MusicalTimebase, RoundingMode, SampleRange},
};
use tempfile::TempDir;

struct Fixture {
    _temp: TempDir,
    pcm: PathBuf,
    source: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let pcm = temp.path().join("source.raw");
    fs::write(&pcm, (0_u8..16).collect::<Vec<_>>()).expect("write synthetic PCM");
    let pcm_hash = sha256_path(&pcm).expect("hash PCM");
    let source = temp.path().join("source.yaml");
    let manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "synthetic-phrase".into(),
        media: Media {
            path: PathBuf::from("source.raw"),
            sha256: pcm_hash.clone(),
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 16,
            },
            decoded_pcm_sha256: pcm_hash.clone(),
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 960,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "generated-test-tone".into(),
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
        serde_yaml::to_string(&manifest).expect("serialize source"),
    )
    .expect("write source");
    Fixture {
        _temp: temp,
        pcm,
        source,
    }
}

fn repair_manifest(source_path: &Path) -> RepairManifest {
    let source_report = reel_music::source::validate(source_path).expect("source validates");
    RepairManifest {
        schema: "reel.music-repair.v0.1".into(),
        repair_id: "synthetic-single-cut".into(),
        source: SourceRef {
            manifest: source_path.to_path_buf(),
            sha256: source_report.manifest_sha256,
        },
        source_id: "synthetic-phrase".into(),
        decoded_pcm_sha256: source_report.decoded_pcm_sha256,
        timebase: AudioTimebase {
            sample_rate_hz: 8_000,
            channels: 1,
            samples_per_channel: 16,
        },
        operations: vec![Operation::Cut {
            id: "remove-repeat".into(),
            range: SampleRange { start: 4, end: 8 },
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
            decision_refs: Vec::new(),
        },
    }
}

#[test]
fn validates_raw_pcm_source_and_rejects_stale_hash() {
    let fixture = fixture();
    let report = reel_music::source::validate(&fixture.source).expect("source validates");
    assert!(report.verified);
    assert_eq!(report.bytes, 16);
    assert_eq!(report.media_sha256, report.decoded_pcm_sha256);
    assert!(!report.shareable);

    fs::write(&fixture.pcm, [99_u8; 16]).expect("mutate PCM");
    let error = reel_music::source::validate(&fixture.source).expect_err("stale hash rejected");
    assert!(error.to_string().contains("media sha256"));
}

#[test]
fn proves_neutral_pcm_identity_and_rejects_changed_candidate() {
    let fixture = fixture();
    let plan = fixture._temp.path().join("neutral.json");
    let planned = neutral::write_plan(&fixture.source, &plan).expect("plan writes");
    assert!(planned.verified);
    assert_eq!(planned.locked_samples, 16);
    assert!(!planned.shareable);

    let checked = neutral::check(&plan, &fixture.source, &fixture.pcm).expect("neutral checks");
    assert!(checked.decoded_pcm_equal);
    assert_eq!(planned.plan_contract_sha256, checked.plan_contract_sha256);
    assert!(!checked.shareable);

    let changed = fixture._temp.path().join("changed.raw");
    fs::write(&changed, [42_u8; 16]).expect("write changed candidate");
    let error =
        neutral::check(&plan, &fixture.source, &changed).expect_err("changed candidate rejected");
    assert!(error.to_string().contains("does not equal"));
}

#[test]
fn validates_complete_changed_and_locked_coverage() {
    let fixture = fixture();
    let repair = fixture._temp.path().join("repair.yaml");
    fs::write(
        &repair,
        serde_yaml::to_string(&repair_manifest(&fixture.source)).expect("serialize repair"),
    )
    .expect("write repair");

    let report = reel_music::repair::validate(&repair).expect("repair validates");
    assert!(report.complete_coverage);
    assert_eq!(report.changed_samples, 4);
    assert_eq!(report.locked_samples, 12);
    assert!(report.required_roles_present);
    assert!(!report.shareable);
}

#[test]
fn rejects_inferred_approval_and_unknown_operation_fields() {
    let fixture = fixture();
    let mut manifest = repair_manifest(&fixture.source);
    manifest.review.status = "approved".into();
    let repair = fixture._temp.path().join("inferred-approval.yaml");
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize"),
    )
    .expect("write repair");
    let error = reel_music::repair::validate(&repair).expect_err("decision evidence required");
    assert!(error.to_string().contains("requires decision_refs"));

    manifest.review.status = "not-reviewed".into();
    let text = serde_yaml::to_string(&manifest)
        .expect("serialize")
        .replacen("  range:\n", "  unexpected: true\n  range:\n", 1);
    let repair = fixture._temp.path().join("unknown-operation-field.yaml");
    fs::write(&repair, text).expect("write repair");
    let error = reel_music::repair::validate(&repair).expect_err("unknown field rejected");
    assert!(error.to_string().contains("not valid YAML"));
}

#[test]
fn rejects_lock_trespass_and_overlapping_operations() {
    let fixture = fixture();
    let mut manifest = repair_manifest(&fixture.source);
    manifest.locks[0].end = 5;
    let repair = fixture._temp.path().join("lock-trespass.yaml");
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize"),
    )
    .expect("write repair");
    let error = reel_music::repair::validate(&repair).expect_err("lock trespass rejected");
    assert!(error.to_string().contains("must not intersect locks"));

    let mut manifest = repair_manifest(&fixture.source);
    manifest.operations = vec![
        Operation::Cut {
            id: "first".into(),
            range: SampleRange { start: 4, end: 7 },
        },
        Operation::Crossfade {
            id: "second".into(),
            range: SampleRange { start: 6, end: 8 },
            curve: reel_music::repair::FadeCurve::EqualPower,
        },
    ];
    let repair = fixture._temp.path().join("overlap.yaml");
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize"),
    )
    .expect("write repair");
    let error = reel_music::repair::validate(&repair).expect_err("overlap rejected");
    assert!(error.to_string().contains("overlaps a prior operation"));
}

#[test]
fn canonical_contract_hash_ignores_yaml_key_order() {
    let fixture = fixture();
    let original = reel_music::source::validate(&fixture.source).expect("source validates");
    let text = fs::read_to_string(&fixture.source).expect("read source");
    let value: serde_yaml::Value = serde_yaml::from_str(&text).expect("parse yaml");
    let mapping = value.as_mapping().expect("mapping");
    let mut entries = mapping
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    entries.reverse();
    let reversed = serde_yaml::Mapping::from_iter(entries);
    let reordered_path = fixture._temp.path().join("source-reordered.yaml");
    fs::write(
        &reordered_path,
        serde_yaml::to_string(&serde_yaml::Value::Mapping(reversed)).expect("serialize reordered"),
    )
    .expect("write reordered");
    let reordered = reel_music::source::validate(&reordered_path).expect("reordered validates");
    assert_ne!(original.manifest_sha256, reordered.manifest_sha256);
    assert_eq!(original.contract_sha256, reordered.contract_sha256);
}
