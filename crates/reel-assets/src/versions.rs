//! Immutable version locks and read-only retention decisions. No deletion API.
use crate::{Result, admission, config, load, valid_oid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub logical_id: String,
    pub version_id: String,
    pub manifest_sha256: String,
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DependencyContract {
    pub primary_inputs: BTreeSet<String>,
    pub outputs: BTreeSet<String>,
    /// Provenance/history/approval records are retained, but are not build inputs.
    pub references: BTreeSet<String>,
}
#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Version {
    pub logical_id: String,
    pub version_id: String,
    pub manifest_sha256: String,
    pub payloads: BTreeSet<String>,
    #[serde(default)]
    pub dependencies: Vec<Dependency>,
    #[serde(default)]
    pub supersedes: Option<String>,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dependency_contract: Option<DependencyContract>,
}
impl Version {
    /// Legacy rows remain conservatively protected until explicitly reclassified.
    pub fn active_payloads(&self) -> BTreeSet<String> {
        self.dependency_contract.as_ref().map_or_else(
            || self.payloads.clone(),
            |c| c.primary_inputs.union(&c.outputs).cloned().collect(),
        )
    }
}
/// The active verification scope of a classified version. Legacy callers keep
/// their declared-asset verification behavior; no archive reads occur here.
pub(crate) fn verification_payloads(
    state: &State,
    lock: &Dependency,
    registered: &std::collections::HashMap<String, u64>,
) -> Result<Option<BTreeMap<String, u64>>> {
    let key = format!("{}@{}", lock.logical_id, lock.version_id);
    let version = state
        .versions
        .get(&key)
        .ok_or("Verification version missing")?;
    if version.manifest_sha256 != lock.manifest_sha256 {
        return Err("Verification manifest lock mismatch".into());
    }
    if version.dependency_contract.is_none() {
        return Ok(None);
    }
    Ok(Some(
        crate::accounting::expand(state, [key])?
            .into_iter()
            .map(|hash| {
                let bytes = registered
                    .get(&hash)
                    .ok_or("Unregistered active artifact dependency")?;
                Ok((hash, *bytes))
            })
            .collect::<Result<_>>()?,
    ))
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    #[serde(skip)]
    pub metadata_roots: BTreeSet<String>,
    pub generation: u64,
    pub versions: BTreeMap<String, Version>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub version_records: BTreeMap<String, String>,
    pub current: BTreeMap<String, String>,
    pub pins: BTreeSet<String>,
    pub leases: BTreeMap<String, BTreeSet<String>>,
    pub coverage_complete: bool,
}
fn key(id: &str, version: &str) -> String {
    format!("{id}@{version}")
}
impl State {
    pub fn validate_dependency_contract(&self, version: &Version) -> Result<()> {
        let mut queue = vec![version];
        let mut seen = BTreeSet::new();
        while let Some(version) = queue.pop() {
            if !seen.insert(key(&version.logical_id, &version.version_id)) {
                continue;
            }
            let Some(contract) = &version.dependency_contract else {
                continue;
            };
            let classified: BTreeSet<_> = contract
                .primary_inputs
                .iter()
                .chain(&contract.outputs)
                .chain(&contract.references)
                .cloned()
                .collect();
            let mut payloads = version.payloads.clone();
            payloads.remove(&version.manifest_sha256);
            let mut classified_without_manifest = classified.clone();
            classified_without_manifest.remove(&version.manifest_sha256);
            if classified.iter().any(|h| !valid_oid(h))
                || payloads != classified_without_manifest
                || !contract.primary_inputs.is_disjoint(&contract.outputs)
                || !contract.primary_inputs.is_disjoint(&contract.references)
                || !contract.outputs.is_disjoint(&contract.references)
            {
                return Err(
                    "Every payload must have exactly one primary-input, output or reference role"
                        .into(),
                );
            }
            for dep in &version.dependencies {
                let target = self
                    .versions
                    .get(&key(&dep.logical_id, &dep.version_id))
                    .ok_or("Dependency version missing")?;
                let inputs = target.dependency_contract.as_ref().ok_or(
                    "Primary-input dependency cannot target an unclassified legacy version",
                )?;
                if target.manifest_sha256 != dep.manifest_sha256 {
                    return Err("Dependency manifest hash mismatch".into());
                }
                if !inputs.outputs.is_empty() {
                    return Err("Compiled/output versions cannot be dependencies; use primary inputs and retain output references separately".into());
                }
                queue.push(target);
            }
            for existing in self
                .versions
                .values()
                .filter_map(|v| v.dependency_contract.as_ref())
            {
                if !contract.primary_inputs.is_disjoint(&existing.outputs)
                    || !contract.outputs.is_disjoint(&existing.primary_inputs)
                {
                    return Err(
                        "A declared output cannot also be registered as a primary input".into(),
                    );
                }
            }
        }
        Ok(())
    }
    pub fn register(&mut self, version: Version) -> Result<()> {
        if version.logical_id.is_empty()
            || version.version_id.is_empty()
            || !valid_oid(&version.manifest_sha256)
            || version.payloads.iter().any(|h| !valid_oid(h))
        {
            return Err("Invalid immutable version".into());
        }
        let k = key(&version.logical_id, &version.version_id);
        if self.versions.contains_key(&k) {
            return Err("Immutable version already registered".into());
        }
        for dep in &version.dependencies {
            let existing = self
                .versions
                .get(&key(&dep.logical_id, &dep.version_id))
                .ok_or("Dependency version missing")?;
            if existing.manifest_sha256 != dep.manifest_sha256 {
                return Err("Dependency manifest hash mismatch".into());
            }
        }
        if let Some(prior) = &version.supersedes
            && !self.versions.contains_key(&key(&version.logical_id, prior))
        {
            return Err("Superseded version missing".into());
        }
        self.validate_dependency_contract(&version)?;
        self.versions.insert(k, version);
        self.generation += 1;
        Ok(())
    }
    pub fn select(&mut self, id: &str, version: &str) -> Result<()> {
        let v = self
            .versions
            .get(&key(id, version))
            .ok_or("Version missing")?;
        if v.deprecated {
            return Err("Cannot select deprecated version for new use".into());
        }
        self.validate_dependency_contract(v)?;
        self.current.insert(id.into(), version.into());
        self.generation += 1;
        Ok(())
    }
    pub fn deprecate(&mut self, id: &str, version: &str) -> Result<()> {
        self.versions
            .get_mut(&key(id, version))
            .ok_or("Version missing")?
            .deprecated = true;
        self.generation += 1;
        Ok(())
    }
    pub fn unselect(&mut self, id: &str, expected: &str) -> Result<()> {
        if self.current.get(id).is_none_or(|v| v != expected) {
            return Err("Current selection changed; refusing stale withdrawal".into());
        }
        self.current.remove(id);
        self.generation += 1;
        Ok(())
    }
    pub fn lease(&mut self, owner: &str, versions: BTreeSet<String>) -> Result<()> {
        if owner.is_empty() || versions.iter().any(|v| !self.versions.contains_key(v)) {
            return Err("Invalid lease".into());
        }
        self.leases.insert(owner.into(), versions);
        self.generation += 1;
        Ok(())
    }
    pub fn release(&mut self, owner: &str) -> Result<()> {
        self.leases.remove(owner).ok_or("Lease missing")?;
        self.generation += 1;
        Ok(())
    }
    pub(crate) fn closure(&self, roots: impl Iterator<Item = String>) -> Result<BTreeSet<String>> {
        let mut found = BTreeSet::new();
        let mut queue: Vec<_> = roots.collect();
        while let Some(k) = queue.pop() {
            if !found.insert(k.clone()) {
                continue;
            }
            let v = self.versions.get(&k).ok_or("Protected version missing")?;
            queue.extend(
                v.dependencies
                    .iter()
                    .map(|d| key(&d.logical_id, &d.version_id)),
            );
        }
        Ok(found)
    }
    pub fn retirement_plan(&self, id: &str, version: &str) -> Result<serde_json::Value> {
        let target = key(id, version);
        let v = self.versions.get(&target).ok_or("Version missing")?;
        let protected = self.closure(
            self.current
                .iter()
                .map(|(id, v)| key(id, v))
                .chain(self.pins.iter().cloned())
                .chain(self.leases.values().flat_map(|v| v.iter().cloned())),
        )?;
        let protected_hashes: BTreeSet<_> = protected
            .iter()
            .flat_map(|k| self.versions[k].payloads.iter().cloned())
            .chain(self.metadata_roots.iter().cloned())
            .collect();
        let shared: Vec<_> = v.payloads.intersection(&protected_hashes).collect();
        Ok(
            serde_json::json!({"generation":self.generation,"target":target,"deprecated":v.deprecated,"protected":protected.contains(&target),"shared_protected_hashes":shared,"coverage_complete":self.coverage_complete,"eligible_for_archive_assessment":v.deprecated && !protected.contains(&target) && self.coverage_complete,"removal_authorized":false,"reason":"Dry-run only. Independent recovery, physical identity and exact execution revalidation are required; no deletion API exists."}),
        )
    }
    pub fn validate_plan(&self, plan: &serde_json::Value) -> Result<()> {
        if plan["generation"].as_u64() != Some(self.generation) {
            return Err("Stale retirement plan: selections, pins or leases changed".into());
        }
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Index {
    schema: String,
    base_sha256: String,
    base_bytes: u64,
    patch: serde_json::Value,
}

// Cached metadata can grow independently of the small Git control/patch file.
const MAX_EXPANDED_INDEX_BYTES: u64 = 4 * 1024 * 1024;

fn baseline(
    root: &Path,
    index: &Index,
    storage: Option<&crate::Config>,
) -> Result<serde_json::Value> {
    if index.schema != "caminos.version-index.v2"
        || !valid_oid(&index.base_sha256)
        || index.base_bytes > MAX_EXPANDED_INDEX_BYTES
        || !index.patch.is_object()
    {
        return Err("Invalid version-index baseline".into());
    }
    let registered = crate::generic_objects_with(root, storage)?;
    if registered.get(&index.base_sha256) != Some(&index.base_bytes) {
        return Err("Version-index baseline is not authorized".into());
    }
    let cfg = if let Some(c) = storage {
        c.clone()
    } else {
        config(root)?
    };
    let path = crate::active_existing(&cfg, &index.base_sha256, index.base_bytes)?
        .ok_or("Version baseline absent from active caches; archive not probed")?;
    let value = load(&path)?;
    let _: State = serde_json::from_value(value.clone())?; // A single typed base, never a recursive import.
    Ok(value)
}
fn merge(value: &mut serde_json::Value, patch: &serde_json::Value) {
    if let Some(changes) = patch.as_object() {
        if !value.is_object() {
            *value = serde_json::json!({});
        }
        let target = value.as_object_mut().unwrap();
        for (key, change) in changes {
            if change.is_null() {
                target.remove(key);
            } else {
                merge(
                    target.entry(key.clone()).or_insert(serde_json::Value::Null),
                    change,
                );
            }
        }
    } else {
        *value = patch.clone();
    }
}
fn difference(old: &serde_json::Value, new: &serde_json::Value) -> serde_json::Value {
    if let (Some(a), Some(b)) = (old.as_object(), new.as_object()) {
        let mut out = serde_json::Map::new();
        for (key, value) in b {
            if a.get(key) != Some(value) {
                out.insert(
                    key.clone(),
                    difference(a.get(key).unwrap_or(&serde_json::Value::Null), value),
                );
            }
        }
        for key in a.keys().filter(|k| !b.contains_key(*k)) {
            out.insert(key.clone(), serde_json::Value::Null);
        }
        serde_json::Value::Object(out)
    } else {
        new.clone()
    }
}
fn control(
    root: &Path,
    value: serde_json::Value,
    storage: Option<&crate::Config>,
) -> Result<(serde_json::Value, Option<String>)> {
    if value.get("schema").is_none() {
        return Ok((value, None));
    }
    let index: Index = serde_json::from_value(value)?;
    let mut base = baseline(root, &index, storage)?;
    merge(&mut base, &index.patch);
    if serde_json::to_vec(&base)?.len() as u64 > MAX_EXPANDED_INDEX_BYTES {
        return Err("Expanded version index exceeds budget".into());
    }
    let _: State = serde_json::from_value(base.clone())?;
    Ok((base, Some(index.base_sha256)))
}
fn encode_control(root: &Path, control: serde_json::Value) -> Result<Vec<u8>> {
    let path = root.join("registry/versions.json");
    let previous = if path.exists() {
        load(&path)?
    } else {
        serde_json::Value::Null
    };
    let bytes = if previous.get("schema").is_some() {
        let mut index: Index = serde_json::from_value(previous)?;
        index.patch = difference(&baseline(root, &index, None)?, &control);
        serde_json::to_vec(&index)?
    } else {
        serde_json::to_vec(&control)?
    };
    if bytes.len() > 256 * 1024 {
        return Err("Version control index exceeds budget".into());
    }
    Ok(bytes)
}

/// Explicit representation-only compaction. All immutable versions and selections stay identical.
pub fn compact(root: &Path) -> Result<serde_json::Value> {
    let path = root.join("registry/versions.json");
    let before = fs::read(&path)?;
    if before.len() > 256 * 1024 {
        return Err("Version control index exceeds budget".into());
    }
    let old = read(root)?;
    let (expanded, _) = control(root, serde_json::from_slice(&before)?, None)?;
    let bytes = serde_json::to_vec(&expanded)?;
    let local = root.join(".local/version-index-staging");
    fs::create_dir_all(&local)?;
    let stage = tempfile_path(&local.join("base.json"));
    fs::write(&stage, &bytes)?;
    let asset = crate::ingest(root, &stage)?;
    let manifest = tempfile_path(&local.join("registration.json"));
    fs::write(
        &manifest,
        serde_json::to_vec(
            &serde_json::json!({"assets":[{"sha256":asset["sha256"],"bytes":asset["bytes"],"provenance":"Explicit immutable version-index baseline; locks, pins and selections unchanged","rights":"Private technical metadata; no creative/publication authority","selection_state":"retained-control-metadata"}]}),
        )?,
    )?;
    crate::registry::register(root, &manifest)?;
    let _lock = admission::worktree_lock(root)?;
    if fs::read(&path)? != before {
        return Err(
            "Version index changed during compaction; verified baseline retained, index unchanged"
                .into(),
        );
    }
    let index = Index {
        schema: "caminos.version-index.v2".into(),
        base_sha256: asset["sha256"]
            .as_str()
            .ok_or("Missing baseline hash")?
            .into(),
        base_bytes: bytes.len() as u64,
        patch: serde_json::json!({}),
    };
    baseline(root, &index, None)?;
    let encoded = serde_json::to_vec(&index)?;
    let tmp = tempfile_path(&path);
    let mut file = fs::File::create(&tmp)?;
    file.write_all(&encoded)?;
    file.sync_all()?;
    drop(file);
    replace(&tmp, &path)?;
    Ok(
        serde_json::json!({"baseline":asset,"index_bytes_before":before.len(),"index_bytes_after":encoded.len(),"generation":old.generation,"versions":old.versions.len(),"selections":old.current.len(),"archive_probed":false,"media_deleted":false}),
    )
}

pub fn read(root: &Path) -> Result<State> {
    read_with(root, None)
}
pub(crate) fn read_with(root: &Path, storage: Option<&crate::Config>) -> Result<State> {
    let path = root.join("registry/versions.json");
    if !path.exists() {
        return Ok(State::default());
    }
    if fs::metadata(&path)?.len() > 256 * 1024 {
        return Err("Version control file exceeds budget".into());
    }
    let (control, base) = control(root, load(&path)?, storage)?;
    let mut state: State = serde_json::from_value(control)?;
    state.metadata_roots.extend(base);
    let mut registered = None;
    for (key, oid) in std::mem::take(&mut state.version_records) {
        if !valid_oid(&oid) {
            return Err("Invalid version record hash".into());
        }
        let mut path = root
            .join("registry/version-records")
            .join(&oid[..2])
            .join(format!("{oid}.json"));
        match fs::symlink_metadata(&path) {
            Ok(meta) if !meta.file_type().is_file() => {
                return Err("Version record must be a regular file".into());
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if registered.is_none() {
                    registered = Some(crate::generic_objects_with(root, storage)?);
                }
                let index = registered.as_ref().unwrap();
                let size = *index.get(&oid).ok_or("Version record is not registered")?;
                if size > 256 * 1024 {
                    return Err("Version record exceeds budget".into());
                }
                let cfg = if let Some(c) = storage {
                    c.clone()
                } else {
                    config(root)?
                };
                path = crate::active_existing(&cfg, &oid, size)?
                    .ok_or("Version record absent from active caches; archive not probed")?;
            }
            Err(e) => return Err(e.into()),
        }
        if fs::metadata(&path)?.len() > 256 * 1024 {
            return Err("Version record exceeds budget".into());
        }
        let bytes = fs::read(path)?;
        if format!("{:x}", Sha256::digest(&bytes)) != oid {
            return Err("Version record hash mismatch".into());
        }
        let version: Version = serde_json::from_slice(&bytes)?;
        if key != format!("{}@{}", version.logical_id, version.version_id)
            || state.versions.insert(key, version).is_some()
        {
            return Err("Duplicate or mismatched version record identity".into());
        }
        state.metadata_roots.insert(oid);
    }
    Ok(state)
}

fn encode(root: &Path, state: &State) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(state)?;
    if bytes.len() <= 128 * 1024 {
        return encode_control(root, serde_json::from_slice(&bytes)?);
    }
    let mut control = serde_json::to_value(state)?;
    let mut records = BTreeMap::new();
    let registered = crate::generic_objects(root)?;
    // Publish immutable records first. The atomic control replacement below
    // is the sole commit point; interruption cannot retarget the old state.
    for (key, version) in &state.versions {
        let bytes = serde_json::to_vec(version)?;
        if bytes.len() > 256 * 1024 {
            return Err("Version record exceeds budget".into());
        }
        let oid = format!("{:x}", Sha256::digest(&bytes));
        // A deliberately cache-registered immutable record stays outside Git.
        // New small records remain local until explicitly admitted to cache/LFS.
        if let Some(size) = registered.get(&oid) {
            if *size != bytes.len() as u64
                || crate::active_existing(&config(root)?, &oid, *size)?.is_none()
            {
                return Err("Registered version record unavailable or wrong size".into());
            }
            records.insert(key, oid);
            continue;
        }
        let path = root
            .join("registry/version-records")
            .join(&oid[..2])
            .join(format!("{oid}.json"));
        fs::create_dir_all(path.parent().unwrap())?;
        match fs::read(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let tmp = tempfile_path(&path);
                let mut file = fs::File::create(&tmp)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                drop(file);
                replace(&tmp, &path)?;
            }
            Ok(previous) => {
                if previous != bytes {
                    return Err("Existing immutable version record differs".into());
                }
            }
            Err(e) => return Err(e.into()),
        }
        records.insert(key, oid);
    }
    control["versions"] = serde_json::json!({});
    control["version_records"] = serde_json::to_value(records)?;
    let bytes = encode_control(root, control)?;
    if bytes.len() > 256 * 1024 {
        return Err("Version control index exceeds budget".into());
    }
    Ok(bytes)
}

