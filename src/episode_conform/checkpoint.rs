//! Bounded native preparation custody. Preparation is never a final receipt.
use super::*;

mod completion;

const CHECKPOINT_LIMIT: u64 = 4 * 1024 * 1024;

fn bounded_bytes(path: &Path) -> Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(CHECKPOINT_LIMIT + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > CHECKPOINT_LIMIT {
        bail!("episode checkpoint exceeds its bound");
    }
    Ok(bytes)
}

fn publish_normalization_progress(work: &Path, bytes: &[u8]) -> Result<()> {
    if bytes.len() as u64 > CHECKPOINT_LIMIT {
        bail!("episode checkpoint exceeds its bound");
    }
    let mut staged = tempfile::NamedTempFile::new_in(work)?;
    staged.write_all(bytes)?;
    staged.as_file().sync_all()?;
    staged.persist(work.join("normalization-progress.json"))?;
    Ok(())
}

pub(super) fn verify_preparation(prepared: &Path, assets: &Path) -> Result<()> {
    completion::verify_preparation(prepared, assets)
}

pub(super) fn encode_verified(prepared: &Path, assets: &Path) -> Result<()> {
    completion::encode_verified(prepared, assets)
}

pub(super) fn finish_encoded(prepared: &Path, assets: &Path, output: &Path) -> Result<Receipt> {
    completion::finish_encoded(prepared, assets, output)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RootedFile {
    root: String,
    file: scene_delivery::FileRef,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Prepared {
    schema: String,
    validator_sha256: String,
    ffmpeg_sha256: String,
    ffprobe_sha256: String,
    source_scope: Vec<scene_delivery::FileRef>,
    manifest_json: String,
    manifest_sha256: String,
    first: MediaFacts,
    records: Vec<SegmentReceipt>,
    normalized: Vec<RootedFile>,
    master: RootedFile,
    frame_cursor: u64,
    sample_cursor: u64,
    presentation_fingerprint_sha256: String,
    presentation_cache_hits: usize,
    presentation_cache_misses: usize,
    full_episode_verified: bool,
}

pub(super) fn tool_hash(name: &str) -> Result<String> {
    file_sha(Path::new(command(name).get_program()))
}

/// Incomplete normalization custody; never a final preparation or receipt.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NormalizationProgress {
    schema: String,
    validator_sha256: String,
    ffmpeg_sha256: String,
    ffprobe_sha256: String,
    job_sha256: String,
    source_scope: Vec<scene_delivery::FileRef>,
    manifest_json: String,
    manifest_sha256: String,
    first: MediaFacts,
    records: Vec<SegmentReceipt>,
    normalized: Vec<RootedFile>,
    frame_cursor: u64,
    sample_cursor: u64,
    presentation_fingerprint_sha256: String,
    presentation_cache_hits: usize,
    presentation_cache_misses: usize,
    full_episode_verified: bool,
}

pub(super) struct NormalizationState {
    pub source_scope: Vec<scene_delivery::FileRef>,
    pub manifest_bytes: Vec<u8>,
    pub first: MediaFacts,
    pub records: Vec<SegmentReceipt>,
    pub normalized: Vec<PathBuf>,
    pub frame_cursor: u64,
    pub sample_cursor: u64,
    pub presentation_fingerprint_sha256: String,
    pub presentation_cache_hits: usize,
    pub presentation_cache_misses: usize,
}

pub(super) fn save_normalization(
    state: &NormalizationState,
    work: &Path,
    assets: &Path,
) -> Result<()> {
    for file in &state.source_scope {
        scene_delivery::checked_file(assets, file)?;
    }
    let progress = NormalizationProgress {
        schema: "reel.episode-normalization-progress.v1".into(),
        validator_sha256: file_sha(&std::env::current_exe()?)?,
        ffmpeg_sha256: tool_hash("ffmpeg")?,
        ffprobe_sha256: tool_hash("ffprobe")?,
        job_sha256: file_sha(&assets.join("job.json"))?,
        source_scope: state.source_scope.clone(),
        manifest_json: String::from_utf8(state.manifest_bytes.clone())?,
        manifest_sha256: sha(&state.manifest_bytes),
        first: state.first.clone(),
        records: serde_json::from_value(serde_json::to_value(&state.records)?)?,
        normalized: state
            .normalized
            .iter()
            .map(|p| capture_file(p, work, assets))
            .collect::<Result<_>>()?,
        frame_cursor: state.frame_cursor,
        sample_cursor: state.sample_cursor,
        presentation_fingerprint_sha256: state.presentation_fingerprint_sha256.clone(),
        presentation_cache_hits: state.presentation_cache_hits,
        presentation_cache_misses: state.presentation_cache_misses,
        full_episode_verified: false,
    };
    publish_normalization_progress(work, &serde_json::to_vec_pretty(&progress)?)
}

struct VerifiedNormalization {
    progress: NormalizationProgress,
    bytes: Vec<u8>,
    paths: Vec<PathBuf>,
}

fn verify_normalization(prepared: &Path, assets: &Path) -> Result<VerifiedNormalization> {
    if prepared.canonicalize()?.parent() != Some(assets.canonicalize()?.as_path())
        || prepared.join("prepared.json").exists()
    {
        bail!(
            "normalization progress requires its incomplete preparation inside the same workspace"
        );
    }
    let bytes = bounded_bytes(&prepared.join("normalization-progress.json"))?;
    let progress: NormalizationProgress = serde_json::from_slice(&bytes)?;
    if progress.schema != "reel.episode-normalization-progress.v1"
        || progress.full_episode_verified
        || progress.validator_sha256 != file_sha(&std::env::current_exe()?)?
        || progress.ffmpeg_sha256 != tool_hash("ffmpeg")?
        || progress.ffprobe_sha256 != tool_hash("ffprobe")?
        || progress.job_sha256 != file_sha(&assets.join("job.json"))?
        || progress.manifest_sha256 != sha(progress.manifest_json.as_bytes())
        || progress.source_scope.is_empty()
        || progress.source_scope.len() > 65536
    {
        bail!("normalization progress identity or tool changed");
    }
    for source in &progress.source_scope {
        scene_delivery::checked_file(assets, source)?;
    }
    let manifest: Manifest = serde_json::from_str(&progress.manifest_json)?;
    if progress.records.is_empty()
        || progress.records.len() > manifest.segments.len()
        || progress.records.len() != progress.normalized.len()
    {
        bail!("normalization progress coverage differs from manifest");
    }
    let mut frames = 0u64;
    let mut samples = 0u64;
    for (record, selected) in progress.records.iter().zip(&manifest.segments) {
        if record.id != selected.id
            || record.selected_master_sha256 != selected.master.sha256
            || record.selected_master_bytes != selected.master.bytes
            || record.source_receipt_sha256 != selected.source_receipt.sha256
            || record.start_frame != frames
            || record.start_sample != samples
            || record.frames == 0
            || record.samples == 0
            || record.output_sample_rate != manifest.output_sample_rate
            || !valid_sha(&record.decoded_picture_sha256)
            || !valid_sha(&record.decoded_audio_sha256)
        {
            bail!("normalization progress differs from source or clock");
        }
        frames = frames
            .checked_add(record.frames)
            .context("progress frame overflow")?;
        samples = samples
            .checked_add(record.samples)
            .context("progress sample overflow")?;
    }
    if frames != progress.frame_cursor || samples != progress.sample_cursor {
        bail!("normalization progress clock differs from segments");
    }
    let normalized = progress
        .normalized
        .iter()
        .map(|f| resolve(f, prepared, assets))
        .collect::<Result<_>>()?;
    Ok(VerifiedNormalization {
        progress,
        bytes,
        paths: normalized,
    })
}

pub(super) fn load_normalization(prepared: &Path, assets: &Path) -> Result<NormalizationState> {
    let VerifiedNormalization {
        progress, paths, ..
    } = verify_normalization(prepared, assets)?;
    Ok(NormalizationState {
        source_scope: progress.source_scope,
        manifest_bytes: progress.manifest_json.into_bytes(),
        first: progress.first,
        records: progress.records,
        normalized: paths,
        frame_cursor: progress.frame_cursor,
        sample_cursor: progress.sample_cursor,
        presentation_fingerprint_sha256: progress.presentation_fingerprint_sha256,
        presentation_cache_hits: progress.presentation_cache_hits,
        presentation_cache_misses: progress.presentation_cache_misses,
    })
}

/// Export only verified generated files. Original inputs are already in the pinned recipe.
pub(super) fn resume_descriptor(
    prepared: &Path,
    assets: &Path,
    start_step: usize,
) -> Result<serde_json::Value> {
    let assets = assets.canonicalize()?;
    let prepared = prepared.canonicalize()?;
    let source_job_id = assets
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| {
            name.starts_with("job-")
                && name.len() <= 100
                && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
        .context("resume descriptor requires an owned job directory name")?;
    let VerifiedNormalization {
        progress, bytes, ..
    } = verify_normalization(&prepared, &assets)?;
    let recipe = bounded_bytes(&assets.join("job.json"))?;
    if recipe.len() > 256 * 1024 || sha(&recipe) != progress.job_sha256 {
        bail!("resume descriptor recipe differs or exceeds its bound");
    }
    let job: serde_json::Value = serde_json::from_slice(&recipe)?;
    if job["steps"]
        .as_array()
        .is_none_or(|steps| start_step >= steps.len())
    {
        bail!("resume step is outside the pinned recipe");
    }
    let prefix = prepared
        .file_name()
        .and_then(|name| name.to_str())
        .context("non-UTF8 preparation name")?;
    let mut files = BTreeMap::from([(
        format!("{prefix}/normalization-progress.json"),
        serde_json::json!({"sha256": sha(&bytes), "bytes": bytes.len()}),
    )]);
    for rooted in &progress.normalized {
        if rooted.root == "asset" {
            if !progress.source_scope.iter().any(|source| {
                source.path == rooted.file.path
                    && source.sha256 == rooted.file.sha256
                    && source.bytes == rooted.file.bytes
            }) {
                bail!("resume descriptor references undeclared source media");
            }
            continue;
        }
        let reference = &rooted.file;
        let name = reference
            .path
            .to_str()
            .context("non-UTF8 normalization path")?;
        if files
            .insert(
                format!("{prefix}/{}", name.replace('\\', "/")),
                serde_json::json!({"sha256": reference.sha256, "bytes": reference.bytes}),
            )
            .is_some()
        {
            bail!("duplicate resume checkpoint file");
        }
    }
    let descriptor = serde_json::json!({
        "schema": "reel.native-job-resume.v1",
        "job_sha256": progress.job_sha256,
        "source_job_id": source_job_id,
        "start_step": start_step,
        "files": files,
    });
    if files.len() > 4096 || serde_json::to_vec(&descriptor)?.len() >= 256 * 1024 {
        bail!("resume descriptor exceeds its bound");
    }
    Ok(descriptor)
}

/// The generic native job contract is the closed staging authority. This is a
/// bounded verification census of that one workspace, never a source importer.
pub(super) fn snapshot_job_scope(root: &Path) -> Result<Vec<scene_delivery::FileRef>> {
    let root = root.canonicalize()?;
    let job = root.join("job.json");
    if fs::metadata(&job)?.len() > 4 * 1024 * 1024 {
        bail!("native job scope is too large");
    }
    let job_bytes = fs::read(job)?;
    let job: serde_json::Value = serde_json::from_slice(&job_bytes)?;
    let mut scope = BTreeMap::new();
    let mut total = 0u64;
    for group in ["inputs", "tools"] {
        for (path, value) in job[group]
            .as_object()
            .context("native job input scope missing")?
        {
            let file = scene_delivery::FileRef {
                path: path.clone().into(),
                sha256: value["sha256"]
                    .as_str()
                    .context("native input hash missing")?
                    .into(),
                bytes: value["bytes"]
                    .as_u64()
                    .context("native input bytes missing")?,
            };
            scene_delivery::checked_file(&root, &file)?;
            total = total
                .checked_add(file.bytes)
                .context("native input scope overflow")?;
            if scope.insert(path.clone(), file).is_some() {
                bail!("duplicate native input scope");
            }
        }
    }
    if let Some(files) = job["files"].as_object() {
        for (path, value) in files {
            let text = value.as_str().context("native inline input must be text")?;
            let file = scene_delivery::FileRef {
                path: path.clone().into(),
                sha256: sha(text.as_bytes()),
                bytes: text.len() as u64,
            };
            scene_delivery::checked_file(&root, &file)?;
            total = total
                .checked_add(file.bytes)
                .context("native input scope overflow")?;
            if scope.insert(path.clone(), file).is_some() {
                bail!("duplicate native input scope");
            }
        }
    }
    if scope.is_empty() || scope.len() > 65536 || total > 8 * 1024 * 1024 * 1024 {
        bail!("native preparation scope exceeds its bound");
    }
    let mut pending = vec![root.clone()];
    let mut entries = 0usize;
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(dir)? {
            entries += 1;
            if entries > 65536 {
                bail!("native preparation workspace census exceeds its bound");
            }
            let path = entry?.path();
            let meta = fs::symlink_metadata(&path)?;
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    bail!("native preparation workspace contains a reparse point");
                }
            }
            if meta.file_type().is_symlink() {
                bail!("native preparation workspace contains a link");
            }
            if meta.is_dir() {
                pending.push(path);
                continue;
            }
            if !meta.is_file() {
                bail!("native preparation workspace contains a special file");
            }
            let relative = path
                .strip_prefix(&root)?
                .to_str()
                .context("non-UTF8 native input path")?
                .replace('\\', "/");
            let log = relative
                .strip_prefix("step-")
                .and_then(|v| v.strip_suffix(".log"))
                .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()));
            if relative != "job.json" && !log && !scope.contains_key(&relative) {
                bail!("undeclared file in native preparation workspace: {relative}");
            }
        }
    }
    let job_file = scene_delivery::FileRef {
        path: "job.json".into(),
        sha256: sha(&job_bytes),
        bytes: job_bytes.len() as u64,
    };
    scene_delivery::checked_file(&root, &job_file)?;
    scope.insert("job.json".into(), job_file);
    Ok(scope.into_values().collect())
}

