//! Explicit one-command review from a portable project declaration and cached inventory.
use crate::{Result, accounting, active_existing, config, selected, tier_review, tiering};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inventory {
    sha256: String,
    bytes: u64,
    cache_uri: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    schema: String,
    policy: String,
    inventory: Option<Inventory>,
    active_branches: BTreeSet<String>,
    #[serde(default)]
    inactive_branches: BTreeSet<String>,
    #[serde(default)]
    legacy_branches: BTreeSet<String>,
    max_verify_seconds: u64,
}

fn consumers(root: &Path, p: &Project) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>)> {
    if !p.active_branches.is_disjoint(&p.inactive_branches)
        || !p.legacy_branches.is_subset(&p.active_branches)
        || p.active_branches.len() + p.inactive_branches.len() > 64
    {
        return Err("Invalid manager consumer declarations".into());
    }
    let q = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()?;
    if !q.status.success() || q.stdout.len() > 4 * 1024 * 1024 {
        return Err("Cannot read registered manager worktrees".into());
    }
    let mut active = vec![];
    let mut inactive = vec![];
    let mut legacy = vec![];
    for record in q
        .stdout
        .split(|b| *b == 0)
        .collect::<Vec<_>>()
        .split(|s| s.is_empty())
    {
        let fields: Vec<&str> = record
            .iter()
            .map(|s| std::str::from_utf8(s))
            .collect::<std::result::Result<_, _>>()?;
        let path = fields.iter().find_map(|s| s.strip_prefix("worktree "));
        let Some(path) = path else { continue };
        let branch = fields
            .iter()
            .find_map(|s| s.strip_prefix("branch refs/heads/"))
            .ok_or(
                "Detached/bare worktrees require an explicit ownership resolution before review",
            )?;
        let path = Path::new(path).canonicalize()?;
        if p.active_branches.contains(branch) {
            if p.legacy_branches.contains(branch) {
                legacy.push(path.clone());
            }
            active.push(path);
        } else if p.inactive_branches.contains(branch) {
            inactive.push(path);
        } else {
            return Err(format!(
                "Registered branch {branch} lacks an active/inactive manager declaration"
            )
            .into());
        }
    }
    active.sort();
    inactive.sort();
    legacy.sort();
    if active.len() > 32 || inactive.len() > 32 || !active.contains(&root.canonicalize()?) {
        return Err(
            "Manager scope must include the invoking worktree and fit consumer budgets".into(),
        );
    }
    Ok((active, inactive, legacy))
}

fn new_bytes(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}

pub fn run(root: &Path, project_name: &str, review_name: &str, now: u64) -> Result<Value> {
    run_with_refresh(root, project_name, review_name, now, false)
}

// A new census is an explicit operation, never a side effect of viewing a review.
fn snapshot(root: &Path, path: &Path) -> Result<Inventory> {
    let report = accounting::report_with_progress(root, |done, total| {
        eprintln!("Cache metadata: {done}/{total} objects")
    })?;
    let bytes = serde_json::to_vec(&report)?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err("Fresh inventory exceeds the 512 MiB input budget".into());
    }
    let sha256 = tiering::digest(&bytes);
    new_bytes(path, &bytes)?;
    Ok(Inventory {
        cache_uri: format!("cache://sha256/{sha256}"),
        sha256,
        bytes: bytes.len() as u64,
    })
}

