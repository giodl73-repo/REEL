//! Immutable matched treatments from one exact selected native scene job.
use crate::{
    motioncraft_cadence, motioncraft_review,
    scene_delivery::{self, FileRef, Job, PictureKind, PictureMotion, Plan},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub source_job: FileRef,
    pub asset_root: PathBuf,
    pub engine_commit: String,
}

fn local_dir(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.is_absolute()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("comparison asset root must be a relative local directory");
    }
    let path = root.join(relative).canonicalize()?;
    if !path.starts_with(root.canonicalize()?) || !path.is_dir() {
        bail!("comparison asset root escapes request root");
    }
    Ok(path)
}

fn copy_ref(root: &Path, item: &FileRef, destination: &Path) -> Result<()> {
    let source = scene_delivery::checked_file(root, item)?;
    let target = destination.join(&item.path);
    if target.exists() {
        if crate::sha256_file(&target)? != item.sha256 {
            bail!("comparison dependency path collision");
        }
        return Ok(());
    }
    fs::create_dir_all(target.parent().context("missing dependency parent")?)?;
    fs::copy(source, &target)?;
    scene_delivery::checked_file(destination, item)?;
    Ok(())
}

fn copy_refs(value: &Value, source: &Path, destination: &Path) -> Result<()> {
    match value {
        Value::Object(map)
            if map.contains_key("path")
                && map.contains_key("sha256")
                && map.contains_key("bytes") =>
        {
            copy_ref(source, &serde_json::from_value(value.clone())?, destination)?;
        }
        Value::Object(map) => {
            for value in map.values() {
                copy_refs(value, source, destination)?;
            }
        }
        Value::Array(items) => {
            for value in items {
                copy_refs(value, source, destination)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn variants(job: &Job, plan: &Plan) -> Result<BTreeMap<String, Job>> {
    if job.post_compose_camera.is_some() {
        bail!("comparison needs explicit post-compose camera migration");
    }
    let mut still = job.clone();
    let mut reduced = job.clone();
    let mut directed_count = 0;
    for (index, picture) in job.pictures.iter().enumerate() {
        if picture.crop.is_some() || picture.motion_group_id.is_some() {
            bail!("comparison needs explicit legacy crop/group migration");
        }
        match &picture.motion {
            Some(PictureMotion::PhasedCamera { plan: motion }) => {
                if picture.kind != PictureKind::Still || motion.direction.reduced_motion {
                    bail!("comparison requires an unreduced directed still camera");
                }
                directed_count += 1;
                still.pictures[index].motion = None;
                let mut direction = motion.direction.clone();
                direction.reduced_motion = true;
                let span = &plan.pictures[index];
                let mut resolved = reel_assembly::motioncraft::compile(
                    &direction,
                    &motion.safe_area,
                    span.end_sample - span.start_sample,
                    plan.sample_rate,
                    plan.fps_numerator,
                    plan.fps_denominator,
                )?;
                reel_assembly::motioncraft::allocate_delivery_frames(
                    &mut resolved,
                    span.end_frame - span.start_frame,
                )?;
                reduced.pictures[index].motion =
                    Some(PictureMotion::PhasedCamera { plan: resolved });
            }
            Some(_) => bail!("comparison needs explicit legacy camera migration"),
            None => {}
        }
    }
    if directed_count == 0 {
        bail!("comparison source has no phased directed camera");
    }
    Ok(BTreeMap::from([
        ("still".into(), still),
        ("directed".into(), job.clone()),
        ("reduced".into(), reduced),
    ]))
}

fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn inventory(root: &Path, directory: &Path, items: &mut BTreeMap<String, FileRef>) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            bail!("comparison inventory rejects symbolic links");
        }
        if path.is_dir() {
            inventory(root, &path, items)?;
        } else {
            let relative = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            items.insert(
                relative.clone(),
                FileRef {
                    path: relative.into(),
                    sha256: crate::sha256_file(&path)?,
                    bytes: fs::metadata(path)?.len(),
                },
            );
        }
    }
    Ok(())
}

fn indexed_pixels(
    video: &Path,
    indices: &BTreeSet<u64>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>> {
    let expected = indices.len() as u128 * u128::from(width) * u128::from(height) * 3;
    if indices.is_empty() || indices.len() > 256 || expected > 512 * 1024 * 1024 {
        bail!("comparison indexed evidence exceeds extraction budget");
    }
    let select = indices
        .iter()
        .map(|frame| format!("eq(n\\,{frame})"))
        .collect::<Vec<_>>()
        .join("+");
    let result = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-nostdin",
            "-protocol_whitelist",
            "file,pipe",
            "-i",
        ])
        .arg(video)
        .args([
            "-vf",
            &format!("select={select}"),
            "-fps_mode",
            "passthrough",
            "-pix_fmt",
            "rgb24",
            "-f",
            "rawvideo",
            "-",
        ])
        .output()?;
    if !result.status.success() || result.stdout.len() as u128 != expected {
        bail!("comparison indexed decoding failed or has wrong frame count");
    }
    Ok(result.stdout)
}

