//! Explicit synthetic developer walkthrough; no project media or native custody.
#[path = "../tests/support/film_review_fixture.rs"]
mod fixture;
use anyhow::{Result, ensure};
use reel::film_presentation_overlay::review;
use reel_workflow::{Artifact, film_review as findings};
use std::{fs, path::PathBuf};
fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    ensure!(
        args.len() == 2 && args[1] == "--apply",
        "usage: film-review-workflow NEW_ABSOLUTE_DIRECTORY --apply"
    );
    let root = PathBuf::from(&args[0]);
    ensure!(root.is_absolute(), "choose a new absolute directory");
    fs::create_dir(&root)?;
    fixture::fixture(&root);
    let request = fixture::manifest(&root);
    let files = review::Local {
        request: &request,
        root: &root,
    };
    let output = root.join("review");
    review::build_with_inputs(&request, &files, &output)?;
    review::check_with_inputs(&request, &files, &output)?;
    let map: review::ReviewMap =
        serde_json::from_slice(&fs::read(output.join("review-map.json"))?)?;
    let target = map
        .targets
        .last()
        .ok_or_else(|| anyhow::anyhow!("no synthetic target"))?;
    let descriptor = |name: &str, mime: &str| -> Result<Artifact> {
        let path = output.join(name);
        let raw = fs::read(path)?;
        Ok(Artifact {
            logical_id: format!("review/{name}"),
            sha256: reel_workflow::digest(&raw),
            bytes: raw.len() as u64,
            media_type: mime.into(),
        })
    };
    let report = findings::Report {
        schema: findings::REPORT_SCHEMA.into(),
        review: descriptor("review.mp4", "video/mp4")?,
        map: descriptor("review-map.json", "application/json")?,
        receipt: descriptor("receipt.json", "application/json")?,
        findings: vec![findings::Finding {
            id: "demo-transition".into(),
            target_id: target.attachment_id.clone(),
            frame: target.start_frame,
            category: findings::Category::Timing,
            method: findings::Method::DeterministicCheck,
            observation: format!(
                "Synthetic demonstration: inspect the transition to {}. No real production issue is asserted.",
                target.cel_id
            ),
            evidence: vec![],
        }],
    };
    let bindings = [
        (&report.review, "review.mp4"),
        (&report.map, "review-map.json"),
        (&report.receipt, "receipt.json"),
    ]
    .into_iter()
    .map(|(a, path)| (a.logical_id.clone(), output.join(path)))
    .collect();
    let export = findings::ExportRequest {
        schema: "reel.workflow-film-findings-export.v1".into(),
        report,
        files: bindings,
    };
    fixture::save(&root, "issue-request.json", &export);
    let path = root.join("issue-request.json");
    let pin = reel_workflow::digest(&fs::read(&path)?);
    findings::export(&path, &pin, &root.join("issues"))?;
    println!(
        "{}",
        serde_json::json!({"ok":true,"synthetic":true,"review_movie":"review/review.mp4","issue_directory":"issues","issue_submitted":false,"native_retention_verified":false,"selection_applied":false})
    );
    Ok(())
}
fn main() {
    if run().is_err() {
        eprintln!("synthetic walkthrough refused or failed; preserve its directory for diagnosis");
        std::process::exit(1);
    }
}
