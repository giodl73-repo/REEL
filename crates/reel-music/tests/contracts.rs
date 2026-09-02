use std::{
    fs,
    path::{Path, PathBuf},
};

use reel_music::{
    AuthorityRef, edl, evidence,
    hash::{canonical_sha256, sha256_path},
    neutral,
    repair::{
        AssetRange, BeatGrid, EqBand, FadeCurve, Operation, RepairManifest, Review, SourceRef,
    },
    repair_render,
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
        beat_grid: None,
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

fn seam_fixture() -> (Fixture, PathBuf) {
    let temp = tempfile::tempdir().expect("tempdir");
    let pcm = temp.path().join("source.raw");
    let pattern = [
        128_u8, 150, 169, 181, 185, 181, 169, 150, 128, 106, 87, 75, 71, 75, 87, 112,
    ];
    let bytes = pattern.repeat(12);
    fs::write(&pcm, &bytes).expect("write periodic PCM");
    let pcm_hash = sha256_path(&pcm).expect("hash PCM");
    let source = temp.path().join("source.yaml");
    let source_manifest = SourceManifest {
        schema: "reel.music-source.v0.1".into(),
        source_id: "synthetic-periodic-phrase".into(),
        media: Media {
            path: PathBuf::from("source.raw"),
            sha256: pcm_hash.clone(),
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 192,
            },
            decoded_pcm_sha256: pcm_hash.clone(),
        },
        musical_timebase: MusicalTimebase {
            pulses_per_quarter: 960,
            rounding: RoundingMode::HalfAwayFromZero,
        },
        authority: AuthorityRef {
            namespace: "synthetic-fixture".into(),
            artifact_id: "generated-periodic-tone".into(),
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
    let source_report = reel_music::source::validate(&source).expect("source validates");
    let repair = temp.path().join("repair.yaml");
    let repair_manifest = RepairManifest {
        schema: "reel.music-repair.v0.1".into(),
        repair_id: "remove-four-periods".into(),
        source: SourceRef {
            manifest: source.clone(),
            sha256: source_report.manifest_sha256,
        },
        source_id: "synthetic-periodic-phrase".into(),
        decoded_pcm_sha256: source_report.decoded_pcm_sha256,
        timebase: source_manifest.media.timebase,
        beat_grid: None,
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
    (
        Fixture {
            _temp: temp,
            pcm,
            source,
        },
        repair,
    )
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

#[test]
fn compiles_cut_only_edl_and_proves_exact_outside_regions_and_seam() {
    let (fixture, repair) = seam_fixture();
    let edl_path = fixture._temp.path().join("repair-edl.json");
    let compiled = edl::write(&repair, &edl_path).expect("EDL compiles");
    assert_eq!(compiled.segments, 2);
    assert_eq!(compiled.cuts, 1);
    assert_eq!(compiled.output_samples_per_channel, 128);
    assert!(!compiled.shareable);

    let source = fs::read(&fixture.pcm).expect("read source");
    let candidate = fixture._temp.path().join("candidate.raw");
    let expected = [&source[..64], &source[128..]].concat();
    fs::write(&candidate, expected).expect("write cut candidate");
    let analyzed = evidence::analyze(
        &edl_path,
        &repair,
        &candidate,
        "test-exact-concatenation",
        "v1",
    )
    .expect("evidence analyzes");
    assert!(analyzed.outside_regions_exact);
    assert!(analyzed.passed, "violations: {:?}", analyzed.violations);
    assert_eq!(analyzed.joins[0].window_correlation_millionths, 1_000_000);
    assert!(analyzed.joins[0].right_tail_exact);

    let evidence_path = fixture._temp.path().join("evidence.json");
    evidence::write(&evidence_path, &analyzed).expect("evidence writes");
    let checked =
        evidence::check(&evidence_path, &edl_path, &repair, &candidate).expect("evidence rechecks");
    assert!(checked.passed);
}

#[test]
fn evidence_rejects_mutation_outside_the_declared_cut() {
    let (fixture, repair) = seam_fixture();
    let edl_path = fixture._temp.path().join("repair-edl.json");
    edl::write(&repair, &edl_path).expect("EDL compiles");
    let source = fs::read(&fixture.pcm).expect("read source");
    let mut changed = [&source[..64], &source[128..]].concat();
    changed[3] ^= 1;
    let candidate = fixture._temp.path().join("mutated.raw");
    fs::write(&candidate, changed).expect("write candidate");

    let analyzed = evidence::analyze(
        &edl_path,
        &repair,
        &candidate,
        "test-mutated-concatenation",
        "v1",
    )
    .expect("evidence analyzes");
    assert!(!analyzed.outside_regions_exact);
    assert!(!analyzed.passed);
    assert!(
        analyzed
            .violations
            .iter()
            .any(|violation| violation.contains("keep-001"))
    );
}

#[test]
fn cut_only_edl_rejects_planned_non_cut_operation() {
    let (fixture, repair) = seam_fixture();
    let mut manifest = reel_music::repair::load(&repair).expect("load repair");
    manifest.operations = vec![Operation::Crossfade {
        id: "crossfade".into(),
        range: SampleRange {
            start: 64,
            end: 128,
        },
        curve: reel_music::repair::FadeCurve::EqualPower,
    }];
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize repair"),
    )
    .expect("rewrite repair");

    let error = edl::write(&repair, &fixture._temp.path().join("unsupported.json"))
        .expect_err("unsupported operation rejected");
    assert!(error.to_string().contains("not executable in cut-only EDL"));
}

#[test]
fn edl_recheck_rejects_a_changed_repair_manifest() {
    let (fixture, repair) = seam_fixture();
    let edl_path = fixture._temp.path().join("repair-edl.json");
    edl::write(&repair, &edl_path).expect("EDL compiles");
    let mut manifest = reel_music::repair::load(&repair).expect("load repair");
    manifest.repair_id = "changed-after-compilation".into();
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize repair"),
    )
    .expect("rewrite repair");

    let error = edl::validate(&edl_path, &repair).expect_err("stale EDL rejected");
    assert!(
        error
            .to_string()
            .contains("does not match the current repair")
    );
}

#[test]
fn cut_only_edl_requires_retained_signal_between_cuts() {
    let (fixture, repair) = seam_fixture();
    let mut manifest = reel_music::repair::load(&repair).expect("load repair");
    manifest.operations = vec![
        Operation::Cut {
            id: "cut-one".into(),
            range: SampleRange { start: 32, end: 64 },
        },
        Operation::Cut {
            id: "cut-two".into(),
            range: SampleRange {
                start: 64,
                end: 128,
            },
        },
    ];
    manifest.changed_envelopes = vec![
        SampleRange { start: 32, end: 64 },
        SampleRange {
            start: 64,
            end: 128,
        },
    ];
    manifest.locks = vec![
        SampleRange { start: 0, end: 32 },
        SampleRange {
            start: 128,
            end: 192,
        },
    ];
    fs::write(
        &repair,
        serde_yaml::to_string(&manifest).expect("serialize repair"),
    )
    .expect("rewrite repair");

    let error = edl::write(&repair, &fixture._temp.path().join("adjacent.json"))
        .expect_err("adjacent cuts rejected");
    assert!(error.to_string().contains("retained signal between them"));
}

#[test]
fn structural_renderer_executes_insert_and_replace_with_exact_lock_evidence() {
    let fixture = fixture();
    let asset_path = fixture._temp.path().join("asset.raw");
    fs::write(&asset_path, [200_u8, 201, 202, 203]).expect("write asset");
    let asset_hash = sha256_path(&asset_path).expect("hash asset");
    let asset = AssetRange {
        path: PathBuf::from("asset.raw"),
        sha256: asset_hash.clone(),
        decoded_pcm_sha256: asset_hash,
        format: RawPcmFormat::RawPcmU8,
        timebase: AudioTimebase {
            sample_rate_hz: 8_000,
            channels: 1,
            samples_per_channel: 4,
        },
        range: SampleRange { start: 0, end: 4 },
    };
    let mut manifest = repair_manifest(&fixture.source);
    manifest.repair_id = "insert-and-replace".into();
    manifest.operations = vec![
        Operation::Insert {
            id: "insert-before-phrase".into(),
            destination: SampleRange { start: 4, end: 8 },
            asset: asset.clone(),
        },
        Operation::Replace {
            id: "replace-phrase".into(),
            destination: SampleRange { start: 8, end: 12 },
            asset,
        },
    ];
    manifest.changed_envelopes = vec![SampleRange { start: 4, end: 12 }];
    manifest.locks = vec![
        SampleRange { start: 0, end: 4 },
        SampleRange { start: 12, end: 16 },
    ];
    let repair_path = fixture._temp.path().join("structural.yaml");
    fs::write(&repair_path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    let output = fixture._temp.path().join("structural.raw");
    let receipt = fixture._temp.path().join("structural.receipt.json");
    let report = repair_render::render(&repair_path, &output, &receipt, "test-v1").unwrap();
    assert!(report.outside_regions_exact);
    assert_eq!(report.output_samples_per_channel, 20);
    let source = fs::read(&fixture.pcm).unwrap();
    assert_eq!(
        fs::read(&output).unwrap(),
        [
            &source[0..4],
            &[200, 201, 202, 203],
            &source[4..8],
            &[200, 201, 202, 203],
            &source[12..16],
        ]
        .concat()
    );
    assert!(repair_render::check(&repair_path, &output, &receipt).is_ok());
    let mut tampered = fs::read(&output).unwrap();
    tampered[0] ^= 1;
    fs::write(&output, tampered).unwrap();
    assert!(repair_render::check(&repair_path, &output, &receipt).is_err());
    let receipt_text = fs::read_to_string(receipt).unwrap();
    assert!(!receipt_text.contains(fixture._temp.path().to_str().unwrap()));
    assert!(!receipt_text.contains("path"));
}

#[test]
fn renderer_executes_repeat_move_and_beat_locked_bar_extension() {
    let fixture = fixture();
    let render = |name: &str, manifest: &RepairManifest| {
        let repair = fixture._temp.path().join(format!("{name}.yaml"));
        let output = fixture._temp.path().join(format!("{name}.raw"));
        let receipt = fixture._temp.path().join(format!("{name}.receipt.json"));
        fs::write(&repair, serde_yaml::to_string(manifest).unwrap()).unwrap();
        let report = repair_render::render(&repair, &output, &receipt, "test-v1").unwrap();
        (fs::read(output).unwrap(), report)
    };
    let source = fs::read(&fixture.pcm).unwrap();

    let mut repeat = repair_manifest(&fixture.source);
    repeat.operations = vec![Operation::Repeat {
        id: "repeat".into(),
        source: SampleRange { start: 0, end: 4 },
        destination: SampleRange { start: 4, end: 8 },
    }];
    let (bytes, report) = render("repeat", &repeat);
    assert_eq!(report.output_samples_per_channel, 20);
    assert_eq!(bytes, [&source[..4], &source[..]].concat());

    let mut moved = repair_manifest(&fixture.source);
    moved.repair_id = "move".into();
    moved.operations = vec![Operation::Move {
        id: "move".into(),
        source: SampleRange { start: 4, end: 8 },
        destination: SampleRange { start: 12, end: 16 },
    }];
    moved.changed_envelopes = vec![
        SampleRange { start: 4, end: 8 },
        SampleRange { start: 12, end: 16 },
    ];
    moved.locks = vec![
        SampleRange { start: 0, end: 4 },
        SampleRange { start: 8, end: 12 },
    ];
    let (bytes, report) = render("move", &moved);
    assert_eq!(report.output_samples_per_channel, 16);
    assert_eq!(
        bytes,
        [&source[..4], &source[8..12], &source[4..8], &source[12..]].concat()
    );

    let mut extended = repair_manifest(&fixture.source);
    extended.repair_id = "extend".into();
    extended.beat_grid = Some(BeatGrid {
        origin_sample: 0,
        samples_per_beat: 1,
        beats_per_bar: 4,
        boundary_tolerance_samples: 0,
    });
    extended.operations = vec![Operation::ExtendBars {
        id: "extend".into(),
        range: SampleRange { start: 4, end: 8 },
        bars: 1,
    }];
    let (bytes, report) = render("extend", &extended);
    assert_eq!(report.output_samples_per_channel, 20);
    assert!(report.beat_alignment_passed);
    assert_eq!(
        bytes,
        [&source[..4], &source[4..8], &source[4..8], &source[8..]].concat()
    );
}

#[test]
fn renderer_executes_crossfade_tail_gain_and_hash_bound_eq() {
    let fixture = fixture();
    let render = |name: &str, manifest: &RepairManifest| {
        let repair = fixture._temp.path().join(format!("{name}.yaml"));
        let output = fixture._temp.path().join(format!("{name}.raw"));
        let receipt = fixture._temp.path().join(format!("{name}.receipt.json"));
        fs::write(&repair, serde_yaml::to_string(manifest).unwrap()).unwrap();
        repair_render::render(&repair, &output, &receipt, "test-v1").unwrap();
        fs::read(output).unwrap()
    };

    let mut crossfade = repair_manifest(&fixture.source);
    crossfade.operations = vec![Operation::Crossfade {
        id: "crossfade".into(),
        range: SampleRange { start: 4, end: 8 },
        curve: FadeCurve::EqualPower,
    }];
    assert_eq!(render("crossfade", &crossfade).len(), 14);

    let mut tail = repair_manifest(&fixture.source);
    tail.operations = vec![Operation::PreserveTail {
        id: "tail".into(),
        source: SampleRange { start: 0, end: 4 },
        destination: SampleRange { start: 4, end: 8 },
    }];
    assert_ne!(render("tail", &tail), fs::read(&fixture.pcm).unwrap());

    let mut gain = repair_manifest(&fixture.source);
    gain.operations = vec![Operation::MatchGain {
        id: "gain".into(),
        range: SampleRange { start: 4, end: 8 },
        target_millilufs: -12_000,
    }];
    assert_ne!(render("gain", &gain), fs::read(&fixture.pcm).unwrap());

    let bands = vec![EqBand {
        frequency_millihz: 1_000_000,
        q_milli: 1_000,
        gain_millidb: -6_000,
    }];
    let mut eq = repair_manifest(&fixture.source);
    eq.operations = vec![Operation::MatchEq {
        id: "eq".into(),
        range: SampleRange { start: 4, end: 8 },
        profile_sha256: canonical_sha256(&bands).unwrap(),
        bands,
    }];
    assert_ne!(render("eq", &eq), fs::read(&fixture.pcm).unwrap());
}
