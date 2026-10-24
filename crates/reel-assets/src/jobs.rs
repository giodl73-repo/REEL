//! Explicit trusted production jobs in admitted existing-cache staging.
//! This runner is not a sandbox for untrusted programs/manifests.
use crate::{
    Result, active_existing, admission, config, load, object, recover_job_output, selected,
    valid_oid, verify,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Asset {
    sha256: String,
    bytes: u64,
}

/// Installed runtimes need their neighboring DLLs and environment. Execute an
/// explicitly registered, hash-pinned installation instead of copying its EXE.
/// This remains a trusted-job contract, not a sandbox for arbitrary commands.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InstalledTool {
    path: PathBuf,
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    path: String,
    max_bytes: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    tool: String,
    args: Vec<String>,
    timeout_seconds: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    budget_bytes: u64,
    scratch_bytes: u64,
    inputs: BTreeMap<String, Asset>,
    tools: BTreeMap<String, Asset>,
    #[serde(default)]
    installed_tools: BTreeMap<String, InstalledTool>,
    /// Only these inputs receive writable private copies. Others are pinned read-only views.
    #[serde(default)]
    writable_inputs: BTreeSet<String>,
    /// Explicit diagnostics exception; normal jobs publish and retire their workspace.
    #[serde(default)]
    retain_workspace: bool,
    /// Native no-clobber producers create their own new output directories.
    #[serde(default = "prepare_output_parents_default")]
    prepare_output_parents: bool,
    #[serde(default)]
    files: BTreeMap<String, String>,
    steps: Vec<Step>,
    outputs: BTreeMap<String, Output>,
}

fn prepare_output_parents_default() -> bool {
    true
}

fn relative(name: &str) -> Result<PathBuf> {
    if name.is_empty() || name.len() > 200 || name.contains('\\') || name.contains(':') {
        return Err("Job path must be a bounded portable relative path".into());
    }
    for part in name.split('/') {
        let base = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end()
            .to_ascii_uppercase();
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.ends_with('.')
            || part.trim() != part
            || !part
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_. ".contains(&c))
            || [
                "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
                "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8",
                "LPT9",
            ]
            .contains(&base.as_str())
        {
            return Err("Unsafe job path component".into());
        }
    }
    Ok(PathBuf::from(name))
}

fn add(total: &mut u64, amount: u64) -> Result<()> {
    *total = total.checked_add(amount).ok_or("Job budget overflow")?;
    Ok(())
}
fn validate(job: &Job) -> Result<()> {
    if job.steps.is_empty()
        || job.steps.len() > 64
        || job.outputs.is_empty()
        || job.outputs.len() > 64
    {
        return Err("Job requires bounded steps and declared outputs".into());
    }
    let mut total = job.scratch_bytes;
    let mut paths = BTreeSet::from([
        "job.json".to_owned(),
        "result.json".to_owned(),
        "job-retirement-request.json".into(),
        "job-retirement-evidence.json.gz".into(),
        "job-retirement-register.json".into(),
    ]);
    if job
        .writable_inputs
        .iter()
        .any(|name| !job.inputs.contains_key(name))
    {
        return Err("Writable inputs must name declared inputs only".into());
    }
    for (name, asset) in job.inputs.iter().chain(job.tools.iter()) {
        relative(name)?;
        if !paths.insert(name.to_ascii_lowercase()) || !valid_oid(&asset.sha256) {
            return Err("Invalid or duplicate job input".into());
        }
        add(&mut total, asset.bytes)?;
    }
    for (name, tool) in &job.installed_tools {
        relative(name)?;
        if !paths.insert(name.to_ascii_lowercase())
            || !tool.path.is_absolute()
            || !valid_oid(&tool.sha256)
            || tool.bytes == 0
        {
            return Err("Invalid or conflicting installed tool".into());
        }
    }
    for (name, text) in &job.files {
        relative(name)?;
        if !paths.insert(name.to_ascii_lowercase()) || text.len() > 64 * 1024 {
            return Err("Invalid job control file".into());
        }
        add(&mut total, text.len() as u64)?;
    }
    for output in job.outputs.values() {
        relative(&output.path)?;
        if !paths.insert(output.path.to_ascii_lowercase()) || output.max_bytes == 0 {
            return Err("Invalid job output".into());
        }
        // Retained output plus the atomic publication copy coexist temporarily.
        add(
            &mut total,
            output
                .max_bytes
                .checked_mul(2)
                .ok_or("Output budget overflow")?,
        )?;
    }
    if total > job.budget_bytes {
        return Err(
            "Job budget does not cover inputs, scratch and atomic output publication".into(),
        );
    }
    for step in &job.steps {
        if !(job.tools.contains_key(&step.tool) || job.installed_tools.contains_key(&step.tool))
            || step.timeout_seconds == 0
            || step.timeout_seconds > 3600
            || step.args.len() > 256
            || step.args.iter().any(|a| a.len() > 8192 || a.contains('\0'))
        {
            return Err("Invalid bounded job step".into());
        }
    }
    Ok(())
}

