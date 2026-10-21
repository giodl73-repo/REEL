//! Read-only, project-scoped accounting. Classifications never authorize removal.
use crate::{Result, admission, artifacts, config, load, object, selected, valid_oid, versions};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path, PathBuf},
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    pub schema: String,
    pub project_id: String,
    pub storage_config: String,
    pub selections: Vec<Selection>,
    pub scan_cas: bool,
    pub max_inventory_objects: usize,
    pub lfs_namespaces: Vec<String>,
    #[serde(default)]
    pub measure_unregistered_physical: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub index: String,
    pub entries: String,
    pub record_field: String,
    pub format: String,
    pub role_prefixes: Vec<String>,
}
pub fn relative(root: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if name.is_empty()
        || name.contains('\\')
        || path
            .components()
            .any(|p| !matches!(p, Component::Normal(_)))
    {
        return Err("Project path must be a normal repository-relative path".into());
    }
    let candidate = root.join(path).canonicalize()?;
    if !candidate.starts_with(root.canonicalize()?) {
        return Err("Project path escapes root".into());
    }
    Ok(candidate)
}
fn bounded(path: &Path) -> Result<Value> {
    if fs::metadata(path)?.len() > 256 * 1024 {
        return Err("Project control exceeds 256 KiB".into());
    }
    load(path)
}
pub fn project(root: &Path) -> Result<Project> {
    let p: Project = serde_json::from_value(bounded(&relative(root, "asset-project.json")?)?)?;
    if p.schema != "reel.asset-project.v1"
        || p.project_id.is_empty()
        || p.max_inventory_objects == 0
        || p.max_inventory_objects > 1_000_000
    {
        return Err("Invalid asset project profile".into());
    }
    relative(root, &p.storage_config)?;
    for namespace in &p.lfs_namespaces {
        if !namespace.starts_with("lfs/")
            || Path::new(namespace)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
            || namespace.contains('\\')
        {
            return Err("Invalid LFS namespace".into());
        }
    }
    Ok(p)
}
fn version_key(lock: &versions::Dependency) -> String {
    format!("{}@{}", lock.logical_id, lock.version_id)
}
pub(crate) fn expand(
    s: &versions::State,
    roots: impl IntoIterator<Item = String>,
) -> Result<BTreeSet<String>> {
    let mut found = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut queue: Vec<_> = roots.into_iter().collect();
    while let Some(k) = queue.pop() {
        if !seen.insert(k.clone()) {
            continue;
        }
        let v = s.versions.get(&k).ok_or("Usage root version missing")?;
        found.insert(v.manifest_sha256.clone());
        found.extend(v.payloads.iter().cloned());
        for dep in &v.dependencies {
            let key = version_key(dep);
            let target = s.versions.get(&key).ok_or("Usage dependency missing")?;
            if target.manifest_sha256 != dep.manifest_sha256 {
                return Err("Usage dependency manifest mismatch".into());
            }
            queue.push(key);
        }
    }
    Ok(found)
}
pub(crate) fn selected_roots(
    root: &Path,
    p: &Project,
    s: &versions::State,
    registered: &std::collections::HashMap<String, u64>,
    cfg: &crate::Config,
) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
    let mut outputs = BTreeSet::new();
    let mut roots = BTreeSet::new();
    for rule in &p.selections {
        if rule.role_prefixes.is_empty() || rule.role_prefixes.iter().any(String::is_empty) {
            return Err("Selection requires nonempty output role prefixes".into());
        }
        let index = bounded(&relative(root, &rule.index)?)?;
        for row in index
            .pointer(&rule.entries)
            .and_then(Value::as_array)
            .ok_or("Selection index is not an array")?
        {
            let Some(record) = row.get(&rule.record_field).and_then(Value::as_str) else {
                continue;
            };
            let path = relative(root, record)?;
            let (assets, lock) = match rule.format.as_str() {
                "artifact" => {
                    let (_, assets) = artifacts::inspect_with(
                        root,
                        Path::new(record),
                        None,
                        false,
                        registered,
                        s,
                        cfg,
                    )?;
                    let doc = bounded(&path)?;
                    let current = doc["current_version"]
                        .as_str()
                        .ok_or("Missing selected artifact version")?;
                    let v = doc["versions"]
                        .as_array()
                        .ok_or("Missing artifact versions")?
                        .iter()
                        .find(|v| v["version_id"] == current)
                        .ok_or("Missing current artifact version")?;
                    (
                        serde_json::to_value(assets)?,
                        serde_json::from_value::<versions::Dependency>(v["version_lock"].clone())?,
                    )
                }
                "bindings" => {
                    let scene = row["scene"]
                        .as_str()
                        .ok_or("Selected bindings need their native scene record")?;
                    let (receipt, _native_assets) =
                        crate::scenes::inspect_with(root, Path::new(scene), registered, s)?;
                    let lock: versions::Dependency =
                        serde_json::from_value(row["version_lock"].clone())?;
                    if s.current.get(&lock.logical_id) != Some(&lock.version_id) {
                        return Err("Stale selected bindings version".into());
                    }
                    versions::payload_closure(s, &lock)?;
                    if receipt["snapshot_sha256"] != lock.manifest_sha256
                        || relative(
                            root,
                            row["bindings"].as_str().ok_or("Missing bindings path")?,
                        )? != path
                    {
                        return Err("Selected bindings differ from native snapshot".into());
                    }
                    (bounded(&path)?["assets"].clone(), lock)
                }
                _ => return Err("Unsupported selection record format".into()),
            };
            let protected = versions::payload_closure(s, &lock)?;
            let mut matches = 0;
            for (role, a) in assets.as_object().ok_or("Missing selected assets")? {
                if rule
                    .role_prefixes
                    .iter()
                    .any(|prefix| role.starts_with(prefix))
                {
                    let hash = a["sha256"].as_str().ok_or("Missing selected output hash")?;
                    if !protected.contains(hash) {
                        return Err("Selected output absent from version closure".into());
                    }
                    outputs.insert(hash.to_owned());
                    matches += 1;
                }
            }
            if matches == 0 {
                return Err(format!("No selected output roles matched {record}").into());
            }
            roots.insert(version_key(&lock));
        }
    }
    Ok((outputs, roots))
}
#[derive(Clone)]
struct Physical {
    id: String,
    allocated: u64,
    links: u64,
}
#[cfg(windows)]
fn physical(path: &Path, volume: &str) -> Result<Physical> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, FILE_STANDARD_INFO, FileStandardInfo, GetFileInformationByHandle,
        GetFileInformationByHandleEx,
    };
    let file = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(path)?;
    let mut id: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    let mut standard: FILE_STANDARD_INFO = unsafe { std::mem::zeroed() };
    // Both POD output buffers and the opened file handle remain alive for the calls.
    unsafe {
        if GetFileInformationByHandle(file.as_raw_handle(), &mut id) == 0
            || GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileStandardInfo,
                (&mut standard as *mut FILE_STANDARD_INFO).cast(),
                std::mem::size_of::<FILE_STANDARD_INFO>() as u32,
            ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    if standard.AllocationSize < 0 {
        return Err("Negative file allocation".into());
    }
    Ok(Physical {
        id: format!("{}:{}:{}", volume, id.nFileIndexHigh, id.nFileIndexLow),
        allocated: standard.AllocationSize as u64,
        links: u64::from(id.nNumberOfLinks),
    })
}
#[cfg(unix)]
fn physical(path: &Path, _volume: &str) -> Result<Physical> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::metadata(path)?;
    Ok(Physical {
        id: format!("{}:{}", m.dev(), m.ino()),
        allocated: m.blocks() * 512,
        links: m.nlink(),
    })
}
fn census(
    cache: &Path,
    hashes: &mut BTreeSet<String>,
    observed: &mut BTreeMap<String, u64>,
    limit: usize,
) -> Result<()> {
    let cas = cache.join("objects/sha256");
    if !cas.is_dir() {
        return Ok(());
    }
    for shard in fs::read_dir(cas)? {
        let shard = shard?;
        let prefix = shard.file_name().to_string_lossy().to_string();
        if prefix.len() != 2
            || !prefix
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || !shard.file_type()?.is_dir()
        {
            continue;
        }
        for entry in fs::read_dir(shard.path())? {
            let entry = entry?;
            let hash = entry.file_name().to_string_lossy().to_string();
            if valid_oid(&hash) && hash.starts_with(&prefix) {
                if !entry.file_type()?.is_file() {
                    return Err("Canonical CAS entry is not a regular file".into());
                }
                observed.insert(hash.clone(), entry.metadata()?.len());
                hashes.insert(hash);
            }
            if hashes.len() > limit {
                return Err("CAS census exceeds project object budget".into());
            }
        }
    }
    Ok(())
}
/// Private detailed report; caller explicitly chooses whether/where to retain it.
pub fn report(root: &Path) -> Result<Value> {
    report_with_progress(root, |_, _| {})
}
pub fn report_with_progress(root: &Path, mut progress: impl FnMut(usize, usize)) -> Result<Value> {
    let census_started_unix_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    use sha2::{Digest, Sha256};
    let profile_bytes = fs::read(root.join("asset-project.json"))?;
    let p = project(root)?;
    let cfg = config(root)?;
    let before = fs::read(root.join("registry/versions.json"))?;
    let state = versions::read(root)?;
    let registered = selected(root)?;
    let (outputs, golden_roots) = selected_roots(root, &p, &state, &registered, &cfg)?;
    let golden = expand(&state, golden_roots)?;
    let current = expand(
        &state,
        state.current.iter().map(|(id, v)| format!("{id}@{v}")),
    )?;
    let pinned = expand(&state, state.pins.iter().cloned())?;
    let leased = expand(
        &state,
        state.leases.values().flat_map(|v| v.iter().cloned()),
    )?;
    let deprecated = expand(
        &state,
        state
            .versions
            .iter()
            .filter(|(_, v)| v.deprecated)
            .map(|(k, _)| k.clone()),
    )?;
    let all = expand(&state, state.versions.keys().cloned())?;
    let noncurrent = expand(
        &state,
        state
            .versions
            .iter()
            .filter(|(_, v)| state.current.get(&v.logical_id) != Some(&v.version_id))
            .map(|(k, _)| k.clone()),
    )?;
    let mut direct_versions: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, v) in &state.versions {
        let mut assets = v.payloads.clone();
        assets.insert(v.manifest_sha256.clone());
        for h in assets {
            direct_versions.entry(h).or_default().push(key.clone());
        }
    }
    let mut controls = BTreeSet::new();
    controls.extend(state.metadata_roots.iter().cloned());
    let raw: Value = serde_json::from_slice(&before)?;
    if let Some(records) = raw["version_records"].as_object() {
        controls.extend(
            records
                .values()
                .filter_map(Value::as_str)
                .map(str::to_owned),
        );
    }
    if root.join("registry/object-index.json").is_file() {
        let a = bounded(&root.join("registry/object-index.json"))?;
        if let Some(h) = a["asset"]["sha256"].as_str() {
            controls.insert(h.to_owned());
        }
    }
    let mut hashes: BTreeSet<_> = registered.keys().cloned().collect();
    if hashes.len() > p.max_inventory_objects {
        return Err("Registered inventory exceeds project object budget".into());
    }
    let mut tiers = Vec::new();
    let mut observed_by_tier = Vec::new();
    for (tier, cache) in cfg.cache_roots.iter().enumerate() {
        let mut observed = BTreeMap::new();
        match fs::metadata(cache) {
            Ok(_) => {
                if p.scan_cas {
                    census(cache, &mut hashes, &mut observed, p.max_inventory_objects)?;
                }
                let total = fs2::total_space(cache)?;
                let free = fs2::available_space(cache)?;
                let floor = cfg
                    .capacity
                    .minimum_free_bytes
                    .max(
                        *cfg.capacity
                            .tier_minimum_free_bytes
                            .get(cache)
                            .unwrap_or(&0),
                    )
                    .max(
                        (u128::from(total) * u128::from(cfg.capacity.minimum_free_basis_points)
                            / 10000) as u64,
                    );
                tiers.push(json!({"tier":tier,"root":cache,"available":true,"volume_id":admission::volume_id(cache)?,"total_bytes":total,"free_bytes":free,"reserve_bytes":floor,"headroom_above_reserve_bytes":free.saturating_sub(floor)}));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                tiers.push(json!({"tier":tier,"root":cache,"available":false}))
            }
            Err(e) => return Err(e.into()),
        }
        observed_by_tier.push(observed);
    }
    let mut rows = Vec::new();
    let mut physical_files: BTreeMap<String, Physical> = BTreeMap::new();
    let mut categories: BTreeMap<String, Value> = BTreeMap::new();
    let mut allocated_by_category: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut allocated_by_tier: BTreeMap<(String, usize), BTreeSet<String>> = BTreeMap::new();
    let mut missing = 0u64;
    let mut mismatches = 0u64;
    let mut duplicate_tier_bytes = 0u64;
    let mut unmeasured_by_class: BTreeMap<String, u64> = BTreeMap::new();
    let mut copy_bytes_by_tier: BTreeMap<(String, usize), u64> = BTreeMap::new();
    let inventory_count = hashes.len();
    progress(0, inventory_count);
    for (position, hash) in hashes.into_iter().enumerate() {
        let expected = registered.get(&hash).copied();
        let class = if controls.contains(&hash) {
            "registry-control"
        } else if outputs.contains(&hash) {
            "selected-output"
        } else if golden.contains(&hash) {
            "selected-version-required"
        } else if current.contains(&hash) {
            "current-working"
        } else if pinned.contains(&hash) || leased.contains(&hash) {
            "pinned-or-leased-history"
        } else if deprecated.contains(&hash) {
            "deprecated-unprotected"
        } else if all.contains(&hash) {
            "retained-noncurrent"
        } else {
            "unclassified"
        };
        let mut copies = Vec::new();
        let mut ids = BTreeSet::new();
        let mut logical_size = expected;
        let mut usable = false;
        for (tier, cache) in cfg.cache_roots.iter().enumerate() {
            if expected.is_none() && !p.measure_unregistered_physical {
                if let Some(bytes) = observed_by_tier[tier].get(&hash) {
                    logical_size = logical_size.or(Some(*bytes));
                    usable = true;
                    copies.push(json!({"tier":tier,"bytes":bytes,"physical_identity_measured":false,"allocated_bytes":null,"hard_links":null}));
                }
                continue;
            }
            let path = object(cache, &hash);
            match fs::symlink_metadata(&path) {
                Ok(meta) => {
                    if !meta.file_type().is_file() {
                        return Err(format!("CAS object is not a regular file: {hash}").into());
                    }
                    let volume = tiers[tier]["volume_id"]
                        .as_str()
                        .ok_or("Missing active volume identity")?;
                    let info = physical(&path, volume)?;
                    let matches = expected.is_none_or(|n| n == meta.len());
                    if !matches {
                        mismatches += 1;
                    } else {
                        usable = true;
                    }
                    logical_size = logical_size.or(Some(meta.len()));
                    if !ids.is_empty() && !ids.contains(&info.id) {
                        duplicate_tier_bytes += info.allocated;
                    }
                    ids.insert(info.id.clone());
                    physical_files
                        .entry(info.id.clone())
                        .or_insert(info.clone());
                    allocated_by_category
                        .entry(class.to_owned())
                        .or_default()
                        .insert(info.id.clone());
                    allocated_by_tier
                        .entry((class.to_owned(), tier))
                        .or_default()
                        .insert(info.id.clone());
                    let mut aliases = 0;
                    for ns in &p.lfs_namespaces {
                        let alias = cache
                            .join(ns)
                            .join("objects")
                            .join(&hash[..2])
                            .join(&hash[2..4])
                            .join(&hash);
                        match fs::symlink_metadata(&alias) {
                            Ok(m) => {
                                if !m.file_type().is_file() {
                                    return Err("LFS object is not regular".into());
                                }
                                let f = physical(&alias, volume)?;
                                if f.id == info.id {
                                    aliases += 1;
                                }
                                physical_files.entry(f.id.clone()).or_insert(f);
                            }
                            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                            Err(e) => return Err(e.into()),
                        }
                    }
                    copies.push(json!({"tier":tier,"bytes":meta.len(),"descriptor_size_matches":matches,"physical_id":info.id,"allocated_bytes":info.allocated,"hard_links":info.links,"known_lfs_hard_links":aliases,"other_hard_links":info.links.saturating_sub(1+aliases)}));
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
        if expected.is_none() && !p.measure_unregistered_physical && !copies.is_empty() {
            *unmeasured_by_class.entry(class.to_owned()).or_default() += 1;
        }
        for copy in &copies {
            *copy_bytes_by_tier
                .entry((class.to_owned(), copy["tier"].as_u64().unwrap() as usize))
                .or_default() += copy["bytes"].as_u64().unwrap();
        }
        if expected.is_some() && !usable {
            missing += 1;
        }
        let entry = categories.entry(class.to_owned()).or_insert(
            json!({"objects":0,"declared_or_observed_logical_bytes":0,"present_objects":0}),
        );
        entry["objects"] = json!(entry["objects"].as_u64().unwrap() + 1);
        entry["declared_or_observed_logical_bytes"] = json!(
            entry["declared_or_observed_logical_bytes"]
                .as_u64()
                .unwrap()
                + logical_size.unwrap_or(0)
        );
        if usable {
            entry["present_objects"] = json!(entry["present_objects"].as_u64().unwrap() + 1);
        }
        rows.push(json!({"sha256":hash,"registered":expected.is_some(),"bytes":logical_size,"category":class,"required_by_current_version":current.contains(&hash),"required_by_selected_version":golden.contains(&hash),"pinned":pinned.contains(&hash),"leased":leased.contains(&hash),"deprecated_reference":deprecated.contains(&hash),"noncurrent_reference":noncurrent.contains(&hash),"direct_version_ids":direct_versions.get(&hash).cloned().unwrap_or_default(),"copies":copies}));
        if (position + 1) % 10000 == 0 || position + 1 == inventory_count {
            progress(position + 1, inventory_count);
        }
    }
    for (class, stats) in &mut categories {
        let ids = allocated_by_category.get(class);
        stats["canonical_unique_allocated_bytes"] = json!(
            ids.map(|ids| ids
                .iter()
                .map(|id| physical_files[id].allocated)
                .sum::<u64>())
                .unwrap_or(0)
        );
        let per_tier: BTreeMap<_, _> = cfg
            .cache_roots
            .iter()
            .enumerate()
            .map(|(tier, _)| {
                (
                    tier.to_string(),
                    allocated_by_tier
                        .get(&(class.clone(), tier))
                        .map(|ids| {
                            ids.iter()
                                .map(|id| physical_files[id].allocated)
                                .sum::<u64>()
                        })
                        .unwrap_or(0),
                )
            })
            .collect();
        stats["measured_canonical_allocated_bytes_by_tier"] = json!(per_tier);
        stats["measured_canonical_unique_allocated_bytes"] =
            stats["canonical_unique_allocated_bytes"].clone();
        stats["allocation_measurement_complete"] = json!(!unmeasured_by_class.contains_key(class));
        if unmeasured_by_class.contains_key(class) {
            stats["canonical_unique_allocated_bytes"] = Value::Null;
        }
        let logical: BTreeMap<_, _> = cfg
            .cache_roots
            .iter()
            .enumerate()
            .map(|(tier, _)| {
                (
                    tier.to_string(),
                    copy_bytes_by_tier
                        .get(&(class.clone(), tier))
                        .copied()
                        .unwrap_or(0),
                )
            })
            .collect();
        stats["observed_copy_bytes_by_tier"] = json!(logical);
    }
    if fs::read(root.join("registry/versions.json"))? != before
        || selected(root)? != registered
        || fs::read(root.join("asset-project.json"))? != profile_bytes
    {
        return Err(
            "Registry changed during accounting; discard this report and run explicitly again"
                .into(),
        );
    }
    let profile_sha256 = format!("{:x}", Sha256::digest(&profile_bytes));
    let overlaps = json!({"current_and_noncurrent_objects":current.intersection(&noncurrent).count(),"current_and_deprecated_objects":current.intersection(&deprecated).count(),"noncurrent_only_objects":noncurrent.difference(&current).count(),"deprecated_only_of_current_objects":deprecated.difference(&current).count()});
    let mut result = json!({"schema":"reel.asset-cache-report.v1","project_id":p.project_id,"registry_generation":state.generation,"scope":if p.scan_cas{"registered plus canonical active-cache CAS census"}else{"registered only"},"archive_probed":false,"payload_hashes_verified":false,"payload_bytes_copied":0,"media_deleted":false,"removal_authorized":false,"creative_approval_inferred":false,"coverage_complete":state.coverage_complete,"safe_reclaimable_bytes":null,"reclaimability_reason":"Project usage is not global consumer or backup proof; hard links and other cache content may retain storage. No deletion is authorized.","categories":categories,"registered_objects":registered.len(),"inventoried_objects":rows.len(),"registered_objects_missing_or_wrong_size":missing,"wrong_size_copies":mismatches,"observed_unique_physical_files":physical_files.len(),"observed_unique_allocated_bytes":physical_files.values().map(|f|f.allocated).sum::<u64>(),"extra_canonical_tier_copy_allocated_bytes":duplicate_tier_bytes,"tiers":tiers,"objects":rows,"limits":"Canonical CAS and named LFS object paths only; payload statistics are metadata-only, not byte integrity or full-volume attribution. Logical categories are exclusive; reference flags overlap."});
    result["profile_sha256"] = json!(profile_sha256);
    result["census_started_unix_seconds"] = json!(census_started_unix_seconds);
    result["census_completed_unix_seconds"] = json!(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs()
    );
    result["overlaps"] = overlaps;
    result["unmeasured_physical_objects"] = json!(unmeasured_by_class.values().sum::<u64>());
    result["unregistered_physical_measurement_enabled"] = json!(p.measure_unregistered_physical);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    struct Fixture {
        dir: tempfile::TempDir,
        e: PathBuf,
        d: PathBuf,
        m: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().canonicalize().unwrap();
            for name in [".local", "registry", "E", "D", "M"] {
                fs::create_dir(root.join(name)).unwrap();
            }
            let e = root.join("E");
            let d = root.join("D");
            let m = root.join("M");
            fs::write(root.join(".local/storage-rust.json"),json!({"cache_roots":[e,d],"archive_roots":[m],"capacity":{"minimum_free_bytes":0,"minimum_free_basis_points":0}}).to_string()).unwrap();
            fs::write(root.join("catalog.json"), r#"{"videos":[]}"#).unwrap();
            fs::write(
                root.join("registry/versions.json"),
                serde_json::to_vec(&versions::State::default()).unwrap(),
            )
            .unwrap();
            fs::write(root.join("asset-project.json"),json!({"schema":"reel.asset-project.v1","project_id":"synthetic","storage_config":".local/storage-rust.json","selections":[],"scan_cas":true,"max_inventory_objects":100,"lfs_namespaces":["lfs/test"]}).to_string()).unwrap();
            Self { dir, e, d, m }
        }
        fn root(&self) -> PathBuf {
            self.dir.path().canonicalize().unwrap()
        }
        fn put(&self, cache: &Path, bytes: &[u8], register: bool) -> String {
            let hash = format!("{:x}", Sha256::digest(bytes));
            let dest = object(cache, &hash);
            fs::create_dir_all(dest.parent().unwrap()).unwrap();
            fs::write(dest, bytes).unwrap();
            if register {
                use std::io::Write;
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(self.root().join("registry/objects.tsv"))
                    .unwrap();
                writeln!(file, "{hash}\t{}", bytes.len()).unwrap();
            }
            hash
        }
        fn state(&self, s: &versions::State) {
            fs::write(
                self.root().join("registry/versions.json"),
                serde_json::to_vec(s).unwrap(),
            )
            .unwrap();
        }
    }
    fn version(id: &str, v: &str, h: &str) -> versions::Version {
        versions::Version {
            logical_id: id.into(),
            version_id: v.into(),
            manifest_sha256: h.into(),
            payloads: BTreeSet::from([h.into()]),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
        }
    }
    fn row<'a>(r: &'a Value, h: &str) -> &'a Value {
        r["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["sha256"] == h)
            .unwrap()
    }
    #[test]
    fn unknown_cas_and_archive_are_not_misclassified_as_disposable() {
        let f = Fixture::new();
        let e = f.put(&f.e, b"registered", true);
        let unknown = f.put(&f.d, b"unknown", false);
        let archive = f.put(&f.m, b"archive-only", false);
        let before = fs::read(f.root().join("registry/versions.json")).unwrap();
        let r = report(&f.root()).unwrap();
        assert_eq!(r["inventoried_objects"], 2);
        assert_eq!(row(&r, &e)["category"], "unclassified");
        assert_eq!(row(&r, &unknown)["registered"], false);
        assert!(
            !r["objects"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["sha256"] == archive)
        );
        assert_eq!(r["archive_probed"], false);
        assert_eq!(r["removal_authorized"], false);
        assert!(r["safe_reclaimable_bytes"].is_null());
        assert_eq!(
            fs::read(f.root().join("registry/versions.json")).unwrap(),
            before
        );
    }
    #[test]
    fn unregistered_allocation_is_unknown_until_explicit_deep_measurement() {
        let f = Fixture::new();
        let h = f.put(&f.e, b"unregistered", false);
        let normal = report(&f.root()).unwrap();
        assert_eq!(normal["unmeasured_physical_objects"], 1);
        assert!(normal["categories"]["unclassified"]["canonical_unique_allocated_bytes"].is_null());
        assert_eq!(
            row(&normal, &h)["copies"][0]["physical_identity_measured"],
            false
        );
        let path = f.root().join("asset-project.json");
        let mut p = load(&path).unwrap();
        p["measure_unregistered_physical"] = json!(true);
        fs::write(path, p.to_string()).unwrap();
        let deep = report(&f.root()).unwrap();
        assert_eq!(deep["unmeasured_physical_objects"], 0);
        assert_eq!(deep["observed_unique_physical_files"], 1);
        assert_eq!(
            deep["categories"]["unclassified"]["allocation_measurement_complete"],
            true
        );
        assert!(deep["safe_reclaimable_bytes"].is_null());
    }
    #[test]
    fn shared_current_old_and_deprecated_payloads_keep_overlapping_flags() {
        let f = Fixture::new();
        let h = f.put(&f.e, b"shared", true);
        let onlyold = f.put(&f.d, b"old", true);
        let mut s = versions::State::default();
        s.register(version("item", "v1", &h)).unwrap();
        s.versions
            .get_mut("item@v1")
            .unwrap()
            .payloads
            .insert(onlyold.clone());
        s.deprecate("item", "v1").unwrap();
        s.register(version("item", "v2", &h)).unwrap();
        s.select("item", "v2").unwrap();
        f.state(&s);
        let r = report(&f.root()).unwrap();
        assert_eq!(row(&r, &h)["category"], "current-working");
        assert_eq!(row(&r, &h)["deprecated_reference"], true);
        assert_eq!(row(&r, &h)["noncurrent_reference"], true);
        assert_eq!(row(&r, &onlyold)["category"], "deprecated-unprotected");
        s.pins.insert("item@v1".into());
        f.state(&s);
        let r = report(&f.root()).unwrap();
        assert_eq!(row(&r, &onlyold)["category"], "pinned-or-leased-history");
    }
    #[test]
    fn hard_links_are_one_physical_file_even_across_configured_cache_aliases() {
        let f = Fixture::new();
        let h = f.put(&f.e, b"linked", true);
        let source = object(&f.e, &h);
        let second = object(&f.d, &h);
        fs::create_dir_all(second.parent().unwrap()).unwrap();
        fs::hard_link(&source, second).unwrap();
        let alias =
            f.e.join("lfs/test/objects")
                .join(&h[..2])
                .join(&h[2..4])
                .join(&h);
        fs::create_dir_all(alias.parent().unwrap()).unwrap();
        fs::hard_link(source, alias).unwrap();
        let r = report(&f.root()).unwrap();
        assert_eq!(r["observed_unique_physical_files"], 1);
        assert_eq!(r["extra_canonical_tier_copy_allocated_bytes"], 0);
        assert_eq!(row(&r, &h)["copies"][0]["known_lfs_hard_links"], 1);
        assert_eq!(row(&r, &h)["copies"][0]["hard_links"], 3);
    }
    #[test]
    fn independent_copies_and_wrong_size_are_reported_without_mutation() {
        let f = Fixture::new();
        let h = f.put(&f.e, b"data", true);
        let d = object(&f.d, &h);
        fs::create_dir_all(d.parent().unwrap()).unwrap();
        fs::write(&d, b"wrong size").unwrap();
        let r = report(&f.root()).unwrap();
        assert_eq!(r["wrong_size_copies"], 1);
        assert_eq!(r["registered_objects_missing_or_wrong_size"], 0);
        assert!(
            r["extra_canonical_tier_copy_allocated_bytes"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(fs::read(d).unwrap(), b"wrong size");
    }
    #[test]
    fn absent_registered_asset_is_not_an_archive_recovery_request() {
        let f = Fixture::new();
        let h = f.put(&f.m, b"missing", true);
        let r = report(&f.root()).unwrap();
        assert_eq!(r["registered_objects_missing_or_wrong_size"], 1);
        assert_eq!(row(&r, &h)["copies"], json!([]));
    }
    #[test]
    fn output_profile_marks_only_declared_roles_and_keeps_required_inputs() {
        let f = Fixture::new();
        let out = f.put(&f.e, b"selected", true);
        let input = f.put(&f.e, b"input", true);
        let mut s = versions::State::default();
        let scene = json!({"schema":"reel.scene-authoring.v2","scene_id":"test","authoring_state":"imported-evidence"});
        let bindings = json!({"schema":"reel.scene-asset-bindings.v1","scope_id":"test","assets":{"delivery/movie":{"sha256":out,"bytes":8,"cache_uri":format!("cache://sha256/{out}"),"logical_id":"movie","selection_state":"selected-private"},"input/cel":{"sha256":input,"bytes":5,"cache_uri":format!("cache://sha256/{input}"),"logical_id":"cel","selection_state":"retained"}}});
        let manifest = f.put(
            &f.e,
            &crate::scenes::snapshot(&scene, &bindings).unwrap(),
            true,
        );
        let mut v = version("scene/test", "v1", &manifest);
        v.payloads.insert(out.clone());
        v.payloads.insert(input.clone());
        s.register(v).unwrap();
        s.select("scene/test", "v1").unwrap();
        f.state(&s);
        let lock = json!({"logical_id":"scene/test","version_id":"v1","manifest_sha256":manifest});
        let record = json!({"record":"bindings.json","bindings":"bindings.json","scene":"scene.json","version_lock":lock});
        fs::write(
            f.root().join("catalog.json"),
            json!({"videos":[record]}).to_string(),
        )
        .unwrap();
        fs::write(f.root().join("scene.json"), scene.to_string()).unwrap();
        fs::write(
            f.root().join("index.json"),
            json!({"items":[record]}).to_string(),
        )
        .unwrap();
        fs::write(f.root().join("bindings.json"), bindings.to_string()).unwrap();
        let p = f.root().join("asset-project.json");
        let mut doc = load(&p).unwrap();
        doc["selections"] = json!([{"index":"index.json","entries":"/items","record_field":"record","format":"bindings","role_prefixes":["delivery/"]}]);
        fs::write(p, doc.to_string()).unwrap();
        let r = report(&f.root()).unwrap();
        assert_eq!(row(&r, &out)["category"], "selected-output");
        assert_eq!(row(&r, &input)["category"], "selected-version-required");
        let mut changed = bindings;
        changed["assets"]["delivery/movie"]["selection_state"] =
            json!("edited-without-new-version");
        fs::write(f.root().join("bindings.json"), changed.to_string()).unwrap();
        assert!(report(&f.root()).is_err());
    }
    #[test]
    fn profile_escape_and_inventory_budget_fail_closed() {
        let f = Fixture::new();
        let p = f.root().join("asset-project.json");
        let mut doc = load(&p).unwrap();
        doc["storage_config"] = json!("../outside.json");
        fs::write(&p, doc.to_string()).unwrap();
        assert!(report(&f.root()).is_err());
        doc["storage_config"] = json!(".local/storage-rust.json");
        doc["max_inventory_objects"] = json!(1);
        fs::write(p, doc.to_string()).unwrap();
        f.put(&f.e, b"one", false);
        f.put(&f.e, b"two", false);
        assert!(report(&f.root()).is_err());
    }
    #[test]
    fn mismatched_dependency_hash_is_rejected() {
        let mut s = versions::State::default();
        let h = "a".repeat(64);
        s.register(version("a", "v1", &h)).unwrap();
        s.register(version("b", "v1", &h)).unwrap();
        s.versions
            .get_mut("b@v1")
            .unwrap()
            .dependencies
            .push(versions::Dependency {
                logical_id: "a".into(),
                version_id: "v1".into(),
                manifest_sha256: "b".repeat(64),
            });
        assert!(expand(&s, ["b@v1".into()]).is_err());
    }
}
