use super::*;
use reel::scene_delivery::{FileRef, InputFiles};
use reel::scene_primary_reference::{self, Budget};

pub(super) fn check(root: &Path, job_path: &Path, rendered: &Path) {
    let control_root = root.to_path_buf();
    let output_root = rendered.to_path_buf();
    let job_name = job_path.strip_prefix(root).unwrap().to_str().unwrap();
    let job: FileRef = serde_json::from_value(file(root, job_name)).unwrap();
    let production: FileRef = serde_json::from_value(file(root, "production.json")).unwrap();
    let master: FileRef = serde_json::from_value(file(rendered, "master.mkv")).unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let parsed: scene_delivery::Job = serde_json::from_slice(&fs::read(job_path).unwrap()).unwrap();
    if parsed.post_compose_camera.is_some()
        || parsed.external_layers.iter().any(|layer| {
            matches!(
                layer.render_mode,
                scene_delivery::ExternalLayerRenderMode::AssOverlay
                    | scene_delivery::ExternalLayerRenderMode::TimedVideoOverlay
                    | scene_delivery::ExternalLayerRenderMode::TimedStillOverlay
            )
        })
    {
        let refused = scene_primary_reference::compare_master_to_primaries(
            &job,
            &production,
            &control_root,
            &control_root,
            &output_root,
            &master,
            scratch.path(),
            &Budget {
                max_staged_bytes: 64 * 1024 * 1024,
                timeout_seconds: 120,
                max_picture_processes: 2,
            },
        );
        assert!(refused.is_err());
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    let result = scene_primary_reference::compare_master_to_primaries(
        &job,
        &production,
        &control_root,
        &control_root,
        &output_root,
        &master,
        scratch.path(),
        &Budget {
            max_staged_bytes: 64 * 1024 * 1024,
            timeout_seconds: 120,
            max_picture_processes: 16,
        },
    )
    .unwrap();
    let report = result.declaration();
    assert_eq!(report["movie_files_written"], 0);
    assert_eq!(report["approval"], false);
    assert!(
        report["staged_bytes"].as_u64().unwrap()
            <= report["staging_reservation_bytes"].as_u64().unwrap()
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}

#[test]
fn independent_reference_ignores_old_receipts_but_rejects_forged_primary_correspondence() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let control_root = root.to_path_buf();
    let mut job = fixture(root);
    let job_path = root.join("job.json");
    let output = root.join("retained");
    scene_delivery::render(&job_path, root, &output).unwrap();
    fs::rename(
        output.join("receipt.json"),
        output.join("historical-receipt.json"),
    )
    .unwrap();
    check(root, &job_path, &output);
    let scratch = tempfile::tempdir().unwrap();
    let reference: FileRef = serde_json::from_value(file(root, "job.json")).unwrap();
    let production: FileRef = serde_json::from_value(file(root, "production.json")).unwrap();
    let master: FileRef = serde_json::from_value(file(&output, "master.mkv")).unwrap();
    for budget in [
        Budget {
            max_staged_bytes: 1,
            timeout_seconds: 120,
            max_picture_processes: 16,
        },
        Budget {
            max_staged_bytes: 64 * 1024 * 1024,
            timeout_seconds: 0,
            max_picture_processes: 16,
        },
    ] {
        assert!(
            scene_primary_reference::compare_master_to_primaries(
                &reference,
                &production,
                &control_root,
                &control_root,
                &output as &dyn InputFiles,
                &master,
                scratch.path(),
                &budget
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    }
    // A forged old receipt may describe the new plan and unchanged output pins.
    // Pixel/audio reconstruction must remain independent of that declaration.
    job["pictures"][0]["source"] = file(root, "blue.ppm");
    write_json(&job_path, &job);
    let (_, plan) = scene_delivery::plan(&job_path, root).unwrap();
    let mut forged: Value =
        serde_json::from_slice(&fs::read(output.join("historical-receipt.json")).unwrap()).unwrap();
    forged["plan"] = serde_json::to_value(plan).unwrap();
    write_json(&output.join("receipt.json"), &forged);
    let reference: FileRef = serde_json::from_value(file(root, "job.json")).unwrap();
    let error = scene_primary_reference::compare_master_to_primaries(
        &reference,
        &production,
        &control_root,
        &control_root,
        &output as &dyn InputFiles,
        &master,
        scratch.path(),
        &Budget {
            max_staged_bytes: 64 * 1024 * 1024,
            timeout_seconds: 120,
            max_picture_processes: 16,
        },
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("differs from its current primary reference"),
        "{error:#}"
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    job["pictures"][0]["source"] = file(root, "red.ppm");
    wav(&root.join("changed-audio.wav"), 48001, 200000);
    job["audio"][0]["source"] = file(root, "changed-audio.wav");
    write_json(&job_path, &job);
    let (_, plan) = scene_delivery::plan(&job_path, root).unwrap();
    forged["plan"] = serde_json::to_value(plan).unwrap();
    write_json(&output.join("receipt.json"), &forged);
    let reference: FileRef = serde_json::from_value(file(root, "job.json")).unwrap();
    let budget = Budget {
        max_staged_bytes: 64 * 1024 * 1024,
        timeout_seconds: 120,
        max_picture_processes: 16,
    };
    let error = scene_primary_reference::compare_master_to_primaries(
        &reference,
        &production,
        &control_root,
        &control_root,
        &output,
        &master,
        scratch.path(),
        &budget,
    )
    .err()
    .unwrap();
    assert!(
        error
            .to_string()
            .contains("differs from its current primary reference"),
        "{error:#}"
    );
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
    assert!(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-copyts",
                "-itsoffset",
                "1",
                "-i"
            ])
            .arg(output.join("master.mkv"))
            .args(["-c", "copy"])
            .arg(output.join("shifted.mkv"))
            .status()
            .unwrap()
            .success()
    );
    let shifted: FileRef = serde_json::from_value(file(&output, "shifted.mkv")).unwrap();
    let error = scene_primary_reference::compare_master_to_primaries(
        &reference,
        &production,
        &control_root,
        &control_root,
        &output,
        &shifted,
        scratch.path(),
        &budget,
    )
    .err()
    .unwrap();
    assert!(error.to_string().contains("clock"), "{error:#}");
    assert_eq!(fs::read_dir(scratch.path()).unwrap().count(), 0);
}
