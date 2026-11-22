use reel::{
    film_presentation_overlay::review,
    scene_delivery::{FileRef, InputFiles, MediaProgram},
};
use serde_json::json;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[path = "support/film_review_fixture.rs"]
mod fixture;
use fixture::{fixture, frame, manifest, reference, save};

#[test]
fn real_native_receipt_drives_visible_frame_exact_ids_with_unchanged_audio_and_timing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let request = manifest(root);
    let files = review::Local {
        request: &request,
        root,
    };
    let output = root.join("review");
    let receipt = match review::build_with_inputs(&request, &files, &output) {
        Ok(receipt) => receipt,
        Err(error) => {
            let evidence = temp.keep();
            panic!(
                "synthetic review failure preserved at {}: {error:#}",
                evidence.display()
            );
        }
    };
    review::check_with_inputs(&request, &files, &output).unwrap();
    assert_eq!(receipt.frame_count, 48);
    assert_eq!(receipt.source_clocks_sha256, receipt.output_clocks_sha256);
    assert!(
        !receipt.visual_reviewed
            && !receipt.approvals_verified
            && !receipt.native_retention_verified
    );
    let map: review::ReviewMap =
        serde_json::from_slice(&fs::read(output.join("review-map.json")).unwrap()).unwrap();
    assert_eq!(
        (
            map.targets[0].start_frame,
            map.targets[0].end_frame,
            map.targets[1].start_frame,
            map.targets[1].end_frame
        ),
        (0, 24, 24, 48)
    );
    reel_publication::film_review::check_map(&map).unwrap();
    // Exercise the portable workflow consumer on actual producer bytes, rather
    // than a separately copied review schema or a pretend media fixture.
    let descriptor = |name: &str, mime: &str| {
        let file: FileRef = serde_json::from_value(reference(&output, name)).unwrap();
        reel_workflow::Artifact {
            logical_id: format!("review/{name}"),
            sha256: file.sha256,
            bytes: file.bytes,
            media_type: mime.into(),
        }
    };
    let report = reel_workflow::film_review::Report {
        schema: reel_workflow::film_review::REPORT_SCHEMA.into(),
        review: descriptor("review.mp4", "video/mp4"),
        map: descriptor("review-map.json", "application/json"),
        receipt: descriptor("receipt.json", "application/json"),
        findings: vec![reel_workflow::film_review::Finding {
            id: "synthetic-cut".into(),
            target_id: "p-b".into(),
            frame: 24,
            category: reel_workflow::film_review::Category::Timing,
            method: reel_workflow::film_review::Method::DeterministicCheck,
            observation: "Synthetic cut target check.".into(),
            evidence: vec![],
        }],
    };
    let bindings = [
        (&report.review, "review.mp4"),
        (&report.map, "review-map.json"),
        (&report.receipt, "receipt.json"),
    ]
    .into_iter()
    .map(|(a, name)| (a.logical_id.clone(), output.join(name)))
    .collect();
    let drafts = reel_workflow::film_review::draft(
        &report,
        &mut reel_workflow::film_review::Files::new(bindings),
    )
    .unwrap();
    assert_eq!(drafts[0].target, map.targets[1]);
    assert!(!drafts[0].issue_submitted && !drafts[0].selection_applied);
    let frames: Vec<_> = [0, 23, 24, 47]
        .into_iter()
        .map(|n| frame(&output.join("review.mp4"), n))
        .collect();
    // Frame 23 still carries C100; frame 24 carries C101, even though the cue
    // sample boundary is not exactly on frame 24. Inspect visible white glyphs.
    let mask = |raw: &Vec<u8>| {
        raw[..320 * 34 * 3]
            .chunks_exact(3)
            .map(|p| p.iter().all(|v| *v > 180))
            .collect::<Vec<_>>()
    };
    let masks: Vec<_> = frames.iter().map(mask).collect();
    let difference = |a: &Vec<bool>, b: &Vec<bool>| a.iter().zip(b).filter(|(a, b)| a != b).count();
    assert!(masks.iter().all(|m| m.iter().filter(|v| **v).count() > 100));
    assert!(difference(&masks[0], &masks[1]) < 20);
    assert!(difference(&masks[2], &masks[3]) < 20);
    assert!(difference(&masks[1], &masks[2]) > 20);
    if let Ok(path) = std::env::var("REEL_CEL_REVIEW_EVIDENCE") {
        let evidence = Path::new(&path);
        fs::create_dir(evidence).unwrap();
        for (n, pixels) in [0, 23, 24, 47].into_iter().zip(&frames) {
            image::save_buffer(
                evidence.join(format!("frame-{n}.png")),
                pixels,
                320,
                180,
                image::ColorType::Rgb8,
            )
            .unwrap();
        }
        fs::copy(
            output.join("review-map.json"),
            evidence.join("review-map.json"),
        )
        .unwrap();
        fs::copy(output.join("receipt.json"), evidence.join("receipt.json")).unwrap();
    }
    let original = fs::read(output.join("review.mp4")).unwrap();
    assert!(review::build_with_inputs(&request, &files, &output).is_err());
    assert_eq!(fs::read(output.join("review.mp4")).unwrap(), original);
    fs::write(output.join("label-0.txt"), "C999 | story-1").unwrap();
    assert!(review::check_with_inputs(&request, &files, &output).is_err());
}
#[test]
fn wrong_film_binding_missing_ids_and_declared_budget_refuse_before_creating_output() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let request = fixture(root);
    for case in [
        "output-namespace",
        "film-subject",
        "missing-id",
        "frames",
        "probe",
    ] {
        let mut bad = request.clone();
        match case {
            "output-namespace" => bad["film"]["path"] = json!("lookalike.mp4"),
            "film-subject" => bad["film"]["sha256"] = json!("f".repeat(64)),
            "missing-id" => {
                bad["identities"].as_object_mut().unwrap().remove("p-b");
            }
            "frames" => bad["limits"]["frames"] = json!(47),
            "probe" => bad["limits"]["probe_bytes"] = json!(1024),
            _ => unreachable!(),
        }
        save(root, "request.json", &bad);
        let pin = manifest(root);
        let files = review::Local {
            request: &pin,
            root,
        };
        let output = root.join(case);
        assert!(
            review::build_with_inputs(&pin, &files, &output).is_err(),
            "{case}"
        );
        assert!(!output.exists(), "{case}");
    }
}
#[test]
fn explicit_provider_tool_denial_has_no_ambient_fallback_and_cli_rejects_wrong_pin() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fixture(root);
    let pin = manifest(root);
    struct Denied<'a>(review::Local<'a>);
    impl InputFiles for Denied<'_> {
        fn verified_path(&self, item: &FileRef) -> anyhow::Result<PathBuf> {
            self.0.verified_path(item)
        }
        fn validate_reference_name(&self, item: &FileRef) -> anyhow::Result<()> {
            self.0.validate_reference_name(item)
        }
        fn media_program(&self, _: MediaProgram) -> anyhow::Result<PathBuf> {
            anyhow::bail!("synthetic denied program")
        }
    }
    let files = Denied(review::Local {
        request: &pin,
        root,
    });
    let output = root.join("denied");
    assert!(
        review::build_with_inputs(&pin, &files, &output)
            .unwrap_err()
            .to_string()
            .contains("synthetic denied program")
    );
    assert!(!output.exists());
    let result = Command::new(env!("CARGO_BIN_EXE_reel-film-review"))
        .arg("build")
        .arg(root.join("request.json"))
        .arg("f".repeat(64))
        .arg("--root")
        .arg(root)
        .arg("--output-dir")
        .arg(root.join("cli-refused"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!root.join("cli-refused").exists());
    assert!(
        !String::from_utf8(result.stderr)
            .unwrap()
            .contains(root.to_str().unwrap())
    );
}
