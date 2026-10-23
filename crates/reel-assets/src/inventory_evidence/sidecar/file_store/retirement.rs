//! Exact append-only index retention and dry-run/explicit staged-link retirement.
//! This does not change selections, unlink CAS names or evict media.
use super::super::validation;
use super::*;
const RETAINED: &str = "reel.inventory-sidecar.retained.v1";
const RETIRING: &str = "reel.inventory-sidecar.retiring.v1";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: String,
    root: Descriptor,
    source: Value,
    closure: String,
    nodes: usize,
    bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: String,
    session: PathBuf,
    cache: PathBuf,
    retained: PathBuf,
    receipt: Receipt,
}

/// Private plan bound to exact session, source, root and immutable retention.
/// Counts describe names/bytes protected, not physically reclaimable payload.
pub struct RetirementPlan {
    intent: Intent,
    source: Source,
}
impl RetirementPlan {
    pub fn nodes(&self) -> usize {
        self.intent.receipt.nodes
    }
    pub fn logical_bytes(&self) -> u64 {
        self.intent.receipt.bytes
    }
    /// Retained hard links keep every payload allocated. This operation is not
    /// media eviction and promises no payload-space savings.
    pub fn payload_bytes_reclaimed(&self) -> u64 {
        0
    }
}
pub struct Progress {
    pub links_removed: usize,
    pub links_remaining: usize,
}

fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
pub(super) fn marker_exists(store: &FileStore) -> Result<bool> {
    exists(&store.session.join("retirement.json"))
}
pub(super) fn started(store: &FileStore, root: &Blob) -> Result<bool> {
    if !marker_exists(store)? {
        return Ok(false);
    }
    let intent: Intent = control(&store.session.join("retirement.json"))?;
    if intent.schema != RETIRING
        || intent.session != store.session
        || intent.cache != store.cache
        || intent.receipt.schema != RETAINED
        || intent.receipt.root.blob() != *root
        || !valid_oid(&intent.receipt.closure)
    {
        return Err("Invalid staged retirement scope".into());
    }
    Ok(true)
}
fn atomic_control(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = encode(value)?;
    let pending = path.with_extension("pending");
    if exists(&pending)? {
        verified(
            &pending,
            &Blob {
                sha256: hash(&bytes),
                bytes: bytes.len() as u64,
            },
        )?;
    } else {
        create_control(&pending, value)?;
    }
    match fs::hard_link(&pending, path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    verified(
        path,
        &Blob {
            sha256: hash(&bytes),
            bytes: bytes.len() as u64,
        },
    )?;
    sync_directory(path.parent().ok_or("Missing control parent")?)
}
fn root_directory(store: &FileStore, parent: &Path, root: &Blob) -> Result<PathBuf> {
    let parent = checked_directory(parent)?;
    let staging_parent = store.session.parent().ok_or("Missing staging parent")?;
    if !parent.starts_with(&store.cache)
        || parent == store.cache
        || parent.starts_with(staging_parent)
        || staging_parent.starts_with(&parent)
    {
        return Err(
            "Retained-index parent must be separate from staging inside the same selected cache"
                .into(),
        );
    }
    if !valid_oid(&root.sha256) {
        return Err("Invalid retained root".into());
    }
    if crate::admission::volume_id(&parent)? != crate::admission::volume_id(&store.cache)? {
        return Err("Retained indexes must share the selected cache filesystem".into());
    }
    Ok(parent.join(&root.sha256))
}
fn retained_lock(root: &Path) -> Result<File> {
    let path = root.join("owner.lock");
    // Per exact immutable root only, never a shared-cache lock.
    if !exists(&path)? {
        match owner(&path, true) {
            Ok(file) => return Ok(file),
            Err(_) if exists(&path)? => (),
            Err(e) => return Err(e),
        }
    }
    owner(&path, false)
}
fn closure(store: &FileStore, root: &Blob, source: &Source) -> Result<validation::ValidatedIndex> {
    let mut reader = ReadOnlyStore::new(&store.cache, store.limits.node_bytes)?;
    validation::validate(
        &mut reader,
        root,
        source,
        ReadLimits {
            node_bytes: store.limits.node_bytes,
            total_bytes: store.limits.staged_bytes,
            nodes: store.limits.nodes,
            rows: 1_000_000,
        },
    )
}
fn receipt(root: &Blob, source: &Source, graph: &validation::ValidatedIndex) -> Result<Receipt> {
    let descriptors: Vec<_> = graph.descriptors.iter().map(Descriptor::from).collect();
    Ok(Receipt {
        schema: RETAINED.into(),
        root: Descriptor::from(root),
        source: source_key(source),
        closure: hash(&encode(&descriptors)?),
        nodes: graph.nodes_read,
        bytes: graph.bytes_read,
    })
}
fn matches_receipt(actual: &Receipt, expected: &Receipt) -> Result<()> {
    if encode(actual)? != encode(expected)? {
        return Err("Retained index identity/closure mismatch".into());
    }
    Ok(())
}
fn verify_retained(
    root: &Path,
    graph: &validation::ValidatedIndex,
    expected: &Receipt,
) -> Result<()> {
    checked_directory(root)?;
    checked_directory(&root.join("objects"))?;
    let actual: Receipt = control(&root.join("root.json"))?;
    matches_receipt(&actual, expected)?;
    for blob in &graph.descriptors {
        verified(&root.join("objects").join(&blob.sha256), blob)?;
    }
    Ok(())
}
fn verify_staging(
    store: &FileStore,
    graph: &validation::ValidatedIndex,
    retry: bool,
) -> Result<()> {
    let members: BTreeMap<_, _> = graph
        .descriptors
        .iter()
        .map(|b| (b.sha256.as_str(), b.bytes))
        .collect();
    let mut actual = BTreeMap::new();
    checked_directory(&store.session.join("objects"))?;
    for (ordinal, entry) in fs::read_dir(store.session.join("objects"))?.enumerate() {
        if ordinal >= store.limits.nodes {
            return Err("Staging enumeration exceeds session budget".into());
        }
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "Invalid staged filename")?;
        let bytes = plain(&entry.path(), false)?.len();
        if members.get(name.as_str()) != Some(&bytes) {
            return Err("Unknown staged node in retirement scope".into());
        }
        verified(
            &entry.path(),
            &Blob {
                sha256: name.clone(),
                bytes,
            },
        )?;
        actual.insert(name, bytes);
    }
    if actual != store.staged || (!retry && actual.len() != graph.descriptors.len()) {
        return Err("Staged state changed; explicit session recovery required".into());
    }
    Ok(())
}

