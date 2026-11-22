//! Controlled synthetic drawing repair; no live provider or selected version.
use super::fixture;
use anyhow::{Result, ensure};
use reel::{film_presentation_overlay::review, scene_delivery};
use reel_workflow::{film_repair, film_review, manual, *};
use serde_json::json;
use std::{collections::BTreeMap, fs, path::Path};

pub fn run(root: &Path, export: &film_review::ExportRequest) -> Result<()> {
    let report = export.report.clone();
    let pixel = fixture::frame(&root.join("review/review.mp4"), 24);
    let center = (90 * 320 + 160) * 3;
    ensure!(
        pixel[center + 2] > pixel[center + 1] + 50,
        "old synthetic cel is not blue"
    );
    fixture::save(
        root,
        "synthetic-task.json",
        &json!({"synthetic_metadata_only":true}),
    );
    let descriptor = |name: &str, mime: &str| -> Artifact {
        let v = fixture::reference(root, name);
        Artifact {
            logical_id: format!("fixture/{name}"),
            sha256: v["sha256"].as_str().unwrap().into(),
            bytes: v["bytes"].as_u64().unwrap(),
            media_type: mime.into(),
        }
    };
    let inputs = vec![
        descriptor("job.json", "application/json"),
        descriptor("synthetic-task.json", "application/json"),
        descriptor("contract.json", "application/json"),
        descriptor("production.json", "application/json"),
        descriptor("red.ppm", "image/x-portable-pixmap"),
        descriptor("blue.ppm", "image/x-portable-pixmap"),
        descriptor("a.wav", "audio/wav"),
        descriptor("b.wav", "audio/wav"),
    ];
    // This is an authored synthetic plan for the metadata bridge, not an
    // assertion that the first fixture render was a retained native task.
    let task = Task {
        id: "scene-es".into(),
        native_inputs: None,
        upstream_inputs: None,
        operation: Operation::DeterministicTool,
        tool: Some(ToolContract {
            schema: "reel.workflow-tool.v1".into(),
            capability: ToolCapability::FilmRender,
            executable: Artifact {
                logical_id: "fixture/declared-renderer".into(),
                sha256: digest(b"synthetic declared renderer only"),
                bytes: 32,
                media_type: "application/octet-stream".into(),
            },
            auxiliary_executables: BTreeMap::new(),
            publication: inputs[0].clone(),
            request: inputs[1].clone(),
            edition_id: "scene-es".into(),
            source_bindings: inputs
                .iter()
                .map(|a| {
                    (
                        a.logical_id.strip_prefix("fixture/").unwrap().into(),
                        a.logical_id.clone(),
                    )
                })
                .collect(),
            part: None,
        }),
        language: Some("es".into()),
        prompt: String::new(),
        inputs: inputs.clone(),
        depends_on: vec![],
        outputs: BTreeMap::from([(
            "movie".into(),
            OutputContract {
                media_type: "video/mp4".into(),
                max_bytes: 16 * 1024 * 1024,
                required: true,
            },
        )]),
    };
    let mut en = task.clone();
    en.id = "scene-en".into();
    en.language = Some("en".into());
    en.tool.as_mut().unwrap().edition_id = "scene-en".into();
    let plan = Plan {
        schema: PLAN_SCHEMA.into(),
        project: "synthetic-film".into(),
        source_commit: "a".repeat(40),
        task_snapshots: BTreeMap::new(),
        tasks: vec![task, en],
    };
    let mut bindings = export.files.clone();
    for a in &inputs {
        bindings.insert(
            a.logical_id.clone(),
            root.join(a.logical_id.strip_prefix("fixture/").unwrap()),
        );
    }
    let key = film_review::draft(&report, &mut film_review::Files::new(bindings.clone()))?[0]
        .key
        .clone();
    let request=film_repair::Request {schema:"reel.workflow-film-image-repair-export.v1".into(),baseline:plan,report,
        repair:film_repair::Spec {schema:"reel.workflow-film-image-repair.v1".into(),reviewed_task:"scene-es".into(),finding_key:key,task_id:"repair-C101".into(),instructions:"For this controlled fixture, return a solid green PNG. Preserve every other drawing and all audio/timing.".into(),reference_inputs:vec![],native_inputs:None,
            output:OutputContract {media_type:"image/png".into(),max_bytes:1024*1024,required:true},input_bytes:1024*1024,object_bytes:1024*1024},attempt_id:"synthetic-attempt-1".into(),files:bindings};
    fixture::save(root, "repair-request.json", &request);
    let request_path = root.join("repair-request.json");
    film_repair::export(
        &request_path,
        &digest(&fs::read(&request_path)?),
        &root.join("repair"),
    )?;
    // A deterministic synthetic stand-in for a returned image-generation result.
    // The production CLI never manufactures this candidate or calls a provider.
    let picture = root.join("repaired-picture.png");
    image::RgbImage::from_pixel(2, 2, image::Rgb([20, 170, 40])).save(&picture)?;
    let returned = BTreeMap::from([("picture".into(), picture)]);
    ensure!(
        !manual::capture_work(
            &root.join("repair/work"),
            "synthetic-attempt-1",
            &returned,
            &root.join("repair-candidate")
        )?,
        "unexpected candidate replay"
    );
    ensure!(
        manual::capture_work(
            &root.join("repair/work"),
            "synthetic-attempt-1",
            &returned,
            &root.join("repair-candidate")
        )?,
        "candidate replay failed"
    );
    let old_job = fs::read(root.join("job.json"))?;
    let job: scene_delivery::Job = serde_json::from_slice(&old_job)?;
    let replacement = scene_delivery::repair::StillReplacement {
        schema: "reel.scene-still-replacement.v1".into(),
        attachments: vec!["p-b".into()],
        expected: job.pictures[1].source.clone(),
        replacement: serde_json::from_value(fixture::reference(root, "repaired-picture.png"))?,
    };
    fixture::save(root, "still-replacement.json", &replacement);
    let derived = scene_delivery::repair::write_candidate(
        &root.join("job.json"),
        &digest(&old_job),
        &root.join("still-replacement.json"),
        &digest(&fs::read(root.join("still-replacement.json"))?),
        &root.join("job-repaired.json"),
    )?;
    ensure!(
        !derived.creative_approval,
        "technical control generation granted approval"
    );
    scene_delivery::render(
        &root.join("job-repaired.json"),
        root,
        &root.join("repaired-source"),
    )?;
    for name in ["D.wav", "M.wav", "E.wav", "mix.wav"] {
        ensure!(
            fs::read(root.join("source").join(name))?
                == fs::read(root.join("repaired-source").join(name))?,
            "synthetic image repair changed audio"
        );
    }
    let mut q: serde_json::Value = serde_json::from_slice(&fs::read(root.join("request.json"))?)?;
    q["job"] = fixture::reference(root, "job-repaired.json");
    q["film"] = fixture::reference(root, "repaired-source/review.mp4");
    q["delivery_receipt"] = fixture::reference(root, "repaired-source/receipt.json");
    // Both commit strings are synthetic declarations; no Git selection occurs.
    q["source_commit"] = json!("b".repeat(40));
    fixture::save(root, "repaired-review-request.json", &q);
    let mut request: scene_delivery::FileRef =
        serde_json::from_value(fixture::reference(root, "repaired-review-request.json"))?;
    request.path = root.join("repaired-review-request.json");
    let files = review::Local {
        request: &request,
        root,
    };
    review::build_with_inputs(&request, &files, &root.join("repaired-review"))?;
    review::check_with_inputs(&request, &files, &root.join("repaired-review"))?;
    let new = fixture::frame(&root.join("repaired-review/review.mp4"), 24);
    ensure!(
        new[center + 1] > new[center + 2] + 50,
        "new synthetic cel is not green"
    );
    let old: review::ReviewMap =
        serde_json::from_slice(&fs::read(root.join("review/review-map.json"))?)?;
    let new: review::ReviewMap =
        serde_json::from_slice(&fs::read(root.join("repaired-review/review-map.json"))?)?;
    ensure!(
        old.targets[1].cel_id == new.targets[1].cel_id
            && old.targets[1].story_id == new.targets[1].story_id
            && old.targets[1].start_frame == new.targets[1].start_frame
            && old.targets[1].end_frame == new.targets[1].end_frame
            && old.targets[1].selected_picture.sha256 != new.targets[1].selected_picture.sha256
            && old.targets[0] == new.targets[0],
        "repair changed identity or clock"
    );
    ensure!(
        fs::read(root.join("job.json"))? == old_job,
        "original job changed"
    );
    fixture::save(
        root,
        "repair-demo-receipt.json",
        &json!({"synthetic":true,"original_job_unchanged":true,"old_blue_new_green":true,"story_cel_and_frame_interval_preserved":true,"other_target_unchanged":true,"audio_bytes_unchanged":true,"manual_candidate_replay_verified":true,"native_retention_verified":false,"selection_applied":false,"approvals_transferred":false,"bilingual_rebuild_qualified":false}),
    );
    Ok(())
}
