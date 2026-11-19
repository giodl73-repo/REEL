//! Optional review-only labels from exact compiled attachment/cel identities.
use crate::{
    cue_relative::{CompiledAttachment, Target},
    scene_delivery::{FileRef, InputFiles, Job, MediaProgram, Plan},
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub font: FileRef,
    pub font_size: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Label {
    pub attachment_id: String,
    pub shot_id: String,
    pub cel_id: Option<String>,
    pub source: FileRef,
    pub start_sample: u64,
    pub end_sample: u64,
    pub start_frame: u64,
    pub end_frame: u64,
}
fn safe(s: &str) -> bool {
    (1..=128).contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_./-".contains(&c))
}
fn display(label: &Label) -> String {
    match &label.cel_id {
        Some(id) => format!("CEL {id}"),
        None => format!("SHOT {}", label.shot_id),
    }
}
pub(crate) fn labels(
    job: &Job,
    plan: &Plan,
    compiled: &[CompiledAttachment],
) -> Result<Vec<Label>> {
    if job.review_ids.is_none() {
        return Ok(vec![]);
    }
    ensure!(
        job.pictures.len() <= 128 && job.pictures.len() == plan.pictures.len(),
        "review ID picture bound"
    );
    job.pictures
        .iter()
        .zip(&plan.pictures)
        .map(|(p, s)| {
            let a = compiled
                .iter()
                .find(|a| a.id == p.attachment_id)
                .context("review attachment missing")?;
            let (shot_id, cel_id) = match &a.target {
                Target::Cel { shot_id, cel_id } => (shot_id.clone(), Some(cel_id.clone())),
                Target::Shot { shot_id } => (shot_id.clone(), None),
                _ => anyhow::bail!("review target is not a picture"),
            };
            ensure!(
                safe(&a.id) && safe(&shot_id) && cel_id.as_deref().is_none_or(safe),
                "review ID unsupported characters or length"
            );
            Ok(Label {
                attachment_id: a.id.clone(),
                shot_id,
                cel_id,
                source: p.source.clone(),
                start_sample: s.start_sample,
                end_sample: s.end_sample,
                start_frame: s.start_frame,
                end_frame: s.end_frame,
            })
        })
        .collect()
}
pub(crate) fn validate(job: &Job, plan: &Plan, inputs: &(impl InputFiles + ?Sized)) -> Result<()> {
    let Some(policy) = &job.review_ids else {
        ensure!(plan.review_ids.is_empty(), "unexpected review labels");
        return Ok(());
    };
    ensure!(
        (8..=96).contains(&policy.font_size)
            && policy.font.bytes > 0
            && policy.font.bytes <= 16 * 1024 * 1024
            && !plan.review_ids.is_empty(),
        "review font/label bound"
    );
    let path = inputs.verified_path(&policy.font)?;
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == policy.font.bytes
            && crate::production::sha256_bytes(&bytes) == policy.font.sha256,
        "review font bytes differ"
    );
    let face =
        ttf_parser::Face::parse(&bytes, 0).map_err(|_| anyhow::anyhow!("review font parse"))?;
    ensure!(face.units_per_em() > 0, "review font units absent");
    let mut ids = BTreeSet::new();
    for l in &plan.review_ids {
        ensure!(
            ids.insert(&l.attachment_id)
                && l.start_frame < l.end_frame
                && l.end_frame <= plan.frame_count,
            "review span invalid"
        );
        let mut advance = 0u64;
        for c in display(l).chars() {
            let glyph = face.glyph_index(c).context("review font glyph missing")?;
            advance += u64::from(
                face.glyph_hor_advance(glyph)
                    .context("review glyph advance missing")?,
            );
        }
        let width = advance
            .checked_mul(u64::from(policy.font_size))
            .context("review label width overflow")?
            .div_ceil(u64::from(face.units_per_em()));
        ensure!(
            width + 16 <= u64::from(job.width) && policy.font_size + 16 <= job.height,
            "review label does not fit canvas"
        );
    }
    ensure!(
        metadata(job, plan)?.len() <= 1024 * 1024,
        "review ID metadata bound"
    );
    Ok(())
}
fn metadata(job: &Job, plan: &Plan) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(
        &serde_json::json!({"schema":"reel.scene-review-ids.v1","job_sha256":plan.job_sha256,"contract_sha256":plan.contract_sha256,"font":job.review_ids.as_ref().context("review policy missing")?.font,"font_size":job.review_ids.as_ref().unwrap().font_size,"fps_numerator":plan.fps_numerator,"fps_denominator":plan.fps_denominator,"sample_rate":plan.sample_rate,"frame_count":plan.frame_count,"labels":plan.review_ids,"master_modified":false,"creative_approval":false}),
    )?)
}
pub(crate) fn render(
    job: &Job,
    plan: &Plan,
    root: &Path,
    inputs: &(impl InputFiles + ?Sized),
) -> Result<()> {
    validate(job, plan, inputs)?;
    let policy = job.review_ids.as_ref().context("review policy missing")?;
    fs::copy(
        inputs.verified_path(&policy.font)?,
        root.join("review-id-font.ttf"),
    )?;
    fs::write(root.join("review-ids.json"), metadata(job, plan)?)?;
    let filter=plan.review_ids.iter().map(|l|format!("drawtext=fontfile=review-id-font.ttf:text='{}':expansion=none:fontcolor=white:fontsize={}:x=8:y=8:box=1:boxcolor=black@0.8:boxborderw=4:enable='gte(n,{})*lt(n,{})'",display(l),policy.font_size,l.start_frame,l.end_frame)).collect::<Vec<_>>().join(",");
    let output = Command::new(inputs.media_program(MediaProgram::Ffmpeg)?)
        .current_dir(root)
        .args([
            "-hide_banner",
            "-v",
            "error",
            "-nostdin",
            "-n",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
            "master.mkv",
            "-map",
            "0:v:0",
            "-map",
            "0:a:0",
            "-vf",
            &filter,
            "-c:v",
            "libx264",
            "-crf",
            "18",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-movflags",
            "+faststart",
            "review.mp4",
        ])
        .output()?;
    ensure!(
        output.status.success(),
        "review ID encode failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}
pub(crate) fn check(
    job: &Job,
    plan: &Plan,
    inputs: &(impl InputFiles + ?Sized),
    path: impl Fn(&str) -> Result<PathBuf>,
) -> Result<()> {
    let Some(policy) = &job.review_ids else {
        return Ok(());
    };
    ensure!(
        fs::read(path("review-ids.json")?)? == metadata(job, plan)?,
        "review ID metadata differs"
    );
    let actual = path("review-id-font.ttf")?;
    let selected = inputs.verified_path(&policy.font)?;
    ensure!(
        fs::metadata(&actual)?.len() == policy.font.bytes
            && crate::sha256_file(&actual)? == crate::sha256_file(&selected)?,
        "review font differs"
    );
    Ok(())
}