fn read_workspace_directory(
    mut read: impl FnMut() -> std::io::Result<fs::ReadDir>,
) -> std::io::Result<Option<fs::ReadDir>> {
    // Windows can report ACCESS_DENIED/SHARING_VIOLATION while a temporary
    // directory is delete-pending. Re-read briefly, then fail closed if it stays
    // inaccessible. Only NotFound means the directory no longer contributes.
    for attempt in 0..4 {
        match read() {
            Ok(entries) => return Ok(Some(entries)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) if cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32)) && attempt < 3 => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
    unreachable!("bounded directory read always returns after its final attempt")
}

fn usage(dir: &Path, depth: usize, count: &mut usize) -> Result<u64> {
    if depth > 16 {
        return Err("Job workspace depth exceeded".into());
    }
    let mut size = 0;
    let entries = match read_workspace_directory(|| fs::read_dir(dir)) {
        Ok(Some(entries)) => entries,
        Ok(None) => return Ok(0),
        Err(e) => {
            return Err(format!("Job monitor cannot enumerate {}: {e}", dir.display()).into());
        }
    };
    for entry in entries {
        *count += 1;
        if *count > 10_000 {
            return Err("Job workspace file count exceeded".into());
        }
        let entry = entry?;
        // Windows DirEntry metadata comes from the directory enumeration. It
        // avoids reopening a browser temporary file after delete-pending begins.
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                return Err(
                    format!("Job monitor cannot inspect {}: {e}", entry.path().display()).into(),
                );
            }
        };
        if lifecycle::is_reparse(&metadata) {
            return Err("Job workspace symlink rejected".into());
        }
        add(
            &mut size,
            if metadata.is_dir() {
                usage(&entry.path(), depth + 1, count)?
            } else {
                metadata.len()
            },
        )?;
    }
    Ok(size)
}

pub fn run(root: &Path, manifest: &Path) -> Result<Value> {
    run_seeded(root, manifest, None)
}

/// Explicit continuation of a trusted native recipe using registered checkpoints.
pub fn resume(root: &Path, manifest: &Path, checkpoint: &Path) -> Result<Value> {
    let seed = resume::Seed::read(checkpoint)?;
    if seed.evidence_key() != "checkpoint_resume" {
        return Err("Checkpoint resume requires a resume descriptor".into());
    }
    run_seeded(root, manifest, Some(seed))
}

pub fn run_with_execution_window(root: &Path, manifest: &Path, policy: &Path) -> Result<Value> {
    run_seeded(
        root,
        manifest,
        Some(resume::Seed::read_execution_window(policy)?),
    )
}

