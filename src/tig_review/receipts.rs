//! Exclusive, immutable scope receipts in an explicitly enrolled existing root.
//! A pending receipt is never automatic permission to repeat image observations.
use super::*;
use reel_assets::admission;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const META_MAX: usize = 65_536;
const REPORT_MAX: usize = 4 * 1024 * 1024;

fn reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
pub(super) fn directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        path.is_absolute() && meta.is_dir() && !reparse(&meta),
        "review control must be an existing absolute ordinary directory"
    );
    Ok(())
}
pub(super) fn read(path: &Path, max: usize) -> Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    ensure!(
        meta.is_file() && !reparse(&meta),
        "receipt must be an ordinary file"
    );
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "receipt exceeds bound");
    Ok(bytes)
}
fn parse(path: &Path, max: usize) -> Result<(Value, Object)> {
    let bytes = read(path, max)?;
    Ok((parse_json(&bytes)?, Object::from_bytes(&bytes)))
}
pub(super) fn exclusive(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("receipt parent absent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file_mut().sync_all()?;
    temp.persist_noclobber(path)
        .map_err(|_| anyhow!("exclusive receipt already exists or cannot be saved"))?;
    ensure!(read(path, REPORT_MAX)? == bytes, "receipt readback differs");
    Ok(())
}
fn scope(root: &Path, task: &Object) -> Result<PathBuf> {
    directory(root)?;
    let dir = root.join(task.sha256.as_str());
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    directory(&dir)?;
    Ok(dir)
}

#[derive(Debug)]
pub struct ReservedScope {
    pub receipt: Object,
    pub directory: PathBuf,
}

/// Reserve after all source/proof/header checks, before any image views. Duplicate
/// claims refuse even for the same owner; use explicit recovery with the exact
/// original receipt. Changing the policy or method cannot create a second claim.
pub fn reserve(
    root: &Path,
    task: &Object,
    method: &Object,
    policy: &Object,
    owner: &str,
) -> Result<ReservedScope> {
    ensure!(
        !owner.trim().is_empty() && owner.len() <= 256,
        "invalid receipt owner"
    );
    let dir = scope(root, task)?;
    let _guard = admission::lock(&dir).map_err(|e| anyhow!("review metadata lock refused: {e}"))?;
    ensure!(
        !dir.join("report.json").try_exists()? && !dir.join("complete.json").try_exists()?,
        "orphan review state requires explicit recovery"
    );
    let receipt = json!({"schema":"reel.review-scope-receipt.experimental-1","task":task,"method":method,"policy":policy,"owner":owner,"image_observations_recorded":false,"human_approval":false});
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    exclusive(&dir.join("receipt.json"), &bytes)?;
    Ok(ReservedScope {
        receipt: Object::from_bytes(&bytes),
        directory: dir,
    })
}

fn checked_receipt(root: &Path, task: &Object, expected: &Object) -> Result<PathBuf> {
    directory(root)?;
    let dir = root.join(task.sha256.as_str());
    directory(&dir)?;
    let (receipt, actual) = parse(&dir.join("receipt.json"), META_MAX)?;
    ensure!(
        &actual == expected
            && pin(at(&receipt, "task")?)? == *task
            && text(&receipt, "schema")? == "reel.review-scope-receipt.experimental-1",
        "receipt identity differs"
    );
    ensure!(
        !boolean(&receipt, "human_approval")? && !boolean(&receipt, "image_observations_recorded")?,
        "invalid reservation state"
    );
    Ok(dir)
}

/// Bind recovery/submission to the exact method and policy in the reservation.
pub fn verify_binding(
    root: &Path,
    task: &Object,
    method: &Object,
    policy: &Object,
    receipt: &Object,
) -> Result<()> {
    let dir = checked_receipt(root, task, receipt)?;
    let (value, _) = parse(&dir.join("receipt.json"), META_MAX)?;
    ensure!(
        pin(at(&value, "method")?)? == *method && pin(at(&value, "policy")?)? == *policy,
        "receipt method/policy differs"
    );
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Recovery {
    Pending,
    Completed { report: Object },
}

/// Inspect an existing claim. Pending stays pending; this function does not grant
/// another view. An orphan saved report is validated and completed without views.
pub fn recover(
    root: &Path,
    task: &Object,
    receipt: &Object,
    validate: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<Recovery> {
    let dir = checked_receipt(root, task, receipt)?;
    let report_path = dir.join("report.json");
    match fs::symlink_metadata(&report_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !dir.join("complete.json").try_exists()?,
                "completion exists without report"
            );
            return Ok(Recovery::Pending);
        }
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let (report, _) = parse(&report_path, REPORT_MAX)?;
    complete(root, task, receipt, &report, validate)?;
    let (_, object) = parse(&report_path, REPORT_MAX)?;
    Ok(Recovery::Completed { report: object })
}

/// Save an actually observed report once. Interrupted completion can recover the
/// already saved exact report; a different outcome is never allowed to replace it.
pub fn complete(
    root: &Path,
    task: &Object,
    receipt: &Object,
    report: &Value,
    validate: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<Object> {
    let dir = checked_receipt(root, task, receipt)?;
    validate(report)?; // byte/source validation outside the short metadata lock
    let bytes = serde_json::to_vec_pretty(report)?;
    ensure!(bytes.len() <= REPORT_MAX, "report exceeds bound");
    let object = Object::from_bytes(&bytes);
    let _guard = admission::lock(&dir).map_err(|e| anyhow!("review metadata lock refused: {e}"))?;
    checked_receipt(root, task, receipt)?;
    let report_path = dir.join("report.json");
    if report_path.try_exists()? {
        let (_, existing) = parse(&report_path, REPORT_MAX)?;
        ensure!(
            existing == object,
            "existing report differs; outcome rewrite refused"
        );
    } else {
        exclusive(&report_path, &bytes)?;
    }
    let done = json!({"schema":"reel.review-scope-completion.experimental-1","task":task,"receipt":receipt,"report":object,"human_approval":false});
    let done_bytes = serde_json::to_vec_pretty(&done)?;
    let done_path = dir.join("complete.json");
    if done_path.try_exists()? {
        let (existing, _) = parse(&done_path, META_MAX)?;
        ensure!(existing == done, "existing completion differs");
    } else {
        exclusive(&done_path, &done_bytes)?;
    }
    Ok(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duplicate_claim_and_changed_policy_refuse_and_pending_does_not_reclaim() {
        let dir = tempfile::tempdir().unwrap();
        let task = Object::from_bytes(b"task");
        let method = Object::from_bytes(b"method");
        let policy = Object::from_bytes(b"policy");
        let reserved = reserve(dir.path(), &task, &method, &policy, "worker").unwrap();
        assert!(reserve(dir.path(), &task, &method, &policy, "worker").is_err());
        assert!(
            reserve(
                dir.path(),
                &task,
                &method,
                &Object::from_bytes(b"different policy"),
                "other"
            )
            .is_err()
        );
        assert_eq!(
            recover(dir.path(), &task, &reserved.receipt, &mut |_| Ok(())).unwrap(),
            Recovery::Pending
        );
        assert!(
            recover(
                dir.path(),
                &task,
                &Object::from_bytes(b"wrong receipt"),
                &mut |_| Ok(())
            )
            .is_err()
        );
    }
    #[test]
    fn orphan_report_recovers_without_outcome_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let task = Object::from_bytes(b"task");
        let p = Object::from_bytes(b"policy");
        let reserved = reserve(dir.path(), &task, &p, &p, "worker").unwrap();
        let report = json!({"qualified":false,"nullable":null,"holds":["pending"]});
        let report_bytes = serde_json::to_vec_pretty(&report).unwrap();
        exclusive(&reserved.directory.join("report.json"), &report_bytes).unwrap();
        let mut checks = 0;
        let recovered = recover(dir.path(), &task, &reserved.receipt, &mut |value| {
            checks += 1;
            ensure!(value == &report, "changed outcome");
            Ok(())
        })
        .unwrap();
        assert_eq!(checks, 1);
        assert_eq!(
            recovered,
            Recovery::Completed {
                report: Object::from_bytes(&report_bytes)
            }
        );
        let wrong = json!({"qualified":true});
        assert!(
            complete(
                dir.path(),
                &task,
                &reserved.receipt,
                &wrong,
                &mut |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(
            parse(&reserved.directory.join("report.json"), REPORT_MAX)
                .unwrap()
                .0,
            report
        );
    }
    #[test]
    fn concurrent_claims_have_one_winner() {
        let dir = tempfile::tempdir().unwrap();
        let task = Object::from_bytes(b"task");
        let p = Object::from_bytes(b"method");
        let results = std::thread::scope(|scope| {
            let a = scope.spawn(|| reserve(dir.path(), &task, &p, &p, "a"));
            let b = scope.spawn(|| reserve(dir.path(), &task, &p, &p, "b"));
            [a.join().unwrap().is_ok(), b.join().unwrap().is_ok()]
        });
        assert_eq!(results.into_iter().filter(|ok| *ok).count(), 1);
    }
}
