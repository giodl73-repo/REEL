//! Named, advisory custody preflight. Success is not an executable receipt.
use super::*;
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    schema: String,
    policy: PathBuf,
    census: PathBuf,
    protection: Request,
    // A Vec intentionally preserves duplicate detection before set conversion.
    sha256s: Vec<String>,
    max_verify_bytes: u64,
    max_seconds: u64,
}

/// Verify only named native demotion/archive proposals, under fresh positive
/// protection. No inventory scan, fallback source search, copy or removal.
pub fn run(root: &Path, path: &Path, now: u64) -> Result<Value> {
    run_inner(root, path, now, || Ok(()))
}

fn run_inner(
    root: &Path,
    path: &Path,
    now: u64,
    after_payload: impl FnOnce() -> Result<()>,
) -> Result<Value> {
    let started = Instant::now();
    let path = command::input(root, path)?;
    let raw = read_limited(File::open(&path)?, CONTROL_BYTES)?;
    let job: Job = serde_json::from_slice(&raw)?;
    let subjects: BTreeSet<_> = job.sha256s.iter().collect();
    if job.schema != "reel.collector-verify-job.v1"
        || subjects.is_empty()
        || subjects.len() > 512
        || subjects.len() != job.sha256s.len()
        || subjects.iter().any(|h| !valid_oid(h))
        || job.max_verify_bytes == 0
        || job.max_seconds == 0
        || job.max_seconds > 3600
    {
        return Err("Invalid collector verification scope/budget".into());
    }
    let deadline = started
        .checked_add(Duration::from_secs(job.max_seconds))
        .ok_or("Invalid verification deadline")?;
    let policy_path = command::input(root, &job.policy)?;
    let census = command::input(root, &job.census)?;
    let policy_raw = read_limited(File::open(&policy_path)?, 4 * CONTROL_BYTES)?;
    let policy: tiering::Policy = serde_json::from_slice(&policy_raw)?;
    let config = crate::config(root)?;
    policy.validate(&config)?;
    if subjects.len() > policy.max_batch_objects {
        return Err("Collector verification exceeds policy batch objects".into());
    }
    let comparison = compare(root, &policy_path, &census, &job.protection, now)?;
    // Capture again before payload I/O: a new hold after comparison still wins.
    let protection = capture(&job.protection)?;
    let source_root =
        checked_directory(&config.cache_roots[policy.tier(&policy.selected_tier).index])?;
    if !job
        .protection
        .caches
        .iter()
        .any(|s| s.cache.canonicalize().ok().as_ref() == Some(&source_root))
    {
        return Err("Verification source cache missing from collector scope".into());
    }
    let mut pairs = Vec::new();
    let mut verify_bytes = 0u64;
    let mut same_volume_pairs = 0usize;
    // Validate the entire named batch and its budget before opening any payload.
    for subject in subjects {
        if matches!(protection.classify(subject)?, Membership::Held(_)) {
            return Err("Verification candidate has combined protection".into());
        }
        let row = comparison.shadow()["objects"]
            .as_array()
            .ok_or("Missing shadow rows")?
            .iter()
            .find(|r| r["sha256"].as_str() == Some(subject))
            .ok_or("Verification candidate absent from native plan")?;
        let target = match row["proposal"].as_str() {
            Some("verify-and-demote") => &policy.history_tier,
            Some("verify-and-archive-output" | "register-and-archive-first") => &policy.cold_tier,
            _ => return Err("Verification candidate is held or not a removal proposal".into()),
        };
        if row["target_tier"].as_str() != Some(target.as_str()) {
            return Err("Native proposal target mismatch".into());
        }
        let tier = policy.tier(target);
        let retained_root = checked_directory(if tier.storage == "active" {
            &config.cache_roots[tier.index]
        } else {
            &config.archive_roots[tier.index]
        })?;
        let same_volume = crate::admission::volume_id(&source_root)?
            == crate::admission::volume_id(&retained_root)?;
        if same_volume && !policy.allow_same_volume_copies {
            return Err("Verification copies share a volume".into());
        }
        same_volume_pairs += usize::from(same_volume);
        let bytes = row["bytes"].as_u64().ok_or("Missing candidate size")?;
        if bytes > config.capacity.max_object_bytes {
            return Err("Verification object exceeds configured limit".into());
        }
        verify_bytes = verify_bytes
            .checked_add(bytes.checked_mul(2).ok_or("Verification size overflow")?)
            .ok_or("Verification batch overflow")?;
        if verify_bytes > job.max_verify_bytes || verify_bytes / 2 > policy.max_batch_bytes {
            return Err("Collector verification byte budget exceeded before payload I/O".into());
        }
        let source = crate::object(&source_root, subject);
        let retained = crate::object(&retained_root, subject);
        checked_directory(source.parent().ok_or("Missing source parent")?)?;
        checked_directory(retained.parent().ok_or("Missing retained parent")?)?;
        plain(&source, false)?;
        plain(&retained, false)?;
        pairs.push((source, retained, subject.clone(), bytes));
    }
    for (source, retained, subject, bytes) in &pairs {
        crate::demotion::verify_pair(source, retained, subject, *bytes, deadline)?;
    }
    after_payload()?;
    protection.revalidate()?;
    let fresh = compare(root, &policy_path, &census, &job.protection, now)?;
    if comparison.baseline() != fresh.baseline()
        || comparison.shadow() != fresh.shadow()
        || raw != read_limited(File::open(&path)?, CONTROL_BYTES)?
        || policy_raw != read_limited(File::open(&policy_path)?, 4 * CONTROL_BYTES)?
    {
        return Err("Collector verification plan or controls changed".into());
    }
    let fresh_config = crate::config(root)?;
    if fresh_config.cache_roots != config.cache_roots
        || fresh_config.archive_roots != config.archive_roots
        || started.elapsed() >= Duration::from_secs(job.max_seconds)
    {
        return Err("Collector verification bindings changed or deadline exceeded".into());
    }
    Ok(json!({
        "schema":"reel.collector-verify-summary.v1",
        "verified_pairs":pairs.len(), "verified_payload_bytes":verify_bytes,
        "same_volume_pairs":same_volume_pairs,
        "different_volume_pairs":pairs.len()-same_volume_pairs,
        "dry_run":true,"declared_scope_only":true,
        "removal_authorized":false,"reusable_custody_proof":false,
        "payload_bytes_copied":0,"files_removed":0
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_and_new_demand_after_hashing_prevent_success() {
        // Exercise the final boundary through the private runner, without
        // timing-dependent sleeps or a large synthetic payload.
        for change in 0..4 {
            let (mut fixture, request) = super::super::tests::setup();
            let subject = fixture.put(b"old input", true);
            let version = fixture.version("old", &subject);
            fixture.policy["history_versions"] = json!([version]);
            fixture.policy["allow_same_volume_copies"] = json!(true);
            fixture.baseline();
            let root = fixture.root();
            let policy = root.join("policy.json");
            fs::write(&policy, serde_json::to_vec(&fixture.policy).unwrap()).unwrap();
            let job = root.join("verify.json");
            let scope = &request.caches[0];
            fs::write(
                &job,
                serde_json::to_vec(&json!({
                    "schema":"reel.collector-verify-job.v1", "policy":policy,
                    "census":root.join("baseline.json"),
                    "protection":{"schema":"reel.collector-protection-scope.v1",
                        "caches":[{"cache":scope.cache,"staging_parent":scope.staging_parent,
                            "retained_parent":scope.retained_parent,"catalogs":[]}],
                        "consumers":[{"root":root.canonicalize().unwrap(),"policy":policy}]},
                    "sha256s":[subject],"max_verify_bytes":1024,"max_seconds":60
                }))
                .unwrap(),
            )
            .unwrap();
            let mut reached = false;
            let result = run_inner(root, &job, 1000, || {
                reached = true;
                match change {
                    0 => {
                        let mut writer = FileStore::begin(
                            &scope.cache,
                            &scope.staging_parent,
                            "new-intent",
                            super::super::super::Limits::default(),
                        )?;
                        writer.put(b"old input")?;
                    }
                    1 => {
                        let mut changed = fixture.policy.clone();
                        changed["max_batch_objects"] = json!(1);
                        fs::write(&policy, serde_json::to_vec(&changed)?)?;
                    }
                    2 => fs::write(&job, b"{}")?,
                    _ => {
                        let storage = root.join(".local/storage-rust.json");
                        let mut value: Value = serde_json::from_slice(&fs::read(&storage)?)?;
                        value["cache_roots"].as_array_mut().unwrap().swap(0, 1);
                        fs::write(storage, serde_json::to_vec(&value)?)?;
                    }
                }
                Ok(())
            });
            assert!(reached, "preflight failed before boundary: {result:?}");
            assert!(result.is_err(), "changed control/demand accepted: {change}");
            assert!(crate::object(&scope.cache, &subject).exists());
        }
    }
}