fn run_seeded(root: &Path, manifest: &Path, seed: Option<resume::Seed>) -> Result<Value> {
    let bytes = fs::read(manifest)?;
    if bytes.len() > 256 * 1024 {
        return Err("Job manifest exceeds intake budget".into());
    }
    let job: Job = serde_json::from_value(load(manifest)?)?;
    validate(&job)?;
    let cfg = config(root)?;
    let index = selected(root)?;
    if let Some(seed) = &seed {
        seed.validate(&job, &bytes)?;
        let job_hash = format!("{:x}", Sha256::digest(&bytes));
        if index.get(&job_hash) != Some(&(bytes.len() as u64))
            || index.get(&seed.sha256) != Some(&seed.bytes)
            || seed
                .assets()
                .any(|asset| index.get(&asset.sha256) != Some(&asset.bytes))
        {
            return Err("Original job, checkpoint descriptor and checkpoint files must be exactly registered".into());
        }
    }
    for tool in job.installed_tools.values() {
        if index.get(&tool.sha256) != Some(&tool.bytes) {
            return Err("Installed tool must be exactly registered".into());
        }
    }
    for asset in job.inputs.values().chain(job.tools.values()) {
        if index.get(&asset.sha256) != Some(&asset.bytes) {
            return Err("Job input/tool must be exactly registered".into());
        }
    }
    // Reserve before resolving source paths: maintenance can then hold all
    // admitted readers, instead of racing a path captured before admission.
    let workspace_budget = seed
        .as_ref()
        .map_or(job.budget_bytes, |seed| seed.workspace_budget(&job));
    // A registered per-job window affects reservation and working space only.
    // Canonical object resolution/publication still uses the unchanged policy.
    let mut reservation_cfg = cfg.clone();
    if workspace_budget > job.budget_bytes {
        reservation_cfg.capacity.max_object_bytes = reservation_cfg
            .capacity
            .max_object_bytes
            .max(workspace_budget);
    }
    let reservation = admission::reserve(root, &reservation_cfg, workspace_budget)
        .map_err(|e| format!("Job not started; admission: {e}"))?;
    let mut sources = BTreeMap::new();
    let mut source_guards = Vec::new();
    if let Some(seed) = &seed {
        source_guards.extend(seed.pin_registered_sources(&cfg, bytes.len() as u64)?);
    }
    for tool in job.installed_tools.values() {
        source_guards.push(lifecycle::pin_read_only(&tool.path)?);
        verify(&tool.path, &tool.sha256, tool.bytes)?;
    }
    for (name, asset) in job.inputs.iter().chain(job.tools.iter()) {
        let mut found = None;
        for cache in &cfg.cache_roots {
            let path = object(cache, &asset.sha256);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    let guard = lifecycle::pin_read_only(&path)?;
                    verify(&path, &asset.sha256, asset.bytes)?;
                    source_guards.push(guard);
                    found = Some(path);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        sources.insert(
            name.clone(),
            found.ok_or("Named job input absent from active caches; no archive access")?,
        );
    }
    let workspace = reservation
        .cache_root
        .join("staging/caminos")
        .join(&reservation.id);
    lifecycle::prepare_staging(&reservation.cache_root)?;
    fs::create_dir(&workspace)?;
    lifecycle::checked_workspace(&cfg, &reservation.cache_root, &workspace)?;
    fs::write(workspace.join("job.json"), &bytes)?;
    let mut linked_input_bytes = 0u64;
    let mut copied_input_bytes = 0u64;
    let attempt = (|| -> Result<Value> {
        // File handles deny writes to immutable views while allowing concurrent readers
        // and removal of their temporary hard links. Canonical bytes are never writable.
        for (name, source) in &sources {
            let target = workspace.join(relative(name)?);
            fs::create_dir_all(target.parent().ok_or("Missing input parent")?)?;
            let asset = job
                .inputs
                .get(name)
                .or_else(|| job.tools.get(name))
                .unwrap();
            let writable = job.writable_inputs.contains(name);
            if !writable && fs::hard_link(source, &target).is_ok() {
                add(&mut linked_input_bytes, asset.bytes)?;
            } else {
                let mut input = File::open(source)?.take(asset.bytes.saturating_add(1));
                let mut output = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&target)?;
                std::io::copy(&mut input, &mut output)?;
                output.sync_all()?;
                drop(output);
                add(&mut copied_input_bytes, asset.bytes)?;
            }
            if !writable {
                source_guards.push(lifecycle::pin_read_only(&target)?);
            }
            verify(&target, &asset.sha256, asset.bytes)?;
        }
        for (name, text) in &job.files {
            let target = workspace.join(relative(name)?);
            fs::create_dir_all(target.parent().ok_or("Missing control parent")?)?;
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)?;
            f.write_all(text.as_bytes())?;
        }
        if job.prepare_output_parents {
            for output in job.outputs.values() {
                let target = workspace.join(relative(&output.path)?);
                fs::create_dir_all(target.parent().ok_or("Missing output parent")?)?;
            }
        }
        if let Some(seed) = &seed {
            seed.stage(&cfg, &workspace, &job)?;
        }
        for (n, step) in job
            .steps
            .iter()
            .enumerate()
            .skip(seed.as_ref().map_or(0, |s| s.start_step()))
        {
            let check = || -> Result<()> {
                if usage(&workspace, 0, &mut 0)? > workspace_budget {
                    return Err("Job exceeded declared workspace budget".into());
                }
                Ok(())
            };
            check()?;
            let executable = if let Some(tool) = job.installed_tools.get(&step.tool) {
                // Recheck immediately before execution in case installation changed.
                verify(&tool.path, &tool.sha256, tool.bytes)?;
                tool.path.clone()
            } else {
                workspace.join(relative(&step.tool)?)
            };
            process::run(
                &executable,
                &step.args,
                &workspace,
                &workspace.join(format!("step-{n}.log")),
                Duration::from_secs(seed.as_ref().map_or(step.timeout_seconds, |seed| {
                    seed.step_timeout_seconds(n, step.timeout_seconds)
                })),
                check,
            )
            .map_err(|e| format!("Job {} step {n}: {e}", reservation.id))?;
        }
        // Check every declared output before publishing any retained result.
        let mut results = BTreeMap::new();
        for (role, output) in &job.outputs {
            let path = workspace.join(relative(&output.path)?);
            let metadata = fs::symlink_metadata(&path)?;
            if !metadata.is_file()
                || metadata.file_type().is_symlink()
                || metadata.len() > output.max_bytes
            {
                return Err("Declared job output missing, unsafe or over budget".into());
            }
            let mut file = File::open(&path)?.take(output.max_bytes.saturating_add(1));
            let mut hasher = Sha256::new();
            let mut buffer = [0; 128 * 1024];
            loop {
                let n = file.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                hasher.update(&buffer[..n]);
            }
            let oid = format!("{:x}", hasher.finalize());
            verify(&path, &oid, metadata.len())?;
            let existing = active_existing(&cfg, &oid, metadata.len())?;
            results.insert(role.clone(), (path, oid, metadata.len(), existing));
        }
        let mut retained = BTreeMap::new();
        for (role, (path, oid, size, existing)) in results {
            let reused = existing.is_some();
            let destination = match existing {
                Some(path) => path,
                None => recover_job_output(root, &path, &reservation.cache_root, &oid, size)?,
            };
            retained.insert(role,json!({"sha256":oid,"bytes":size,"cache_uri":format!("cache://sha256/{oid}"),"path":destination,"reused_active_object":reused}));
        }
        let mut result = json!({"schema":"caminos.production-job-result.v1","job_id":reservation.id,
        "manifest_sha256":format!("{:x}",Sha256::digest(&bytes)),"cache_root":reservation.cache_root,
        "volume_id":reservation.volume_id,"budget_bytes":workspace_budget,"workspace":workspace,
        "outputs":retained,"linked_input_bytes":linked_input_bytes,"copied_input_bytes":copied_input_bytes,"daemon_started":false,"original_inputs_modified":false,
        "workspace_retained":true,"creative_authority":"not-granted-by-this-receipt"});
        if let Some(seed) = &seed {
            result[seed.evidence_key()] = seed.evidence();
            if workspace_budget != job.budget_bytes {
                result["original_budget_bytes"] = json!(job.budget_bytes);
            }
        }
        fs::write(
            workspace.join("result.json"),
            serde_json::to_vec_pretty(&result)?,
        )?;
        if !job.retain_workspace {
            let evidence = lifecycle::publish_and_retire(
                root,
                &cfg,
                &reservation.cache_root,
                &workspace,
                &bytes,
                &result,
                &job,
            )?;
            result["workspace_retained"] = json!(false);
            result["retirement_evidence"] = evidence;
        }
        Ok(result)
    })();
    match attempt {
        Ok(result) => Ok(result),
        Err(error) => {
            if job.retain_workspace {
                return Err(error);
            }
            let mut failure = json!({"schema":"reel.failed-job-result.v1","job_id":reservation.id,
                "manifest_sha256":format!("{:x}",Sha256::digest(&bytes)),"error":error.to_string(),"outputs":{},
                "creative_authority":"not-granted-by-this-receipt"});
            if let Some(seed) = &seed {
                failure[seed.evidence_key()] = seed.evidence();
            }
            match lifecycle::publish_and_retire(
                root,
                &cfg,
                &reservation.cache_root,
                &workspace,
                &bytes,
                &failure,
                &job,
            ) {
                Ok(evidence) => Err(format!(
                    "{error}; staging retired; diagnostic {}",
                    evidence["cache_uri"]
                )
                .into()),
                Err(retention) => Err(format!(
                    "{error}; staging preserved because retirement verification failed: {retention}"
                )
                .into()),
            }
        }
    }
}

