//! Explicit owned-root, small local publication adapter. Process restart only;
//! directory/power-loss durability and adversarial path mutation are not promised.
use crate::*;
use serde::de::DeserializeOwned;
use sha2::{Digest as _, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Seek, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tig_core::{Digest, identity};

pub const MAX_OBJECT_BYTES: u64 = 8 * 1024 * 1024;
static NEXT: AtomicU64 = AtomicU64::new(0);

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    place: Key,
    instance: Digest,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    request: Request,
}

pub struct LocalStore {
    root: PathBuf,
    place: Key,
    instance: Digest,
    staging: Option<PathBuf>,
}
impl LocalStore {
    /// Creates a fresh explicit root only. Never enrolls an existing directory.
    pub fn create(root: &Path, place: Key) -> Result<Self> {
        check_root_spelling(root)?;
        fs::create_dir(root)?;
        for name in [
            "objects",
            "operations",
            "staging",
            "fences",
            "retentions",
            "releases",
        ] {
            fs::create_dir(root.join(name))?;
        }
        let mut nonce = [0u8; 32];
        getrandom::fill(&mut nonce).map_err(|_| Error::InvalidRoot)?;
        let store = Self {
            root: fs::canonicalize(root)?,
            place,
            instance: Digest::blob(&nonce),
            staging: None,
        };
        store.write_record(
            &store.root.join("store.json"),
            &Manifest {
                schema: "tig.local-store.design-5".into(),
                place: store.place.clone(),
                instance: store.instance.clone(),
            },
        )?;
        Ok(store)
    }
    pub fn open(root: &Path) -> Result<Self> {
        check_root_spelling(root)?;
        directory(root)?;
        let root = fs::canonicalize(root)?;
        let manifest: Manifest = read_record(&root.join("store.json"))?;
        if manifest.schema != "tig.local-store.design-5" {
            return Err(Error::CorruptRecord);
        }
        let store = Self {
            root,
            place: manifest.place,
            instance: manifest.instance,
            staging: None,
        };
        store.guard()?;
        Ok(store)
    }
    pub fn place(&self) -> &Key {
        &self.place
    }
    pub fn identity(&self) -> &Digest {
        &self.instance
    }
    pub(crate) fn allocation_quantum(&self) -> Result<u64> {
        self.guard()?;
        let quantum = fs2::allocation_granularity(&self.root)?;
        if quantum == 0 {
            return Err(Error::Unsupported);
        }
        Ok(quantum)
    }
    pub(crate) fn sample_space(&self) -> Result<crate::local_capacity::SpaceSample> {
        self.guard()?;
        let stats = fs2::statvfs(&self.root)?;
        self.guard()?;
        Ok(crate::local_capacity::SpaceSample {
            available: stats.available_space(),
            total: stats.total_space(),
            allocation_granularity: stats.allocation_granularity(),
        })
    }
    pub(crate) fn check_identity(&self) -> Result<()> {
        self.guard()
    }
    /// Read-only verification for an already settled publication. Does not repair
    /// absent markers or create a new anchor under a completed allocation.
    pub(crate) fn completed_receipt(&self, request: &Request) -> Result<Option<Receipt>> {
        Ok(self.completed_reader(request)?.map(|(receipt, _)| receipt))
    }
    /// One bounded hash pass with a guard spanning records, bytes and rechecks.
    pub(crate) fn completed_reader(
        &self,
        request: &Request,
    ) -> Result<Option<(Receipt, ObjectRead)>> {
        self.capabilities().check(request)?;
        self.guard()?;
        let path = self
            .root
            .join("fences")
            .join(request.object.sha256.as_str());
        regular(&path)?;
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::InvalidRoot);
        }
        file.try_lock_shared().map_err(lock_error)?;
        let guard = SharedObject { _file: file };
        let done = self.op_path(&request.operation, "done");
        if !path_present(&done)? {
            return Ok(None);
        }
        self.check_record(&done, request, "tig.publication-completion.design-1")?;
        self.check_record(
            &self.op_path(&request.operation, "intent"),
            request,
            "tig.publication-intent.design-1",
        )?;
        let path = self.object_path(&request.object);
        regular(&path)?;
        let mut data = File::open(path)?;
        if data.metadata()?.len() != request.object.bytes {
            return Err(Error::Integrity);
        }
        copy_verified(&mut data, &mut io::sink(), &request.object)?;
        data.rewind()?;
        self.guard()?;
        self.check_record(&done, request, "tig.publication-completion.design-1")?;
        self.check_record(
            &self.op_path(&request.operation, "intent"),
            request,
            "tig.publication-intent.design-1",
        )?;
        Ok(Some((
            Receipt {
                request: request.clone(),
                level: request.required,
            },
            ObjectRead {
                file: data,
                _lease: guard,
                object: request.object.clone(),
            },
        )))
    }
    pub(crate) fn attempt(&self, request: &Request) -> Result<LocalAttempt> {
        self.attempt_mode(request, true)
    }
    pub(crate) fn existing_attempt(&self, request: &Request) -> Result<LocalAttempt> {
        self.attempt_mode(request, false)
    }
    fn attempt_mode(&self, request: &Request, create: bool) -> Result<LocalAttempt> {
        self.guard()?;
        let hash = Digest::blob(request.operation.as_str().as_bytes());
        let anchor = self
            .root
            .join("fences")
            .join(format!("attempt-{}", hash.as_str()));
        if path_present(&anchor)? {
            regular(&anchor)?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(create)
            .truncate(false)
            .open(&anchor)?;
        regular(&anchor)?;
        if lock.metadata()?.len() != 0 {
            return Err(Error::InvalidRoot);
        }
        lock.try_lock().map_err(lock_error)?;
        let lock = AttemptLock(lock);
        let scope = self
            .root
            .join("staging")
            .join(format!("attempt-{}", hash.as_str()));
        if create {
            match fs::create_dir(&scope) {
                Ok(()) => (),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e.into()),
            }
        }
        directory(&scope)?;
        let attempt = LocalAttempt {
            store: Self {
                root: self.root.clone(),
                place: self.place.clone(),
                instance: self.instance.clone(),
                staging: Some(scope.clone()),
            },
            scope,
            _lock: lock,
        };
        attempt.files()?;
        Ok(attempt)
    }

    fn anchor(&self, object: &Object) -> Result<File> {
        self.guard()?;
        if object.bytes > MAX_OBJECT_BYTES {
            return Err(Error::Budget);
        }
        let path = self.root.join("fences").join(object.sha256.as_str());
        if path_present(&path)? {
            regular(&path)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        regular(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::InvalidRoot);
        }
        Ok(file)
    }
    fn shared(&self, object: &Object) -> Result<SharedObject> {
        let file = self.anchor(object)?;
        file.try_lock_shared().map_err(lock_error)?;
        Ok(SharedObject { _file: file })
    }
    /// Exclusion only; not retirement authority, claim coverage or custody proof.
    pub fn try_exclusive_object(&self, object: &Object) -> Result<ExclusiveObject> {
        let file = self.anchor(object)?;
        file.try_lock().map_err(lock_error)?;
        let guard = ExclusiveObject {
            _file: file,
            object: object.clone(),
            place: self.place.clone(),
            instance: self.instance.clone(),
        };
        // Construct the guard before reading metadata so every failure unlocks.
        for path in self.retention_files(object)? {
            let record: crate::retention::RetentionRecord = read_record(&path)?;
            if !self.retention_released(&record)? {
                return Err(Error::Retained);
            }
        }
        Ok(guard)
    }
    fn retention_dir(&self, object: &Object) -> PathBuf {
        self.root.join("retentions").join(object.sha256.as_str())
    }
    fn handoff_path(&self, record: &crate::retention::RetentionRecord) -> Result<PathBuf> {
        Ok(self
            .root
            .join("releases")
            .join(record.plan.request.publication.object.sha256.as_str())
            .join(format!("{}.json", record.plan.request.pin_id()?.as_str())))
    }
    pub(crate) fn handoff_record(
        &self,
        record: &crate::retention::RetentionRecord,
    ) -> Result<Option<crate::retention::handoff::HandoffRecord>> {
        self.guard()?;
        let path = self.handoff_path(record)?;
        if !path_present(&path)? {
            return Ok(None);
        }
        let marker: crate::retention::handoff::HandoffRecord = read_record(&path)?;
        if !marker.matches_previous(record, &self.instance) {
            return Err(Error::CorruptRecord);
        }
        Ok(Some(marker))
    }
    pub(crate) fn retention_released(
        &self,
        record: &crate::retention::RetentionRecord,
    ) -> Result<bool> {
        Ok(self.handoff_record(record)?.is_some())
    }
    pub(crate) fn handoff_fence(&self, object: &Object) -> Result<ExclusiveObject> {
        self.guard()?;
        let path = self.root.join("fences").join(object.sha256.as_str());
        regular(&path)?;
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::InvalidRoot);
        }
        file.try_lock().map_err(lock_error)?;
        Ok(ExclusiveObject {
            _file: file,
            object: object.clone(),
            place: self.place.clone(),
            instance: self.instance.clone(),
        })
    }
    fn check_handoff_fence(&self, object: &Object, fence: &ExclusiveObject) -> Result<()> {
        self.guard()?;
        if fence.instance != self.instance || fence.object != *object {
            return Err(Error::OperationConflict);
        }
        Ok(())
    }
    pub(crate) fn verify_handoff_source(
        &self,
        record: &crate::retention::RetentionRecord,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        let request = &record.plan.request.publication;
        self.check_handoff_fence(&request.object, fence)?;
        if self.retention_released(record)? {
            return Err(Error::OperationConflict);
        }
        self.read_retention(record)?;
        for (kind, schema) in [
            ("intent", "tig.publication-intent.design-1"),
            ("done", "tig.publication-completion.design-1"),
        ] {
            self.check_record(&self.op_path(&request.operation, kind), request, schema)?;
        }
        let path = self.object_path(&request.object);
        regular(&path)?;
        let mut file = File::open(path)?;
        if file.metadata()?.len() != request.object.bytes {
            return Err(Error::Integrity);
        }
        copy_verified(&mut file, &mut io::sink(), &request.object)
    }
    pub(crate) fn handoff_claim_absent(
        &self,
        record: &crate::retention::RetentionRecord,
    ) -> Result<bool> {
        Ok(!path_present(
            &self
                .retention_dir(&record.plan.request.publication.object)
                .join(format!("{}.json", record.plan.request.pin_id()?.as_str())),
        )?)
    }
    pub(crate) fn retire_handoff_claim(
        &self,
        record: &crate::retention::RetentionRecord,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        self.check_handoff_fence(&record.plan.request.publication.object, fence)?;
        if !self.retention_released(record)? {
            return Err(Error::OperationConflict);
        }
        if self.handoff_claim_absent(record)? {
            return Ok(());
        }
        self.read_retention(record)?;
        fs::remove_file(
            self.retention_dir(&record.plan.request.publication.object)
                .join(format!("{}.json", record.plan.request.pin_id()?.as_str())),
        )?;
        Ok(())
    }
    fn retention_files(&self, object: &Object) -> Result<Vec<PathBuf>> {
        self.guard()?;
        let dir = self.retention_dir(object);
        if !path_present(&dir)? {
            return Ok(Vec::new());
        }
        directory(&dir)?;
        let mut result = Vec::new();
        for entry in fs::read_dir(&dir)? {
            if result.len() >= crate::retention::MAX_RETAINERS {
                return Err(Error::Budget);
            }
            let path = entry?.path();
            regular(&path)?;
            let name = path
                .file_stem()
                .and_then(|n| n.to_str())
                .ok_or(Error::InvalidRoot)?;
            if path.extension().is_none_or(|x| x != "json") || Digest::parse(name).is_err() {
                return Err(Error::CorruptRecord);
            }
            let record: crate::retention::RetentionRecord = read_record(&path)?;
            if record.schema != crate::retention::SCHEMA
                || record.plan.store != self.instance
                || record.plan.request.publication.object != *object
                || record.plan.request.pin_id()?.as_str() != name
            {
                return Err(Error::CorruptRecord);
            }
            result.push(path);
        }
        Ok(result)
    }
    pub(crate) fn read_retention(
        &self,
        record: &crate::retention::RetentionRecord,
    ) -> Result<Digest> {
        self.guard()?;
        let path = self
            .retention_dir(&record.plan.request.publication.object)
            .join(format!("{}.json", record.plan.request.pin_id()?.as_str()));
        let bytes = identity::canonical_json(
            &serde_json::to_value(record).map_err(|_| Error::CorruptRecord)?,
        )
        .map_err(|_| Error::CorruptRecord)?;
        if read_bounded(&path, 65_536)? != bytes {
            return Err(Error::OperationConflict);
        }
        Ok(Digest::blob(&bytes))
    }
    /// Read-only byte access with a shared guard held for the returned handle's life.
    pub fn read_object(&self, object: &Object) -> Result<ObjectRead> {
        let lease = self.shared(object)?;
        let path = self.object_path(object);
        regular(&path)?;
        let mut file = File::open(path)?;
        if file.metadata()?.len() != object.bytes {
            return Err(Error::Integrity);
        }
        copy_verified(&mut file, &mut io::sink(), object)?;
        file.rewind()?;
        Ok(ObjectRead {
            file,
            _lease: lease,
            object: object.clone(),
        })
    }
    fn guard(&self) -> Result<()> {
        directory(&self.root)?;
        for name in [
            "objects",
            "operations",
            "staging",
            "fences",
            "retentions",
            "releases",
        ] {
            directory(&self.root.join(name))?;
        }
        let manifest: Manifest = read_record(&self.root.join("store.json"))?;
        if manifest.schema != "tig.local-store.design-5"
            || (manifest.place != self.place || manifest.instance != self.instance)
        {
            return Err(Error::CorruptRecord);
        }
        Ok(())
    }
    fn op_path(&self, operation: &Key, suffix: &str) -> PathBuf {
        let hash = Digest::blob(operation.as_str().as_bytes());
        self.root
            .join("operations")
            .join(format!("{}.{suffix}", hash.as_str()))
    }
    fn object_path(&self, object: &Object) -> PathBuf {
        self.root.join("objects").join(object.sha256.as_str())
    }
    fn new_partial(&self) -> Result<Partial> {
        for _ in 0..32 {
            let seq = NEXT.fetch_add(1, Ordering::Relaxed);
            let parent = self
                .staging
                .clone()
                .unwrap_or_else(|| self.root.join("staging"));
            directory(&parent)?;
            let path = parent.join(format!("{}-{seq}.part", std::process::id()));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => {
                    return Ok(Partial {
                        path,
                        file: Some(file),
                    });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::Budget)
    }
    fn write_record(&self, destination: &Path, value: &impl Serialize) -> Result<()> {
        let bytes = identity::canonical_json(
            &serde_json::to_value(value).map_err(|_| Error::CorruptRecord)?,
        )
        .map_err(|_| Error::CorruptRecord)?;
        let mut partial = self.new_partial()?;
        let file = partial.file.as_mut().ok_or(Error::Integrity)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(partial.file.take());
        match fs::hard_link(&partial.path, destination) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        if read_bounded(destination, 65_536)? != bytes {
            return Err(Error::OperationConflict);
        }
        Ok(())
    }
    fn check_record(&self, path: &Path, request: &Request, schema: &str) -> Result<()> {
        let record: Record = read_record(path)?;
        if record.schema != schema {
            return Err(Error::CorruptRecord);
        }
        if record.request != *request {
            return Err(Error::OperationConflict);
        }
        Ok(())
    }
}

