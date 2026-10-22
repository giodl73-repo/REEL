//! Publication and bounded diagnostics precede retirement of one owned workspace.
use super::*;
use flate2::{Compression, write::GzEncoder};

pub(super) fn is_reparse(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

pub(super) fn pin_read_only(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_reparse(&metadata) {
        return Err("Job view must be a regular non-reparse file".into());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Readers coexist across jobs; tools cannot write through a hard link.
        options.share_mode(0x1 | 0x4).custom_flags(0x0020_0000);
    }
    Ok(options.open(path)?)
}

fn publish(root: &Path, cfg: &crate::Config, cache: &Path, path: &Path) -> Result<Value> {
    let size = fs::metadata(path)?.len();
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let oid = format!("{:x}", h.finalize());
    if active_existing(cfg, &oid, size)?.is_none() {
        recover(root, path, cache, &oid, size)?;
    }
    Ok(json!({"sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}")}))
}

pub(super) fn publish_and_retire(
    root: &Path,
    cfg: &crate::Config,
    cache: &Path,
    workspace: &Path,
    request: &[u8],
    result: &Value,
    job: &Job,
) -> Result<Value> {
    let canonical = workspace.canonicalize()?;
    let staging = cache.join("staging/caminos").canonicalize()?;
    if canonical.parent() != Some(staging.as_path())
        || !cfg.cache_roots.contains(&cache.to_path_buf())
        || !canonical
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with("job-"))
    {
        return Err("Retirement target escaped its exact admitted job staging parent".into());
    }
    usage(workspace, 0, &mut 0)?; // Reject reparse/symlink entries before any cleanup.
    // A temporary hard link may outlive a canonical name removed by another
    // operation. Never retire that last remaining view of a retained original.
    for asset in job.inputs.values().chain(job.tools.values()) {
        if active_existing(cfg, &asset.sha256, asset.bytes)?.is_none() {
            return Err("Retained original absent from active caches; workspace preserved".into());
        }
    }
    for tool in job.installed_tools.values() {
        verify(&tool.path, &tool.sha256, tool.bytes)?;
    }
    let mut controls = BTreeMap::new();
    for entry in fs::read_dir(workspace)? {
        let p = entry?.path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if (name.starts_with("step-") && name.ends_with(".log")) || name.ends_with(".pid") {
            let m = fs::symlink_metadata(&p)?;
            if !m.is_file() || is_reparse(&m) {
                return Err("Unsafe diagnostic file".into());
            }
            use std::io::{Seek, SeekFrom};
            let mut f = File::open(&p)?;
            let skip = m.len().saturating_sub(16 * 1024);
            f.seek(SeekFrom::Start(skip))?;
            let mut b = Vec::new();
            f.take(16 * 1024).read_to_end(&mut b)?;
            controls.insert(
                name,
                json!({"truncated_prefix_bytes":skip,"text":String::from_utf8_lossy(&b)}),
            );
            if controls.len() > 68 {
                return Err("Diagnostic file count exceeded".into());
            }
        }
    }
    // Completed retained outputs stay authoritative; failed jobs also salvage
    // declared bounded output files as unapproved candidates, never intermediates.
    let mut outputs = result["outputs"].as_object().cloned().unwrap_or_default();
    if result["schema"] == "reel.failed-job-result.v1" {
        for (role, output) in &job.outputs {
            let p = workspace.join(relative(&output.path)?);
            match fs::symlink_metadata(&p) {
                Ok(m) if m.is_file() && !is_reparse(&m) && m.len() <= output.max_bytes => {
                    outputs.insert(role.clone(), publish(root, cfg, cache, &p)?);
                }
                Ok(_) => {
                    return Err(
                        "Failed output is unsafe or exceeds its declared salvage bound".into(),
                    );
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
        }
    }
    let request_path = workspace.join("job-retirement-request.json");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request_path)?
        .write_all(request)?;
    let manifest = publish(root, cfg, cache, &request_path)?;
    let payload = json!({"schema":"reel.job-retirement-evidence.v1","request_utf8":std::str::from_utf8(request)?,
        "result":result,"declared_retained_outputs":outputs,"diagnostics":controls,
        "scope":"Exact job control/log snapshot before scratch retirement; no creative approval or rendered acceptance"});
    let b = serde_json::to_vec(&payload)?;
    if b.len() > 2 * 1024 * 1024 {
        return Err("Job retirement evidence exceeds bound".into());
    }
    let evidence_path = workspace.join("job-retirement-evidence.json.gz");
    let mut encoder = GzEncoder::new(
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&evidence_path)?,
        Compression::fast(),
    );
    encoder.write_all(&b)?;
    encoder.finish()?.sync_all()?;
    let evidence = publish(root, cfg, cache, &evidence_path)?;
    let mut assets = vec![manifest, evidence.clone()];
    assets.extend(outputs.values().cloned());
    for asset in &mut assets {
        asset["provenance"] = json!(format!(
            "Admitted job {} exact request and bounded output/diagnostic publication",
            result["job_id"]
        ));
        asset["rights"] = json!(
            "Private job custody; upstream rights and restrictions remain; no creative, consent or publication approval"
        );
        asset["selection_state"] = json!("retained-private-job-candidate");
    }
    let registration = workspace.join("job-retirement-register.json");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&registration)?
        .write_all(&serde_json::to_vec(&json!({"assets":assets}))?)?;
    crate::registry::register(root, &registration)?;
    // Prove that every retained artifact really exists at its canonical E/D
    // locator before deleting scratch. No archive fallback or reserve override.
    for asset in assets {
        let h = asset["sha256"].as_str().ok_or("Missing retained hash")?;
        let n = asset["bytes"].as_u64().ok_or("Missing retained bytes")?;
        if active_existing(cfg, h, n)?.is_none() {
            return Err("Retained job artifact absent; workspace preserved".into());
        }
    }
    usage(workspace, 0, &mut 0)?;
    if workspace.canonicalize()? != canonical || canonical.parent() != Some(staging.as_path()) {
        return Err("Job target changed before retirement".into());
    }
    fs::remove_dir_all(&canonical)?;
    Ok(evidence)
}