fn capture_file(path: &Path, work: &Path, assets: &Path) -> Result<RootedFile> {
    let path = path.canonicalize()?;
    let work = work.canonicalize()?;
    let assets = assets.canonicalize()?;
    let (root, relative) = if let Ok(relative) = path.strip_prefix(&work) {
        ("prepared", relative)
    } else {
        (
            "asset",
            path.strip_prefix(&assets)
                .context("prepared source escapes both roots")?,
        )
    };
    Ok(RootedFile {
        root: root.into(),
        file: scene_delivery::FileRef {
            path: relative
                .to_str()
                .context("non-UTF8 prepared path")?
                .replace('\\', "/")
                .into(),
            sha256: file_sha(&path)?,
            bytes: fs::metadata(path)?.len(),
        },
    })
}

pub(super) fn capture(
    state: &ConformState,
    work: &Path,
    assets: &Path,
    source_scope: Vec<scene_delivery::FileRef>,
) -> Result<Prepared> {
    // Rehash the closed original scope again after preparation, before issuance.
    for file in &source_scope {
        scene_delivery::checked_file(assets, file)?;
    }
    Ok(Prepared {
        schema: "reel.episode-conform-preparation.v1".into(),
        validator_sha256: file_sha(&std::env::current_exe()?)?,
        ffmpeg_sha256: tool_hash("ffmpeg")?,
        ffprobe_sha256: tool_hash("ffprobe")?,
        source_scope,
        manifest_json: String::from_utf8(state.manifest_bytes.clone())?,
        manifest_sha256: sha(&state.manifest_bytes),
        first: state.first.clone(),
        records: serde_json::from_value(serde_json::to_value(&state.records)?)?,
        normalized: state
            .normalized
            .iter()
            .map(|p| capture_file(p, work, assets))
            .collect::<Result<_>>()?,
        master: capture_file(&state.master, work, assets)?,
        frame_cursor: state.frame_cursor,
        sample_cursor: state.sample_cursor,
        presentation_fingerprint_sha256: state.presentation_fingerprint_sha256.clone(),
        presentation_cache_hits: state.presentation_cache_hits,
        presentation_cache_misses: state.presentation_cache_misses,
        full_episode_verified: false,
    })
}