pub fn run_with_refresh(
    root: &Path,
    project_name: &str,
    review_name: &str,
    now: u64,
    refresh: bool,
) -> Result<Value> {
    let started = std::time::Instant::now();
    if review_name.is_empty()
        || review_name.len() > 80
        || !review_name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("Review name must be 1-80 ASCII letters, digits, '-' or '_'".into());
    }
    let root = root.canonicalize()?;
    let project_file = accounting::relative(&root, project_name)?;
    let project_bytes = tiering::read_bounded(&project_file, 256 * 1024)?;
    let p: Project = serde_json::from_slice(&project_bytes)?;
    if p.schema != "reel.asset-manager-project.v1"
        || p.max_verify_seconds == 0
        || p.max_verify_seconds > 3600
        || p.inventory.as_ref().is_some_and(|i| {
            !crate::valid_oid(&i.sha256)
                || i.bytes > 512 * 1024 * 1024
                || i.cache_uri != format!("cache://sha256/{}", i.sha256)
        })
    {
        return Err("Invalid asset-manager project/inventory/budget".into());
    }
    let policy_file = accounting::relative(&root, &p.policy)?;
    let policy_bytes = tiering::read_bounded(&policy_file, 256 * 1024)?;
    let (active, inactive, legacy) = consumers(&root, &p)?;
    let cached = if refresh {
        None
    } else {
        let i = p.inventory.as_ref().ok_or(
            "No retained inventory configured; explicitly use --refresh-inventory for a new census",
        )?;
        if selected(&root)?.get(&i.sha256) != Some(&i.bytes) {
            return Err("Manager inventory asset must be registered with exact bytes".into());
        }
        Some(
            active_existing(&config(&root)?, &i.sha256, i.bytes)?
                .ok_or("Manager inventory absent from active caches; archive not probed")?,
        )
    };
    let parent = root.join(".local/asset-reviews");
    for path in [root.join(".local"), parent.clone()] {
        if let Ok(m) = fs::symlink_metadata(path) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if m.file_attributes() & 0x400 != 0 {
                    return Err("Review directory reparse point rejected".into());
                }
            }
            if !m.is_dir() || m.file_type().is_symlink() {
                return Err("Review directory link/non-directory rejected".into());
            }
        }
    }
    fs::create_dir_all(&parent)?;
    let dir = parent.join(format!("review-{review_name}"));
    fs::create_dir(&dir)?;
    let policy_path = dir.join("policy.json");
    let plan_path = dir.join("plan.json");
    let scope_path = dir.join("scope.json");
    let html_path = dir.join("review.html");
    let (inventory, identity) = if let Some(path) = cached {
        (path, p.inventory.as_ref().unwrap().sha256.clone())
    } else {
        let path = dir.join("inventory.json");
        let i = snapshot(&root, &path)?;
        let mut policy: tiering::Policy = serde_json::from_slice(&policy_bytes)?;
        policy.baseline = tiering::Descriptor {
            sha256: i.sha256.clone(),
            bytes: i.bytes,
        };
        new_bytes(&policy_path, &serde_json::to_vec(&policy)?)?;
        (path, i.sha256)
    };
    if !refresh {
        new_bytes(&policy_path, &policy_bytes)?;
    }
    let now = now
        .checked_add(started.elapsed().as_secs())
        .ok_or("Review time overflow")?;
    let plan = tiering::plan(&root, &policy_path, &inventory, now)?;
    tiering::write_plan(&plan_path, &plan)?;
    let scope = json!({"schema":"reel.asset-demotion-request.v1","policy":policy_path,"plan":plan_path,
        "plan_sha256":plan["plan_sha256"],"consumer_scope":"all-managed-cache-consumers",
        "consumer_roots":active,"inactive_consumer_roots":inactive,"legacy_consumer_roots":legacy,
        "sha256s":[],"max_verify_bytes":1,"max_seconds":p.max_verify_seconds,
        "source_controls":{project_name:tiering::digest(&project_bytes),p.policy:tiering::digest(&policy_bytes)}});
    tiering::write_plan(&scope_path, &scope)?;
    let view = tier_review::render(&root, &scope_path, &html_path, now)?;
    Ok(
        json!({"schema":"reel.asset-manager-review-result.v1","review":view,"scope_path":scope_path,
        "inventory_sha256":identity,"inventory_bytes":fs::metadata(&inventory)?.len(),
        "inventory_cache_uri":format!("cache://sha256/{identity}"),"inventory_path":inventory,"inventory_refreshed":refresh,
        "inventory_retention_required":refresh,"inventory_copied":false,"archive_probed":false,
        "daemon_started":false,"payloads_removed":0,"next":"Save a block choice in the private review, then use tier-request; apply remains separate"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (crate::tiering::tests::Fixture, String) {
        let (f, _, _) = crate::tier_review::tests::fixture();
        let q = Command::new("git")
            .arg("-C")
            .arg(f.root())
            .args(["init", "-b", "main"])
            .output()
            .unwrap();
        assert!(q.status.success());
        let q = Command::new("git")
            .arg("-C")
            .arg(f.root())
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "commit",
                "--allow-empty",
                "-m",
                "fixture",
            ])
            .output()
            .unwrap();
        assert!(q.status.success());
        let inventory = crate::ingest(f.root(), &f.root().join("baseline.json")).unwrap();
        let registration = f.root().join("registration.json");
        fs::write(&registration,json!({"assets":[{"sha256":inventory["sha256"],"bytes":inventory["bytes"],"provenance":"synthetic","rights":"synthetic","selection_state":"retained"}]}).to_string()).unwrap();
        crate::registry::register(f.root(), &registration).unwrap();
        let project = json!({"schema":"reel.asset-manager-project.v1","policy":"policy.json","inventory":{"sha256":inventory["sha256"],"bytes":inventory["bytes"],"cache_uri":inventory["cache_uri"]},"active_branches":["main"],"max_verify_seconds":60});
        fs::write(f.root().join("asset-manager.json"), project.to_string()).unwrap();
        (f, "asset-manager.json".into())
    }
    #[test]
    fn project_recreates_review_from_cache_without_setup_or_media_changes() {
        let (f, p) = fixture();
        let result = run(f.root(), &p, "first", 1000).unwrap();
        assert_eq!(result["payloads_removed"], 0);
        assert_eq!(result["inventory_copied"], false);
        let scope: Value = crate::load(Path::new(result["scope_path"].as_str().unwrap())).unwrap();
        assert_eq!(scope["sha256s"], json!([]));
        assert_eq!(scope["source_controls"].as_object().unwrap().len(), 2);
        assert!(run(f.root(), &p, "first", 1000).is_err());
        assert!(run(f.root(), &p, "../escape", 1000).is_err());
        assert!(!f.root().join("offline-cold").exists());
        fs::write(f.root().join("policy.json"), b"{}").unwrap();
        let req: crate::demotion::Request = serde_json::from_value(scope).unwrap();
        assert!(crate::demotion::check_source_controls(f.root(), &req, false).is_err());
    }
    #[test]
    fn undeclared_worktree_is_held_instead_of_silently_omitted() {
        let (f, p) = fixture();
        let other = f.root().join("new-lane");
        let q = Command::new("git")
            .arg("-C")
            .arg(f.root())
            .args(["worktree", "add", "-b", "undeclared"])
            .arg(&other)
            .output()
            .unwrap();
        assert!(q.status.success());
        assert!(run(f.root(), &p, "held", 1000).is_err());
        assert!(!f.root().join(".local/asset-reviews/review-held").exists());
    }

    #[test]
    fn explicit_snapshot_observes_additions_and_removals_without_touching_payloads() {
        let (f, _) = fixture();
        let removed = f.put(b"temporary unregistered fixture", false);
        let first = f.root().join("first-census.json");
        snapshot(f.root(), &first).unwrap();
        fs::remove_file(crate::object(&f.root().join("fast"), &removed)).unwrap();
        fs::remove_file(crate::object(&f.root().join("history"), &removed)).unwrap();
        let added = f.put(b"new unregistered fixture", false);
        let second = f.root().join("second-census.json");
        let identity = snapshot(f.root(), &second).unwrap();
        let bytes = fs::read(&second).unwrap();
        assert_eq!(identity.sha256, tiering::digest(&bytes));
        assert_eq!(identity.bytes, bytes.len() as u64);
        let report: Value = serde_json::from_slice(&bytes).unwrap();
        let hashes: BTreeSet<_> = report["objects"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["sha256"].as_str().unwrap())
            .collect();
        assert!(hashes.contains(added.as_str()));
        assert!(!hashes.contains(removed.as_str()));
        assert_eq!(report["archive_probed"], false);
        assert_eq!(report["media_deleted"], false);
        assert!(snapshot(f.root(), &second).is_err());
        assert!(crate::object(&f.root().join("fast"), &added).exists());
        assert!(!f.root().join("offline-cold").exists());
    }

    #[test]
    fn new_project_requires_explicit_refresh_and_rejects_same_volume_retention() {
        let (f, p) = fixture();
        let project_file = f.root().join(&p);
        let mut project = crate::load(&project_file).unwrap();
        project.as_object_mut().unwrap().remove("inventory");
        fs::write(&project_file, project.to_string()).unwrap();
        assert!(
            run(f.root(), &p, "no-refresh", 1000)
                .unwrap_err()
                .to_string()
                .contains("--refresh-inventory")
        );
        assert!(
            !f.root()
                .join(".local/asset-reviews/review-no-refresh")
                .exists()
        );
        let original_policy = fs::read(f.root().join("policy.json")).unwrap();
        let original_project = fs::read(&project_file).unwrap();
        let err = run_with_refresh(f.root(), &p, "fresh", 1000, true).unwrap_err();
        assert!(err.to_string().contains("one volume"), "{err}");
        let dir = f.root().join(".local/asset-reviews/review-fresh");
        assert!(dir.join("inventory.json").exists());
        assert!(!dir.join("review.html").exists());
        assert_eq!(
            fs::read(f.root().join("policy.json")).unwrap(),
            original_policy
        );
        assert_eq!(fs::read(&project_file).unwrap(), original_project);
        let copied: Value = crate::load(&dir.join("policy.json")).unwrap();
        assert_eq!(
            copied["baseline"]["sha256"],
            tiering::digest(&fs::read(dir.join("inventory.json")).unwrap())
        );
    }
}