pub(crate) const MAX_SCOPE_FILES: usize = 16;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivatePartial {
    pub name: String,
    pub bytes: u64,
    pub sha256: Digest,
    pub allocated: u64,
}
pub(crate) struct LocalAttempt {
    pub store: LocalStore,
    scope: PathBuf,
    _lock: AttemptLock,
}
struct AttemptLock(File);
impl Drop for AttemptLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
impl LocalAttempt {
    pub fn install_retention(&self, record: &crate::retention::RetentionRecord) -> Result<()> {
        if self.store.retention_released(record)? {
            return Err(Error::OperationConflict);
        }
        let object = &record.plan.request.publication.object;
        let target = self
            .store
            .retention_dir(object)
            .join(format!("{}.json", record.plan.request.pin_id()?.as_str()));
        if self.has_partials()? && !path_present(&target)? {
            return Err(Error::OperationConflict);
        }
        let _object = self.store.shared(object)?;
        let path = self
            .store
            .root
            .join("fences")
            .join(format!("retain-{}", object.sha256.as_str()));
        if path_present(&path)? {
            regular(&path)?;
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)?;
        regular(&path)?;
        if file.metadata()?.len() != 0 {
            return Err(Error::InvalidRoot);
        }
        file.try_lock().map_err(lock_error)?;
        let _metadata = AttemptLock(file);
        let files = self.store.retention_files(object)?;
        let target = self
            .store
            .retention_dir(object)
            .join(format!("{}.json", record.plan.request.pin_id()?.as_str()));
        if path_present(&target)? {
            self.store.read_retention(record)?;
            return Ok(());
        }
        if files.len() >= crate::retention::MAX_RETAINERS {
            return Err(Error::Budget);
        }
        if self.has_partials()? {
            return Err(Error::OperationConflict);
        }
        let dir = self.store.retention_dir(object);
        match fs::create_dir(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        directory(&dir)?;
        self.store.write_record(&target, record)
    }
    pub fn retention_footprint(&self, record: &crate::retention::RetentionRecord) -> Result<u64> {
        let mut paths = self.files()?;
        paths.push(
            self.store
                .retention_dir(&record.plan.request.publication.object)
                .join(format!("{}.json", record.plan.request.pin_id()?.as_str())),
        );
        let mut total = 0u64;
        for path in paths {
            regular(&path)?;
            total = total
                .checked_add(fs2::FileExt::allocated_size(&File::open(path)?)?)
                .ok_or(Error::Budget)?;
        }
        Ok(total)
    }
    pub fn install_handoff(
        &self,
        record: &crate::retention::handoff::HandoffRecord,
        fence: &ExclusiveObject,
    ) -> Result<()> {
        let previous = record.previous();
        self.store
            .check_handoff_fence(&previous.plan.request.publication.object, fence)?;
        let path = self.store.handoff_path(previous)?;
        if let Some(existing) = self.store.handoff_record(previous)? {
            if !existing.same(record) {
                return Err(Error::OperationConflict);
            }
            return Ok(());
        }
        if self.has_partials()? {
            return Err(Error::OperationConflict);
        }
        let dir = path.parent().ok_or(Error::InvalidRoot)?;
        match fs::create_dir(dir) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        directory(dir)?;
        self.store.write_record(&path, record)
    }
    pub fn handoff_footprint(
        &self,
        record: &crate::retention::handoff::HandoffRecord,
    ) -> Result<u64> {
        let mut paths = self.files()?;
        paths.push(self.store.handoff_path(record.previous())?);
        let mut total = 0u64;
        for path in paths {
            regular(&path)?;
            total = total
                .checked_add(fs2::FileExt::allocated_size(&File::open(path)?)?)
                .ok_or(Error::Budget)?;
        }
        Ok(total)
    }
    pub fn check_binding(&self, binding: &impl Serialize) -> Result<()> {
        let value = identity::canonical_json(
            &serde_json::to_value(binding).map_err(|_| Error::CorruptRecord)?,
        )
        .map_err(|_| Error::CorruptRecord)?;
        let existing: serde_json::Value = read_record(&self.scope.join("binding.json"))?;
        if identity::canonical_json(&existing).map_err(|_| Error::CorruptRecord)? != value {
            return Err(Error::OperationConflict);
        }
        Ok(())
    }
    pub fn has_final_or_completion(&self, request: &Request) -> Result<bool> {
        self.store.guard()?;
        let intent = self.store.op_path(&request.operation, "intent");
        if path_present(&intent)? {
            self.store
                .check_record(&intent, request, "tig.publication-intent.design-1")?;
        }
        Ok(path_present(&self.store.object_path(&request.object))?
            || path_present(&self.store.op_path(&request.operation, "done"))?)
    }
    pub fn partial_evidence(&self, budget: u64) -> Result<Vec<PrivatePartial>> {
        use sha2::{Digest as _, Sha256};
        let mut result = Vec::new();
        let mut remaining = budget;
        for path in self.files()? {
            if path.extension().is_none_or(|x| x != "part") {
                continue;
            }
            let mut file = File::open(&path)?;
            let bytes = file.metadata()?.len();
            remaining = remaining.checked_sub(bytes).ok_or(Error::Budget)?;
            let mut hash = Sha256::new();
            let mut buffer = [0; 65_536];
            let mut left = bytes;
            while left > 0 {
                let n =
                    usize::try_from(left.min(buffer.len() as u64)).map_err(|_| Error::Budget)?;
                file.read_exact(&mut buffer[..n])?;
                hash.update(&buffer[..n]);
                left -= n as u64;
            }
            if file.read(&mut buffer[..1])? != 0 {
                return Err(Error::Integrity);
            }
            result.push(PrivatePartial {
                name: path
                    .file_name()
                    .and_then(|x| x.to_str())
                    .ok_or(Error::InvalidRoot)?
                    .to_owned(),
                bytes,
                sha256: Digest::from_sha256(hash.finalize().into()),
                allocated: fs2::FileExt::allocated_size(&file)?,
            });
        }
        result.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(result)
    }
    pub fn remove_partial(&self, name: &str) -> Result<()> {
        // Resolve from the validated current scope, never from a caller path.
        let path = self
            .files()?
            .into_iter()
            .find(|p| {
                p.file_name().is_some_and(|x| x == name)
                    && p.extension().is_some_and(|x| x == "part")
            })
            .ok_or(Error::OperationConflict)?;
        regular(&path)?;
        fs::remove_file(path)?;
        Ok(())
    }
    pub fn retained_metadata(&self, request: &Request) -> Result<u64> {
        let mut paths = self.files()?;
        if paths
            .iter()
            .any(|p| p.extension().is_some_and(|x| x == "part"))
        {
            return Err(Error::OperationConflict);
        }
        let intent = self.store.op_path(&request.operation, "intent");
        if path_present(&intent)? {
            self.store
                .check_record(&intent, request, "tig.publication-intent.design-1")?;
            paths.push(intent);
        }
        let mut total = 0u64;
        for path in paths {
            regular(&path)?;
            total = total
                .checked_add(fs2::FileExt::allocated_size(&File::open(path)?)?)
                .ok_or(Error::Budget)?;
        }
        Ok(total)
    }
    fn files(&self) -> Result<Vec<PathBuf>> {
        directory(&self.scope)?;
        let mut paths = Vec::new();
        for entry in fs::read_dir(&self.scope)? {
            if paths.len() >= MAX_SCOPE_FILES {
                return Err(Error::Budget);
            }
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().ok_or(Error::InvalidRoot)?;
            let part = name
                .strip_suffix(".part")
                .and_then(|n| n.split_once('-'))
                .is_some_and(|(a, b)| {
                    !a.is_empty()
                        && !b.is_empty()
                        && a.bytes().all(|c| c.is_ascii_digit())
                        && b.bytes().all(|c| c.is_ascii_digit())
                });
            if name != "binding.json" && !part {
                return Err(Error::CorruptRecord);
            }
            regular(&entry.path())?;
            paths.push(entry.path());
        }
        Ok(paths)
    }
    pub fn bind(&self, binding: &impl Serialize) -> Result<()> {
        self.store
            .write_record(&self.scope.join("binding.json"), binding)
    }
    pub fn has_partials(&self) -> Result<bool> {
        Ok(self
            .files()?
            .iter()
            .any(|p| p.extension().is_some_and(|x| x == "part")))
    }
    /// Sum by exact owned scope/path. Aliases/shared data may be counted again;
    /// this is a conservative file-data footprint, not net volume growth.
    pub fn footprint(&self, request: &Request) -> Result<u64> {
        let mut paths = self.files()?;
        paths.extend([
            self.store.object_path(&request.object),
            self.store.op_path(&request.operation, "intent"),
            self.store.op_path(&request.operation, "done"),
        ]);
        let mut total = 0u64;
        for path in paths {
            regular(&path)?;
            let size = fs2::FileExt::allocated_size(&File::open(path)?)?;
            total = total.checked_add(size).ok_or(Error::Budget)?;
        }
        Ok(total)
    }
}

impl PublicationBackend for LocalStore {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            max_object_bytes: MAX_OBJECT_BYTES,
            max_ack: AckLevel::ProcessRestart,
        }
    }
    fn publication_guard(&mut self, object: &Object) -> Result<Box<dyn PublicationGuard>> {
        Ok(Box::new(self.shared(object)?))
    }
    fn protect(&mut self, request: &Request) -> Result<State> {
        self.capabilities().check(request)?;
        let _guard = self.shared(&request.object)?;
        self.guard()?;
        // Completion can survive a stale/partial journal restore independently
        // of intent. Validate it before creating any replacement intent.
        let done = self.op_path(&request.operation, "done");
        let committed = path_present(&done)?;
        if committed {
            self.check_record(&done, request, "tig.publication-completion.design-1")?;
        }
        let intent = self.op_path(&request.operation, "intent");
        if path_present(&intent)? {
            self.check_record(&intent, request, "tig.publication-intent.design-1")?;
        } else {
            self.write_record(
                &intent,
                &Record {
                    schema: "tig.publication-intent.design-1".into(),
                    request: request.clone(),
                },
            )?;
        }
        if committed {
            Ok(State::Committed)
        } else {
            Ok(State::Protected)
        }
    }
    fn verified_object(&mut self, object: &Object) -> Result<bool> {
        let _guard = self.shared(object)?;
        self.guard()?;
        if object.bytes > MAX_OBJECT_BYTES {
            return Err(Error::Budget);
        }
        let path = self.object_path(object);
        if !path_present(&path)? {
            return Ok(false);
        }
        regular(&path)?;
        let mut file = File::open(&path)?;
        if file.metadata()?.len() != object.bytes {
            return Err(Error::Integrity);
        }
        copy_verified(&mut file, &mut io::sink(), object)?;
        Ok(true)
    }
    fn publish_object(&mut self, request: &Request, source: &mut dyn Read) -> Result<()> {
        self.capabilities().check(request)?;
        let _guard = self.shared(&request.object)?;
        self.guard()?;
        self.check_record(
            &self.op_path(&request.operation, "intent"),
            request,
            "tig.publication-intent.design-1",
        )?;
        let mut partial = self.new_partial()?;
        let file = partial.file.as_mut().ok_or(Error::Integrity)?;
        // POSIX allocation rejects a zero-length range. An empty payload has
        // no file-data extent to preallocate; its metadata is budgeted separately.
        if self.staging.is_some() && request.object.bytes != 0 {
            fs2::FileExt::allocate(file, request.object.bytes)?;
        }
        copy_verified(source, file, &request.object)?;
        file.sync_all()?;
        drop(partial.file.take());
        match fs::hard_link(&partial.path, self.object_path(&request.object)) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.into()),
        }
        if !self.verified_object(&request.object)? {
            return Err(Error::Integrity);
        }
        Ok(())
    }
    fn commit(&mut self, request: &Request) -> Result<()> {
        self.capabilities().check(request)?;
        let _guard = self.shared(&request.object)?;
        self.guard()?;
        self.check_record(
            &self.op_path(&request.operation, "intent"),
            request,
            "tig.publication-intent.design-1",
        )?;
        if !self.verified_object(&request.object)? {
            return Err(Error::Integrity);
        }
        self.write_record(
            &self.op_path(&request.operation, "done"),
            &Record {
                schema: "tig.publication-completion.design-1".into(),
                request: request.clone(),
            },
        )
    }
}