fn resolve(file: &RootedFile, prepared: &Path, assets: &Path) -> Result<PathBuf> {
    let root = match file.root.as_str() {
        "prepared" => prepared,
        "asset" => assets,
        _ => bail!("unknown prepared source root"),
    };
    scene_delivery::checked_file(root, &file.file)
}

pub(super) fn finish(
    prepared: &Path,
    assets: &Path,
    output: &Path,
    consume_normalized: bool,
) -> Result<Receipt> {
    finish_mode(prepared, assets, output, consume_normalized, false)
}

fn finish_mode(
    prepared: &Path,
    assets: &Path,
    output: &Path,
    consume_normalized: bool,
    defer_compact: bool,
) -> Result<Receipt> {
    if output.exists() {
        bail!("episode output must be a new directory");
    }
    if consume_normalized
        && prepared.canonicalize()?.parent() != Some(assets.canonicalize()?.as_path())
    {
        bail!(
            "normalization consumption requires a preparation directly inside its closed workspace"
        );
    }
    let bytes = bounded_bytes(&prepared.join("prepared.json"))?;
    let checkpoint: Prepared = serde_json::from_slice(&bytes)?;
    if checkpoint.schema != "reel.episode-conform-preparation.v1"
        || checkpoint.full_episode_verified
        || checkpoint.validator_sha256 != file_sha(&std::env::current_exe()?)?
        || checkpoint.ffmpeg_sha256 != tool_hash("ffmpeg")?
        || checkpoint.ffprobe_sha256 != tool_hash("ffprobe")?
        || sha(checkpoint.manifest_json.as_bytes()) != checkpoint.manifest_sha256
    {
        bail!("episode preparation identity or tool changed");
    }
    if checkpoint.source_scope.is_empty() || checkpoint.source_scope.len() > 65536 {
        bail!("invalid prepared source scope");
    }
    for source in &checkpoint.source_scope {
        scene_delivery::checked_file(assets, source)?;
    }
    let manifest: Manifest = serde_json::from_str(&checkpoint.manifest_json)?;
    if checkpoint.records.len() != manifest.segments.len()
        || checkpoint.normalized.len() != manifest.segments.len()
    {
        bail!("prepared segment coverage differs from manifest");
    }
    let mut frames = 0u64;
    let mut samples = 0u64;
    for (record, selected) in checkpoint.records.iter().zip(&manifest.segments) {
        if record.id != selected.id
            || record.selected_master_sha256 != selected.master.sha256
            || record.selected_master_bytes != selected.master.bytes
            || record.source_receipt_sha256 != selected.source_receipt.sha256
            || record.start_frame != frames
            || record.start_sample != samples
            || record.frames == 0
            || record.samples == 0
            || record.output_sample_rate != manifest.output_sample_rate
            || !valid_sha(&record.decoded_picture_sha256)
            || !valid_sha(&record.decoded_audio_sha256)
        {
            bail!("prepared segment differs from selected source or clock");
        }
        frames = frames
            .checked_add(record.frames)
            .context("prepared frame overflow")?;
        samples = samples
            .checked_add(record.samples)
            .context("prepared sample overflow")?;
    }
    if frames != checkpoint.frame_cursor || samples != checkpoint.sample_cursor {
        bail!("prepared episode clock differs from segments");
    }
    let master = resolve(&checkpoint.master, prepared, assets)?;
    let facts = probe(&master)?;
    if facts.width != checkpoint.first.width
        || facts.height != checkpoint.first.height
        || facts.fps != checkpoint.first.fps
        || facts.sample_rate != manifest.output_sample_rate
        || facts.sample_aspect_ratio != checkpoint.first.sample_aspect_ratio
        || facts.color_properties != checkpoint.first.color_properties
    {
        bail!("prepared master media properties changed");
    }
    let normalized = checkpoint
        .normalized
        .iter()
        .map(|f| resolve(f, prepared, assets))
        .collect::<Result<Vec<_>>>()?;
    let normalization_retirement = if consume_normalized {
        let mut files = Vec::new();
        for file in &checkpoint.normalized {
            if file.root != "prepared" {
                continue; // Original immutable input; never consume it.
            }
            let parts = file.file.path.components().collect::<Vec<_>>();
            let name = file
                .file
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            let generated_name = name.strip_suffix(".mkv").is_some_and(|n| {
                ["normalized-", "padded-", "held-"].iter().any(|prefix| {
                    n.strip_prefix(prefix).is_some_and(|index| {
                        index.len() == 3 && index.bytes().all(|b| b.is_ascii_digit())
                    })
                })
            });
            if parts.len() != 2
                || !parts[0].as_os_str().to_string_lossy().starts_with(".tmp")
                || !generated_name
            {
                bail!(
                    "normalization consumption is restricted to exact generated preparation files"
                );
            }
            let path = resolve(file, prepared, assets)?;
            if path == master
                || checkpoint.source_scope.iter().any(|source| {
                    assets.join(&source.path).canonicalize().ok().as_ref() == Some(&path)
                })
            {
                bail!(
                    "normalization consumption cannot remove an original input or assembled master"
                );
            }
            files.push(file.file.clone());
        }
        Some(NormalizationRetirementRequest {
            root: prepared.canonicalize()?,
            files,
        })
    } else {
        None
    };
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let stage = tempfile::Builder::new()
        .prefix(".episode-finish-")
        .tempdir_in(parent)?;
    let receipt = finalize_conform_mode(
        ConformState {
            normalization_retirement,
            preparation_sha256: Some(sha(&bytes)),
            manifest_bytes: checkpoint.manifest_json.into_bytes(),
            first: checkpoint.first,
            records: checkpoint.records,
            normalized,
            master,
            frame_cursor: frames,
            sample_cursor: samples,
            presentation_fingerprint_sha256: checkpoint.presentation_fingerprint_sha256,
            presentation_cache_hits: checkpoint.presentation_cache_hits,
            presentation_cache_misses: checkpoint.presentation_cache_misses,
        },
        stage.path(),
        true,
        defer_compact,
    )?;
    if defer_compact {
        completion::save_verified(prepared, assets, &bytes, &receipt, stage.path())?;
    }
    fs::write(stage.path().join("preparation.json"), bytes)?;
    fs::rename(stage.path(), output)
        .with_context(|| format!("publishing completed preparation {}", output.display()))?;
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_progress_replacement_preserves_previous_checkpoint_on_refusal() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("normalization-progress.json");
        publish_normalization_progress(workspace.path(), b"first").unwrap();
        let at_limit = vec![b' '; CHECKPOINT_LIMIT as usize];
        publish_normalization_progress(workspace.path(), &at_limit).unwrap();
        assert_eq!(bounded_bytes(&path).unwrap(), at_limit);

        let oversized = vec![b' '; CHECKPOINT_LIMIT as usize + 1];
        assert!(publish_normalization_progress(workspace.path(), &oversized).is_err());
        assert_eq!(bounded_bytes(&path).unwrap(), at_limit);
        fs::write(&path, oversized).unwrap();
        assert!(bounded_bytes(&path).is_err());
    }
}
