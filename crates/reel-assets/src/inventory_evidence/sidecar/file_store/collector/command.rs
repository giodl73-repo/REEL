//! Count-only operator boundary for an advisory, restrictive shadow comparison.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    schema: String,
    policy: PathBuf,
    census: PathBuf,
    protection: Request,
}
pub(super) fn input(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    };
    checked_directory(path.parent().ok_or("Missing collector input parent")?)?;
    plain(&path, false)?;
    Ok(path)
}
pub fn run(root: &Path, path: &Path, now: u64) -> Result<Value> {
    let path = input(root, path)?;
    let job: Job = serde_json::from_slice(&read_limited(File::open(path)?, CONTROL_BYTES)?)?;
    if job.schema != "reel.collector-shadow-job.v1" {
        return Err("Unsupported collector shadow job".into());
    }
    let report = compare(
        root,
        &input(root, &job.policy)?,
        &input(root, &job.census)?,
        &job.protection,
        now,
    )?;
    Ok(serde_json::to_value(report.summary())?)
}