struct SharedObject {
    _file: File,
}
impl Drop for SharedObject {
    fn drop(&mut self) {
        // Explicitly end this guard's lock even if a supervisor/fork temporarily
        // duplicated its OS handle. Closing one handle alone may not be enough.
        let _ = self._file.unlock();
    }
}
impl PublicationGuard for SharedObject {}
pub struct ExclusiveObject {
    _file: File,
    object: Object,
    place: Key,
    instance: Digest,
}
impl Drop for ExclusiveObject {
    fn drop(&mut self) {
        let _ = self._file.unlock();
    }
}
impl ExclusiveObject {
    pub fn object(&self) -> &Object {
        &self.object
    }
    pub fn place(&self) -> &Key {
        &self.place
    }
}
pub struct ObjectRead {
    file: File,
    _lease: SharedObject,
    object: Object,
}
impl ObjectRead {
    pub fn object(&self) -> &Object {
        &self.object
    }
}
impl Read for ObjectRead {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.file.read(buffer)
    }
}
fn lock_error(error: fs::TryLockError) -> Error {
    match error {
        fs::TryLockError::WouldBlock => Error::Busy,
        fs::TryLockError::Error(e) => e.into(),
    }
}

struct Partial {
    path: PathBuf,
    file: Option<File>,
}
impl Drop for Partial {
    fn drop(&mut self) {
        drop(self.file.take());
        // Only this call's create-new private partial, never a final object.
        // Cleanup failure/crash debris needs later bounded observation/recovery.
        let _ = fs::remove_file(&self.path);
    }
}
pub(crate) fn check_root_spelling(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(Error::InvalidRoot);
    }
    Ok(())
}
pub(crate) fn directory(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err(Error::InvalidRoot);
    }
    Ok(())
}
pub(crate) fn regular(path: &Path) -> Result<()> {
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        return Err(Error::InvalidRoot);
    }
    Ok(())
}
fn path_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>> {
    regular(path)?;
    let mut bytes = Vec::new();
    File::open(path)?.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(Error::CorruptRecord);
    }
    Ok(bytes)
}
pub(crate) fn read_record<T: DeserializeOwned + Serialize>(path: &Path) -> Result<T> {
    let bytes = read_bounded(path, 65_536)?;
    let value = identity::parse_json(&bytes).map_err(|_| Error::CorruptRecord)?;
    if identity::canonical_json(&value).map_err(|_| Error::CorruptRecord)? != bytes {
        return Err(Error::CorruptRecord);
    }
    let record: T = serde_json::from_value(value).map_err(|_| Error::CorruptRecord)?;
    let value = serde_json::to_value(&record).map_err(|_| Error::CorruptRecord)?;
    if identity::canonical_json(&value).map_err(|_| Error::CorruptRecord)? != bytes {
        return Err(Error::CorruptRecord);
    }
    Ok(record)
}
fn copy_verified(source: &mut dyn Read, target: &mut impl Write, object: &Object) -> Result<()> {
    let mut limited = source.take(object.bytes.checked_add(1).ok_or(Error::Budget)?);
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = match limited.read(&mut buffer) {
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > object.bytes {
            return Err(Error::Integrity);
        }
        hasher.update(&buffer[..n]);
        target.write_all(&buffer[..n])?;
    }
    let digest = Digest::from_sha256(hasher.finalize().into());
    if total != object.bytes || digest != object.sha256 {
        return Err(Error::Integrity);
    }
    Ok(())
}

#[cfg(test)]
mod release_tests {
    use super::*;
    #[test]
    fn guard_drop_explicitly_unlocks_despite_a_duplicated_handle() {
        let temp = tempfile::tempdir().unwrap();
        let store =
            LocalStore::create(&temp.path().join("store"), Key::parse("place").unwrap()).unwrap();
        let object = Object::from_bytes(b"synthetic");
        let shared = store.shared(&object).unwrap();
        let inherited = shared._file.try_clone().unwrap();
        drop(shared);
        let exclusive = store.try_exclusive_object(&object).unwrap();
        drop(inherited);
        let inherited = exclusive._file.try_clone().unwrap();
        drop(exclusive);
        let shared = store.shared(&object).unwrap();
        drop(inherited);
        drop(shared);
        let request = Request {
            operation: Key::parse("attempt").unwrap(),
            owner: Key::parse("owner").unwrap(),
            object,
            required: AckLevel::ProcessRestart,
        };
        let attempt = store.attempt(&request).unwrap();
        let inherited = attempt._lock.0.try_clone().unwrap();
        drop(attempt);
        let replacement = store.attempt(&request).unwrap();
        drop(inherited);
        drop(replacement);
    }
}
