use std::path::Path;

#[test]
fn native_recast_recompiles_every_lane_from_semantic_anchors() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cue-relative");
    let old = reel::cue_relative::compile(
        &reel::cue_relative::load(root.join("old.yaml")).unwrap(),
        &root,
    )
    .unwrap();
    let new = reel::cue_relative::compile(
        &reel::cue_relative::load(root.join("recast.yaml")).unwrap(),
        &root,
    )
    .unwrap();
    assert_eq!(new.duration_samples - old.duration_samples, 48_000);
    assert_eq!(old.shared_boundaries["spill"], 108_000);
    assert_eq!(new.shared_boundaries["spill"], 138_000);
    assert_eq!(old.shared_boundaries["next-cue"], 144_000);
    assert_eq!(new.shared_boundaries["next-cue"], 192_000);
    for id in [
        "cel-counter",
        "spill-picture-effect",
        "spill-sonic",
        "visitor-caption",
        "next-title",
    ] {
        assert!(old.attachments.iter().any(|x| x.id == id));
        assert!(new.attachments.iter().any(|x| x.id == id));
    }
    // The marker moves by its new forced-alignment position (+30k), while the
    // following boundary moves by the full native cue delta (+48k): no global
    // proportional remap or audio time scaling is involved.
    assert_ne!(30_000, 48_000);
}

#[test]
fn rejects_missing_marker_and_disagreeing_shared_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cue-relative");
    let mut contract = reel::cue_relative::load(root.join("old.yaml")).unwrap();
    if let reel::cue_relative::Anchor::Marker { marker_id, .. } = &mut contract.attachments[1].start
    {
        *marker_id = "missing".into();
    }
    assert!(
        reel::cue_relative::compile(&contract, &root)
            .unwrap_err()
            .to_string()
            .contains("missing or ambiguous")
    );

    let mut contract = reel::cue_relative::load(root.join("old.yaml")).unwrap();
    contract.attachments[2].start = reel::cue_relative::Anchor::CueStart {
        cue_id: "cue-recast".into(),
        offset_samples: 0,
    };
    assert!(
        reel::cue_relative::compile(&contract, &root)
            .unwrap_err()
            .to_string()
            .contains("shared boundary spill disagrees")
    );
}

#[test]
fn rejects_out_of_range_markers_and_offsets() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cue-relative");
    let mut contract = reel::cue_relative::load(root.join("old.yaml")).unwrap();
    contract.cues[1].markers[0].end_sample = 999_999;
    assert!(
        reel::cue_relative::compile(&contract, &root)
            .unwrap_err()
            .to_string()
            .contains("out of range")
    );

    let mut contract = reel::cue_relative::load(root.join("old.yaml")).unwrap();
    contract.attachments[0].start = reel::cue_relative::Anchor::CueStart {
        cue_id: "cue-before".into(),
        offset_samples: -1,
    };
    assert!(
        reel::cue_relative::compile(&contract, &root)
            .unwrap_err()
            .to_string()
            .contains("outside sequence")
    );
}
