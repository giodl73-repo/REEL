//! Existing retained synthetic film/comic -> producer-derived review controls.
use super::*;
use reel_workflow_native::handoff;

#[test]
#[ignore = "Explicit retained synthetic whole issue only; complete native replay and review metadata export after control-slot reuse, no admission/production/selection effects"]
fn retained_whole_issue_exports_all_page_panel_balloon_targets_after_control_reuse() {
    use std::io::Write;
    let root = PathBuf::from(
        std::env::var_os("REEL_WHOLE_ISSUE_REVIEW_ROOT").expect("retained issue root"),
    );
    let evidence = PathBuf::from(
        std::env::var_os("REEL_WHOLE_ISSUE_REVIEW_EVIDENCE").expect("new evidence directory"),
    );
    assert!(root.is_absolute() && evidence.is_absolute() && evidence.is_dir());
    let packet: Packet =
        serde_json::from_slice(&fs::read(root.join("issue-packet.json")).unwrap()).unwrap();
    let profile: handoff::Profile =
        serde_json::from_slice(&fs::read(root.join("issue-profile.json")).unwrap()).unwrap();
    let result: ReturnedResult =
        serde_json::from_slice(&fs::read(root.join("issue-result.json")).unwrap()).unwrap();
    let acknowledgement: reel_workflow_native::retention::Acknowledgement =
        serde_json::from_slice(&fs::read(root.join("issue-retained.json")).unwrap()).unwrap();
    assert_eq!(packet.project, "synthetic-comic");
    assert_eq!(packet.task.id, "issue-assemble");
    assert_eq!(
        packet.task.tool.as_ref().unwrap().capability,
        ToolCapability::ComicIssueAssembly
    );
    let checkpoint = || {
        let index = tig_store::source_claims::protection::Index::open(
            &profile.admission.index_root,
            &profile.binding.admission.index_checkpoint,
        )
        .unwrap();
        canonical_digest(&index.checkpoint()).unwrap()
    };
    let before = checkpoint();
    let exported = reel_workflow_native::retention::review_copy(
        &packet,
        &profile.source,
        &profile.admission,
        &profile.binding.admission,
        &result,
        &acknowledgement,
    )
    .unwrap();
    let copy = exported.copy();
    review::check_copy(copy).unwrap();
    assert_eq!(
        copy.format,
        review::Format::Comic {
            page_order: vec!["page01".into(), "page02".into()]
        }
    );
    assert_eq!(copy.units.len(), 6);
    assert_eq!(copy.source_commit, packet.source_commit);
    assert_eq!(
        copy.publication,
        packet.task.tool.as_ref().unwrap().publication
    );
    let manifest: serde_json::Value = serde_json::from_slice(exported.manifest().unwrap()).unwrap();
    assert_eq!(manifest["task_key"], packet.task_key);
    assert_eq!(manifest["page_order"], json!(["page01", "page02"]));
    for (page, part) in [("page01", "opening"), ("page02", "closing")] {
        assert_eq!(
            manifest["reading"][page]["logical_id"],
            format!("synthetic-issue/es/parts/{part}/{page}-reading.png")
        );
        assert!(copy.units.values().any(|unit|matches!(unit,review::Unit::Comic {page_id,panel_id:Some(_),balloon_id:Some(_)} if page_id==page)));
    }
    assert_eq!(before, checkpoint());
    let write_new = |name: &str, raw: &[u8]| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(evidence.join(name))
            .unwrap();
        file.write_all(raw).unwrap();
        file.sync_all().unwrap();
    };
    write_new("review-copy.json", &serde_json::to_vec(copy).unwrap());
    write_new("review-sources.json", exported.manifest().unwrap());
    write_new("result.json",&serde_json::to_vec(&json!({"schema":"reel.native-whole-issue-review-qualification.v1",
        "producer_task_key":packet.task_key,"source_commit":packet.source_commit,"pages":2,"units":copy.units.len(),
        "reading":manifest["reading"],"actual_index_checkpoint_unchanged":true,"index_checkpoint_sha256":before,
        "native_producer_verified":true,"derived_controls_retained":false,"viewing_verified":false,"approved":false,"selected":false})).unwrap());
}
#[test]
#[ignore = "Existing retained synthetic movie/comic only; native output proof and private control export, no production or publication"]
fn retained_film_and_comic_export_actual_targets_without_changing_claims() {
    let executable =
        PathBuf::from(std::env::var_os("REEL_NATIVE_HANDOFF_TOOL").expect("Set pinned exporter"));
    let evidence = PathBuf::from(
        std::env::var_os("REEL_REVIEW_EXPORT_EVIDENCE_DIRECTORY").expect("Set evidence directory"),
    );
    let mut temp = tempfile::tempdir().unwrap();
    temp.disable_cleanup(true);
    let root = temp.path();
    let film = PathBuf::from(
        std::env::var_os("REEL_MOVIE_REVIEW_FIXTURE").expect("Set completed movie fixture"),
    );
    let comic = PathBuf::from(
        std::env::var_os("REEL_COMIC_REVIEW_FIXTURE").expect("Set retained comic fixture"),
    );
    let save = |path: &Path, value: &serde_json::Value| {
        fs::write(path, serde_json::to_vec(value).unwrap()).unwrap()
    };
    let marker = json!({"root":root,"film":film,"comic":comic,"exporter":executable});
    save(&root.join("original-operation.json"), &marker);
    save(&evidence.join("original-operation-roots.json"), &marker);
    for (name, original, packet_name, profile_name, result_name) in [
        ("film", film, "packet.json", "profile.json", "result.json"),
        (
            "comic",
            comic,
            "comic-packet.json",
            "comic-profile.json",
            "comic-result.json",
        ),
    ] {
        save(
            &root.join("phase.json"),
            &json!({"phase":"export","format":name}),
        );
        let at = root.join(name);
        fs::create_dir(&at).unwrap();
        let profile: handoff::Profile =
            serde_json::from_slice(&fs::read(original.join(profile_name)).unwrap()).unwrap();
        let packet: Packet =
            serde_json::from_slice(&fs::read(original.join(packet_name)).unwrap()).unwrap();
        let result: ReturnedResult =
            serde_json::from_slice(&fs::read(original.join(result_name)).unwrap()).unwrap();
        let ack = profile.acknowledgements.join(format!(
            "retained-{}-{}.json",
            packet.task_key, profile.binding.admission.attempt_id
        ));
        let controls = [
            ("packet.json", serde_json::to_vec(&packet).unwrap()),
            ("source.json", serde_json::to_vec(&profile.source).unwrap()),
            (
                "admission.json",
                serde_json::to_vec(&profile.admission).unwrap(),
            ),
            (
                "receipt.json",
                serde_json::to_vec(&profile.binding.admission).unwrap(),
            ),
            ("result.json", serde_json::to_vec(&result).unwrap()),
            ("ack.json", fs::read(ack).unwrap()),
        ];
        for (file, raw) in controls {
            fs::write(at.join(file), raw).unwrap();
        }
        let claims = fs::read(profile.admission.index_root.join("claims.json")).unwrap();
        let copy_path = at.join("review-copy.json");
        let manifest_path = at.join("review-manifest.json");
        let mut command = Command::new(&executable);
        command.arg("export-review-copy");
        for file in [
            "packet.json",
            "source.json",
            "admission.json",
            "receipt.json",
            "result.json",
            "ack.json",
        ] {
            command.arg(at.join(file));
        }
        command.arg(&copy_path);
        if name == "comic" {
            command.arg(&manifest_path);
        }
        let output = command.arg("--apply").output().unwrap();
        fs::write(at.join("export-stdout.json"), &output.stdout).unwrap();
        fs::write(at.join("export-stderr.txt"), &output.stderr).unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let copy: review::Copy = serde_json::from_slice(&fs::read(&copy_path).unwrap()).unwrap();
        review::check_copy(&copy).unwrap();
        assert_eq!(copy.project, packet.project);
        assert_eq!(copy.source_commit, packet.source_commit);
        assert_eq!(Some(&copy.language), packet.task.language.as_ref());
        if name == "film" {
            assert_eq!(copy.format, review::Format::Film { frames: 48 });
            assert_eq!(copy.units.len(), 2);
            assert!(
                matches!(&copy.units["p-a"], review::Unit::Film { cel_id, shot_id:Some(shot), start_frame:0,end_frame:24,.. } if cel_id=="red" && shot=="shot")
            );
            assert!(
                matches!(&copy.units["p-b"], review::Unit::Film { cel_id, start_frame:24,end_frame:48,.. } if cel_id=="blue")
            );
            assert!(result.artifacts.values().any(|a| *a == copy.review));
            assert!(!manifest_path.exists());
        } else {
            let manifest = fs::read(&manifest_path).unwrap();
            assert_eq!(copy.review.sha256, digest(&manifest));
            assert_eq!(copy.review.bytes, manifest.len() as u64);
            let manifest: serde_json::Value = serde_json::from_slice(&manifest).unwrap();
            let review::Format::Comic { page_order } = &copy.format else {
                panic!("comic format")
            };
            assert!(!page_order.is_empty());
            for page in page_order {
                assert_eq!(
                    manifest["reading"][page],
                    serde_json::to_value(&result.artifacts[&format!("{page}-reading.png")])
                        .unwrap()
                );
            }
            assert!(copy.units.values().any(|u| matches!(
                u,
                review::Unit::Comic {
                    balloon_id: Some(_),
                    ..
                }
            )));
        }
        let report = review::Report {
            schema: "reel.workflow-review-report.v1".into(),
            copy_sha256: canonical_digest(&copy).unwrap(),
            findings: vec![review::Finding {
                code: "synthetic-review".into(),
                unit: copy.units.keys().next().unwrap().clone(),
                frames: None,
                modality: review::Modality::Visual,
                route: review::Route::Project,
                requested_change:
                    "Synthetic review observation; no actual creative change requested.".into(),
                evidence: vec![copy.review.clone()],
            }],
        };
        assert_eq!(
            review::check_report(&copy, &report)
                .unwrap()
                .findings()
                .len(),
            1
        );
        save(
            &at.join("finding-report.json"),
            &serde_json::to_value(report).unwrap(),
        );
        assert_eq!(
            claims,
            fs::read(profile.admission.index_root.join("claims.json")).unwrap()
        );
        fs::copy(copy_path, evidence.join(format!("{name}-review-copy.json"))).unwrap();
        if manifest_path.exists() {
            fs::copy(manifest_path, evidence.join("comic-review-manifest.json")).unwrap();
        }
    }
    let outcome = json!({"root":root,"film_and_comic_native_proofs":true,"film_frames":48,"cut_frame":24,"existing_target_ids_preserved":true,"comic_ordered_reading_pins_verified":true,
        "findings_bound_to_exported_copies":true,"claims_unchanged":true,"media_rendered":false,"derived_controls_retained":false,"issues_filed":false,"selected":false});
    save(&root.join("qualification-result.json"), &outcome);
    save(&evidence.join("qualification-result.json"), &outcome);
    save(&root.join("phase.json"), &json!({"phase":"verified"}));
}
