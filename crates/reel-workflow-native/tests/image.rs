#![cfg(windows)]
use reel_workflow::*;
use reel_workflow_native::{image::pack_files, response::Returns};
use std::{collections::BTreeMap, fs};
fn packet() -> Packet {
    let task = Task {
        id: "images".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::ImageCandidate,
        tool: None,
        language: Some("es".into()),
        prompt: "Explicit synthetic image handoff".into(),
        inputs: vec![],
        depends_on: vec![],
        outputs: ["one.png", "two.png"]
            .into_iter()
            .map(|n| {
                (
                    n.into(),
                    OutputContract {
                        media_type: "image/png".into(),
                        max_bytes: 1024 * 1024,
                        required: true,
                    },
                )
            })
            .collect(),
    };
    make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: "synthetic".into(),
            source_commit: "a".repeat(40),
            tasks: vec![task],
        },
        "images",
        BTreeMap::new(),
    )
    .unwrap()
}
#[test]
fn png_pack_preserves_exact_files_and_unknown_handoff_provenance() {
    let root = tempfile::tempdir().unwrap();
    let pix = resvg::tiny_skia::Pixmap::new(16, 16).unwrap();
    let bytes = pix.encode_png().unwrap();
    fs::write(root.path().join("one.png"), &bytes).unwrap();
    fs::write(root.path().join("two.png"), &bytes).unwrap();
    let returns = Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: root.path().into(),
        files: BTreeMap::from([
            ("one.png".into(), "one.png".into()),
            ("two.png".into(), "two.png".into()),
        ]),
    };
    let packed = pack_files(&packet(), "original-attempt", &returns).unwrap();
    assert_eq!(packed.result.artifacts.len(), 2);
    assert_eq!(packed.result.artifacts["one.png"].sha256, digest(&bytes));
    assert_eq!(packed.result.artifacts["one.png"].bytes, bytes.len() as u64);
    assert_eq!(packed.result.provenance.mode, Mode::Handoff);
    assert!(packed.result.provenance.provider.is_none());
    assert!(packed.result.provenance.model.is_none());
    assert_eq!(packed.result.attempt_id, "original-attempt");
}
#[test]
fn png_pack_refuses_malformed_missing_aliased_and_trace_companion_returns() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("bad.png"), b"not PNG").unwrap();
    let mut returns = Returns {
        schema: "reel.workflow-returns.v1".into(),
        root: root.path().into(),
        files: BTreeMap::from([("one.png".into(), "bad.png".into())]),
    };
    assert_eq!(
        pack_files(&packet(), "original", &returns)
            .err()
            .unwrap()
            .kind,
        ErrorKind::Integrity
    );
    let bytes = resvg::tiny_skia::Pixmap::new(16, 16)
        .unwrap()
        .encode_png()
        .unwrap();
    fs::write(root.path().join("good.png"), bytes).unwrap();
    returns.files.insert("one.png".into(), "good.png".into());
    assert!(pack_files(&packet(), "original", &returns).is_err());
    returns.files.insert("two.png".into(), "GOOD.PNG".into());
    assert_eq!(
        pack_files(&packet(), "original", &returns)
            .err()
            .unwrap()
            .code,
        "image-return-type-or-path"
    );
    let mut p = packet();
    p.task.outputs.insert(
        "provider-response.json".into(),
        OutputContract {
            media_type: "application/json".into(),
            max_bytes: 1024,
            required: true,
        },
    );
    p = make_packet(
        &Plan {
            schema: PLAN_SCHEMA.into(),
            project: p.project,
            source_commit: p.source_commit,
            tasks: vec![p.task],
        },
        "images",
        BTreeMap::new(),
    )
    .unwrap();
    assert_eq!(
        pack_files(&p, "original", &returns).err().unwrap().code,
        "image-handoff-required-companion"
    );
}
