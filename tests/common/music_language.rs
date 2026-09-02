use std::{
    fs,
    path::{Path, PathBuf},
};

use reel_music::{
    AuthorityRef, DecisionRef,
    hash::sha256_path,
    language_adaptation::{
        AudioBinding, DraftBinding, LanguageAdaptation, ProsodyException, ProsodyExceptionKind,
        Stress, TextLayer, TextLayerKind, TextUnit, TranslationLink, Underlay,
    },
    source::RawPcmFormat,
    time::AudioTimebase,
};

pub fn decision(id: &str, digit: char) -> DecisionRef {
    DecisionRef {
        artifact_id: id.into(),
        sha256: digit.to_string().repeat(64),
    }
}

pub fn authority(id: &str, digit: char, status: &str, with_decision: bool) -> AuthorityRef {
    AuthorityRef {
        namespace: "synthetic-fixture".into(),
        artifact_id: id.into(),
        content_sha256: digit.to_string().repeat(64),
        status: status.into(),
        required_roles: vec!["lyrics-vocal-adaptation-editor".into(), "editor".into()],
        decision_refs: with_decision
            .then(|| decision(&format!("{id}-decision"), digit))
            .into_iter()
            .collect(),
    }
}

pub fn build_adaptation(root: &Path) -> PathBuf {
    fs::create_dir_all(root).unwrap();
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_text = root.join("source.txt");
    let target_text = root.join("target.txt");
    fs::copy(
        repository.join("manifests/fixtures/music-language-adaptation/source.txt"),
        &source_text,
    )
    .unwrap();
    fs::copy(
        repository.join("manifests/fixtures/music-language-adaptation/target.txt"),
        &target_text,
    )
    .unwrap();
    let accompaniment = root.join("accompaniment.u8");
    fs::write(&accompaniment, vec![128_u8; 32_000]).unwrap();
    let accompaniment_sha = sha256_path(&accompaniment).unwrap();

    let draft_path = repository.join("manifests/fixtures/music-model-corrected/draft.yaml");
    let draft_report = reel_music::model_draft::validate(&draft_path).unwrap();
    let model_path = repository.join("manifests/fixtures/music-model-corrected/model.yaml");
    let model = reel_music::model::load(&model_path).unwrap();

    let manifest = LanguageAdaptation {
        schema: reel_music::language_adaptation::SCHEMA.into(),
        adaptation_id: "synthetic-same-music-english".into(),
        model_draft: DraftBinding {
            manifest: draft_path,
            manifest_sha256: draft_report.manifest_sha256,
            contract_sha256: draft_report.contract_sha256,
            draft_id: draft_report.draft_id,
        },
        accompaniment: AudioBinding {
            path: PathBuf::from("accompaniment.u8"),
            sha256: accompaniment_sha.clone(),
            decoded_pcm_sha256: accompaniment_sha,
            format: RawPcmFormat::RawPcmU8,
            timebase: AudioTimebase {
                sample_rate_hz: 8_000,
                channels: 1,
                samples_per_channel: 32_000,
            },
            source_contract_sha256: model.source.contract_sha256,
            derivation_decision: decision("synthetic-accompaniment-derivation", '1'),
        },
        source_text: TextLayer {
            kind: TextLayerKind::CanonicalSource,
            language: "x-source".into(),
            path: PathBuf::from("source.txt"),
            sha256: sha256_path(&source_text).unwrap(),
            authority: authority("synthetic-source-text", '2', "fixture-only", false),
            units: vec![
                TextUnit {
                    id: "s1".into(),
                    byte_start: 0,
                    byte_end: 2,
                },
                TextUnit {
                    id: "s2".into(),
                    byte_start: 3,
                    byte_end: 5,
                },
                TextUnit {
                    id: "s3".into(),
                    byte_start: 6,
                    byte_end: 9,
                },
                TextUnit {
                    id: "s4".into(),
                    byte_start: 10,
                    byte_end: 13,
                },
            ],
        },
        target_text: TextLayer {
            kind: TextLayerKind::ApprovedTarget,
            language: "en".into(),
            path: PathBuf::from("target.txt"),
            sha256: sha256_path(&target_text).unwrap(),
            authority: authority("synthetic-target-text", '3', "approved", true),
            units: vec![
                TextUnit {
                    id: "t1".into(),
                    byte_start: 0,
                    byte_end: 3,
                },
                TextUnit {
                    id: "t2".into(),
                    byte_start: 4,
                    byte_end: 8,
                },
                TextUnit {
                    id: "t3".into(),
                    byte_start: 9,
                    byte_end: 11,
                },
                TextUnit {
                    id: "t4".into(),
                    byte_start: 12,
                    byte_end: 16,
                },
                TextUnit {
                    id: "t5".into(),
                    byte_start: 17,
                    byte_end: 20,
                },
            ],
        },
        translation_decision: decision("synthetic-translation-approval", '4'),
        translation_links: vec![
            TranslationLink {
                id: "opening-link".into(),
                source_unit_ids: vec!["s1".into(), "s2".into()],
                target_unit_ids: vec!["t1".into(), "t2".into()],
                rationale: "Synthetic opening alignment.".into(),
            },
            TranslationLink {
                id: "closing-link".into(),
                source_unit_ids: vec!["s3".into(), "s4".into()],
                target_unit_ids: vec!["t3".into(), "t4".into(), "t5".into()],
                rationale: "Synthetic unequal closing alignment.".into(),
            },
        ],
        preserved_model_targets: vec![
            "tempo:0".into(),
            "meter:0".into(),
            "form:phrase-a".into(),
            "form:phrase-a-prime".into(),
            "note:note-1".into(),
            "note:note-2".into(),
            "note:note-3".into(),
            "note:note-4".into(),
            "harmony:tonic-span".into(),
            "rhythm:quarter-cell".into(),
            "hook:opening-hook".into(),
        ],
        underlay: vec![
            Underlay {
                target_unit_id: "t1".into(),
                note_ids: vec!["note-1".into()],
                stress: Stress::Unstressed,
                melisma: false,
            },
            Underlay {
                target_unit_id: "t2".into(),
                note_ids: vec!["note-2".into()],
                stress: Stress::Primary,
                melisma: false,
            },
            Underlay {
                target_unit_id: "t3".into(),
                note_ids: vec!["note-3".into()],
                stress: Stress::Unstressed,
                melisma: false,
            },
            Underlay {
                target_unit_id: "t4".into(),
                note_ids: vec!["note-4".into()],
                stress: Stress::Primary,
                melisma: false,
            },
            Underlay {
                target_unit_id: "t5".into(),
                note_ids: vec!["note-4".into()],
                stress: Stress::Unstressed,
                melisma: false,
            },
        ],
        prosody_exceptions: vec![ProsodyException {
            id: "closing-duration-exception".into(),
            translation_link_id: "closing-link".into(),
            kind: ProsodyExceptionKind::Duration,
            target_unit_ids: vec!["t4".into(), "t5".into()],
            note_ids: vec!["note-4".into()],
            rationale: "Synthetic target adds one closing unit.".into(),
            required_review_roles: vec![
                "lyrics-vocal-adaptation-editor".into(),
                "score-arrangement-director".into(),
            ],
            decision: decision("synthetic-prosody-exception", '5'),
        }],
        review: reel_music::repair::Review {
            status: "not-reviewed".into(),
            required_roles: vec![
                "music-reconstruction-engineer".into(),
                "score-arrangement-director".into(),
                "lyrics-vocal-adaptation-editor".into(),
                "sound-designer".into(),
                "editor".into(),
                "rights-provenance-steward".into(),
            ],
            decision_refs: vec![],
        },
    };
    let path = root.join("adaptation.yaml");
    fs::write(&path, serde_yaml::to_string(&manifest).unwrap()).unwrap();
    path
}
