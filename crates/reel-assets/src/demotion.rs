//! Narrow redundant-copy removal. There is no last-copy retirement or archive access.
use crate::{Result, admission, config, object, tiering};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub policy: PathBuf,
    pub plan: PathBuf,
    pub plan_sha256: String,
    /// Explicit managed consumers, including the invoking root. Not global discovery.
    pub consumer_roots: Vec<PathBuf>,
    /// Explicit completed/quiescent worktrees, not silently omitted live lanes.
    #[serde(default)]
    pub inactive_consumer_roots: Vec<PathBuf>,
    pub consumer_scope: String,
    pub sha256s: BTreeSet<String>,
    pub max_verify_bytes: u64,
    pub max_seconds: u64,
    #[serde(default)]
    pub allow_same_volume_copies: bool,
}

#[derive(Serialize, Clone)]
struct Identity {
    id: String,
    links: u64,
    allocated: u64,
    bytes: u64,
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::*;
    pub fn open(path: &Path, remove: bool) -> Result<fs::File> {
        // Existing writers fail sharing checks. Only exact opened source links
        // get DELETE permission; destination allows neither writes nor deletion.
        let access = 0x8000_0000 | if remove { 0x0001_0000 } else { 0 };
        Ok(fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(FILE_SHARE_READ | if remove { FILE_SHARE_DELETE } else { 0 })
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?)
    }
    pub fn identity(file: &fs::File) -> Result<Identity> {
        let mut id: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
        let mut info: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
        unsafe {
            if GetFileInformationByHandle(file.as_raw_handle(), &mut id) == 0
                || GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileStandardInfo,
                    (&mut info as *mut FILE_STANDARD_INFO).cast(),
                    std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
                ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        if id.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
            || info.AllocationSize < 0
        {
            return Err("Nonregular/reparse file rejected".into());
        }
        Ok(Identity {
            id: format!(
                "{}:{}:{}",
                id.dwVolumeSerialNumber, id.nFileIndexHigh, id.nFileIndexLow
            ),
            links: u64::from(id.nNumberOfLinks),
            allocated: info.AllocationSize as u64,
            bytes: file.metadata()?.len(),
        })
    }
    pub fn guard_dirs(path: &Path) -> Result<Vec<fs::File>> {
        let mut out = Vec::new();
        let mut cursor = PathBuf::new();
        for part in path.parent().ok_or("Missing cache parent")?.components() {
            cursor.push(part.as_os_str());
            if matches!(part, std::path::Component::Prefix(_)) || !cursor.has_root() {
                continue;
            }
            if !cursor.is_dir() {
                return Err("Missing owned cache directory".into());
            }
            let file = fs::OpenOptions::new()
                .access_mode(FILE_READ_ATTRIBUTES)
                .share_mode(FILE_SHARE_READ) // Deny directory replacement/reparse mutation.
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(&cursor)?;
            let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err("Reparse ancestor rejected".into());
            }
            out.push(file);
        }
        Ok(out)
    }
    pub fn remove(file: fs::File) -> Result<()> {
        // Delete the exact opened link, never re-resolve a mutable path name.
        // POSIX semantics remove that directory entry when this handle closes.
        let info = FILE_DISPOSITION_INFO_EX {
            Flags: FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
        };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfoEx,
                (&info as *const FILE_DISPOSITION_INFO_EX).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO_EX>() as u32,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
        drop(file);
        Ok(())
    }
    pub fn assert_path(file: &fs::File, path: &Path) -> Result<()> {
        use std::os::windows::ffi::OsStringExt;
        let mut buf = vec![0u16; 32768];
        let len = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buf.as_mut_ptr(),
                buf.len() as u32,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        };
        if len == 0 || len as usize >= buf.len() {
            return Err("Cannot verify opened source path".into());
        }
        let actual = PathBuf::from(std::ffi::OsString::from_wide(&buf[..len as usize]));
        if actual.to_string_lossy().to_uppercase()
            != path.canonicalize()?.to_string_lossy().to_uppercase()
        {
            return Err("Opened source link was renamed/replaced; held before removal".into());
        }
        Ok(())
    }
}
#[cfg(not(windows))]
mod platform {
    use super::*;
    pub fn open(path: &Path, _remove: bool) -> Result<fs::File> {
        Ok(fs::File::open(path)?)
    }
    pub fn identity(file: &fs::File) -> Result<Identity> {
        use std::os::unix::fs::MetadataExt;
        let m = file.metadata()?;
        if !m.is_file() {
            return Err("Nonregular object".into());
        }
        Ok(Identity {
            id: format!("{}:{}", m.dev(), m.ino()),
            links: m.nlink(),
            allocated: m.blocks() * 512,
            bytes: m.len(),
        })
    }
    pub fn guard_dirs(_path: &Path) -> Result<Vec<fs::File>> {
        Ok(vec![])
    }
    pub fn remove(_file: fs::File) -> Result<()> {
        Err("Handle-safe apply is currently Windows-only; no path-based fallback".into())
    }
    pub fn assert_path(_file: &fs::File, _path: &Path) -> Result<()> {
        Err("Apply unsupported".into())
    }
}

