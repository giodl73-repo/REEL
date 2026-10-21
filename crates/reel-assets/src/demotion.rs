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
    /// Explicit read-only legacy consumers resolved through this project's caches.
    #[serde(default)]
    pub legacy_consumer_roots: Vec<PathBuf>,
    pub consumer_scope: String,
    pub sha256s: BTreeSet<String>,
    pub max_verify_bytes: u64,
    pub max_seconds: u64,
    /// Optional repository-relative declarations frozen by an owner review.
    #[serde(default)]
    pub source_controls: BTreeMap<String, String>,
    #[serde(default)]
    pub allow_same_volume_copies: bool,
    /// Explicit cold custody operation; omitted preserves history-only behavior.
    #[serde(default)]
    pub retained_tier: Option<String>,
    /// Explicit owner-authorized reuse of already hash-verified immutable cold
    /// backups. Ordinary execution still hashes retained payloads freshly.
    #[serde(default)]
    pub verified_cold_index: Option<ColdIndexRef>,
    /// Explicit cache-owned disposable namespaces; exact alias paths must also
    /// occur in the hash-bound verified index. Worktree paths are never inferred.
    #[serde(default)]
    pub source_alias_namespaces: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ColdIndexRef {
    pub path: PathBuf,
    pub sha256: String,
    pub owner_attests_append_only_archive: bool,
    #[serde(default)]
    pub reuse_unchanged_source_proofs: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdIndex {
    schema: String,
    archive_root: PathBuf,
    objects: BTreeMap<String, ColdProof>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ColdProof {
    bytes: u64,
    verified_unix_ns: u64,
    manifest_sha256: String,
    #[serde(default)]
    source: Option<SourceProof>,
    #[serde(default)]
    source_aliases: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceProof {
    identity: String,
    modified_unix_ns: u64,
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
    // Windows executable main threads can have a 1-MiB stack. Keep the streaming
    // buffer on the heap; unit-test worker stacks must not hide CLI overflows.
    let mut buffer = vec![0u8; 256 * 1024];
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
    if admissions_present(control)? {
        return Err("Live or unreconciled admissions present; redundant-copy removal held".into());
    }
    Ok(())
}

fn admissions_present(control: &Path) -> Result<bool> {
    let dir = control.join("admissions");
    Ok(dir.exists() && fs::read_dir(dir)?.next().is_some())
}

pub(crate) fn check_source_controls(
    root: &Path,
    req: &Request,
    hold: bool,
) -> Result<Vec<fs::File>> {
    if req.source_controls.len() > 16 {
        return Err("Too many review source controls".into());
    }
    let mut guards = vec![];
    for (name, expected) in &req.source_controls {
        if !crate::valid_oid(expected) {
            return Err("Invalid source control identity".into());
        }
        crate::accounting::relative(root, name)?;
        let path = root.join(name);
        if !fs::symlink_metadata(&path)?.file_type().is_file() {
            return Err("Review source control must be a regular file".into());
        }
        guards.extend(platform::guard_dirs(&path)?);
        let file = platform::open(&path, false)?;
        if tiering::digest(&tiering::read_bounded(&path, 256 * 1024)?) != *expected {
            return Err("Project review declarations changed; generate a new review".into());
        }
        if hold {
            guards.push(file);
        }
    }
    if !hold {
        guards.clear();
    }
    Ok(guards)
}

fn consumer_registration(
    root: &Path,
    active: &BTreeSet<PathBuf>,
    inactive: &BTreeSet<PathBuf>,
) -> Result<String> {
    if !active.is_disjoint(inactive) {
        return Err("Active/inactive consumer scopes overlap".into());
    }
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
    let mut paths = BTreeSet::from([
        PathBuf::from("registry/versions.json"),
        PathBuf::from("registry/object-index.json"),
    ]);
    let modern = root.join("asset-project.json").exists();
    let rules = if modern {
        let p = crate::accounting::project(root)?;
        paths.insert(PathBuf::from("asset-project.json"));
        paths.insert(PathBuf::from(&p.storage_config));
        p.selections
    } else {
        vec![crate::accounting::Selection {
            index: "catalog.json".into(),
            entries: "/videos".into(),
            record_field: "bindings".into(),
            format: "bindings".into(),
            role_prefixes: vec!["delivery/".into()],
        }]
    };
    for rule in rules {
        let index = crate::accounting::relative(root, &rule.index)?;
        paths.insert(PathBuf::from(&rule.index));
        let doc = crate::load(&index)?;
        for row in doc
            .pointer(&rule.entries)
            .and_then(Value::as_array)
            .ok_or("Selection index invalid")?
        {
            for key in [&rule.record_field, "scene", "bindings", "record"] {
                if let Some(s) = row.get(key).and_then(Value::as_str) {
                    paths.insert(PathBuf::from(s));
                    if !modern {
                        let cel = Path::new(s)
                            .parent()
                            .ok_or("Missing legacy record parent")?
                            .join("cels.jsonl");
                        if root.join(&cel).exists() {
                            paths.insert(cel);
                        }
                    }
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

fn legacy_protection(root: &Path, cfg: &crate::Config) -> Result<BTreeSet<String>> {
    let state = crate::versions::read_with(root, Some(cfg))?;
    tiering::validate_graph(&state, 1_000_000)?;
    let mut protected = crate::accounting::expand(
        &state,
        state
            .current
            .iter()
            .map(|(id, v)| format!("{id}@{v}"))
            .chain(state.leases.values().flatten().cloned()),
    )?;
    protected.extend(state.metadata_roots);
    if root.join("registry/object-index.json").exists() {
        let d = crate::load(&root.join("registry/object-index.json"))?;
        protected.insert(
            d["asset"]["sha256"]
                .as_str()
                .ok_or("Missing legacy authorization root")?
                .into(),
        );
    }
    let catalog: Value = serde_json::from_slice(&tiering::read_bounded(
        &root.join("catalog.json"),
        256 * 1024,
    )?)?;
    for row in catalog["videos"]
        .as_array()
        .ok_or("Missing legacy catalogue videos")?
    {
        let record = row["bindings"]
            .as_str()
            .or_else(|| row["record"].as_str())
            .ok_or("Missing legacy bindings")?;
        let path = crate::accounting::relative(root, record)?;
        let bindings: Value = serde_json::from_slice(&tiering::read_bounded(&path, 256 * 1024)?)?;
        let mut retain = |a: &Value| -> Result<()> {
            let h = a["sha256"].as_str().ok_or("Missing legacy asset hash")?;
            if !crate::valid_oid(h) || a["bytes"].as_u64().is_none() {
                return Err("Invalid legacy asset descriptor".into());
            }
            protected.insert(h.into());
            Ok(())
        };
        for a in bindings["assets"]
            .as_object()
            .ok_or("Missing legacy assets")?
            .values()
        {
            retain(a)?;
        }
        let cels = path
            .parent()
            .ok_or("Missing legacy parent")?
            .join("cels.jsonl");
        if cels.exists() {
            for line in std::str::from_utf8(&tiering::read_bounded(&cels, 512 * 1024)?)?
                .lines()
                .filter(|s| !s.is_empty())
            {
                retain(&serde_json::from_str(line)?)?;
            }
        }
    }
    Ok(protected)
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

/// Exact named custody only. No archive enumeration, copying or last-copy
/// retirement. The returned handles keep the durable receipt immutable until
/// this object's source removals finish.
fn register_cold(archive: &Path, hash: &str, bytes: u64, path: &Path) -> Result<Vec<fs::File>> {
    let mut dir = archive.to_path_buf();
    let mut guards = platform::guard_dirs(&dir.join("custody-anchor"))?;
    for component in ["snapshots", "reel-assets-cold-v1", &hash[..2]] {
        dir.push(component);
        match fs::create_dir(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        guards.extend(platform::guard_dirs(&dir.join("custody-anchor"))?);
    }
    let name = format!("{hash}.json");
    let receipt = json!({"schema":"reel.cold-custody.v1","sha256":hash,"bytes":bytes,
        "cache_uri":format!("cache://sha256/{hash}"),"cold_path":path,
        "retention_state":"cold-registered","provenance":"Unregistered active-cache object; original provenance unknown",
        "recovery":"Explicit named-object recovery only; ordinary active lookup does not probe this archive",
        "not_a_permanent_retention_pin":true});
    stamp(&dir, &name, &receipt)?;
    let file = platform::open(&dir.join(&name), false)?;
    if crate::load(&dir.join(&name))? != receipt {
        return Err("Cold custody receipt changed".into());
    }
    guards.push(file);
    Ok(guards)
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
    let policy_bytes = tiering::read_bounded(&req.policy, 256 * 1024)?;
    let policy: tiering::Policy = serde_json::from_slice(&policy_bytes)?;
    policy.validate(&cfg)?;
    let cold = match req.retained_tier.as_deref() {
        None => false,
        Some(id) if id == policy.history_tier => false,
        Some(id) if id == policy.cold_tier => true,
        _ => return Err("Retained tier must be the configured history or cold tier".into()),
    };
    if !cold && req.verified_cold_index.is_some() {
        return Err("Verified cold index is valid only for explicit cold custody".into());
    }
    let normal_relative = |s: &str| {
        !s.is_empty()
            && Path::new(s)
                .components()
                .all(|c| matches!(c, std::path::Component::Normal(_)))
    };
    if req.source_alias_namespaces.len() > 16
        || req
            .source_alias_namespaces
            .iter()
            .any(|s| !normal_relative(s))
        || (!req.source_alias_namespaces.is_empty() && (!cold || req.verified_cold_index.is_none()))
    {
        return Err(
            "Cold alias namespaces require an explicit verified index and normal relative paths"
                .into(),
        );
    }
    if apply && !cold {
        let _guard = admission::lock(&control)?;
        ensure_no_admissions(&control)?;
    }
    if req.schema != "reel.asset-demotion-request.v1"
        || req.consumer_scope != "all-managed-cache-consumers"
        || req.consumer_roots.is_empty()
        || req.consumer_roots.len() > 32
        || req.inactive_consumer_roots.len() > 32
        || req.legacy_consumer_roots.len() > 32
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
        let eligible = if cold {
            row["proposal"] == "register-and-archive-first"
                && row["class"] == "unregistered-cold-candidate"
        } else {
            row["proposal"] == "verify-and-demote" && row["class"] == "previously-used"
        };
        if !eligible {
            return Err("Requested object is not eligible for the explicit retained tier".into());
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
    let legacy: BTreeSet<PathBuf> = req
        .legacy_consumer_roots
        .iter()
        .map(|r| r.canonicalize())
        .collect::<std::io::Result<_>>()?;
    if legacy.len() != req.legacy_consumer_roots.len() || !legacy.is_subset(&consumers) {
        return Err("Legacy consumer scope invalid".into());
    }
    let mut protection_policy = policy.clone();
    protection_policy.history_versions.clear();
    let mut consumer_tokens = BTreeMap::new();
    for consumer in &consumers {
        if !consumer.join("asset-project.json").is_file() {
            if !legacy.contains(consumer) {
                return Err(format!(
                    "Managed consumer needs an explicit compatible legacy scope: {}",
                    consumer.display()
                )
                .into());
            }
            let before = metadata_token(consumer)?;
            let protected = legacy_protection(consumer, &cfg)?;
            if let Some(h) = req.sha256s.iter().find(|h| protected.contains(*h)) {
                return Err(format!("Object protected by legacy managed consumer: {h}").into());
            }
            if metadata_token(consumer)? != before && !cold {
                return Err("Legacy consumer changed during reconciliation".into());
            }
            consumer_tokens.insert(consumer.clone(), before);
            continue;
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
                || (cold && state.registered.contains_key(h))
            {
                return Err(format!("Object protected by managed consumer: {h}").into());
            }
        }
        if metadata_token(consumer)? != before && !cold {
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
    let warm = if cold {
        &cfg.archive_roots[policy.tier(&policy.cold_tier).index]
    } else {
        &cfg.cache_roots[policy.tier(&policy.history_tier).index]
    };
    let mut index_guards = vec![];
    let trusted = if let Some(reference) = &req.verified_cold_index {
        if !reference.owner_attests_append_only_archive || !crate::valid_oid(&reference.sha256) {
            return Err("Cold index reuse requires explicit append-only owner attestation".into());
        }
        index_guards.extend(platform::guard_dirs(&reference.path)?);
        let mut file = platform::open(&reference.path, false)?;
        if file.metadata()?.len() > 64 * 1024 * 1024 {
            return Err("Verified cold index exceeds budget".into());
        }
        let mut bytes = vec![];
        Read::by_ref(&mut file)
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if tiering::digest(&bytes) != reference.sha256 {
            return Err("Verified cold index hash mismatch".into());
        }
        let index: ColdIndex = serde_json::from_slice(&bytes)?;
        if index.schema != "reel.verified-cold-index.v1"
            || index.archive_root.canonicalize()? != warm.canonicalize()?
        {
            return Err("Verified cold index archive binding mismatch".into());
        }
        index_guards.push(file);
        Some(index)
    } else {
        None
    };
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
        let retained_verification = if let Some(index) = &trusted {
            let proof = index
                .objects
                .get(h)
                .ok_or("Object absent from verified cold index")?;
            let modified = retained
                .metadata()?
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            if proof.bytes != size
                || retained_id.bytes != size
                || !crate::valid_oid(&proof.manifest_sha256)
                || modified > u128::from(proof.verified_unix_ns)
                || u128::from(proof.verified_unix_ns) > u128::from(now) * 1_000_000_000
            {
                return Err(
                    "Cold copy changed or verified index proof is invalid; freshly hash it".into(),
                );
            }
            "prior-verified-index-and-current-size-mtime"
        } else {
            verify(&mut retained, h, size, deadline)?;
            "fresh-sha256"
        };
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
        if let Some(proof) = trusted.as_ref().and_then(|index| index.objects.get(h)) {
            if proof.source_aliases.len() > 128 {
                return Err("Verified source alias list exceeds budget".into());
            }
            for alias in &proof.source_aliases {
                if !normal_relative(alias) {
                    return Err("Verified source alias is not a normal relative cache path".into());
                }
                if req
                    .source_alias_namespaces
                    .iter()
                    .any(|ns| Path::new(alias).starts_with(ns))
                {
                    paths.push(hot.join(alias));
                }
            }
        }
        let mut seen_paths = BTreeSet::new();
        paths.retain(|p| seen_paths.insert(p.clone()));
        let mut guards = vec![];
        let mut links = vec![];
        let mut source_id: Option<Identity> = None;
        let mut source_verification = "source-already-absent";
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
                let unchanged = if req
                    .verified_cold_index
                    .as_ref()
                    .is_some_and(|r| r.reuse_unchanged_source_proofs)
                {
                    if let Some(proof) = trusted
                        .as_ref()
                        .and_then(|i| i.objects.get(h))
                        .and_then(|p| p.source.as_ref())
                    {
                        proof.identity == info.id
                            && file
                                .metadata()?
                                .modified()?
                                .duration_since(std::time::UNIX_EPOCH)?
                                .as_nanos()
                                == u128::from(proof.modified_unix_ns)
                    } else {
                        false
                    }
                } else {
                    false
                };
                source_verification = if unchanged {
                    "prior-verified-source-index-and-current-identity-size-mtime"
                } else {
                    verify(&mut file, h, size, deadline)?;
                    "fresh-sha256"
                };
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
        let _state_lock = if apply {
            Some(if cold {
                admission::lock_for(&control, std::time::Duration::from_secs(2))?
            } else {
                admission::lock(&control)?
            })
        } else {
            None
        };
        if apply && !cold {
            ensure_no_admissions(&control)?;
        }
        if consumer_registration(root, &consumers, &inactive)? != registration {
            return Err("Registered worktree set changed; discard stale request".into());
        }
        let _request_guard = platform::open(request_path, false)?;
        let _policy_guard = platform::open(&req.policy, false)?;
        if fs::read(request_path)? != request_bytes || fs::read(&req.policy)? != policy_bytes {
            return Err("Demotion request/policy changed".into());
        }
        let _source_controls = check_source_controls(root, &req, apply)?;
        let mut control_guards = vec![];
        for (consumer, token) in &consumer_tokens {
            let (fresh, held) = metadata_snapshot(consumer, apply)?;
            if fresh != *token {
                if !cold {
                    return Err(
                        "Managed consumer changed before removal; discard stale request".into(),
                    );
                }
                // New unrelated registrations must not stop cold cleanup. The
                // exact current metadata is held here: re-evaluate this object,
                // including every registered hash, instead of waiving freshness.
                if consumer.join("asset-project.json").is_file() {
                    let other_cfg = config(consumer)?;
                    if other_cfg.cache_roots != cfg.cache_roots
                        || admission::control(consumer, &other_cfg)?.canonicalize()? != control
                    {
                        return Err("Consumer cache/coordination changed".into());
                    }
                    let state = tiering::live(consumer, &protection_policy)?;
                    if state.registered.contains_key(h)
                        || state.selected.contains(h)
                        || state.current.contains(h)
                        || state.leased.contains(h)
                        || state.controls.contains(h)
                    {
                        return Err(
                            "Cold object became protected or registered before removal".into()
                        );
                    }
                } else if !legacy.contains(consumer)
                    || legacy_protection(consumer, &cfg)?.contains(h)
                {
                    return Err("Cold object became protected by legacy consumer".into());
                }
                if metadata_token(consumer)? != fresh {
                    return Err("Consumer changed during exact cold protection refresh".into());
                }
            }
            control_guards.extend(held);
        }
        if std::time::Instant::now() >= deadline {
            return Err("Demotion time budget exceeded before removal".into());
        }
        if now.saturating_add(started.elapsed().as_secs())
            >= saved["expires_unix_seconds"]
                .as_u64()
                .ok_or("Missing expiry")?
        {
            return Err("Plan expired before removal".into());
        }
        let proof = json!({"sha256":h,"bytes":size,"retained_path":destination,"retained_identity":retained_id,"verified":true,
            "retained_verification":retained_verification,
            "verified_cold_index_sha256":req.verified_cold_index.as_ref().map(|r|&r.sha256)});
        stamp(journal, &format!("{h}-retained.json"), &proof)?;
        // Cold custody is separate from the active registry. Publish it before
        // the first source unlink, while the exact independently verified cold
        // object and all selection metadata remain locked against replacement.
        let _cold_custody = if cold && apply {
            Some(register_cold(warm, h, size, &destination)?)
        } else {
            None
        };
        checkpoint("after-cold-registration")?;
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
            "retained_verification":retained_verification,
            "source_verification":source_verification,
            "hot_copy_already_absent":source_id.is_none(),"estimated_removed_allocation":if apply&&removed>0{source_id.as_ref().map(|s|s.allocated)}else{None}}));
    }
    let free_after = fs2::available_space(hot)?;
    let result = json!({"schema":"reel.asset-demotion-result.v1","apply":apply,"plan_sha256":req.plan_sha256,
        "objects":results,"free_bytes_before":free_before,"free_bytes_after":free_after,
        "observed_free_space_delta":i128::from(free_after)-i128::from(free_before),
        "free_space_delta_is_exclusively_attributed":false,"archive_probed":cold,
        "cold_custody_registered":cold && apply,"last_copy_retired":false});
    let mut result = result;
    result["admissions_present_at_completion"] = json!(admissions_present(&control)?);
    result["removal_authorized"] = json!(apply);
    // Resume may observe fewer links/free-space changes; keep earlier receipts intact.
    let out = crate::unique(journal, "result");
    tiering::write_plan(&out, &result)?;
    Ok(result)
}

/// Read-only protection export, including explicitly declared older State/index consumers.
/// Consumers' files/configuration are never written or synthesized by this operation.
pub fn export_protections(root: &Path, request_path: &Path) -> Result<Value> {
    let raw = tiering::read_bounded(request_path, 256 * 1024)?;
    let req: Request = serde_json::from_slice(&raw)?;
    let cfg = config(root)?;
    let policy_raw = tiering::read_bounded(&req.policy, 256 * 1024)?;
    let mut policy: tiering::Policy = serde_json::from_slice(&policy_raw)?;
    policy.validate(&cfg)?;
    policy.history_versions.clear();
    if req.schema != "reel.asset-demotion-request.v1"
        || req.consumer_scope != "all-managed-cache-consumers"
        || req.consumer_roots.is_empty()
        || req.consumer_roots.len() > 32
        || req.legacy_consumer_roots.len() > 32
        || req.inactive_consumer_roots.len() > 32
    {
        return Err("Invalid protection export scope".into());
    }
    let paths = |r: &Vec<PathBuf>| -> Result<BTreeSet<PathBuf>> {
        Ok(r.iter()
            .map(|p| p.canonicalize())
            .collect::<std::io::Result<_>>()?)
    };
    let active = paths(&req.consumer_roots)?;
    let legacy = paths(&req.legacy_consumer_roots)?;
    let inactive = paths(&req.inactive_consumer_roots)?;
    if !active.contains(&root.canonicalize()?)
        || !legacy.is_subset(&active)
        || active.len() != req.consumer_roots.len()
        || legacy.len() != req.legacy_consumer_roots.len()
        || inactive.len() != req.inactive_consumer_roots.len()
    {
        return Err("Protection export coverage invalid".into());
    }
    let registration = consumer_registration(root, &active, &inactive)?;
    let mut hashes: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut consumers = vec![];
    for (n, path) in active.iter().enumerate() {
        let before = metadata_token(path)?;
        let protected = if path.join("asset-project.json").exists() {
            let other = config(path)?;
            if other.cache_roots != cfg.cache_roots {
                return Err("Consumer cache bindings differ".into());
            }
            let view = tiering::live(path, &policy)?;
            view.current
                .into_iter()
                .chain(view.selected)
                .chain(view.leased)
                .chain(view.controls)
                .collect::<BTreeSet<_>>()
        } else {
            if !legacy.contains(path) {
                return Err("Legacy consumer must be explicitly declared".into());
            }
            legacy_protection(path, &cfg)?
        };
        for h in protected {
            hashes.entry(h).or_default().push(n);
        }
        if metadata_token(path)? != before {
            return Err("Consumer changed during protection export".into());
        }
        consumers
            .push(json!({"root":path,"fingerprint_sha256":before,"legacy":legacy.contains(path)}));
    }
    if consumer_registration(root, &active, &inactive)? != registration
        || fs::read(request_path)? != raw
        || fs::read(&req.policy)? != policy_raw
    {
        return Err("Protection scope changed during export".into());
    }
    for (path, row) in active.iter().zip(&consumers) {
        if metadata_token(path)? != row["fingerprint_sha256"] {
            return Err("Consumer changed before protection export completed".into());
        }
    }
    Ok(
        json!({"schema":"reel.asset-protection-export.v1","consumer_registration_sha256":registration,
        "consumers":consumers,"protected_hashes":hashes,"archive_probed":false,"files_changed":false,
        "removal_authorized":false,"scope":"Declared active consumers only; revalidate at apply time"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tiering::tests::Fixture;

    fn cold_case() -> Case {
        let mut f = Fixture::new();
        let data = b"unregistered cold candidate";
        let h = f.put(data, false);
        let cold = object(&f.root().join("offline-cold"), &h);
        fs::create_dir_all(cold.parent().unwrap()).unwrap();
        fs::write(cold, data).unwrap();
        f.baseline();
        let plan = f.run().unwrap();
        let saved = f.root().join("saved-plan.json");
        tiering::write_plan(&saved, &plan).unwrap();
        let request = f.root().join("request.json");
        fs::write(&request,json!({"schema":"reel.asset-demotion-request.v1","policy":f.root().join("policy.json"),"plan":saved,"plan_sha256":plan["plan_sha256"],"consumer_roots":[f.root().canonicalize().unwrap()],"consumer_scope":"all-managed-cache-consumers","sha256s":[h],"max_verify_bytes":1024,"max_seconds":60,"allow_same_volume_copies":true,"retained_tier":"cold"}).to_string()).unwrap();
        let journal = f.root().canonicalize().unwrap().join(".local/cold-journal");
        Case {
            f,
            h,
            request,
            journal,
        }
    }

    #[test]
    fn explicit_cold_dry_run_verifies_named_object_without_registration() {
        let c = cold_case();
        let r = c.run(false).unwrap();
        assert_eq!(r["archive_probed"], true);
        assert_eq!(r["cold_custody_registered"], false);
        assert!(c.e().is_file());
        assert!(!c.f.root().join("offline-cold/snapshots").exists());
    }

    #[test]
    fn cold_missing_corrupt_and_registered_history_are_held() {
        let c = cold_case();
        let path = object(&c.f.root().join("offline-cold"), &c.h);
        fs::write(&path, b"corrupt").unwrap();
        assert!(c.run(false).is_err());
        fs::remove_file(&path).unwrap();
        assert!(c.run(false).is_err());
        assert!(c.e().is_file());
        let c = Case::new();
        let mut req = crate::load(&c.request).unwrap();
        req["retained_tier"] = json!("cold");
        fs::write(&c.request, req.to_string()).unwrap();
        assert!(c.run(false).is_err());
        assert!(c.e().is_file());
    }

    fn indexed_cold_case() -> Case {
        let c = cold_case();
        let archive = c.f.root().join("offline-cold");
        let file = fs::OpenOptions::new()
            .write(true)
            .open(object(&archive, &c.h))
            .unwrap();
        file.set_times(
            fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(900)),
        )
        .unwrap();
        drop(file);
        let index = json!({"schema":"reel.verified-cold-index.v1","archive_root":archive,
            "objects":{&c.h:{"bytes":fs::metadata(c.e()).unwrap().len(),"verified_unix_ns":950_000_000_000u64,"manifest_sha256":"b".repeat(64)}}});
        let raw = serde_json::to_vec(&index).unwrap();
        let path = c.f.root().join("cold-index.json");
        fs::write(&path, &raw).unwrap();
        let mut request = crate::load(&c.request).unwrap();
        request["verified_cold_index"] = json!({"path":path,"sha256":tiering::digest(&raw),"owner_attests_append_only_archive":true});
        fs::write(&c.request, request.to_string()).unwrap();
        c
    }

    #[test]
    fn verified_cold_index_reuse_is_explicit_bound_and_reports_prior_verification() {
        let c = indexed_cold_case();
        let result = c.run(false).unwrap();
        assert_eq!(
            result["objects"][0]["retained_verification"],
            "prior-verified-index-and-current-size-mtime"
        );
        let mut request = crate::load(&c.request).unwrap();
        request["verified_cold_index"]["owner_attests_append_only_archive"] = json!(false);
        fs::write(&c.request, request.to_string()).unwrap();
        assert!(c.run(false).is_err());
        let c = indexed_cold_case();
        fs::write(c.f.root().join("cold-index.json"), b"{}").unwrap();
        assert!(c.run(false).is_err());
    }

    #[test]
    fn verified_cold_index_never_hides_later_modification_or_source_corruption() {
        let c = indexed_cold_case();
        fs::write(
            object(&c.f.root().join("offline-cold"), &c.h),
            b"unregistered cold candidate",
        )
        .unwrap();
        assert!(c.run(false).is_err());
        let c = indexed_cold_case();
        fs::write(c.e(), b"different same-length bytes").unwrap();
        assert!(c.run(false).is_err());
        assert!(c.e().is_file());
    }

    #[test]
    fn unchanged_source_proofs_require_exact_identity_and_mtime_and_explicit_opt_in() {
        let c = indexed_cold_case();
        let file = platform::open(&c.e(), false).unwrap();
        let identity = platform::identity(&file).unwrap();
        let modified = file
            .metadata()
            .unwrap()
            .modified()
            .unwrap()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        drop(file);
        let path = c.f.root().join("cold-index.json");
        let mut index = crate::load(&path).unwrap();
        index["objects"][&c.h]["source"] =
            json!({"identity":identity.id,"modified_unix_ns":modified});
        let raw = serde_json::to_vec(&index).unwrap();
        fs::write(&path, &raw).unwrap();
        let mut req = crate::load(&c.request).unwrap();
        req["verified_cold_index"]["sha256"] = json!(tiering::digest(&raw));
        fs::write(&c.request, req.to_string()).unwrap();
        assert_eq!(
            c.run(false).unwrap()["objects"][0]["source_verification"],
            "fresh-sha256"
        );
        req["verified_cold_index"]["reuse_unchanged_source_proofs"] = json!(true);
        fs::write(&c.request, req.to_string()).unwrap();
        let journal =
            c.f.root()
                .canonicalize()
                .unwrap()
                .join(".local/indexed-source");
        let r = run(c.f.root(), &c.request, &journal, false, 1001).unwrap();
        assert_eq!(
            r["objects"][0]["source_verification"],
            "prior-verified-source-index-and-current-identity-size-mtime"
        );
        fs::write(c.e(), b"different same-length bytes").unwrap();
        assert!(run(c.f.root(), &c.request, &journal, false, 1001).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn exact_indexed_job_aliases_require_explicit_namespace_and_matching_physical_file() {
        for independent in [false, true] {
            let c = indexed_cold_case();
            let alias = c.f.root().join("fast/jobs/prior/output.mkv");
            fs::create_dir_all(alias.parent().unwrap()).unwrap();
            if independent {
                fs::copy(c.e(), &alias).unwrap();
            } else {
                fs::hard_link(c.e(), &alias).unwrap();
            }
            let path = c.f.root().join("cold-index.json");
            let mut index = crate::load(&path).unwrap();
            index["objects"][&c.h]["source_aliases"] = json!(["jobs/prior/output.mkv"]);
            let raw = serde_json::to_vec(&index).unwrap();
            fs::write(&path, &raw).unwrap();
            let mut req = crate::load(&c.request).unwrap();
            req["verified_cold_index"]["sha256"] = json!(tiering::digest(&raw));
            fs::write(&c.request, req.to_string()).unwrap();
            if !independent {
                assert!(c.run(false).is_err());
            }
            req["source_alias_namespaces"] = json!(["jobs"]);
            fs::write(&c.request, req.to_string()).unwrap();
            let journal =
                c.f.root()
                    .canonicalize()
                    .unwrap()
                    .join(".local/job-alias-apply");
            let result = run(c.f.root(), &c.request, &journal, true, 1001);
            if independent {
                assert!(result.is_err());
                assert!(c.e().is_file() && alias.is_file());
            } else {
                assert_eq!(result.unwrap()["objects"][0]["hot_links_removed"], 2);
                assert!(!c.e().exists() && !alias.exists());
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn verified_cold_index_can_apply_with_independent_retained_copy() {
        let c = indexed_cold_case();
        let result = c.run(true).unwrap();
        assert_eq!(result["cold_custody_registered"], true);
        assert!(!c.e().exists());
        assert!(object(&c.f.root().join("offline-cold"), &c.h).is_file());
    }

    #[cfg(windows)]
    #[test]
    fn cold_custody_precedes_removal_and_survives_repeat_apply() {
        let c = cold_case();
        let archive = c.f.root().join("offline-cold");
        let r = c.run(true).unwrap();
        assert_eq!(r["cold_custody_registered"], true);
        assert!(!c.e().exists());
        assert!(object(&archive, &c.h).is_file());
        let receipt = archive
            .join("snapshots/reel-assets-cold-v1")
            .join(&c.h[..2])
            .join(format!("{}.json", c.h));
        assert_eq!(crate::load(&receipt).unwrap()["sha256"], c.h);
        c.run(true).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_cold_registration_keeps_hot_source() {
        let c = cold_case();
        fs::write(
            c.f.root().join("offline-cold/snapshots"),
            b"not a directory",
        )
        .unwrap();
        assert!(c.run(true).is_err());
        assert!(c.e().is_file());
    }

    #[cfg(windows)]
    #[test]
    fn interrupted_cold_registration_is_recoverable_and_conflicts_hold() {
        let c = cold_case();
        TEST_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(|stage| {
                if stage == "after-cold-registration" {
                    Err("simulated interruption".into())
                } else {
                    Ok(())
                }
            }))
        });
        assert!(c.run(true).is_err());
        TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
        assert!(c.e().is_file());
        c.run(true).unwrap();
        assert!(!c.e().exists());

        let c = cold_case();
        let dir =
            c.f.root()
                .join("offline-cold/snapshots/reel-assets-cold-v1")
                .join(&c.h[..2]);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{}.json", c.h)), b"{}").unwrap();
        assert!(c.run(true).is_err());
        assert!(c.e().is_file());
    }

    #[cfg(windows)]
    #[test]
    fn cold_cleanup_allows_live_jobs_and_unrelated_registration_but_holds_exact_new_input() {
        for same_object in [false, true] {
            let c = cold_case();
            let cfg = config(c.f.root()).unwrap();
            let reservation = admission::reserve(c.f.root(), &cfg, 1).unwrap();
            let root = c.f.root().to_path_buf();
            let hash = if same_object {
                c.h.clone()
            } else {
                "a".repeat(64)
            };
            TEST_HOOK.with(|hook| {
                *hook.borrow_mut() = Some(Box::new(move |stage| {
                    if stage == "before-lock" {
                        fs::write(root.join("registry/objects.tsv"), format!("{hash}\t26\n"))?;
                    }
                    Ok(())
                }))
            });
            let result = c.run(true);
            TEST_HOOK.with(|hook| *hook.borrow_mut() = None);
            if same_object {
                assert!(result.is_err());
                assert!(c.e().exists());
            } else {
                assert!(result.is_ok(), "{result:?}");
                assert!(!c.e().exists());
            }
            drop(reservation);
        }
    }
    #[cfg(windows)]
    #[test]
    fn reviewed_project_controls_exclude_writers_and_reject_new_declarations() {
        let c = Case::new();
        let path = c.f.root().join("manager.json");
        fs::write(&path, b"original owner policy").unwrap();
        let mut v: Value = crate::load(&c.request).unwrap();
        v["source_controls"] = json!({"manager.json":tiering::digest(b"original owner policy")});
        let req: Request = serde_json::from_value(v).unwrap();
        let writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert!(check_source_controls(c.f.root(), &req, true).is_err());
        drop(writer);
        let guards = check_source_controls(c.f.root(), &req, true).unwrap();
        assert!(fs::write(&path, b"new owner policy").is_err());
        assert!(fs::rename(&path, path.with_extension("moved")).is_err());
        drop(guards);
        fs::write(&path, b"new owner policy").unwrap();
        assert!(check_source_controls(c.f.root(), &req, true).is_err());
        assert!(crate::object(&c.f.root().join("fast"), &c.h).exists());
    }
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
    fn explicit_legacy_consumer_exports_and_protects_current_assets_without_setup() {
        let c = Case::new();
        let legacy = c.f.root().join(".local/legacy-consumer");
        fs::create_dir_all(legacy.join("registry")).unwrap();
        let mut state = crate::versions::State::default();
        state
            .register(crate::versions::Version {
                logical_id: "old".into(),
                version_id: "v1".into(),
                manifest_sha256: c.h.clone(),
                payloads: BTreeSet::from([c.h.clone()]),
                dependencies: vec![],
                supersedes: None,
                deprecated: false,
            })
            .unwrap();
        state.select("old", "v1").unwrap();
        fs::write(
            legacy.join("registry/versions.json"),
            serde_json::to_vec(&state).unwrap(),
        )
        .unwrap();
        fs::write(legacy.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
        let mut req = crate::load(&c.request).unwrap();
        req["consumer_roots"]
            .as_array_mut()
            .unwrap()
            .push(json!(legacy));
        req["legacy_consumer_roots"] = json!([legacy]);
        fs::write(&c.request, req.to_string()).unwrap();
        let export = export_protections(c.f.root(), &c.request).unwrap();
        assert!(export["protected_hashes"].get(&c.h).is_some());
        assert!(!legacy.join(".local").exists());
        assert!(
            c.run(false)
                .unwrap_err()
                .to_string()
                .contains("protected by legacy")
        );
        req["legacy_consumer_roots"] = json!([]);
        fs::write(&c.request, req.to_string()).unwrap();
        assert!(export_protections(c.f.root(), &c.request).is_err());
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
        assert_eq!(
            c.run(false).unwrap()["admissions_present_at_completion"],
            true
        );
        assert!(c.run(true).is_err());
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
