//! Optional native job demand publication, never a render-wide lock.
use super::*;
use crate::inventory_evidence::sidecar::file_store::collector::fence::{Binding, Mode};
use serde::Serialize;

pub(super) const LIMIT: u64 = 64 * 1024;
pub(super) const EVIDENCE_LIMIT: u64 = 2 * 1024 * 1024 + 64 * 1024;
pub(super) const REGISTRATION_LIMIT: u64 = 256 * 1024;
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
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Descriptor {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RetirementIntent {
    schema: String,
    job_id: String,
    manifest_sha256: String,
    pub manifest: Descriptor,
    pub evidence: Descriptor,
    pub outputs: BTreeMap<String, Descriptor>,
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
    if protection_inputs(raw)?.len() > 512 || job.outputs.keys().any(|key| key.len() > 200) {
        return Err("Enrolled jobs require bounded input/output scopes".into());
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
            "job-publication.json" | "job-output-intent.json" | "job-retirement-intent.json"
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
    if !job.retain_workspace {
        add(
            &mut required,
            (raw.len() as u64)
                .checked_mul(2)
                .ok_or("Request budget overflow")?,
        )?;
        add(
            &mut required,
            2 * EVIDENCE_LIMIT + REGISTRATION_LIMIT + LIMIT,
        )?;
    }
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
        schema: if decode(raw)?.retain_workspace {
            "reel.job-publication.v1"
        } else {
            "reel.job-publication.v2"
        }
        .into(),
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
    require_domain(workspace, raw, domain)?;
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
    let expected_schema = if job.retain_workspace {
        "reel.job-publication.v1"
    } else {
        "reel.job-publication.v2"
    };
    if control.schema != expected_schema
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

pub(super) fn retirement(
    workspace: &Path,
    id: &str,
    raw: &[u8],
    domain: &Domain,
    manifest: Descriptor,
    evidence: Descriptor,
    outputs: BTreeMap<String, Descriptor>,
) -> Result<()> {
    require_domain(workspace, raw, domain)?;
    let control = RetirementIntent {
        schema: "reel.job-retirement-intent.v1".into(),
        job_id: id.into(),
        manifest_sha256: format!("{:x}", Sha256::digest(raw)),
        manifest,
        evidence,
        outputs,
    };
    let bytes = serde_json::to_vec(&control)?;
    check_retirement(workspace, raw, &bytes)?;
    let subjects = control
        .outputs
        .values()
        .chain([&control.manifest, &control.evidence])
        .map(|d| d.sha256.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let _publication = domain.acquire(&subjects, Mode::Publish, Duration::ZERO)?;
    write(&workspace.join("job-retirement-intent.json"), &bytes)
}
fn require_domain(workspace: &Path, raw: &[u8], domain: &Domain) -> Result<()> {
    let cache = workspace
        .ancestors()
        .nth(3)
        .ok_or("Missing enrolled job cache")?
        .canonicalize()?;
    if crate::tiering::read_bounded(&workspace.join("job.json"), 256 * 1024)? != raw {
        return Err("Enrolled job manifest changed before publication".into());
    }
    let binding = check_enrollment(
        &cache,
        workspace,
        raw,
        &crate::tiering::read_bounded(&workspace.join("job-publication.json"), LIMIT)?,
    )?;
    if !domain.same_domain(&Domain::from_binding(&cache, &binding)?)? {
        return Err("Publication domain differs from enrolled job".into());
    }
    Ok(())
}
pub(crate) fn check_retirement(
    workspace: &Path,
    raw: &[u8],
    bytes: &[u8],
) -> Result<RetirementIntent> {
    let intent: RetirementIntent = serde_json::from_slice(bytes)?;
    let job = decode(raw)?;
    validate(&job)?;
    validate_enrolled(&job, raw)?;
    if job.retain_workspace
        || intent.schema != "reel.job-retirement-intent.v1"
        || Some(intent.job_id.as_str()) != workspace.file_name().and_then(|s| s.to_str())
        || intent.manifest_sha256 != format!("{:x}", Sha256::digest(raw))
        || intent.manifest.sha256 != intent.manifest_sha256
        || intent.manifest.bytes != raw.len() as u64
        || !valid_oid(&intent.evidence.sha256)
        || intent.evidence.bytes > EVIDENCE_LIMIT
        || intent.outputs.len() > job.outputs.len()
    {
        return Err("Retirement intent schema/manifest/scope mismatch".into());
    }
    for (role, descriptor) in &intent.outputs {
        if !valid_oid(&descriptor.sha256)
            || job
                .outputs
                .get(role)
                .is_none_or(|o| descriptor.bytes > o.max_bytes)
        {
            return Err("Retirement output intent exceeds declared scope".into());
        }
    }
    Ok(intent)
}
pub(super) fn retirement_assets(workspace: &Path, raw: &[u8]) -> Result<Vec<Descriptor>> {
    let path = workspace.join("job-retirement-intent.json");
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || lifecycle::is_reparse(&metadata) {
                return Err("Invalid retirement intent control".into());
            }
            let intent =
                check_retirement(workspace, raw, &crate::tiering::read_bounded(&path, LIMIT)?)?;
            Ok(intent
                .outputs
                .into_values()
                .chain([intent.manifest, intent.evidence])
                .collect())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
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
    let retiring = retirement_assets(workspace, manifest)?;
    if !retiring.is_empty() && domain.is_none() {
        return Err("Retirement intent requires enrolled job control".into());
    }
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