pub fn payload_closure(state: &State, dependency: &Dependency) -> Result<BTreeSet<String>> {
    fn visit(
        state: &State,
        dep: &Dependency,
        seen: &mut BTreeSet<String>,
        hashes: &mut BTreeSet<String>,
    ) -> Result<()> {
        let key = format!("{}@{}", dep.logical_id, dep.version_id);
        let version = state
            .versions
            .get(&key)
            .ok_or("Missing immutable dependency")?;
        if version.manifest_sha256 != dep.manifest_sha256 {
            return Err("Dependency manifest differs".into());
        }
        if !seen.insert(key) {
            return Ok(());
        }
        hashes.insert(version.manifest_sha256.clone());
        hashes.extend(version.payloads.iter().cloned());
        for child in &version.dependencies {
            visit(state, child, seen, hashes)?;
        }
        Ok(())
    }
    let mut hashes = BTreeSet::new();
    visit(state, dependency, &mut BTreeSet::new(), &mut hashes)?;
    Ok(hashes)
}

pub fn transact<T>(root: &Path, operation: impl FnOnce(&mut State) -> Result<T>) -> Result<T> {
    let _lock = admission::worktree_lock(root)?;
    let path = root.join("registry/versions.json");
    let mut state = read(root)?;
    let result = operation(&mut state)?;
    fs::create_dir_all(path.parent().ok_or("Invalid state path")?)?;
    // Persist a complete replacement while holding the worktree metadata lock.
    let tmp = tempfile_path(&path);
    let mut staged = fs::File::create(&tmp)?;
    staged.write_all(&encode(root, &state)?)?;
    staged.sync_all()?;
    drop(staged);
    // On Windows rename cannot overwrite: retain the prior small metadata file
    // on failure rather than altering any media. File replacement is atomic via
    // MoveFileExW in the shared metadata helper below.
    replace(&tmp, &path)?;
    Ok(result)
}
fn tempfile_path(path: &Path) -> std::path::PathBuf {
    crate::unique(path.parent().unwrap(), "state")
}
#[cfg(windows)]
pub fn replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let f: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let t: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    if unsafe {
        MoveFileExW(
            f.as_ptr(),
            t.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn version(id: &str, v: &str, h: char) -> Version {
        Version {
            logical_id: id.into(),
            version_id: v.into(),
            manifest_sha256: h.to_string().repeat(64),
            payloads: BTreeSet::from(["a".repeat(64)]),
            dependencies: vec![],
            supersedes: None,
            deprecated: false,
            dependency_contract: None,
        }
    }
    #[test]
    fn immutable_versions_deprecation_and_new_leases_invalidate_dry_run() {
        let mut s = State::default();
        s.register(version("cel", "v1", 'b')).unwrap();
        s.select("cel", "v1").unwrap();
        s.deprecate("cel", "v1").unwrap();
        assert_eq!(s.current["cel"], "v1");
        assert!(s.register(version("cel", "v1", 'c')).is_err());
        let p = s.retirement_plan("cel", "v1").unwrap();
        assert_eq!(p["protected"], true);
        s.lease("editor", BTreeSet::from(["cel@v1".into()]))
            .unwrap();
        assert!(s.validate_plan(&p).is_err());
        assert_eq!(
            s.retirement_plan("cel", "v1").unwrap()["removal_authorized"],
            false
        );
        assert!(s.unselect("cel", "stale").is_err());
        assert_eq!(s.current["cel"], "v1");
        s.unselect("cel", "v1").unwrap();
        assert!(!s.current.contains_key("cel"));
        assert_eq!(s.retirement_plan("cel", "v1").unwrap()["protected"], true);
    }
    #[test]
    fn typed_dependencies_reject_outputs_and_keep_provenance_out_of_active_inputs() {
        let mut s = State::default();
        let mut primary = version("cel", "v1", 'b');
        primary.dependency_contract = Some(DependencyContract {
            primary_inputs: primary.payloads.clone(),
            outputs: BTreeSet::new(),
            references: BTreeSet::new(),
        });
        s.register(primary).unwrap();
        let mut compiled = version("episode", "v1", 'c');
        compiled.payloads = BTreeSet::from(["d".repeat(64), "e".repeat(64)]);
        compiled.dependency_contract = Some(DependencyContract {
            primary_inputs: BTreeSet::new(),
            outputs: BTreeSet::from(["d".repeat(64)]),
            references: BTreeSet::from(["e".repeat(64)]),
        });
        compiled.dependencies.push(Dependency {
            logical_id: "cel".into(),
            version_id: "v1".into(),
            manifest_sha256: "b".repeat(64),
        });
        s.register(compiled.clone()).unwrap();
        let active = crate::accounting::expand(&s, ["episode@v1".into()]).unwrap();
        assert!(active.contains(&"a".repeat(64)) && active.contains(&"d".repeat(64)));
        assert!(!active.contains(&"e".repeat(64)));
        let mut invalid = compiled.clone();
        invalid.version_id = "v2".into();
        invalid.dependencies = vec![Dependency {
            logical_id: "episode".into(),
            version_id: "v1".into(),
            manifest_sha256: "c".repeat(64),
        }];
        assert!(
            s.register(invalid)
                .unwrap_err()
                .to_string()
                .contains("cannot be dependencies")
        );
        let mut disguised = version("disguised", "v1", 'f');
        disguised.payloads = BTreeSet::from(["d".repeat(64)]);
        disguised.dependency_contract = Some(DependencyContract {
            primary_inputs: disguised.payloads.clone(),
            outputs: BTreeSet::new(),
            references: BTreeSet::new(),
        });
        assert!(s.register(disguised).is_err());
        let mut ambiguous = compiled;
        ambiguous.version_id = "v3".into();
        ambiguous
            .dependency_contract
            .as_mut()
            .unwrap()
            .primary_inputs
            .insert("d".repeat(64));
        assert!(s.register(ambiguous).is_err());
    }
    #[test]
    fn version_index_compaction_preserves_state_and_small_transaction_patches() {
        let mut f = crate::tiering::tests::Fixture::new();
        let h = f.put(b"version metadata fixture", true);
        for n in 0..30 {
            f.version(&format!("item-{n}"), &h);
        }
        let before = serde_json::to_value(read(f.root()).unwrap()).unwrap();
        let size = fs::metadata(f.root().join("registry/versions.json"))
            .unwrap()
            .len();
        let receipt = compact(f.root()).unwrap();
        assert!(receipt["index_bytes_after"].as_u64().unwrap() < size);
        assert_eq!(
            serde_json::to_value(read(f.root()).unwrap()).unwrap(),
            before
        );
        let control = load(&f.root().join("registry/versions.json")).unwrap();
        let base = control["base_sha256"].as_str().unwrap();
        assert!(read(f.root()).unwrap().metadata_roots.contains(base));
        transact(f.root(), |s| {
            s.select("item-0", "v1")?;
            Ok(())
        })
        .unwrap();
        let changed = load(&f.root().join("registry/versions.json")).unwrap();
        assert_eq!(changed["base_sha256"], base);
        assert!(
            fs::metadata(f.root().join("registry/versions.json"))
                .unwrap()
                .len()
                < 1024
        );
        let after = read(f.root()).unwrap();
        assert_eq!(after.current["item-0"], "v1");
        assert_eq!(after.versions.len(), 30);
        let again = compact(f.root()).unwrap();
        assert_eq!(again["generation"], after.generation);
    }
    #[test]
    fn cached_version_index_grows_without_relaxing_git_control_limit() {
        let f = crate::tiering::tests::Fixture::new();
        let mut state = State::default();
        for n in 0..3000 {
            state
                .current
                .insert(format!("artifact/{n:04}/{}", "x".repeat(80)), "v1".into());
        }
        let bytes = serde_json::to_vec(&state).unwrap();
        assert!(bytes.len() > 256 * 1024);
        let hash = f.put(&bytes, true);
        let index = Index {
            schema: "caminos.version-index.v2".into(),
            base_sha256: hash,
            base_bytes: bytes.len() as u64,
            patch: serde_json::json!({}),
        };
        let value = serde_json::to_value(&index).unwrap();
        assert!(control(f.root(), value.clone(), None).is_ok());
        fs::write(
            f.root().join("registry/versions.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        assert!(read(f.root()).is_ok());
        assert!(
            encode_control(f.root(), serde_json::to_value(&state).unwrap())
                .unwrap()
                .len()
                < 1024
        );
        let mut oversized = serde_json::to_value(&state).unwrap();
        oversized["current"]["large"] =
            serde_json::json!("x".repeat(MAX_EXPANDED_INDEX_BYTES as usize));
        assert!(encode_control(f.root(), oversized.clone()).is_err());
        let mut changed = value.clone();
        changed["patch"] = oversized;
        assert!(control(f.root(), changed, None).is_err());
        let mut changed = value;
        changed["base_bytes"] = serde_json::json!(MAX_EXPANDED_INDEX_BYTES + 1);
        assert!(control(f.root(), changed, None).is_err());
    }
    #[test]
    fn version_baseline_corruption_stops_and_explicit_storage_is_read_only() {
        let mut f = crate::tiering::tests::Fixture::new();
        let h = f.put(b"payload", true);
        f.version("a", &h);
        compact(f.root()).unwrap();
        let cfg = config(f.root()).unwrap();
        let index = load(&f.root().join("registry/versions.json")).unwrap();
        let oid = index["base_sha256"].as_str().unwrap();
        let source = crate::object(&cfg.cache_roots[0], oid);
        let other = crate::object(&cfg.cache_roots[1], oid);
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::copy(&source, &other).unwrap();
        fs::write(&source, b"corrupt").unwrap();
        assert!(read(f.root()).is_err());
        fs::remove_file(source).unwrap();
        assert!(read(f.root()).is_ok());
        fs::remove_file(f.root().join("asset-project.json")).unwrap();
        fs::remove_file(f.root().join(".local/storage-rust.json")).unwrap();
        assert!(read(f.root()).is_err());
        assert_eq!(read_with(f.root(), Some(&cfg)).unwrap().versions.len(), 1);
        assert!(!f.root().join(".local/storage-rust.json").exists());
    }
    #[test]
    fn version_index_rejects_unauthorized_and_recursive_bases() {
        let mut f = crate::tiering::tests::Fixture::new();
        let h = f.put(b"payload", true);
        f.version("a", &h);
        compact(f.root()).unwrap();
        let path = f.root().join("registry/versions.json");
        let original = load(&path).unwrap();
        let mut invalid = original.clone();
        invalid["base_sha256"] = serde_json::json!("f".repeat(64));
        fs::write(&path, invalid.to_string()).unwrap();
        assert!(read(f.root()).is_err());
        let bytes = serde_json::to_vec(&original).unwrap();
        let recursive = f.put(&bytes, true);
        invalid = original;
        invalid["base_sha256"] = serde_json::json!(recursive);
        invalid["base_bytes"] = serde_json::json!(bytes.len());
        fs::write(path, invalid.to_string()).unwrap();
        assert!(read(f.root()).is_err());
    }
    #[test]
    fn compact_version_base_is_protected_in_accounting_and_tiering() {
        let mut f = crate::tiering::tests::Fixture::new();
        let h = f.put(b"payload", true);
        f.version("a", &h);
        f.baseline();
        let receipt = compact(f.root()).unwrap();
        let hash = receipt["baseline"]["sha256"].as_str().unwrap();
        let report = crate::accounting::report(f.root()).unwrap();
        let row = report["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["sha256"] == hash)
            .unwrap();
        assert_eq!(row["category"], "registry-control");
        let plan = f.run().unwrap();
        let row = plan["objects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["sha256"] == hash)
            .unwrap();
        assert_eq!(row["class"], "registry-control");
        assert_eq!(row["proposal"], "hold");
    }
    #[test]
    fn index_patch_rejects_missing_required_state_and_unknown_fields() {
        let mut f = crate::tiering::tests::Fixture::new();
        let h = f.put(b"payload", true);
        f.version("a", &h);
        compact(f.root()).unwrap();
        let path = f.root().join("registry/versions.json");
        let original = load(&path).unwrap();
        for patch in [
            serde_json::json!({"generation":null}),
            serde_json::json!({"unexpected":true}),
        ] {
            let mut changed = original.clone();
            changed["patch"] = patch;
            fs::write(&path, changed.to_string()).unwrap();
            assert!(read(f.root()).is_err());
        }
    }
    #[test]
    fn exact_dependency_hash_and_transitive_pin_protect_old_payloads() {
        let mut s = State::default();
        s.register(version("cel", "v1", 'b')).unwrap();
        let mut scene = version("scene", "v1", 'c');
        scene.dependencies.push(Dependency {
            logical_id: "cel".into(),
            version_id: "v1".into(),
            manifest_sha256: "d".repeat(64),
        });
        assert!(s.register(scene.clone()).is_err());
        scene.dependencies[0].manifest_sha256 = "b".repeat(64);
        s.register(scene).unwrap();
        s.select("scene", "v1").unwrap();
        s.deprecate("cel", "v1").unwrap();
        assert_eq!(s.retirement_plan("cel", "v1").unwrap()["protected"], true);
    }
    #[test]
    fn large_version_state_splits_without_changing_commit_point() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("registry")).unwrap();
        let mut state = State::default();
        for (id, ch) in [("a", 'a'), ("b", 'b')] {
            let mut v = version(id, "v1", ch);
            v.payloads = (0..1100).map(|n| format!("{n:064x}")).collect();
            state.register(v).unwrap();
        }
        state.select("a", "v1").unwrap();
        let first = encode(root, &state).unwrap();
        assert!(first.len() < 4096);
        fs::write(root.join("registry/versions.json"), &first).unwrap();
        assert_eq!(read(root).unwrap().versions.len(), 2);
        state.deprecate("a", "v1").unwrap();
        let next = encode(root, &state).unwrap();
        // New records are safe before the atomic control pointer advances.
        assert!(!read(root).unwrap().versions["a@v1"].deprecated);
        fs::write(root.join("registry/versions.json"), next).unwrap();
        assert!(read(root).unwrap().versions["a@v1"].deprecated);
        assert_eq!(read(root).unwrap().current["a"], "v1");
        let doc: serde_json::Value = serde_json::from_slice(&first).unwrap();
        let oid = doc["version_records"]["b@v1"].as_str().unwrap();
        fs::write(
            root.join("registry/version-records")
                .join(&oid[..2])
                .join(format!("{oid}.json")),
            b"bad",
        )
        .unwrap();
        assert!(read(root).is_err());
        assert!(encode(root, &state).is_err());
    }
}