/// Discard only a named owned job workspace after proving retained originals.
/// No objects/sha256, shared LFS path or old worktree is ever removed here.
pub fn discard(root: &Path, id: &str, manifest_oid: &str, incomplete: bool) -> Result<Value> {
    if !id.starts_with("job-")
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || !valid_oid(manifest_oid)
    {
        return Err("Exact job ID and registered manifest SHA-256 required".into());
    }
    let cfg = config(root)?;
    let control = admission::control(root, &cfg)?;
    {
        let _lock = admission::lock(&control)?;
        if control
            .join("admissions")
            .join(format!("{id}.json"))
            .exists()
            || control
                .join("admissions")
                .join(format!("{id}.lease"))
                .exists()
        {
            return Err(
                "Job reservation remains recorded; do not discard live or unreconciled scratch"
                    .into(),
            );
        }
    }
    let mut candidates = BTreeSet::new();
    for cache in &cfg.cache_roots {
        let workspace = cache.join("staging/caminos").join(id);
        match fs::symlink_metadata(&workspace) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("Job workspace cannot be a link".into());
                }
                let workspace = workspace.canonicalize()?;
                let cache = cache.canonicalize()?;
                let staging = cache.join("staging/caminos").canonicalize()?;
                if !workspace.starts_with(&cache)
                    || workspace.parent() != Some(staging.as_path())
                    || workspace.file_name().is_none_or(|n| n != id)
                {
                    return Err(
                        "Resolved job workspace escaped its named cache staging directory".into(),
                    );
                }
                candidates.insert(workspace);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    if candidates.len() != 1 {
        return Err("Named job scratch is absent or ambiguous across active tiers".into());
    }
    let workspace = candidates.into_iter().next().unwrap();
    let index = selected(root)?;
    let manifest_size = *index
        .get(manifest_oid)
        .ok_or("Job manifest must be a registered retained asset")?;
    verify(&workspace.join("job.json"), manifest_oid, manifest_size)?;
    let job: Job = serde_json::from_value(load(&workspace.join("job.json"))?)?;
    validate(&job)?;
    let mut required: BTreeMap<String, u64> = job
        .inputs
        .values()
        .chain(job.tools.values())
        .map(|a| (a.sha256.clone(), a.bytes))
        .collect();
    for tool in job.installed_tools.values() {
        required.insert(tool.sha256.clone(), tool.bytes);
    }
    let result_path = workspace.join("result.json");
    let completed = result_path.exists();
    if completed {
        let result = load(&result_path)?;
        if result["job_id"] != id || result["manifest_sha256"] != manifest_oid {
            return Err("Job result identity mismatch".into());
        }
        let outputs = result["outputs"]
            .as_object()
            .ok_or("Missing retained job outputs")?;
        if outputs.keys().collect::<BTreeSet<_>>() != job.outputs.keys().collect::<BTreeSet<_>>() {
            return Err("Job result output set mismatch".into());
        }
        for asset in outputs.values() {
            let oid = asset["sha256"]
                .as_str()
                .ok_or("Missing retained output hash")?;
            let size = asset["bytes"]
                .as_u64()
                .ok_or("Missing retained output size")?;
            if index.get(oid) != Some(&size) {
                return Err("Output must be registered before scratch is discarded".into());
            }
            required.insert(oid.to_owned(), size);
        }
    } else if !incomplete {
        return Err("Incomplete job requires explicit --discard-incomplete; only temporary results will be discarded".into());
    }
    required.insert(manifest_oid.to_owned(), manifest_size);
    for (oid, size) in &required {
        let mut found = false;
        for cache in &cfg.cache_roots {
            let path = object(cache, oid);
            match fs::symlink_metadata(&path) {
                Ok(_) => {
                    verify(&path, oid, *size)?;
                    found = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        if !found {
            return Err(
                "Retained job input/output absent from active caches; scratch preserved".into(),
            );
        }
    }
    let bytes = usage(&workspace, 0, &mut 0)?; // Reject links anywhere in the owned tree.
    // The absolute canonical target was checked against this exact cache and
    // staging parent above. Job IDs are never reused. No shared object deletion.
    fs::remove_dir_all(&workspace)?;
    Ok(
        json!({"job_id":id,"discarded_workspace":workspace,"temporary_bytes_removed":bytes,
        "retained_assets_verified":required.len(),"cache_objects_removed":0,"incomplete_scratch_discarded":!completed && incomplete}),
    )
}

mod lifecycle;
mod resume;

#[cfg(all(test, windows))]
#[path = "jobs/tests.rs"]
mod lifecycle_tests;

#[cfg(windows)]
mod process;

#[cfg(not(windows))]
mod process {
    use super::*;
    pub fn run(
        _: &Path,
        _: &[String],
        _: &Path,
        _: &Path,
        _: Duration,
        _: impl Fn() -> Result<()>,
    ) -> Result<()> {
        Err("Bounded production process jobs currently require Windows Job Objects".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workspace_monitor_preserves_counts_and_rejects_persistent_errors() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("input.bin"), [0u8; 23]).unwrap();
        fs::create_dir(root.path().join("nested")).unwrap();
        fs::write(root.path().join("nested/output.bin"), [0u8; 31]).unwrap();
        let mut count = 0;
        assert_eq!(usage(root.path(), 0, &mut count).unwrap(), 54);
        assert_eq!(count, 3);
        assert!(
            read_workspace_directory(|| Err(std::io::Error::other("permanent read failure")))
                .is_err()
        );
        assert!(
            read_workspace_directory(|| fs::read_dir(root.path().join("removed")))
                .unwrap()
                .is_none()
        );
    }

    #[cfg(windows)]
    #[test]
    fn delete_pending_directory_is_retried_but_permanent_access_denied_still_fails() {
        for code in [5, 32] {
            let root = tempfile::tempdir().unwrap();
            let mut calls = 0;
            let entries = read_workspace_directory(|| {
                calls += 1;
                if calls < 3 {
                    return Err(std::io::Error::from_raw_os_error(code));
                }
                fs::read_dir(root.path())
            })
            .unwrap();
            assert!(entries.is_some());
            assert_eq!(calls, 3);
            let mut calls = 0;
            let error = read_workspace_directory(|| {
                calls += 1;
                Err(std::io::Error::from_raw_os_error(code))
            })
            .unwrap_err();
            assert_eq!(error.raw_os_error(), Some(code));
            assert_eq!(calls, 4);
        }
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("temporary");
        fs::create_dir(&child).unwrap();
        let mut calls = 0;
        assert!(
            read_workspace_directory(|| {
                calls += 1;
                if calls == 1 {
                    fs::remove_dir(&child).unwrap();
                    return Err(std::io::Error::from_raw_os_error(5));
                }
                fs::read_dir(&child)
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(calls, 2);
    }
    #[test]
    fn workspace_paths_cannot_escape_or_alias_devices() {
        for path in [
            "../x", "/x", "C:/x", "a\\b", "NUL.txt", "a/../b", "a//b", "COM1", "a.",
        ] {
            assert!(relative(path).is_err(), "{path}");
        }
        assert!(relative("inputs/source.mp4").is_ok());
    }
    #[test]
    fn budgets_include_atomic_publication_and_reject_input_output_aliases() {
        let mut job: Job = serde_json::from_value(json!({"budget_bytes":100,"scratch_bytes":50,
            "inputs":{},"tools":{"tool.exe":{"sha256":"a".repeat(64),"bytes":10}},
            "steps":[{"tool":"tool.exe","args":[],"timeout_seconds":10}],
            "outputs":{"movie":{"path":"out.mp4","max_bytes":21}}}))
        .unwrap();
        assert!(validate(&job).is_err());
        job.budget_bytes = 200;
        assert!(validate(&job).is_ok());
        job.outputs.get_mut("movie").unwrap().path = "tool.exe".into();
        assert!(validate(&job).is_err());
    }

    #[test]
    fn installed_tools_require_absolute_paths_and_cannot_alias_inputs() {
        let mut job: Job = serde_json::from_value(json!({"budget_bytes":100,"scratch_bytes":0,
            "inputs":{},"tools":{},"installed_tools":{"python":{"path":"relative.exe","sha256":"a".repeat(64),"bytes":10}},
            "steps":[{"tool":"python","args":[],"timeout_seconds":10}],
            "outputs":{"report":{"path":"report.json","max_bytes":20}}})).unwrap();
        assert!(validate(&job).is_err());
        job.installed_tools.get_mut("python").unwrap().path = std::env::current_exe().unwrap();
        assert!(validate(&job).is_ok()); // installed bytes aren't copied into scratch
        job.inputs.insert(
            "PYTHON".into(),
            Asset {
                sha256: "b".repeat(64),
                bytes: 1,
            },
        );
        assert!(validate(&job).is_err());
    }
}