fn verify(file: &mut fs::File, hash: &str, bytes: u64, deadline: std::time::Instant) -> Result<()> {
    use sha2::{Digest, Sha256};
    if file.metadata()?.len() != bytes {
        return Err("Fresh copy size mismatch".into());
    }
    file.seek(SeekFrom::Start(0))?;
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    loop {
        if std::time::Instant::now() >= deadline {
            return Err("Verification time budget exceeded".into());
        }
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        sha.update(&buffer[..n]);
    }
    if format!("{:x}", sha.finalize()) != hash {
        return Err("Fresh copy hash mismatch; nothing removed".into());
    }
    Ok(())
}

fn ensure_no_admissions(control: &Path) -> Result<()> {
    let dir = control.join("admissions");
    if dir.exists() && fs::read_dir(dir)?.next().is_some() {
        return Err("Live or unreconciled admissions present; redundant-copy removal held".into());
    }
    Ok(())
}

fn consumer_registration(
    root: &Path,
    active: &BTreeSet<PathBuf>,
    inactive: &BTreeSet<PathBuf>,
) -> Result<String> {
    if !root.join(".git").exists() {
        return Ok(tiering::digest(&serde_json::to_vec(&(active, inactive))?));
    }
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()?;
    if !out.status.success() || out.stdout.len() > 4 * 1024 * 1024 {
        return Err("Cannot reconcile registered Git consumers".into());
    }
    let text = std::str::from_utf8(&out.stdout)?;
    let mut registered = BTreeSet::new();
    for field in text.split('\0') {
        if let Some(path) = field.strip_prefix("worktree ") {
            registered.insert(Path::new(path).canonicalize()?);
        }
    }
    if registered.is_empty()
        || registered
            .iter()
            .any(|r| !active.contains(r) && !inactive.contains(r))
        || inactive
            .iter()
            .any(|r| !registered.contains(r) || active.contains(r))
    {
        return Err(
            "Registered worktree missing from active or explicit inactive consumer scope".into(),
        );
    }
    Ok(tiering::digest(&serde_json::to_vec(&registered)?))
}

fn metadata_snapshot(root: &Path, hold: bool) -> Result<(String, Vec<fs::File>)> {
    let p = crate::accounting::project(root)?;
    let mut paths = BTreeSet::from([
        PathBuf::from("asset-project.json"),
        PathBuf::from("registry/versions.json"),
        PathBuf::from("registry/object-index.json"),
        PathBuf::from(&p.storage_config),
    ]);
    for rule in p.selections {
        let index = crate::accounting::relative(root, &rule.index)?;
        paths.insert(PathBuf::from(&rule.index));
        let doc = crate::load(&index)?;
        for row in doc
            .pointer(&rule.entries)
            .and_then(Value::as_array)
            .ok_or("Selection index invalid")?
        {
            for key in [&rule.record_field, "scene", "bindings"] {
                if let Some(s) = row.get(key).and_then(Value::as_str) {
                    paths.insert(PathBuf::from(s));
                }
            }
        }
    }
    // Detect small authorization deltas even without a version generation change.
    if root.join("registry/objects.tsv").exists() {
        paths.insert(PathBuf::from("registry/objects.tsv"));
    }
    if root.join("registry/objects").exists() {
        for e in fs::read_dir(root.join("registry/objects"))? {
            let p = e?.path();
            if p.extension().is_some_and(|e| e == "tsv") {
                paths.insert(p.strip_prefix(root)?.to_path_buf());
            }
        }
    }
    let mut hashes = BTreeMap::new();
    let mut guards = vec![];
    for path in paths {
        if !root.join(&path).exists() && path == Path::new("registry/object-index.json") {
            continue;
        }
        let s = path
            .to_str()
            .ok_or("Non-UTF8 control path")?
            .replace('\\', "/");
        let resolved = crate::accounting::relative(root, &s)?;
        let mut file = if hold {
            platform::open(&resolved, false)?
        } else {
            fs::File::open(&resolved)?
        };
        if file.metadata()?.len() > 4 * 1024 * 1024 {
            return Err("Consumer control exceeds budget".into());
        }
        let mut bytes = vec![];
        Read::by_ref(&mut file)
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err("Consumer control grew beyond budget".into());
        }
        hashes.insert(s.to_owned(), tiering::digest(&bytes));
        if hold {
            guards.push(file);
        }
    }
    Ok((tiering::digest(&serde_json::to_vec(&hashes)?), guards))
}
fn metadata_token(root: &Path) -> Result<String> {
    Ok(metadata_snapshot(root, false)?.0)
}

