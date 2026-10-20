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

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub logical_id: String,
    pub version_id: String,
    pub manifest_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
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
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
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
    fn closure(&self, roots: impl Iterator<Item = String>) -> Result<BTreeSet<String>> {
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

pub fn read(root: &Path) -> Result<State> {
    let path = root.join("registry/versions.json");
    if !path.exists() {
        return Ok(State::default());
    }
    if fs::metadata(&path)?.len() > 256 * 1024 {
        return Err("Version control file exceeds budget".into());
    }
    let mut state: State = serde_json::from_value(load(&path)?)?;
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
                    registered = Some(crate::generic_objects(root)?);
                }
                let index = registered.as_ref().unwrap();
                let size = *index.get(&oid).ok_or("Version record is not registered")?;
                if size > 256 * 1024 {
                    return Err("Version record exceeds budget".into());
                }
                path = crate::active_existing(&config(root)?, &oid, size)?
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
    }
    Ok(state)
}

fn encode(root: &Path, state: &State) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(state)?;
    if bytes.len() <= 128 * 1024 {
        return Ok(bytes);
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
    let bytes = serde_json::to_vec(&control)?;
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
    let config = config(root)?;
    let dir = admission::control(root, &config)?;
    let _lock = admission::lock(&dir)?;
    let path = root.join("registry/versions.json");
    let mut state = read(root)?;
    let result = operation(&mut state)?;
    fs::create_dir_all(path.parent().ok_or("Invalid state path")?)?;
    // Persist a complete replacement while holding the common short metadata lock.
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
