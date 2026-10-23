//! Native production workspace demand, without crawling scratch/media paths.
use super::*;
#[derive(Deserialize)]
struct Output {
    sha256: String,
    bytes: u64,
}
#[derive(Deserialize)]
struct Published {
    schema: String,
    job_id: String,
    manifest_sha256: String,
    cache_root: PathBuf,
    workspace: PathBuf,
    outputs: BTreeMap<String, Output>,
}
fn same_path(a: &Path, b: &Path) -> bool {
    if !a.is_absolute() || a.components().any(|c| matches!(c, Component::ParentDir)) {
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
pub(super) fn capture(
    reader: &mut Reader,
    scope: &CacheScope,
    holds: &mut BTreeMap<String, BTreeSet<Reason>>,
) -> Result<()> {
    let Some(parent) = &scope.job_parent else {
        return Ok(());
    };
    for workspace in reader.directory(parent, reader.limits.sessions)? {
        checked_directory(&workspace)?;
        let id = workspace
            .file_name()
            .unwrap()
            .to_str()
            .ok_or("Invalid job directory name")?;
        if !id.starts_with("job-")
            || id.len() > 128
            || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err("Unknown job workspace scope".into());
        }
        let raw = reader.file(&workspace.join("job.json"), false, 16 * CONTROL_BYTES)?;
        for subject in crate::jobs::protection_inputs(&raw)? {
            hold(
                holds,
                &subject,
                Reason::JobWorkspaceInput,
                reader.limits.holds,
            )?;
        }
        let result = workspace.join("result.json");
        match fs::symlink_metadata(&result) {
            Ok(_) => {
                let published: Published =
                    serde_json::from_slice(&reader.file(&result, false, 16 * CONTROL_BYTES)?)?;
                if published.schema != "caminos.production-job-result.v1"
                    || published.job_id != id
                    || published.manifest_sha256 != hash(&raw)
                    || !same_path(&published.cache_root, &scope.cache)
                    || !same_path(&published.workspace, &workspace)
                {
                    return Err("Job result schema/manifest/scope mismatch".into());
                }
                for output in published.outputs.values() {
                    let _ = output.bytes; // observation descriptor, not fresh custody
                    hold(
                        holds,
                        &output.sha256,
                        Reason::JobWorkspaceOutput,
                        reader.limits.holds,
                    )?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