fn stamp(journal: &Path, name: &str, value: &Value) -> Result<()> {
    let path = journal.join(name);
    if path.exists() {
        if crate::load(&path)? != *value {
            return Err("Journal state conflict".into());
        }
        return Ok(());
    }
    tiering::write_plan(&path, value)
}

#[cfg(test)]
type TestHook = Box<dyn FnMut(&str) -> Result<()>>;
#[cfg(test)]
thread_local! {static TEST_HOOK:std::cell::RefCell<Option<TestHook>>=std::cell::RefCell::new(None);}
fn checkpoint(_stage: &str) -> Result<()> {
    #[cfg(test)]
    TEST_HOOK.with(|hook| {
        if let Some(f) = hook.borrow_mut().as_mut() {
            f(_stage)
        } else {
            Ok(())
        }
    })?;
    Ok(())
}

/// Explicit scoped verification or apply. Only redundant hot CAS/LFS links may go.
/// Apply requires no live/unreconciled admissions and revalidates managed consumers.
pub fn run(
    root: &Path,
    request_path: &Path,
    journal: &Path,
    apply: bool,
    now: u64,
) -> Result<Value> {
    if apply && !cfg!(windows) {
        return Err("Apply is Windows-only in this slice".into());
    }
    let started = std::time::Instant::now();
    let request_bytes = tiering::read_bounded(request_path, 256 * 1024)?;
    let req: Request = serde_json::from_slice(&request_bytes)?;
    let deadline = started
        .checked_add(std::time::Duration::from_secs(req.max_seconds))
        .ok_or("Invalid deadline")?;
    let cfg = config(root)?;
    let control = admission::control(root, &cfg)?.canonicalize()?;
    if apply {
        let _guard = admission::lock(&control)?;
        ensure_no_admissions(&control)?;
    }
    let policy_bytes = tiering::read_bounded(&req.policy, 256 * 1024)?;
    let policy: tiering::Policy = serde_json::from_slice(&policy_bytes)?;
    policy.validate(&cfg)?;
    if req.schema != "reel.asset-demotion-request.v1"
        || req.consumer_scope != "all-managed-cache-consumers"
        || req.consumer_roots.is_empty()
        || req.consumer_roots.len() > 32
        || req.inactive_consumer_roots.len() > 32
        || req.sha256s.is_empty()
        || req.sha256s.len() > policy.max_batch_objects
        || req.sha256s.iter().any(|h| !crate::valid_oid(h))
        || req.max_seconds == 0
        || req.max_seconds > 3600
        || req.max_verify_bytes == 0
    {
        return Err("Invalid demotion request/scope/budget".into());
    }
    let fresh = tiering::check_plan(root, &req.policy, &req.plan, now)?;
    if fresh["plan_sha256"] != req.plan_sha256 {
        return Err("Request plan identity mismatch".into());
    }
    let saved: Value =
        serde_json::from_slice(&tiering::read_bounded(&req.plan, 512 * 1024 * 1024)?)?;
    let planned: BTreeMap<_, _> = saved["objects"]
        .as_array()
        .ok_or("Missing planned rows")?
        .iter()
        .filter_map(|r| r["sha256"].as_str().map(|h| (h, r)))
        .collect();
    let mut bytes = 0u64;
    for h in &req.sha256s {
        let row = planned
            .get(h.as_str())
            .ok_or("Object absent from saved plan")?;
        if row["proposal"] != "verify-and-demote" || row["class"] != "previously-used" {
            return Err("Requested object is not an eligible historical proposal".into());
        }
        let n = row["bytes"].as_u64().ok_or("Missing planned bytes")?;
        bytes = bytes
            .checked_add(n.checked_mul(2).ok_or("Byte overflow")?)
            .ok_or("Byte overflow")?;
        if n > cfg.capacity.max_object_bytes {
            return Err("Object exceeds operation budget".into());
        }
    }
    if bytes > req.max_verify_bytes || bytes / 2 > policy.max_batch_bytes {
        return Err("Demotion verification/batch budget exceeded".into());
    }
    let consumers: BTreeSet<PathBuf> = req
        .consumer_roots
        .iter()
        .map(|r| r.canonicalize())
        .collect::<std::io::Result<_>>()?;
    if consumers.len() != req.consumer_roots.len() || !consumers.contains(&root.canonicalize()?) {
        return Err("Consumer coverage must include invoking root without aliases".into());
    }
    let inactive: BTreeSet<PathBuf> = req
        .inactive_consumer_roots
        .iter()
        .map(|r| r.canonicalize())
        .collect::<std::io::Result<_>>()?;
    if inactive.len() != req.inactive_consumer_roots.len() {
        return Err("Inactive consumer aliases rejected".into());
    }
    let registration = consumer_registration(root, &consumers, &inactive)?;
    let mut protection_policy = policy.clone();
    protection_policy.history_versions.clear();
    let mut consumer_tokens = BTreeMap::new();
    for consumer in &consumers {
        if !consumer.join("asset-project.json").is_file() {
            return Err(format!("Managed consumer needs a compatible protection export (project profile absent): {}",consumer.display()).into());
        }
        let other_cfg = config(consumer)?;
        if other_cfg.cache_roots != cfg.cache_roots
            || admission::control(consumer, &other_cfg)?.canonicalize()? != control
        {
            return Err("Consumer cache/coordination bindings differ".into());
        }
        let before = metadata_token(consumer)?;
        let state = tiering::live(consumer, &protection_policy)?;
        for h in &req.sha256s {
            if state.selected.contains(h)
                || state.current.contains(h)
                || state.leased.contains(h)
                || state.controls.contains(h)
            {
                return Err(format!("Object protected by managed consumer: {h}").into());
            }
        }
        if metadata_token(consumer)? != before {
            return Err("Consumer changed during protection reconciliation".into());
        }
        consumer_tokens.insert(consumer.clone(), before);
    }
    // Only this repository's explicit local output namespace may be written.
    let normalized = |p: &Path| {
        p.to_string_lossy()
            .replace('/', "\\")
            .trim_start_matches("\\\\?\\")
            .to_uppercase()
    };
    let owned = format!("{}\\", normalized(&root.canonicalize()?.join(".local")));
    if !journal.is_absolute()
        || journal
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        || !normalized(journal).starts_with(&owned)
        || cfg
            .cache_roots
            .iter()
            .chain(&cfg.archive_roots)
            .any(|r| journal.starts_with(r))
    {
        return Err("Journal must be an explicit absolute path outside asset caches".into());
    }
    fs::create_dir_all(journal)?;
    if fs::symlink_metadata(journal)?.file_type().is_symlink() {
        return Err("Journal link rejected".into());
    }
    let _journal_lock = admission::lock(journal)?;
    let _journal_dirs = platform::guard_dirs(&journal.join("request.json"))?;
    stamp(
        journal,
        "request.json",
        &json!({"request_sha256":tiering::digest(&request_bytes),"apply":apply,"plan_sha256":req.plan_sha256}),
    )?;
    let hot = &cfg.cache_roots[policy.tier(&policy.selected_tier).index];
    let warm = &cfg.cache_roots[policy.tier(&policy.history_tier).index];
    if !req.allow_same_volume_copies && admission::volume_id(hot)? == admission::volume_id(warm)? {
        return Err("Source/destination share a volume; apply held".into());
    }
    let free_before = fs2::available_space(hot)?;
    let mut results = vec![];
    for h in &req.sha256s {
        if started.elapsed().as_secs() >= req.max_seconds {
            return Err("Demotion time budget exceeded; resume the named journal".into());
        }
        let row = planned[h.as_str()];
        let size = row["bytes"].as_u64().unwrap();
        let destination = object(warm, h);
        let _dest_dirs = platform::guard_dirs(&destination)?;
        let mut retained = platform::open(&destination, false)?;
        let retained_id = platform::identity(&retained)?;
        verify(&mut retained, h, size, deadline)?;
        let mut paths = vec![object(hot, h)];
        for ns in &crate::accounting::project(root)?.lfs_namespaces {
            paths.push(
                hot.join(ns)
                    .join("objects")
                    .join(&h[..2])
                    .join(&h[2..4])
                    .join(h),
            );
        }
        let mut guards = vec![];
        let mut links = vec![];
        let mut source_id: Option<Identity> = None;
        for path in &paths {
            match fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
                Ok(m) if !m.file_type().is_file() => return Err("Nonregular source link".into()),
                _ => (),
            }
            guards.extend(platform::guard_dirs(path)?);
            let mut file = platform::open(path, apply)?;
            let info = platform::identity(&file)?;
            if info.bytes != size {
                return Err("Fresh hot copy size mismatch".into());
            }
            if let Some(id) = &source_id {
                if id.id != info.id {
                    return Err("Independent LFS copy needs a separate action; held".into());
                }
            } else {
                verify(&mut file, h, size, deadline)?;
                source_id = Some(info.clone());
            }
            links.push((path.clone(), file));
        }
        if let Some(id) = &source_id
            && (id.links != links.len() as u64 || id.id == retained_id.id)
        {
            return Err("Unmanaged aliases or identical retained file; held".into());
        }
        checkpoint("before-lock")?;
        let _state_lock = admission::lock(&control)?;
        ensure_no_admissions(&control)?;
        if consumer_registration(root, &consumers, &inactive)? != registration {
            return Err("Registered worktree set changed; discard stale request".into());
        }
        let _request_guard = platform::open(request_path, false)?;
        let _policy_guard = platform::open(&req.policy, false)?;
        if fs::read(request_path)? != request_bytes || fs::read(&req.policy)? != policy_bytes {
            return Err("Demotion request/policy changed".into());
        }
        let mut control_guards = vec![];
        for (consumer, token) in &consumer_tokens {
            let (fresh, held) = metadata_snapshot(consumer, apply)?;
            if fresh != *token {
                return Err(
                    "Managed consumer changed before removal; discard stale request".into(),
                );
            }
            control_guards.extend(held);
        }
        if now.saturating_add(started.elapsed().as_secs())
            >= saved["expires_unix_seconds"]
                .as_u64()
                .ok_or("Missing expiry")?
        {
            return Err("Plan expired before removal".into());
        }
        let proof = json!({"sha256":h,"bytes":size,"retained_path":destination,"retained_identity":retained_id,"verified":true});
        stamp(journal, &format!("{h}-retained.json"), &proof)?;
        // Recover a crash between handle deletion and the completion stamp.
        // Absence is observed, not attributed to this process or counted twice.
        for path in &paths {
            let key = tiering::digest(path.to_string_lossy().as_bytes());
            let intent = journal.join(format!("{h}-{key}-intent.json"));
            if intent.exists() && !journal.join(format!("{h}-{key}-done.json")).exists() {
                match fs::symlink_metadata(path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let recorded = crate::load(&intent)?;
                        stamp(
                            journal,
                            &format!("{h}-{key}-recovered.json"),
                            &json!({"path":path,"sha256":h,"observed_absent_after_retained_copy_reverification":true}),
                        )?;
                        stamp(journal, &format!("{h}-{key}-done.json"), &recorded)?;
                    }
                    Err(e) => return Err(e.into()),
                    Ok(_) => (),
                }
            }
        }
        let mut removed = 0usize;
        for (path, file) in links {
            let key = tiering::digest(path.to_string_lossy().as_bytes());
            let intent = json!({"sha256":h,"path":path,"plan_sha256":req.plan_sha256,"retained_copy_verified":true});
            if apply {
                stamp(journal, &format!("{h}-{key}-intent.json"), &intent)?;
                // Recheck hard-link count after every completed unlink. A new alias holds the rest.
                let info = platform::identity(&file)?;
                if info.links != source_id.as_ref().unwrap().links - removed as u64 {
                    return Err("Source alias count changed; remaining removals held".into());
                }
                platform::assert_path(&file, &path)?;
                platform::remove(file)?;
                checkpoint("after-unlink")?;
                match fs::symlink_metadata(&path) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                    Ok(_) => return Err("Source link still present after handle deletion".into()),
                }
                stamp(journal, &format!("{h}-{key}-done.json"), &intent)?;
                removed += 1;
            }
        }
        results.push(json!({"sha256":h,"bytes":size,"verified_history_copy":destination,"hot_links_removed":removed,
            "hot_copy_already_absent":source_id.is_none(),"estimated_removed_allocation":if apply&&removed>0{source_id.as_ref().map(|s|s.allocated)}else{None}}));
    }
    let free_after = fs2::available_space(hot)?;
    let result = json!({"schema":"reel.asset-demotion-result.v1","apply":apply,"plan_sha256":req.plan_sha256,
        "objects":results,"free_bytes_before":free_before,"free_bytes_after":free_after,
        "observed_free_space_delta":i128::from(free_after)-i128::from(free_before),
        "free_space_delta_is_exclusively_attributed":false,"archive_probed":false,"last_copy_retired":false});
    // Resume may observe fewer links/free-space changes; keep earlier receipts intact.
    let out = crate::unique(journal, "result");
    tiering::write_plan(&out, &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;
    struct Case {
        f: Fixture,
        h: String,
        request: PathBuf,
        journal: PathBuf,
    }
    impl Case {
        fn new() -> Self {
            let mut f = Fixture::new();
            let h = f.put(b"historically selected synthetic payload", true);
            let dep = f.version("old", &h);
            f.policy["history_versions"] = json!([dep]);
            f.baseline();
            let plan = f.run().unwrap();
            let saved = f.root().join("saved-plan.json");
            tiering::write_plan(&saved, &plan).unwrap();
            let request = f.root().join("request.json");
            fs::write(&request,json!({"schema":"reel.asset-demotion-request.v1","policy":f.root().join("policy.json"),"plan":saved,"plan_sha256":plan["plan_sha256"],"consumer_roots":[f.root().canonicalize().unwrap()],"consumer_scope":"all-managed-cache-consumers","sha256s":[h],"max_verify_bytes":1024,"max_seconds":60,"allow_same_volume_copies":true}).to_string()).unwrap();
            let journal = f
                .root()
                .canonicalize()
                .unwrap()
                .join(".local/demotion-journal");
            Self {
                f,
                h,
                request,
                journal,
            }
        }
        fn run(&self, apply: bool) -> Result<Value> {
            run(self.f.root(), &self.request, &self.journal, apply, 1001)
        }
        fn e(&self) -> PathBuf {
            object(&self.f.root().join("fast"), &self.h)
        }
        fn d(&self) -> PathBuf {
            object(&self.f.root().join("history"), &self.h)
        }
        fn lfs(&self) -> PathBuf {
            self.f
                .root()
                .join("fast/lfs/test/objects")
                .join(&self.h[..2])
                .join(&self.h[2..4])
                .join(&self.h)
        }
    }
    #[test]
    fn verification_is_nondestructive_and_keeps_archive_offline() {
        let c = Case::new();
        let before = fs::read(c.e()).unwrap();
        let r = c.run(false).unwrap();
        assert_eq!(r["apply"], false);
        assert_eq!(r["objects"][0]["hot_links_removed"], 0);
        assert_eq!(fs::read(c.e()).unwrap(), before);
        assert_eq!(fs::read(c.d()).unwrap(), before);
        assert!(!c.f.root().join("offline-cold").exists());
    }
    #[test]
    fn corrupt_retained_bytes_fail_before_any_hot_removal() {
        let c = Case::new();
        fs::write(c.d(), b"corrupt").unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
    }
    #[test]
    fn missing_retained_copy_never_becomes_a_copy_or_deletion_request() {
        let c = Case::new();
        fs::remove_file(c.d()).unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
    }
    #[test]
    fn unmanaged_hard_link_holds_the_entire_object() {
        let c = Case::new();
        fs::hard_link(c.e(), c.f.root().join("unmanaged-alias")).unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
    }
    #[test]
    fn metadata_snapshot_handles_native_separator_authorization_shards() {
        let c = Case::new();
        let dir = c.f.root().join("registry/objects");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("ff.tsv"), format!("{}\t1\n", "f".repeat(64))).unwrap();
        let a = metadata_token(c.f.root()).unwrap();
        fs::write(dir.join("ff.tsv"), format!("{}\t2\n", "f".repeat(64))).unwrap();
        assert_ne!(a, metadata_token(c.f.root()).unwrap());
    }
    #[test]
    fn git_consumer_inventory_requires_new_worktrees_to_be_declared() {
        let c = Case::new();
        let hooks = c.f.root().join(".local/no-hooks");
        fs::create_dir_all(&hooks).unwrap();
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .arg("-C")
                .arg(c.f.root())
                .args([
                    "-c",
                    "user.name=fixture",
                    "-c",
                    "user.email=fixture@example.invalid",
                    "-c",
                    "commit.gpgsign=false",
                    "-c",
                ])
                .arg(format!("core.hooksPath={}", hooks.display()))
                .args(args)
                .output()
                .unwrap();
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        };
        git(&["init", "--quiet"]);
        git(&[
            "commit",
            "--allow-empty",
            "--quiet",
            "-m",
            "synthetic consumer",
        ]);
        let active = BTreeSet::from([c.f.root().canonicalize().unwrap()]);
        let before = consumer_registration(c.f.root(), &active, &BTreeSet::new()).unwrap();
        let extra = c.f.root().join(".local/extra-consumer");
        git(&[
            "worktree",
            "add",
            "--quiet",
            "-b",
            "extra",
            extra.to_str().unwrap(),
        ]);
        assert!(consumer_registration(c.f.root(), &active, &BTreeSet::new()).is_err());
        let inactive = BTreeSet::from([extra.canonicalize().unwrap()]);
        let after = consumer_registration(c.f.root(), &active, &inactive).unwrap();
        assert_ne!(before, after);
        assert!(consumer_registration(c.f.root(), &active, &active).is_err());
    }
    #[test]
    fn active_or_unreconciled_job_admission_holds_removal() {
        let c = Case::new();
        let dir = c.f.root().join(".local/control/admissions");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("job.lease"), b"").unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
    }
    #[test]
    fn job_cannot_capture_a_source_path_before_admission() {
        let c = Case::new();
        fs::remove_file(c.e()).unwrap();
        fs::remove_file(c.d()).unwrap();
        let cfg = config(c.f.root()).unwrap();
        let control = admission::control(c.f.root(), &cfg).unwrap();
        let _maintenance = admission::lock(&control).unwrap();
        let job = c.f.root().join("job.json");
        let n = b"historically selected synthetic payload".len();
        fs::write(&job,json!({"budget_bytes":1024,"scratch_bytes":0,"inputs":{"in":{"sha256":c.h,"bytes":n}},"tools":{"tool.exe":{"sha256":c.h,"bytes":n}},"steps":[{"tool":"tool.exe","args":[],"timeout_seconds":1}],"outputs":{"out":{"path":"out","max_bytes":1}}}).to_string()).unwrap();
        let err = crate::jobs::run(c.f.root(), &job).unwrap_err().to_string();
        assert!(err.contains("Storage metadata is in use"), "{err}");
    }
    #[test]
    fn stale_selection_and_request_scope_are_rejected() {
        let mut c = Case::new();
        c.f.state.select("old", "v1").unwrap();
        c.f.save();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
        let mut c = Case::new();
        let mut req = crate::load(&c.request).unwrap();
        req["consumer_roots"] = json!([]);
        fs::write(&c.request, req.to_string()).unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        c.f.save();
    }
    #[test]
    fn same_inode_retention_and_wrong_journal_are_rejected() {
        let mut c = Case::new();
        fs::remove_file(c.d()).unwrap();
        fs::hard_link(c.e(), c.d()).unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
        c.journal = c.f.root().canonicalize().unwrap().join("foreign-journal");
        assert!(c.run(false).is_err());
        assert!(!c.journal.exists());
    }
    #[test]
    fn independent_lfs_copy_is_held_instead_of_silently_counted_as_an_alias() {
        let c = Case::new();
        let alias = c.lfs();
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        fs::copy(c.e(), alias).unwrap();
        assert!(c.run(cfg!(windows)).is_err());
        assert!(c.e().exists());
    }
    #[cfg(windows)]
    #[test]
    fn apply_removes_only_verified_hot_links_and_is_idempotent() {
        let c = Case::new();
        let alias = c.lfs();
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        fs::hard_link(c.e(), &alias).unwrap();
        let retained = fs::read(c.d()).unwrap();
        let r = c.run(true).unwrap();
        assert_eq!(r["objects"][0]["hot_links_removed"], 2);
        assert!(!c.e().exists());
        assert!(!alias.exists());
        assert_eq!(fs::read(c.d()).unwrap(), retained);
        let again = c.run(true).unwrap();
        assert_eq!(again["objects"][0]["hot_links_removed"], 0);
        assert_eq!(again["objects"][0]["hot_copy_already_absent"], true);
    }
    #[cfg(windows)]
    #[test]
    fn retained_handle_blocks_writers_and_path_replacement() {
        let c = Case::new();
        let _guard = platform::open(&c.d(), false).unwrap();
        assert!(fs::OpenOptions::new().write(true).open(c.d()).is_err());
        assert!(fs::remove_file(c.d()).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn readonly_hot_copy_is_held_without_changing_attributes() {
        let c = Case::new();
        let mut p = fs::metadata(c.e()).unwrap().permissions();
        let original = p.clone();
        p.set_readonly(true);
        fs::set_permissions(c.e(), p).unwrap();
        assert!(c.run(true).is_err());
        assert!(c.e().exists());
        assert!(c.d().exists());
        assert!(fs::metadata(c.e()).unwrap().permissions().readonly());
        // Restore only this fixture's permission so TempDir can remove its files.
        fs::set_permissions(c.e(), original).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn source_handle_deletion_does_not_delete_a_replacement_path() {
        let c = Case::new();
        let handle = platform::open(&c.e(), true).unwrap();
        let moved = c.f.root().join("moved-original");
        fs::rename(c.e(), &moved).unwrap();
        fs::write(c.e(), b"replacement").unwrap();
        platform::remove(handle).unwrap();
        assert_eq!(fs::read(c.e()).unwrap(), b"replacement");
        assert!(!moved.exists());
        assert!(c.d().exists());
    }
    #[cfg(windows)]
    #[test]
    fn interruption_after_one_link_can_resume_without_last_copy_loss() {
        let c = Case::new();
        let alias = c.lfs();
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        fs::hard_link(c.e(), &alias).unwrap();
        TEST_HOOK.with(|h| {
            *h.borrow_mut() = Some(Box::new(|stage| {
                if stage == "after-unlink" {
                    Err("injected interruption".into())
                } else {
                    Ok(())
                }
            }))
        });
        assert!(c.run(true).is_err());
        TEST_HOOK.with(|h| *h.borrow_mut() = None);
        assert!(alias.exists());
        let result = c.run(true).unwrap();
        assert_eq!(result["objects"][0]["hot_links_removed"], 1);
        assert!(!alias.exists());
        assert!(c.d().exists());
        assert!(fs::read_dir(&c.journal).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with("-recovered.json")
        }));
    }
    #[cfg(windows)]
    #[test]
    fn late_control_or_admission_change_stops_before_any_unlink() {
        for admission in [false, true] {
            let c = Case::new();
            let root = c.f.root().to_path_buf();
            TEST_HOOK.with(|h| {
                *h.borrow_mut() = Some(Box::new(move |stage| {
                    if stage == "before-lock" {
                        if admission {
                            let d = root.join(".local/control/admissions");
                            fs::create_dir_all(&d)?;
                            fs::write(d.join("late.json"), b"{}")?;
                        } else {
                            let p = root.join("registry/versions.json");
                            let mut s = crate::load(&p)?;
                            s["generation"] = json!(100);
                            fs::write(p, s.to_string())?;
                        }
                    }
                    Ok(())
                }))
            });
            assert!(c.run(true).is_err());
            TEST_HOOK.with(|h| *h.borrow_mut() = None);
            assert!(c.e().exists());
            assert!(c.d().exists());
        }
    }
    #[cfg(windows)]
    #[test]
    fn renamed_or_replaced_link_is_held_before_any_removal() {
        let c = Case::new();
        let source = c.e();
        let moved = c.f.root().join("moved-original");
        let saved = moved.clone();
        TEST_HOOK.with(|h| {
            *h.borrow_mut() = Some(Box::new(move |stage| {
                if stage == "before-lock" {
                    fs::rename(&source, &moved)?;
                    fs::write(&source, b"replacement")?;
                }
                Ok(())
            }))
        });
        assert!(c.run(true).is_err());
        TEST_HOOK.with(|h| *h.borrow_mut() = None);
        assert!(saved.exists());
        assert_eq!(fs::read(c.e()).unwrap(), b"replacement");
        assert!(c.d().exists());
    }
    #[cfg(not(windows))]
    #[test]
    fn unsupported_handle_deletion_fails_closed() {
        let c = Case::new();
        assert!(c.run(true).is_err());
        assert!(c.e().exists());
    }
}
