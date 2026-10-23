//! Revisioned exact inventory-index roots. No media selection, eviction or
//! automatic recovery. Immutable decisions precede the replaceable head hint.
use super::*;
use crate::{Config, admission};
pub mod protection;
#[cfg(test)]
mod tests;
const MANIFEST: &str = "reel.inventory-sidecar.catalog.v1";
const STATE: &str = "reel.inventory-sidecar.catalog-state.v1";
const FENCE: &str = "reel.inventory-sidecar.catalog-fence.v1";
const DECISION: &str = "reel.inventory-sidecar.catalog-decision.v1";
const HEAD: &str = "reel.inventory-sidecar.catalog-head.v1";
const MEMBER: &str = "reel.inventory-sidecar.catalog-member.v1";

#[derive(Clone)]
pub struct Scope {
    pub project_id: String,
    pub authority_namespace: String,
    pub catalog_id: String,
}
pub struct MutationContext<'a> {
    pub project_root: &'a Path,
    pub storage: &'a Config,
    pub overhead_bytes: u64,
}
pub struct Candidate {
    binding: Binding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    cache: PathBuf,
    coordination: PathBuf,
    project: String,
    authority: String,
    id: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProofLimits {
    node_bytes: u64,
    total_bytes: u64,
    nodes: usize,
    rows: usize,
}
impl ProofLimits {
    fn read(&self) -> ReadLimits {
        ReadLimits {
            node_bytes: self.node_bytes,
            total_bytes: self.total_bytes,
            nodes: self.nodes,
            rows: self.rows,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    root: Descriptor,
    source: Value,
    cache: PathBuf,
    retained_parent: PathBuf,
    receipt: Descriptor,
    limits: ProofLimits,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    schema: String,
    catalog: String,
    key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema: String,
    catalog: String,
    key: String,
    revision: u64,
    binding: Option<Binding>,
    previous: Option<Descriptor>,
    decision: Option<Descriptor>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fence {
    schema: String,
    catalog: String,
    key: String,
    revision: u64,
    previous: Descriptor,
    binding: Option<Binding>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Decision {
    schema: String,
    catalog: String,
    key: String,
    revision: u64,
    fence: Descriptor,
    commit: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Head {
    schema: String,
    catalog: String,
    key: String,
    revision: u64,
    state: Descriptor,
}

pub struct Snapshot {
    pub revision: u64,
    state: State,
    head: Blob,
}
impl Snapshot {
    pub fn root(&self) -> Option<Blob> {
        self.state.binding.as_ref().map(|b| b.root.blob())
    }
    pub fn source(&self) -> Result<Option<Source>> {
        self.state.binding.as_ref().map(Binding::source).transpose()
    }
}
pub struct Catalog {
    directory: PathBuf,
    manifest: Manifest,
    identity: Blob,
}
struct ControlMeter {
    used: u64,
    max: u64,
}
impl ControlMeter {
    fn charge(&mut self, bytes: u64) -> Result<()> {
        self.used = self
            .used
            .checked_add(bytes)
            .ok_or("Catalog metadata byte overflow")?;
        if self.used > self.max {
            return Err("Catalog metadata byte budget exceeded before read".into());
        }
        Ok(())
    }
    fn read<T: serde::de::DeserializeOwned + Serialize>(&mut self, path: &Path) -> Result<T> {
        let bytes = plain(path, false)?.len();
        if bytes > CONTROL_BYTES {
            return Err("Catalog control byte limit exceeded".into());
        }
        self.charge(bytes)?;
        decode(&read_limited(File::open(path)?, bytes)?)
    }
    fn exact<T: serde::de::DeserializeOwned + Serialize>(
        &mut self,
        path: &Path,
        descriptor: &Descriptor,
    ) -> Result<T> {
        self.charge(descriptor.bytes)?;
        read_exact(path, descriptor)
    }
}

fn digest(value: &impl Serialize) -> Result<Blob> {
    let bytes = encode(value)?;
    Ok(Blob {
        sha256: hash(&bytes),
        bytes: bytes.len() as u64,
    })
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn transaction_record(path: &Path) -> Result<Option<PathBuf>> {
    if exists(path)? {
        return Ok(Some(path.to_owned()));
    }
    let pending = path.with_extension("pending");
    Ok(if exists(&pending)? {
        Some(pending)
    } else {
        None
    })
}
fn read_exact<T: serde::de::DeserializeOwned + Serialize>(
    path: &Path,
    expected: &Descriptor,
) -> Result<T> {
    if expected.bytes > CONTROL_BYTES || !valid_oid(&expected.sha256) {
        return Err("Invalid catalog record descriptor".into());
    }
    let file = verified(path, &expected.blob())?;
    decode(&read_limited(file, CONTROL_BYTES)?)
}
// Private bounded no-overwrite record publication. Incomplete pending bytes
// remain an explicit recovery error, never overwritten or silently discarded.
fn publish_once(path: &Path, value: &impl Serialize) -> Result<Descriptor> {
    let expected = digest(value)?;
    let pending = path.with_extension("pending");
    if expected.bytes > CONTROL_BYTES {
        return Err("Catalog control byte limit exceeded".into());
    }
    if exists(&pending)? {
        verified(&pending, &expected)?;
    } else {
        create_control(&pending, value)?;
    }
    match fs::hard_link(&pending, path) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(e) => return Err(e.into()),
    }
    verified(path, &expected)?;
    sync_directory(path.parent().ok_or("Missing catalog record parent")?)?;
    Ok(Descriptor::from(&expected))
}
fn scope_valid(scope: &Scope) -> bool {
    !scope.project_id.trim().is_empty()
        && scope.project_id.len() <= 256
        && !scope.authority_namespace.trim().is_empty()
        && scope.authority_namespace.len() <= 256
        && valid_oid(&scope.catalog_id)
}
fn shared(context: &MutationContext<'_>) -> Result<PathBuf> {
    if context.overhead_bytes < 64 * 1024 {
        return Err("Catalog mutation needs at least 64 KiB declared filesystem overhead".into());
    }
    checked_directory(
        context
            .storage
            .coordination_root
            .as_ref()
            .ok_or("Catalog mutation requires explicit shared coordination")?,
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceWire {
    encoded: Descriptor,
    decoded: Descriptor,
    encoding: String,
    source_schema: String,
    project: String,
    authority: String,
    adapter: String,
    adapter_version: u32,
}
pub(super) fn source_from_key(value: &Value) -> Result<Source> {
    let wire: SourceWire = serde_json::from_value(value.clone())?;
    if wire.adapter != ADAPTER_ID {
        return Err("Unsupported catalog source adapter".into());
    }
    let encoding = match wire.encoding.as_str() {
        "identity" => Encoding::Identity,
        "gzip" => Encoding::Gzip,
        _ => return Err("Unsupported catalog source encoding".into()),
    };
    let source = Source {
        encoded: wire.encoded.blob(),
        decoded: wire.decoded.blob(),
        encoding,
        schema: wire.source_schema,
        project_id: wire.project,
        authority_namespace: wire.authority,
        adapter_version: wire.adapter_version,
    };
    if source_key(&source) != *value
        || source.schema != SOURCE_SCHEMA
        || source.adapter_version != ADAPTER_VERSION
        || !valid_oid(&source.encoded.sha256)
        || !valid_oid(&source.decoded.sha256)
        || source.project_id.trim().is_empty()
        || source.project_id.len() > 256
        || source.authority_namespace.trim().is_empty()
        || source.authority_namespace.len() > 256
    {
        return Err("Noncanonical catalog source identity".into());
    }
    Ok(source)
}
impl Binding {
    fn source(&self) -> Result<Source> {
        source_from_key(&self.source)
    }
    fn verify(&self) -> Result<validation::ValidatedIndex> {
        let source = self.source()?;
        let (graph, receipt) = retirement::verify_anchor(
            &self.cache,
            &self.retained_parent,
            &self.root.blob(),
            &source,
            self.limits.read(),
        )?;
        if receipt != self.receipt.blob() {
            return Err("Catalog retained receipt changed".into());
        }
        Ok(graph)
    }
}
impl Candidate {
    /// Caller must admit exact root/source from a trusted builder first. This
    /// verifies existing protection, not source inclusion or creative authority.
    pub fn verify(
        cache: &Path,
        retained_parent: &Path,
        root: &Blob,
        source: &Source,
        limits: ReadLimits,
    ) -> Result<Self> {
        if limits.total_bytes > 4 * 1024 * 1024 * 1024
            || limits.nodes > 1_000_000
            || limits.rows > 1_000_000
        {
            return Err("Catalog candidate verification budget exceeds ceiling".into());
        }
        let cache = checked_directory(cache)?;
        let retained_parent = checked_directory(retained_parent)?;
        let (_, receipt) =
            retirement::verify_anchor(&cache, &retained_parent, root, source, limits)?;
        Ok(Self {
            binding: Binding {
                root: Descriptor::from(root),
                source: source_key(source),
                cache,
                retained_parent,
                receipt: Descriptor::from(&receipt),
                limits: ProofLimits {
                    node_bytes: limits.node_bytes,
                    total_bytes: limits.total_bytes,
                    nodes: limits.nodes,
                    rows: limits.rows,
                },
            },
        })
    }
}
impl Catalog {
    /// Explicit initialization in an EXISTING private metadata directory inside
    /// the selected cache. No Git path discovery, cache creation or daemon.
    pub fn initialize(
        context: &MutationContext<'_>,
        cache: &Path,
        directory_path: &Path,
        scope: &Scope,
    ) -> Result<Self> {
        if !scope_valid(scope) {
            return Err("Invalid catalog scope".into());
        }
        let cache = checked_directory(cache)?;
        let directory_path = checked_directory(directory_path)?;
        if directory_path == cache
            || !directory_path.starts_with(&cache)
            || admission::volume_id(&cache)? != admission::volume_id(&directory_path)?
        {
            return Err("Catalog must use an existing private directory inside the selected cache filesystem".into());
        }
        let coord = shared(context)?;
        if fs::read_dir(&directory_path)?.next().transpose()?.is_some() {
            return Err("Catalog initialization requires an empty private directory; never reinitialize a lineage".into());
        }
        let budget = (8 * CONTROL_BYTES)
            .checked_add(context.overhead_bytes)
            .ok_or("Catalog capacity overflow")?;
        let _reservation = admission::reserve_on(
            context.project_root,
            context.storage,
            cache.as_path(),
            budget,
        )?;
        let manifest = Manifest {
            schema: MANIFEST.into(),
            cache,
            coordination: coord,
            project: scope.project_id.clone(),
            authority: scope.authority_namespace.clone(),
            id: scope.catalog_id.clone(),
        };
        create_control(&directory_path.join("catalog.json"), &manifest)?;
        directory(&directory_path.join("members"))?;
        directory(&directory_path.join("keys"))?;
        let identity = digest(&manifest)?;
        Ok(Self {
            directory: directory_path,
            manifest,
            identity,
        })
    }
    pub fn open(cache: &Path, path: &Path, scope: &Scope) -> Result<Self> {
        if !scope_valid(scope) {
            return Err("Invalid catalog scope".into());
        }
        let cache = checked_directory(cache)?;
        let path = checked_directory(path)?;
        let manifest: Manifest = control(&path.join("catalog.json"))?;
        if manifest.schema != MANIFEST
            || manifest.cache != cache
            || path == cache
            || !path.starts_with(&cache)
            || manifest.project != scope.project_id
            || manifest.authority != scope.authority_namespace
            || manifest.id != scope.catalog_id
            || admission::volume_id(&cache)? != admission::volume_id(&path)?
        {
            return Err("Catalog identity/scope mismatch".into());
        }
        checked_directory(&path.join("members"))?;
        checked_directory(&path.join("keys"))?;
        let identity = digest(&manifest)?;
        Ok(Self {
            directory: path,
            manifest,
            identity,
        })
    }
    fn check_manifest(&self) -> Result<()> {
        checked_directory(&self.directory)?;
        let current: Manifest = read_exact(
            &self.directory.join("catalog.json"),
            &Descriptor::from(&self.identity),
        )?;
        if digest(&current)? != self.identity {
            return Err("Catalog manifest changed".into());
        }
        Ok(())
    }
    fn reserve(&self, context: &MutationContext<'_>) -> Result<admission::Reservation> {
        self.check_manifest()?;
        if shared(context)? != self.manifest.coordination {
            return Err("Catalog coordination scope changed".into());
        }
        let budget = (8 * CONTROL_BYTES)
            .checked_add(context.overhead_bytes)
            .ok_or("Catalog capacity overflow")?;
        admission::reserve_on(
            context.project_root,
            context.storage,
            &self.manifest.cache,
            budget,
        )
    }
    fn slot(&self, key: &str) -> Result<PathBuf> {
        if key.trim().is_empty() || key.len() > 256 {
            return Err("Invalid exact catalog key".into());
        }
        Ok(self.directory.join("keys").join(hash(key.as_bytes())))
    }
    fn record(&self, key: &str, kind: &str, revision: u64) -> Result<PathBuf> {
        Ok(self
            .slot(key)?
            .join(kind)
            .join(format!("{revision:020}.json")))
    }
    fn tag(&self) -> &str {
        &self.identity.sha256
    }
    fn member(&self, key: &str, meter: &mut ControlMeter) -> Result<()> {
        let member: Member = meter.read(
            &self
                .directory
                .join("members")
                .join(format!("{}.json", hash(key.as_bytes()))),
        )?;
        if member.schema != MEMBER || member.catalog != self.tag() || member.key != key {
            return Err("Catalog membership scope mismatch".into());
        }
        Ok(())
    }
    fn check_binding(&self, binding: &Binding) -> Result<()> {
        let source = binding.source()?;
        if binding.cache != self.manifest.cache
            || source.project_id != self.manifest.project
            || source.authority_namespace != self.manifest.authority
            || source.schema != SOURCE_SCHEMA
            || source.adapter_version != ADAPTER_VERSION
            || !valid_oid(&binding.root.sha256)
            || !valid_oid(&source.encoded.sha256)
            || !valid_oid(&source.decoded.sha256)
            || !valid_oid(&binding.receipt.sha256)
            || binding.receipt.bytes > CONTROL_BYTES
            || binding.limits.total_bytes > 4 * 1024 * 1024 * 1024
            || binding.limits.nodes > 1_000_000
            || binding.limits.rows > 1_000_000
        {
            return Err("Catalog root binding scope/interpretation mismatch".into());
        }
        check_node_limit(binding.limits.node_bytes)?;
        Ok(())
    }
    pub(super) fn producer_preflight(
        &self,
        cache: &Path,
        source: &Source,
        key: &str,
        expected: u64,
    ) -> Result<()> {
        self.check_manifest()?;
        if checked_directory(cache)? != self.manifest.cache
            || source.project_id != self.manifest.project
            || source.authority_namespace != self.manifest.authority
        {
            return Err("Managed producer catalog/source scope mismatch".into());
        }
        if self.read(key)?.revision != expected {
            return Err("Managed producer expected revision conflict".into());
        }
        Ok(())
    }
    pub fn create_key(&self, context: &MutationContext<'_>, key: &str) -> Result<Snapshot> {
        let _reservation = self.reserve(context)?;
        let slot = self.slot(key)?;
        match fs::create_dir(&slot) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => return self.read(key),
            Err(e) => return Err(e.into()),
        }
        let _owner = owner(&slot.join("owner.lock"), true)?;
        for name in ["states", "fences", "decisions"] {
            directory(&slot.join(name))?;
        }
        publish_once(
            &self
                .directory
                .join("members")
                .join(format!("{}.json", hash(key.as_bytes()))),
            &Member {
                schema: MEMBER.into(),
                catalog: self.tag().into(),
                key: key.into(),
            },
        )?;
        let state = State {
            schema: STATE.into(),
            catalog: self.tag().into(),
            key: key.into(),
            revision: 0,
            binding: None,
            previous: None,
            decision: None,
        };
        let descriptor = publish_once(&self.record(key, "states", 0)?, &state)?;
        self.write_head(key, 0, descriptor)?;
        self.read(key)
    }
    fn write_head(&self, key: &str, revision: u64, state: Descriptor) -> Result<()> {
        let slot = self.slot(key)?;
        let head = Head {
            schema: HEAD.into(),
            catalog: self.tag().into(),
            key: key.into(),
            revision,
            state,
        };
        let pending = slot.join("head.next");
        if exists(&pending)? {
            verified(&pending, &digest(&head)?)?;
        } else {
            create_control(&pending, &head)?;
        }
        let target = slot.join("head.json");
        if exists(&target)? {
            plain(&target, false)?;
        }
        crate::versions::replace(&pending, &target)?;
        sync_directory(&slot)
    }
    fn read_base(&self, key: &str) -> Result<Snapshot> {
        self.read_base_with(
            key,
            &mut ControlMeter {
                used: 0,
                max: 12 * CONTROL_BYTES,
            },
        )
    }
    fn read_base_with(&self, key: &str, meter: &mut ControlMeter) -> Result<Snapshot> {
        meter.charge(self.identity.bytes)?;
        self.check_manifest()?;
        checked_directory(&self.slot(key)?)?;
        self.member(key, meter)?;
        let path = self.slot(key)?.join("head.json");
        let head: Head = meter.read(&path)?;
        if head.schema != HEAD || head.catalog != self.tag() || head.key != key {
            return Err("Invalid catalog head scope".into());
        }
        let state: State = meter.exact(&self.record(key, "states", head.revision)?, &head.state)?;
        self.check_state(&state, key, head.revision)?;
        if head.revision == 0 {
            if state.binding.is_some() || state.previous.is_some() || state.decision.is_some() {
                return Err("Invalid virgin catalog state".into());
            }
        } else {
            let fence: Fence = meter.read(&self.record(key, "fences", head.revision)?)?;
            let decision: Decision = meter.exact(
                &self.record(key, "decisions", head.revision)?,
                state.decision.as_ref().ok_or("Missing catalog decision")?,
            )?;
            let previous: State = meter.exact(
                &self.record(key, "states", head.revision - 1)?,
                &fence.previous,
            )?;
            self.check_state(&previous, key, head.revision - 1)?;
            self.check_fence(&fence, key, head.revision, &digest(&previous)?)?;
            self.check_decision(&decision, key, head.revision, &digest(&fence)?)?;
            let binding = if decision.commit {
                fence.binding
            } else {
                previous.binding
            };
            let derived = State {
                schema: STATE.into(),
                catalog: self.tag().into(),
                key: key.into(),
                revision: head.revision,
                binding,
                previous: Some(fence.previous),
                decision: Some(Descriptor::from(&digest(&decision)?)),
            };
            if digest(&derived)? != head.state.blob() {
                return Err("Catalog state differs from immutable decision".into());
            }
        }
        Ok(Snapshot {
            revision: head.revision,
            state,
            head: digest(&head)?,
        })
    }
    fn check_state(&self, state: &State, key: &str, revision: u64) -> Result<()> {
        if state.schema != STATE
            || state.catalog != self.tag()
            || state.key != key
            || state.revision != revision
        {
            return Err("Invalid immutable catalog state".into());
        }
        if let Some(binding) = &state.binding {
            self.check_binding(binding)?;
        }
        Ok(())
    }
    fn check_fence(&self, fence: &Fence, key: &str, revision: u64, previous: &Blob) -> Result<()> {
        if fence.schema != FENCE
            || fence.catalog != self.tag()
            || fence.key != key
            || fence.revision != revision
            || fence.previous.blob() != *previous
        {
            return Err("Invalid catalog successor fence".into());
        }
        if let Some(binding) = &fence.binding {
            self.check_binding(binding)?;
        }
        Ok(())
    }
    fn check_decision(
        &self,
        decision: &Decision,
        key: &str,
        revision: u64,
        fence: &Blob,
    ) -> Result<()> {
        if decision.schema != DECISION
            || decision.catalog != self.tag()
            || decision.key != key
            || decision.revision != revision
            || decision.fence.blob() != *fence
        {
            return Err("Invalid catalog commit decision".into());
        }
        Ok(())
    }
    pub fn read(&self, key: &str) -> Result<Snapshot> {
        self.read_with(
            key,
            &mut ControlMeter {
                used: 0,
                max: 12 * CONTROL_BYTES,
            },
        )
    }
    fn read_with(&self, key: &str, meter: &mut ControlMeter) -> Result<Snapshot> {
        let snapshot = self.read_base_with(key, meter)?;
        let next = snapshot
            .revision
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        if transaction_record(&self.record(key, "fences", next)?)?.is_some() {
            return Err(
                "Catalog key has a pending/newer revision; explicit recovery required".into(),
            );
        }
        let head: Head = meter.read(&self.slot(key)?.join("head.json"))?;
        if digest(&head)? != snapshot.head {
            return Err("Catalog changed during read; retry".into());
        }
        Ok(snapshot)
    }
    pub fn admit(
        &self,
        context: &MutationContext<'_>,
        key: &str,
        expected_revision: u64,
        candidate: &Candidate,
    ) -> Result<Snapshot> {
        self.check_binding(&candidate.binding)?;
        if self.read(key)?.revision != expected_revision {
            return Err("Catalog compare-and-swap revision conflict".into());
        }
        candidate.binding.verify()?;
        self.update(
            context,
            key,
            expected_revision,
            Some(candidate.binding.clone()),
        )
    }
    pub fn tombstone(
        &self,
        context: &MutationContext<'_>,
        key: &str,
        expected_revision: u64,
    ) -> Result<Snapshot> {
        self.update(context, key, expected_revision, None)
    }
    fn update(
        &self,
        context: &MutationContext<'_>,
        key: &str,
        expected: u64,
        binding: Option<Binding>,
    ) -> Result<Snapshot> {
        let _reservation = self.reserve(context)?;
        let _owner = owner(&self.slot(key)?.join("owner.lock"), false)?;
        let before = self.read(key)?;
        if before.revision != expected {
            return Err("Catalog compare-and-swap revision conflict".into());
        }
        let revision = before
            .revision
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        let fence = Fence {
            schema: FENCE.into(),
            catalog: self.tag().into(),
            key: key.into(),
            revision,
            previous: Descriptor::from(&digest(&before.state)?),
            binding,
        };
        let descriptor = publish_once(&self.record(key, "fences", revision)?, &fence)?;
        let decision = Decision {
            schema: DECISION.into(),
            catalog: self.tag().into(),
            key: key.into(),
            revision,
            fence: descriptor,
            commit: true,
        };
        publish_once(&self.record(key, "decisions", revision)?, &decision)?;
        self.finish(key, &before.state, &fence, &decision)?;
        self.read(key)
    }
    fn finish(&self, key: &str, before: &State, fence: &Fence, decision: &Decision) -> Result<()> {
        let binding = if decision.commit {
            fence.binding.as_ref()
        } else {
            before.binding.as_ref()
        };
        let state = State {
            schema: STATE.into(),
            catalog: self.tag().into(),
            key: key.into(),
            revision: fence.revision,
            binding: binding.cloned(),
            previous: Some(Descriptor::from(&digest(before)?)),
            decision: Some(Descriptor::from(&digest(decision)?)),
        };
        let descriptor = publish_once(&self.record(key, "states", fence.revision)?, &state)?;
        self.write_head(key, fence.revision, descriptor)
    }
    /// Recover ONE successor, including complete pending control bytes. A valid
    /// complete decision replays; absent decisions yield an immutable abort. An
    /// immutable abort consumes the revision and preserves the predecessor root.
    /// No history replay, deletion, orphan scan or automatic startup recovery.
    pub fn recover(&self, context: &MutationContext<'_>, key: &str) -> Result<Snapshot> {
        let before = self.read_base(key)?;
        let revision = before
            .revision
            .checked_add(1)
            .ok_or("Catalog revision exhausted")?;
        let path = self.record(key, "fences", revision)?;
        let Some(fence_path) = transaction_record(&path)? else {
            return self.read(key);
        };
        let fence: Fence = control(&fence_path)?;
        self.check_fence(&fence, key, revision, &digest(&before.state)?)?;
        let decision_path = self.record(key, "decisions", revision)?;
        let observed_decision = if let Some(record) = transaction_record(&decision_path)? {
            let decision: Decision = control(&record)?;
            self.check_decision(&decision, key, revision, &digest(&fence)?)?;
            if decision.commit
                && let Some(binding) = &fence.binding
            {
                binding.verify()?;
            }
            Some(digest(&decision)?)
        } else {
            None
        };
        let _reservation = self.reserve(context)?;
        let _owner = owner(&self.slot(key)?.join("owner.lock"), false)?;
        let current = self.read_base(key)?;
        if current.head != before.head {
            return Err("Catalog changed before recovery; retry".into());
        }
        let fresh: Fence = control(
            &transaction_record(&path)?.ok_or("Catalog fence disappeared before recovery")?,
        )?;
        if digest(&fresh)? != digest(&fence)? {
            return Err("Catalog fence changed before recovery".into());
        }
        publish_once(&path, &fence)?;
        let decision = if let Some(record) = transaction_record(&decision_path)? {
            let d: Decision = control(&record)?;
            if observed_decision.as_ref() != Some(&digest(&d)?) {
                return Err("Catalog decision changed before recovery; retry".into());
            }
            self.check_decision(&d, key, revision, &digest(&fence)?)?;
            publish_once(&decision_path, &d)?;
            d
        } else {
            if observed_decision.is_some() {
                return Err("Committed catalog decision disappeared".into());
            }
            let d = Decision {
                schema: DECISION.into(),
                catalog: self.tag().into(),
                key: key.into(),
                revision,
                fence: Descriptor::from(&digest(&fence)?),
                commit: false,
            };
            publish_once(&decision_path, &d)?;
            d
        };
        self.finish(key, &before.state, &fence, &decision)?;
        self.read(key)
    }
}
