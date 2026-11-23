//! Export one explicit part as an ordinary bounded native runner request.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct JobRequest {
    schema: String,
    manifest: PathBuf,
    range: PathBuf,
    runtime: PathBuf,
    budget_bytes: u64,
    scratch_bytes: u64,
    timeout_seconds: u64,
    lossless_max_bytes: u64,
    video_max_bytes: u64,
    pcm_max_bytes: u64,
}

pub fn describe_job(request: &Path, root: &Path) -> Result<serde_json::Value> {
    let request: JobRequest = serde_json::from_slice(&checkpoint::bounded_bytes(request)?)?;
    if request.schema != "reel.programme-part-job-request.v1" {
        bail!("unsupported programme part job request");
    }
    let scope = checkpoint::snapshot_job_scope(root)?;
    let manifest = declared_file(root, &scope, &request.manifest, 4 * 1024 * 1024)?;
    let range = declared_file(root, &scope, &request.range, 4096)?;
    let runtime = declared_file(root, &scope, &request.runtime, 65536)?;
    let manifest: Manifest = serde_json::from_slice(&manifest)?;
    let range: Range = serde_json::from_slice(&range)?;
    range.validate(&manifest)?;
    let runtime: crate::media_runtime::Bindings = serde_json::from_slice(&runtime)?;
    let _tools = crate::media_runtime::BoundPrograms::bind(&runtime)?;

    let mut inputs = BTreeMap::new();
    for file in scope
        .iter()
        .filter(|file| file.path != Path::new("job.json"))
    {
        inputs.insert(
            name(&file.path)?,
            serde_json::json!({"sha256":file.sha256,"bytes":file.bytes}),
        );
    }
    let mut outputs = BTreeMap::from([(
        "native/part-proof.json".into(),
        serde_json::json!({"path":"native/part-proof.json","max_bytes":4*1024*1024}),
    )]);
    for index in range.start..range.end {
        for (name, maximum) in [
            (
                format!("native/lossless-{index:05}.mkv"),
                request.lossless_max_bytes,
            ),
            (
                format!("native/video-{index:05}.mp4"),
                request.video_max_bytes,
            ),
            (
                format!("native/audio-{index:05}.pcm"),
                request.pcm_max_bytes,
            ),
        ] {
            outputs.insert(
                name.clone(),
                serde_json::json!({"path":name,"max_bytes":maximum}),
            );
        }
    }
    let mut installed_tools = BTreeMap::from([
        (
            "part-worker".to_owned(),
            serde_json::json!({
                "path":std::env::current_exe()?.canonicalize()?,
                "sha256":runtime.host.sha256,"bytes":runtime.host.bytes
            }),
        ),
        (
            "part-encoder".to_owned(),
            serde_json::to_value(runtime.ffmpeg)?,
        ),
        (
            "part-probe".to_owned(),
            serde_json::to_value(runtime.ffprobe)?,
        ),
    ]);
    for (index, dependency) in runtime.dependencies.values().enumerate() {
        installed_tools.insert(
            format!("part-dependency-{index:03}"),
            serde_json::to_value(dependency)?,
        );
    }
    let job = serde_json::json!({
        "budget_bytes":request.budget_bytes,
        "scratch_bytes":request.scratch_bytes,
        "step_timeout_limit_seconds":request.timeout_seconds,
        "inputs":inputs,
        "tools":{},
        "installed_tools":installed_tools,
        "prepare_output_parents":false,
        "steps":[{"tool":"part-worker","timeout_seconds":request.timeout_seconds,
            "args":["produce-parts",name(&request.manifest)?,"--asset-root",".",
                "--range",name(&request.range)?,"--output-dir","native",
                "--runtime",name(&request.runtime)?]}],
        "outputs":outputs
    });
    reel_assets::jobs::check_request(&serde_json::to_vec(&job)?).map_err(|error| {
        anyhow::anyhow!("programme part job does not fit runner limits: {error}")
    })?;
    Ok(job)
}

fn name(path: &Path) -> Result<String> {
    if path.as_os_str().is_empty()
        || !path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
    {
        bail!("programme job control must be a workspace-relative path");
    }
    Ok(path
        .to_str()
        .context("non-UTF8 programme job path")?
        .replace('\\', "/"))
}

fn declared_file(
    root: &Path,
    scope: &[scene_delivery::FileRef],
    path: &Path,
    limit: u64,
) -> Result<Vec<u8>> {
    let _ = name(path)?;
    let path = root.join(path);
    let pinned = declared_control(&path, root, scope)?;
    if pinned.bytes > limit {
        bail!("programme job control exceeds its bound");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(pinned.bytes + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != pinned.bytes || sha(&bytes) != pinned.sha256 {
        bail!("programme job control differs from declared bytes");
    }
    Ok(bytes)
}