/// Prepare a self-contained package; no input is modified and outputs never clobber.
pub fn render(request_path: &Path, output: &Path) -> Result<Value> {
    let request_sha256 = crate::sha256_file(request_path)?;
    let request: Request = serde_json::from_slice(&fs::read(request_path)?)?;
    if request.schema != "reel.motioncraft-comparison-request.v1"
        || request.engine_commit.len() != 40
        || !request.engine_commit.bytes().all(|c| c.is_ascii_hexdigit())
    {
        bail!("invalid comparison request schema or engine commit");
    }
    let root = request_path.parent().unwrap_or(Path::new("."));
    let job_path = scene_delivery::checked_file(root, &request.source_job)?;
    let assets = local_dir(root, &request.asset_root)?;
    let (job, plan) = scene_delivery::plan(&job_path, &assets)?;
    let treatments = variants(&job, &plan)?;
    if output.exists() {
        bail!("comparison output must be new");
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".motioncraft-comparison-")
        .tempdir_in(parent)?;
    let package = stage.path();
    let jobs = package.join("jobs");
    let retained_assets = package.join("assets");
    fs::create_dir(&jobs)?;
    fs::create_dir(&retained_assets)?;
    let job_root = job_path.parent().context("source job parent")?;
    copy_ref(job_root, &job.contract, &jobs)?;
    let contract = crate::cue_relative::load(job_root.join(&job.contract.path))?;
    let contract_root = job_root
        .join(&job.contract.path)
        .parent()
        .context("contract parent")?
        .to_path_buf();
    let production = FileRef {
        path: contract.production_manifest.clone(),
        sha256: job.production_manifest_sha256.clone(),
        bytes: fs::metadata(contract_root.join(&contract.production_manifest))?.len(),
    };
    let retained_contract_root = jobs
        .join(&job.contract.path)
        .parent()
        .context("retained contract parent")?
        .to_path_buf();
    copy_ref(&contract_root, &production, &retained_contract_root)?;
    let mut asset_values = serde_json::to_value(&job)?;
    asset_values.as_object_mut().unwrap().remove("contract");
    copy_refs(&asset_values, &assets, &retained_assets)?;
    for layer in &job.external_layers {
        if let Some(receipt) = &layer.derivation_receipt {
            let value: Value = serde_json::from_slice(&fs::read(scene_delivery::checked_file(
                &assets, receipt,
            )?)?)?;
            copy_refs(&value, &assets, &retained_assets)?;
        }
    }
    write(&jobs.join("source.json"), &job)?;
    // Preserve original bytes separately; a normalized JSON job is not the original revision.
    fs::write(package.join("original-job.json"), fs::read(&job_path)?)?;
    let mut reports = BTreeMap::new();
    let mut reference_stems = BTreeMap::new();
    let mut indices = BTreeSet::from([0, plan.frame_count - 1]);
    for (name, treatment) in &treatments {
        let path = jobs.join(format!("{name}.json"));
        write(&path, treatment)?;
        let (_, native) = scene_delivery::plan(&path, &retained_assets)?;
        for field in [
            "duration_samples",
            "frame_count",
            "sample_rate",
            "fps_numerator",
            "fps_denominator",
            "pictures",
            "audio",
            "external_layer_spans",
        ] {
            if serde_json::to_value(&native)?[field] != serde_json::to_value(&plan)?[field] {
                bail!("comparison changes selected native timing: {field}");
            }
        }
        indices.extend(motioncraft_review::samples(treatment, &native)?.into_keys());
        for span in &native.pictures {
            indices.extend([span.start_frame, span.end_frame - 1]);
        }
        let dir = package.join(name);
        scene_delivery::render(&path, &retained_assets, &dir)?;
        scene_delivery::check(&path, &retained_assets, &dir)?;
        let stems = ["D.wav", "E.wav", "M.wav", "mix.wav"]
            .into_iter()
            .map(|stem| Ok((stem.to_string(), crate::sha256_file(&dir.join(stem))?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        if reference_stems.is_empty() {
            reference_stems = stems.clone();
        }
        if stems != reference_stems {
            bail!("comparison changes exact native stems");
        }
        let cadence = motioncraft_cadence::analyze(treatment, &native, &dir)?;
        write(&package.join(format!("{name}-cadence.json")), &cadence)?;
        reports.insert(name.clone(), json!({"job_sha256":crate::sha256_file(&path)?,
            "receipt_sha256":crate::sha256_file(&dir.join("receipt.json"))?, "cadence_passed":cadence["passed"]}));
    }
    if indices.len() > 256 {
        bail!("comparison exceeds 256 indexed-frame evidence budget; split into scenes");
    }
    for name in treatments.keys() {
        let dir = package.join(name);
        let evidence = dir.join("comparison-frames");
        fs::create_dir(&evidence)?;
        let pixels = indexed_pixels(&dir.join("picture.mkv"), &indices, job.width, job.height)?;
        let stride = job.width as usize * job.height as usize * 3;
        for (index, frame) in indices.iter().enumerate() {
            let image = image::RgbImage::from_raw(
                job.width,
                job.height,
                pixels[index * stride..(index + 1) * stride].to_vec(),
            )
            .context("comparison image geometry")?;
            image.save(evidence.join(format!("frame-{frame:08}.png")))?;
        }
    }
    fs::write(package.join("playback.html"), PLAYBACK)?;
    let mut files = BTreeMap::new();
    inventory(package, package, &mut files)?;
    let executable = std::env::current_exe()?;
    let cadence_passed = reports.values().all(|row| row["cadence_passed"] == true);
    let changed_attachments: Vec<_> = job
        .pictures
        .iter()
        .filter(|p| matches!(p.motion, Some(PictureMotion::PhasedCamera { .. })))
        .map(|p| p.attachment_id.clone())
        .collect();
    if crate::sha256_file(request_path)? != request_sha256 {
        bail!("comparison request changed during execution");
    }
    let report = json!({"schema":"reel.motioncraft-comparison.v1", "request_sha256":request_sha256,
        "source_job_sha256":request.source_job.sha256, "engine_commit":request.engine_commit,
        "producer_executable_sha256":crate::sha256_file(&executable)?, "tool_version":env!("CARGO_PKG_VERSION"),
        "native_clock_verified":true, "duration_samples":plan.duration_samples, "frame_count":plan.frame_count,
        "sample_rate":plan.sample_rate,"fps_numerator":plan.fps_numerator,"fps_denominator":plan.fps_denominator,
        "stems":reference_stems,"variants":reports,"indexed_frames":indices,"files":files,
        "changed_picture_attachments":changed_attachments,"allowed_changes":"phased picture camera treatment only; native clocks, selected assets, captions, layers and audio unchanged",
        "rollback":"original-job.json retains exact source bytes; jobs/source.json and assets are the portable equivalent",
        "all_camera_cadence_passed":cadence_passed,
        "creative_authority":"not-granted", "publication":"not-authorized"});
    write(&package.join("comparison.json"), &report)?;
    check(package)?;
    // Persist the completed stage, then use atomic rename to a new destination.
    let staged = stage.keep();
    if let Err(error) = fs::rename(&staged, output) {
        fs::remove_dir_all(&staged)?;
        return Err(error.into());
    }
    Ok(report)
}

const PLAYBACK: &str = r#"<!doctype html><meta charset="utf-8"><title>Motioncraft comparison</title>
<style>body{font:16px system-ui;background:#161616;color:#eee;margin:24px}.grid{display:flex;gap:12px}section{flex:1;min-width:0}video{width:100%}button{padding:10px}input{width:100%}</style>
<h1>Still / directed / reduced motion</h1><p>Private audition. Normal-speed native audio is primary; quarter speed is a silent diagnostic.</p>
<button id="play">Play / pause</button> <button id="quarter">Quarter / normal speed</button><input id="seek" type="range" min="0" max="1" step="0.0001" value="0" aria-label="Seek all variants">
<div class="grid"><section>Still<video src="still/review.mp4" muted playsinline></video></section><section>Directed<video src="directed/review.mp4" playsinline></video></section><section>Reduced<video src="reduced/review.mp4" muted playsinline></video></section></div>
<script>const v=[...document.querySelectorAll('video')],master=v[1];document.getElementById('play').onclick=()=>{if(master.paused){v.forEach(x=>{x.currentTime=master.currentTime;x.play()})}else v.forEach(x=>x.pause())};document.getElementById('quarter').onclick=()=>{const rate=master.playbackRate===1?.25:1;v.forEach(x=>x.playbackRate=rate);master.muted=rate!==1};const seek=document.getElementById('seek');seek.oninput=()=>v.forEach(x=>x.currentTime=Number(seek.value)*master.duration);master.ontimeupdate=()=>{seek.value=master.currentTime/master.duration;v.forEach(x=>{if(x!==master&&Math.abs(x.currentTime-master.currentTime)>.08)x.currentTime=master.currentTime})};master.onended=()=>v.forEach(x=>x.pause());</script>"#;

pub fn check(root: &Path) -> Result<Value> {
    let report: Value = serde_json::from_slice(&fs::read(root.join("comparison.json"))?)?;
    if report["schema"] != "reel.motioncraft-comparison.v1" {
        bail!("invalid comparison schema");
    }
    let files: BTreeMap<String, FileRef> = serde_json::from_value(report["files"].clone())?;
    for item in files.values() {
        scene_delivery::checked_file(root, item)?;
    }
    let mut actual = BTreeMap::new();
    inventory(root, root, &mut actual)?;
    actual.remove("comparison.json");
    if actual != files {
        bail!("comparison inventory is incomplete or differs");
    }
    if crate::sha256_file(&root.join("original-job.json"))?
        != report["source_job_sha256"]
            .as_str()
            .context("missing source hash")?
    {
        bail!("original job differs from bound source");
    }
    let (source, source_plan) =
        scene_delivery::plan(&root.join("jobs/source.json"), &root.join("assets"))?;
    let original: Job = serde_json::from_slice(&fs::read(root.join("original-job.json"))?)?;
    if serde_json::to_value(&original)? != serde_json::to_value(&source)? {
        bail!("portable source differs from original selection");
    }
    let expected = variants(&source, &source_plan)?;
    let changed_attachments: Vec<_> = source
        .pictures
        .iter()
        .filter(|p| matches!(p.motion, Some(PictureMotion::PhasedCamera { .. })))
        .map(|p| p.attachment_id.clone())
        .collect();
    if report["changed_picture_attachments"] != serde_json::to_value(changed_attachments)? {
        bail!("comparison change list differs");
    }
    let indices: BTreeSet<u64> = serde_json::from_value(report["indexed_frames"].clone())?;
    let mut expected_indices = BTreeSet::from([0, source_plan.frame_count - 1]);
    for (name, job) in &expected {
        let (_, plan) = scene_delivery::plan(
            &root.join(format!("jobs/{name}.json")),
            &root.join("assets"),
        )?;
        expected_indices.extend(motioncraft_review::samples(job, &plan)?.into_keys());
        for span in &plan.pictures {
            expected_indices.extend([span.start_frame, span.end_frame - 1]);
        }
    }
    if indices != expected_indices || report["native_clock_verified"] != true {
        bail!("comparison indexed schedule or clock claim differs");
    }
    let mut all_cadence_passed = true;
    for name in ["still", "directed", "reduced"] {
        let (job, plan) = scene_delivery::plan(
            &root.join(format!("jobs/{name}.json")),
            &root.join("assets"),
        )?;
        if serde_json::to_value(&job)? != serde_json::to_value(&expected[name])? {
            bail!("comparison treatment differs from exact source-derived variant");
        }
        for field in [
            "duration_samples",
            "frame_count",
            "sample_rate",
            "fps_numerator",
            "fps_denominator",
        ] {
            if report[field] != serde_json::to_value(&plan)?[field] {
                bail!("comparison native clock report differs");
            }
        }
        scene_delivery::check(
            &root.join(format!("jobs/{name}.json")),
            &root.join("assets"),
            &root.join(name),
        )?;
        let current_cadence = motioncraft_cadence::analyze(&job, &plan, &root.join(name))?;
        let mut retained_cadence: Value =
            serde_json::from_slice(&fs::read(root.join(format!("{name}-cadence.json")))?)?;
        // Keep producer tool identity, but independently reproduce frame metrics.
        for field in [
            "analyzer_ffmpeg_version",
            "analyzer_executable",
            "analyzer_executable_sha256",
        ] {
            if retained_cadence[field]
                .as_str()
                .is_none_or(|v| v.trim().is_empty())
            {
                bail!("comparison cadence lacks producer identity");
            }
            retained_cadence[field] = current_cadence[field].clone();
        }
        if retained_cadence != current_cadence {
            bail!("comparison cadence metrics differ from native frames");
        }
        let summary = json!({"job_sha256":crate::sha256_file(&root.join(format!("jobs/{name}.json")))?,
            "receipt_sha256":crate::sha256_file(&root.join(name).join("receipt.json"))?,"cadence_passed":current_cadence["passed"]});
        if report["variants"][name] != summary {
            bail!("comparison variant summary differs");
        }
        all_cadence_passed &= current_cadence["passed"] == true;
        let pixels = indexed_pixels(
            &root.join(name).join("picture.mkv"),
            &indices,
            job.width,
            job.height,
        )?;
        let stride = job.width as usize * job.height as usize * 3;
        for (index, frame) in indices.iter().enumerate() {
            let image = image::open(
                root.join(name)
                    .join(format!("comparison-frames/frame-{frame:08}.png")),
            )?
            .to_rgb8();
            if image.width() != job.width
                || image.height() != job.height
                || image.as_raw().as_slice() != &pixels[index * stride..(index + 1) * stride]
            {
                bail!("comparison saved frame differs from exact decoded index");
            }
        }
        for (stem, expected) in report["stems"]
            .as_object()
            .context("missing comparison stems")?
        {
            if crate::sha256_file(&root.join(name).join(stem))?
                != expected.as_str().context("invalid stem hash")?
            {
                bail!("comparison stems differ");
            }
        }
    }
    if report["all_camera_cadence_passed"] != all_cadence_passed {
        bail!("comparison aggregate cadence claim differs");
    }
    Ok(report)
}
