//! Optional native retained-job demand publication, never a render-wide lock.
use super::*;
use crate::inventory_evidence::sidecar::file_store::collector::fence::{Binding, Mode};
use serde::Serialize;

pub(super) const LIMIT: u64 = 64 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    schema: String,
    job_id: String,
    manifest_sha256: String,
    cache: PathBuf,
    workspace: PathBuf,
    binding: Binding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Descriptor {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutputIntent {
    schema: String,
    job_id: String,
    manifest_sha256: String,
    outputs: BTreeMap<String, Descriptor>,
}
fn write(path: &Path, raw: &[u8]) -> Result<()> {
    if raw.len() as u64 > LIMIT {
        return Err("Enrolled job control exceeds byte budget".into());
    }
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(())
}
pub(super) fn validate_enrolled(job: &Job, raw: &[u8]) -> Result<()> {
    if !job.retain_workspace
        || protection_inputs(raw)?.len() > 512
        || job.outputs.keys().any(|key| key.len() > 200)
    {
        return Err(
            "Enrolled jobs require explicit retain_workspace and bounded input/output scopes"
                .into(),
        );
    }
    for name in job
        .inputs
        .keys()
        .chain(job.tools.keys())
        .chain(job.installed_tools.keys())
        .chain(job.files.keys())
        .map(String::as_str)
        .chain(job.outputs.values().map(|out| out.path.as_str()))
    {
        if matches!(
            name.split('/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase()
                .as_str(),
            "job-publication.json" | "job-output-intent.json"
        ) {
            return Err("Enrolled job paths overlap publication controls".into());
        }
    }
    let mut required = job.scratch_bytes;
    for asset in job.inputs.values().chain(job.tools.values()) {
        add(&mut required, asset.bytes)?;
    }
    for text in job.files.values() {
        add(&mut required, text.len() as u64)?;
    }
    for output in job.outputs.values() {
        add(
            &mut required,
            output
                .max_bytes
                .checked_mul(2)
                .ok_or("Output budget overflow")?,
        )?;
    }
    add(&mut required, raw.len() as u64)?;
    add(&mut required, 3 * LIMIT)?;
    if required > job.budget_bytes {
        return Err("Enrolled job budget must cover bounded publication controls".into());
    }
    Ok(())
}
pub(super) fn prepare(
    cfg: &crate::Config,
    reservation: &admission::Reservation,
    domain: &Domain,
    raw: &[u8],
) -> Result<PathBuf> {
    let cache = reservation.cache_root.canonicalize()?;
    let binding = domain.binding(&cache)?;
    let parent = cache.join("staging/caminos");
    binding.separate_from(&cache, &parent)?;
    let subjects: Vec<_> = protection_inputs(raw)?.into_iter().collect();
    let _publication = if subjects.is_empty() {
        None
    } else {
        Some(domain.acquire(&subjects, Mode::Publish, Duration::ZERO)?)
    };
    // The short hash fence covers compact intent publication, not source reads.
    lifecycle::prepare_staging(&reservation.cache_root)?;
    let workspace = reservation
        .cache_root
        .join("staging/caminos")
        .join(&reservation.id);
    fs::create_dir(&workspace)?;
    lifecycle::checked_workspace(cfg, &reservation.cache_root, &workspace)?;
    let control = Enrollment {
        schema: "reel.job-publication.v1".into(),
        job_id: reservation.id.clone(),
        manifest_sha256: format!("{:x}", Sha256::digest(raw)),
        cache,
        workspace: workspace.canonicalize()?,
        binding,
    };
    write(
        &workspace.join("job-publication.json"),
        &serde_json::to_vec(&control)?,
    )?;
    // Preserve exact intake bytes; a partial pair is incomplete, never empty.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(workspace.join("job.json"))?;
    file.write_all(raw)?;
    file.sync_all()?;
    Ok(workspace)
}
pub(super) fn outputs(
    workspace: &Path,
    id: &str,
    raw: &[u8],
    domain: &Domain,
    outputs: BTreeMap<String, Descriptor>,
) -> Result<()> {
    let subjects: Vec<_> = outputs
        .values()
        .map(|v| v.sha256.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let _publication = domain.acquire(&subjects, Mode::Publish, Duration::ZERO)?;
    let control = OutputIntent {
        schema: "reel.job-output-intent.v1".into(),
        job_id: id.into(),
        manifest_sha256: format!("{:x}", Sha256::digest(raw)),
        outputs,
    };
    write(
        &workspace.join("job-output-intent.json"),
        &serde_json::to_vec(&control)?,
    )
}
fn same_path(a: &Path, b: &Path) -> bool {
    if !a.is_absolute()
        || a.components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return false;
    }
    #[cfg(windows)]
    {
        let normalize = |p: &Path| {
            p.to_string_lossy()
                .replace('/', "\\")
                .trim_start_matches("\\\\?\\")
                .to_uppercase()
        };
        normalize(a) == normalize(b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}
pub(crate) fn check_enrollment(
    cache: &Path,
    workspace: &Path,
    manifest: &[u8],
    raw: &[u8],
) -> Result<Binding> {
    let control: Enrollment = serde_json::from_slice(raw)?;
    let id = workspace
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid enrolled workspace ID")?;
    let job = decode(manifest)?;
    validate(&job)?;
    validate_enrolled(&job, manifest)?;
    if control.schema != "reel.job-publication.v1"
        || control.job_id != id
        || control.manifest_sha256 != format!("{:x}", Sha256::digest(manifest))
        || !same_path(&control.cache, cache)
        || !same_path(&control.workspace, workspace)
    {
        return Err("Enrolled job control schema/manifest/scope mismatch".into());
    }
    control.binding.separate_from(
        cache,
        workspace
            .parent()
            .ok_or("Missing enrolled staging parent")?,
    )?;
    Ok(control.binding)
}
pub(crate) fn check_outputs(
    workspace: &Path,
    manifest: &[u8],
    raw: &[u8],
) -> Result<BTreeMap<String, Descriptor>> {
    let control: OutputIntent = serde_json::from_slice(raw)?;
    let job = decode(manifest)?;
    validate(&job)?;
    validate_enrolled(&job, manifest)?;
    if control.schema != "reel.job-output-intent.v1"
        || Some(control.job_id.as_str()) != workspace.file_name().and_then(|s| s.to_str())
        || control.manifest_sha256 != format!("{:x}", Sha256::digest(manifest))
        || control.outputs.len() != job.outputs.len()
    {
        return Err("Enrolled output intent schema/manifest/scope mismatch".into());
    }
    for (role, descriptor) in &control.outputs {
        if !valid_oid(&descriptor.sha256)
            || job
                .outputs
                .get(role)
                .is_none_or(|out| descriptor.bytes > out.max_bytes)
        {
            return Err("Enrolled output intent exceeds declared scope".into());
        }
    }
    Ok(control.outputs)
}

pub(super) fn discard_domain(
    cache: &Path,
    workspace: &Path,
    manifest: &[u8],
) -> Result<Option<Domain>> {
    let path = workspace.join("job-publication.json");
    let domain = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || lifecycle::is_reparse(&metadata) {
                return Err("Invalid enrolled job control".into());
            }
            let binding = check_enrollment(
                cache,
                workspace,
                manifest,
                &crate::tiering::read_bounded(&path, LIMIT)?,
            )?;
            Some(Domain::from_binding(cache, &binding)?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let intent_path = workspace.join("job-output-intent.json");
    let intended = match fs::symlink_metadata(&intent_path) {
        Ok(metadata) => {
            if domain.is_none() || !metadata.is_file() || lifecycle::is_reparse(&metadata) {
                return Err("Invalid enrolled output control".into());
            }
            Some(check_outputs(
                workspace,
                manifest,
                &crate::tiering::read_bounded(&intent_path, LIMIT)?,
            )?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    match fs::symlink_metadata(workspace.join("result.json")) {
        Ok(metadata) => {
            if lifecycle::is_reparse(&metadata) {
                return Err("Invalid job result control".into());
            }
            let result: Value = serde_json::from_slice(&crate::tiering::read_bounded(
                &workspace.join("result.json"),
                1024 * 1024,
            )?)?;
            if result["publication_enrolled"].as_bool().unwrap_or(false) != domain.is_some() {
                return Err("Job result enrollment control mismatch".into());
            }
            if domain.is_some()
                && intended.as_ref().is_none_or(|expected| {
                    result["outputs"].as_object().is_none_or(|actual| {
                        actual.len() != expected.len()
                            || expected.iter().any(|(role, value)| {
                                actual.get(role).is_none_or(|entry| {
                                    entry["sha256"] != value.sha256 || entry["bytes"] != value.bytes
                                })
                            })
                    })
                })
            {
                return Err("Job result does not match enrolled output intent".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    Ok(domain)
}
