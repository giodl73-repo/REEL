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
    publish_described(root, cfg, cache, path, &describe(path)?)
}
fn describe(path: &Path) -> Result<publication::Descriptor> {
    let size = fs::metadata(path)?.len();
    let mut f = pin_read_only(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    let oid = format!("{:x}", h.finalize());
    Ok(publication::Descriptor {
        sha256: oid,
        bytes: size,
    })
}
fn publish_described(
    root: &Path,
    cfg: &crate::Config,
    cache: &Path,
    path: &Path,
    descriptor: &publication::Descriptor,
) -> Result<Value> {
    let oid = &descriptor.sha256;
    let size = descriptor.bytes;
    if active_existing(cfg, oid, size)?.is_none() {
        recover(root, path, cache, oid, size)?;
    }
    Ok(json!({"sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}")}))
}
/// Exact canonical custody pins for enrolled scratch retirement/discard.
/// No hash-range/global lock; other immutable readers remain compatible.
pub(super) fn pin_retained(cfg: &crate::Config, hash: &str, bytes: u64) -> Result<Vec<File>> {
    if !valid_oid(hash) || bytes > cfg.capacity.max_object_bytes {
        return Err("Invalid retained job descriptor".into());
    }
    for cache in &cfg.cache_roots {
        let path = object(cache, hash);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                if !metadata.is_file() || is_reparse(&metadata) {
                    return Err("Invalid retained job copy".into());
                }
                let mut guards = crate::demotion::platform::guard_dirs(&path)?;
                let file = crate::demotion::platform::open(&path, false)?;
                crate::demotion::platform::identity(&file)?;
                verify(&path, hash, bytes)?;
                guards.push(file);
                return Ok(guards);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Err("Retained job artifact absent; workspace preserved".into())
}

pub(super) fn checked_workspace(
    cfg: &crate::Config,
    cache: &Path,
    workspace: &Path,
) -> Result<PathBuf> {
    let canonical = workspace.canonicalize()?;
    let cache_canonical = cache.canonicalize()?;
    let staging = cache.join("staging/caminos").canonicalize()?;
    if !cfg.cache_roots.contains(&cache.to_path_buf())
        || staging != cache_canonical.join("staging/caminos")
        || !canonical.starts_with(&cache_canonical)
        || canonical.parent() != Some(staging.as_path())
        || is_reparse(&fs::symlink_metadata(workspace)?)
        || !canonical
            .file_name()
            .is_some_and(|s| s.to_string_lossy().starts_with("job-"))
    {
        return Err("Job workspace escaped its exact admitted cache staging parent".into());
    }
    Ok(canonical)
}

pub(super) fn prepare_staging(cache: &Path) -> Result<()> {
    let mut parent = cache.canonicalize()?;
    for name in ["staging", "caminos"] {
        parent.push(name);
        match fs::symlink_metadata(&parent) {
            Ok(m) if m.is_dir() && !is_reparse(&m) => (),
            Ok(_) => {
                return Err("Job workspace escaped its exact admitted cache staging parent".into());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&parent) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                    Err(e) => return Err(e.into()),
                }
                let m = fs::symlink_metadata(&parent)?;
                if !m.is_dir() || is_reparse(&m) {
                    return Err(
                        "Job workspace escaped its exact admitted cache staging parent".into(),
                    );
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)] // exact native job scope plus optional domain
pub(super) fn publish_and_retire(
    root: &Path,
    cfg: &crate::Config,
    cache: &Path,
    workspace: &Path,
    request: &[u8],
    result: &Value,
    job: &Job,
    domain: Option<&Domain>,
) -> Result<Value> {
    publish_and_retire_inner(
        root,
        cfg,
        cache,
        workspace,
        request,
        result,
        job,
        domain,
        || Ok(()),
    )
}
#[allow(clippy::too_many_arguments)] // exact existing job scope plus optional enrollment/test boundary
pub(super) fn publish_and_retire_inner(
    root: &Path,
    cfg: &crate::Config,
    cache: &Path,
    workspace: &Path,
    request: &[u8],
    result: &Value,
    job: &Job,
    domain: Option<&Domain>,
    before_unlink: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    let canonical = checked_workspace(cfg, cache, workspace)?;
    let staging = cache.join("staging/caminos").canonicalize()?;
    usage(workspace, 0, &mut 0)?; // Reject reparse/symlink entries before any cleanup.
    let mut _custody = Vec::new();
    // A temporary hard link may outlive a canonical name removed by another
    // operation. Never retire that last remaining view of a retained original.
    for asset in job.inputs.values().chain(job.tools.values()) {
        if domain.is_some() {
            _custody.extend(pin_retained(cfg, &asset.sha256, asset.bytes)?);
        } else if active_existing(cfg, &asset.sha256, asset.bytes)?.is_none() {
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
    let mut salvage = Vec::new();
    if result["schema"] == "reel.failed-job-result.v1" {
        for (role, output) in &job.outputs {
            let p = workspace.join(relative(&output.path)?);
            match fs::symlink_metadata(&p) {
                Ok(m) if m.is_file() && !is_reparse(&m) && m.len() <= output.max_bytes => {
                    if domain.is_some() {
                        let descriptor = describe(&p)?;
                        outputs.insert(role.clone(),json!({"sha256":descriptor.sha256,"bytes":descriptor.bytes,"cache_uri":format!("cache://sha256/{}",descriptor.sha256)}));
                        salvage.push((p, descriptor));
                    } else {
                        outputs.insert(role.clone(), publish(root, cfg, cache, &p)?);
                    }
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
    let manifest_descriptor = if domain.is_some() {
        Some(describe(&request_path)?)
    } else {
        None
    };
    let mut manifest = if domain.is_none() {
        Some(publish(root, cfg, cache, &request_path)?)
    } else {
        None
    };
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
    let evidence = if let Some(domain) = domain {
        let evidence_descriptor = describe(&evidence_path)?;
        if evidence_descriptor.bytes > publication::EVIDENCE_LIMIT {
            return Err("Enrolled retirement evidence exceeds budget".into());
        }
        let manifest_descriptor =
            manifest_descriptor.ok_or("Missing retirement request descriptor")?;
        let intended = outputs
            .iter()
            .map(|(role, value)| {
                Ok((
                    role.clone(),
                    publication::Descriptor {
                        sha256: value["sha256"]
                            .as_str()
                            .ok_or("Missing retained output hash")?
                            .into(),
                        bytes: value["bytes"]
                            .as_u64()
                            .ok_or("Missing retained output size")?,
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        publication::retirement(
            workspace,
            result["job_id"].as_str().ok_or("Missing job ID")?,
            request,
            domain,
            manifest_descriptor.clone(),
            evidence_descriptor.clone(),
            intended,
        )?;
        for (path, descriptor) in salvage {
            publish_described(root, cfg, cache, &path, &descriptor)?;
        }
        manifest = Some(publish_described(
            root,
            cfg,
            cache,
            &request_path,
            &manifest_descriptor,
        )?);
        publish_described(root, cfg, cache, &evidence_path, &evidence_descriptor)?
    } else {
        publish(root, cfg, cache, &evidence_path)?
    };
    let manifest = manifest.ok_or("Missing retained request")?;
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
    let registration_bytes = serde_json::to_vec(&json!({"assets":assets}))?;
    if domain.is_some() && registration_bytes.len() as u64 > publication::REGISTRATION_LIMIT {
        return Err("Enrolled retirement registration exceeds budget".into());
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&registration)?
        .write_all(&registration_bytes)?;
    crate::registry::register(root, &registration)?;
    // Prove that every retained artifact really exists at its canonical E/D
    // locator before deleting scratch. No archive fallback or reserve override.
    for asset in assets {
        let h = asset["sha256"].as_str().ok_or("Missing retained hash")?;
        let n = asset["bytes"].as_u64().ok_or("Missing retained bytes")?;
        if domain.is_some() {
            _custody.extend(pin_retained(cfg, h, n)?);
        } else if active_existing(cfg, h, n)?.is_none() {
            return Err("Retained job artifact absent; workspace preserved".into());
        }
    }
    usage(workspace, 0, &mut 0)?;
    if checked_workspace(cfg, cache, workspace)? != canonical
        || canonical.parent() != Some(staging.as_path())
    {
        return Err("Job target changed before retirement".into());
    }
    if domain.is_some() {
        // Validate exact enrollment/intent again after registration/custody,
        // never trusting the advisory intent as retained-copy proof.
        publication::discard_domain(&cache.canonicalize()?, workspace, request)?;
    }
    before_unlink()?;
    fs::remove_dir_all(&canonical)?;
    Ok(evidence)
}