/// Catalog admission reuses exactly the retained-anchor verification rules.
/// The descriptor set is private and carries no current-selection authority.
pub(super) fn verify_anchor(
    cache: &Path,
    parent: &Path,
    root: &Blob,
    source: &Source,
    limits: ReadLimits,
) -> Result<(validation::ValidatedIndex, Blob)> {
    let cache = checked_directory(cache)?;
    let parent = checked_directory(parent)?;
    if parent == cache
        || !parent.starts_with(&cache)
        || crate::admission::volume_id(&parent)? != crate::admission::volume_id(&cache)?
        || !valid_oid(&root.sha256)
    {
        return Err("Invalid explicit retained-index scope".into());
    }
    let retained = parent.join(&root.sha256);
    checked_directory(&retained)?;
    let _owner = reader_owner(&retained.join("owner.lock"))?;
    let mut reader = ReadOnlyStore::new(&cache, limits.node_bytes)?;
    let graph = validation::validate(&mut reader, root, source, limits)?;
    let expected = receipt(root, source, &graph)?;
    verify_retained(&retained, &graph, &expected)?;
    let bytes = encode(&expected)?;
    Ok((
        graph,
        Blob {
            sha256: hash(&bytes),
            bytes: bytes.len() as u64,
        },
    ))
}
impl FileStore {
    /// Retain a complete verified index under its exact root hash. All current
    /// staged nodes must belong to this closure; extra/unrelated staging rejects.
    /// Append-only retained roots are protection anchors, not a mutable catalog.
    pub fn handoff(&mut self, parent: &Path, root: &Blob, source: &Source) -> Result<()> {
        self.admitted()?;
        if self.poisoned || self.sealed.as_ref() != Some(root) || marker_exists(self)? {
            return Err("Handoff requires a healthy sealed, non-retiring session".into());
        }
        let graph = closure(self, root, source)?;
        verify_staging(self, &graph, false)?;
        let retained = root_directory(self, parent, root)?;
        directory(&retained)?;
        let _owner = retained_lock(&retained)?;
        directory(&retained.join("objects"))?;
        for blob in &graph.descriptors {
            let destination = retained.join("objects").join(&blob.sha256);
            match fs::hard_link(
                self.session.join("objects").join(&blob.sha256),
                &destination,
            ) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(e.into()),
            }
            verified(&destination, blob)?;
        }
        sync_directory(&retained.join("objects"))?;
        atomic_control(&retained.join("root.json"), &receipt(root, source, &graph)?)?;
        Ok(())
    }
    /// Read-only dry run; validates the whole CAS closure and retained links.
    /// A persisted prior intent permits planning a retry after partial retirement.
    pub fn plan_retirement(
        &self,
        parent: &Path,
        root: &Blob,
        source: &Source,
    ) -> Result<RetirementPlan> {
        if self.poisoned || self.sealed.as_ref() != Some(root) {
            return Err("Retirement requires a healthy sealed session".into());
        }
        let graph = closure(self, root, source)?;
        let retained = root_directory(self, parent, root)?;
        let _owner = reader_owner(&retained.join("owner.lock"))?;
        let receipt = receipt(root, source, &graph)?;
        verify_retained(&retained, &graph, &receipt)?;
        let intent = Intent {
            schema: RETIRING.into(),
            session: self.session.clone(),
            cache: self.cache.clone(),
            retained,
            receipt,
        };
        let retry = marker_exists(self)?;
        verify_staging(self, &graph, retry)?;
        if retry {
            let prior: Intent = control(&self.session.join("retirement.json"))?;
            if encode(&prior)? != encode(&intent)? {
                return Err("Retirement retry scope changed".into());
            }
        }
        Ok(RetirementPlan {
            intent,
            source: source.clone(),
        })
    }
    /// Revalidates before first unlink, then verifies each retained counterpart
    /// again immediately before removing ONLY that session's staged name.
    /// `max_links` permits bounded blocks and explicit continuation after a crash.
    pub fn apply_retirement(
        &mut self,
        plan: &RetirementPlan,
        max_links: usize,
    ) -> Result<Progress> {
        self.admitted()?;
        if max_links == 0 {
            return Err("Retirement block must contain at least one link".into());
        }
        let parent = plan
            .intent
            .retained
            .parent()
            .ok_or("Missing retained parent")?;
        let root = plan.intent.receipt.root.blob();
        let current = self.plan_retirement(parent, &root, &plan.source)?;
        if encode(&current.intent)? != encode(&plan.intent)? {
            return Err("Retirement plan is stale or for another session".into());
        }
        let _owner = reader_owner(&plan.intent.retained.join("owner.lock"))?;
        atomic_control(&self.session.join("retirement.json"), &plan.intent)?;
        let graph = closure(self, &root, &plan.source)?;
        let mut removed = 0;
        for blob in &graph.descriptors {
            let staged = self.session.join("objects").join(&blob.sha256);
            if !exists(&staged)? {
                continue;
            }
            if removed >= max_links {
                break;
            }
            verified(
                &plan.intent.retained.join("objects").join(&blob.sha256),
                blob,
            )?;
            verified(&staged, blob)?;
            fs::remove_file(&staged)?;
            self.staged.remove(&blob.sha256);
            self.staged_bytes -= blob.bytes;
            removed += 1;
        }
        sync_directory(&self.session.join("objects"))?;
        Ok(Progress {
            links_removed: removed,
            links_remaining: self.staged.len(),
        })
    }
}
