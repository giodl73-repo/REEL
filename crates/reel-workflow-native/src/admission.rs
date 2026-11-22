//! Bounded positive input claims over an EXISTING effects-enabled index.
//! No initialization, payload availability, publication, retention or selection.
mod closing;
mod response;
mod retention;
use crate::{
    NativeProfile, SnapshotInputs, error,
    qualified::{self, slots},
};
use reel_workflow::{Artifact, ErrorKind, Packet, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tig_core::{
    Digest, LogicalPath, Object, identity,
    tree::{MemoryNodes, NodeStore, Tree, TreeLimits},
};
use tig_store::{
    Key,
    metadata::{self, MetadataStore},
    source_claims::{
        self, Selection,
        projection::{self, Resources, envelope},
        protection::{
            self, Index,
            work::{self, WorkRef},
        },
    },
};

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub coordination_root: PathBuf,
    pub actor: Key,
    pub owner: Key,
    pub operation: Key,
    pub policy: Digest,
    pub store: Digest,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub schema: String,
    pub index_root: PathBuf,
    pub index_identity: Digest,
    pub index_minimum: source_claims::Checkpoint,
    pub metadata_root: PathBuf,
    pub metadata_minimum: metadata::Checkpoint,
    pub scope: Scope,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub task_key: String,
    pub attempt_id: String,
    pub scope: Scope,
    pub input: Selection,
    pub source_archive: Object,
    pub candidate: Object,
    pub activation: Object,
    pub work: WorkRef,
    pub index_checkpoint: source_claims::Checkpoint,
    pub metadata_checkpoint: metadata::Checkpoint,
}
#[derive(Serialize)]
pub struct Preview {
    pub declared_objects: usize,
    pub logical_bytes: u64,
    pub total_actor_limit: usize,
    pub changed_path_limit: usize,
    pub response_close_paths_upper_bound: usize,
    pub response_close_bound_ok: bool,
    pub response_objects_upper_bound: usize,
    pub response_object_limit: usize,
    pub response_bytes_upper_bound: u64,
    pub response_byte_limit: u64,
    pub response_budget_ok: bool,
}
pub struct Prepared {
    reader_limit: u64,
    snapshot: SnapshotInputs,
    memory: MemoryNodes,
    input: Selection,
    resources: Resources,
    candidate: Object,
    source_binding: Digest,
    attempt: String,
    profile: Profile,
    control_minimum: source_claims::Checkpoint,
}
/// Constructed by actual native work/activation/coverage readback; not from a
/// deserialized receipt. Holds the original cooperating actor fence, not bytes.
pub struct ClaimsGuard {
    _fences: Vec<reel_assets::admission::MetadataLock>,
    _prepared: Prepared,
}
impl ClaimsGuard {
    #[cfg(all(windows, feature = "film"))]
    pub(crate) fn check_receipt(&self, receipt: &Receipt) -> Result<()> {
        self._prepared.check_receipt(receipt)?;
        let (mut index, mut nodes) = self._prepared.stores(Some(receipt))?;
        self._prepared.verify_live(receipt, &mut index, &mut nodes)
    }
    #[cfg(all(windows, feature = "film"))]
    pub(crate) fn freeze_film_invocation(
        &self,
        receipt: &Receipt,
        raw: &[u8],
        recover: bool,
    ) -> Result<(std::fs::File, reel_assets::jobs::WorkspaceDirectory)> {
        use std::io::Write;
        self._prepared.check_receipt(receipt)?;
        if raw.len() > metadata::MAX_NODE_BYTES {
            return Err(error(ErrorKind::Budget, "film-invocation-bytes"));
        }
        let directory = reel_assets::jobs::open_workspace_directory(
            &self._prepared.profile.scope.coordination_root,
        )
        .map_err(|_| error(ErrorKind::Denied, "film-invocation-namespace"))?;
        let key = reel_workflow::canonical_digest(&(
            "reel.workflow-film-invocation.v1",
            &self._prepared.profile.index_identity,
            &receipt.task_key,
            &receipt.attempt_id,
        ))?;
        let path = directory.path().join(format!("film-{key}.json"));
        if !recover {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    file.write_all(raw)
                        .and_then(|_| file.sync_all())
                        .map_err(|_| {
                            error(ErrorKind::NeedsRecovery, "film-invocation-write-preserve")
                        })?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(crate::io_error(e)),
            }
        }
        let (existing, file) = crate::staging::read_held(&path, metadata::MAX_NODE_BYTES as u64)?;
        if existing != raw {
            return Err(error(ErrorKind::Conflict, "film-original-dispatch"));
        }
        Ok((file, directory))
    }
    pub(crate) fn check_source(&self, packet: &Packet, source: &NativeProfile) -> Result<()> {
        let current = Prepared::open(
            packet,
            source,
            &self._prepared.profile,
            &self._prepared.attempt,
        )?;
        if current.source_binding != self._prepared.source_binding
            || current.input != self._prepared.input
        {
            return Err(error(ErrorKind::Conflict, "claims-exact-source-required"));
        }
        Ok(())
    }
}
/// Passive original-generation recovery fence. May coexist with a native
/// Prepare; it grants no mutation capability and is not a settled ClaimsGuard.
pub(crate) struct RecoveryGuard {
    _fences: Vec<reel_assets::admission::MetadataLock>,
    _prepared: Prepared,
}
pub(crate) fn limits() -> projection::Limits {
    projection::Limits {
        objects: 512,
        logical_bytes: 1024 * 1024 * 1024,
        tree: TreeLimits::default(),
        ..projection::Limits::default()
    }
}
pub(crate) fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = serde_json::to_vec(value)
        .map_err(|_| error(ErrorKind::Invalid, "admission-control-json"))?;
    if raw.len() > metadata::MAX_NODE_BYTES {
        return Err(error(ErrorKind::Budget, "admission-control-node-bytes"));
    }
    identity::canonical_json(
        &serde_json::to_value(value)
            .map_err(|_| error(ErrorKind::Invalid, "admission-control-json"))?,
    )
    .map_err(|_| error(ErrorKind::Invalid, "admission-control-json"))
}
pub(crate) fn native_error(value: anyhow::Error) -> reel_workflow::Error {
    fn journal(e: &source_claims::Error) -> ErrorKind {
        match e {
            source_claims::Error::Busy => ErrorKind::Busy,
            source_claims::Error::Budget => ErrorKind::Budget,
            source_claims::Error::Stale => ErrorKind::Stale,
            source_claims::Error::Conflict => ErrorKind::Conflict,
            source_claims::Error::Owner
            | source_claims::Error::Restricted
            | source_claims::Error::Cancelled => ErrorKind::Denied,
            source_claims::Error::Corrupt => ErrorKind::Integrity,
            source_claims::Error::Missing => ErrorKind::Missing,
            source_claims::Error::InvalidReference => ErrorKind::Invalid,
            _ => ErrorKind::NeedsRecovery,
        }
    }
    let kind = if value
        .downcast_ref::<reel_assets::admission::MetadataBusy>()
        .is_some()
    {
        ErrorKind::Busy
    } else if let Some(e) = value.downcast_ref::<protection::Error>() {
        match e {
            protection::Error::Budget => ErrorKind::Budget,
            protection::Error::Stale => ErrorKind::Stale,
            protection::Error::Conflict => ErrorKind::Conflict,
            protection::Error::Unknown => ErrorKind::Missing,
            protection::Error::Journal(e) => journal(e),
            _ => ErrorKind::NeedsRecovery,
        }
    } else if let Some(e) = value.downcast_ref::<metadata::Error>() {
        match e {
            metadata::Error::Busy => ErrorKind::Busy,
            metadata::Error::Budget => ErrorKind::Budget,
            metadata::Error::Corrupt => ErrorKind::Integrity,
            _ => ErrorKind::NeedsRecovery,
        }
    } else if let Some(e) = value.downcast_ref::<source_claims::Error>() {
        journal(e)
    } else {
        ErrorKind::NeedsRecovery
    };
    error(kind, "native-admission-reconcile-original")
}
fn put(memory: &mut MemoryNodes, raw: &[u8]) -> Result<Object> {
    if raw.len() > metadata::MAX_NODE_BYTES {
        return Err(error(ErrorKind::Budget, "admission-control-node-bytes"));
    }
    memory
        .put(raw)
        .map_err(|_| error(ErrorKind::Integrity, "admission-memory-node"))
}
impl Prepared {
    /// Metadata only. Caller-supplied operational store/controller enrollment is
    /// trusted admission, as in the native SDK; no store discovery or creation.
    pub fn open(
        packet: &Packet,
        source: &NativeProfile,
        profile: &Profile,
        attempt: &str,
    ) -> Result<Self> {
        if profile.schema != "reel.workflow-native-admission.v1"
            || attempt.is_empty()
            || attempt.len() > 128
            || matches!(attempt, "." | "..")
            || !attempt
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        {
            return Err(error(ErrorKind::Invalid, "admission-profile-attempt"));
        }
        if [
            &profile.index_root,
            &profile.metadata_root,
            &profile.scope.coordination_root,
        ]
        .iter()
        .any(|p| !p.is_absolute() || !p.is_dir())
        {
            return Err(error(ErrorKind::Missing, "admission-existing-roots"));
        }
        let control = source
            .source
            .pool
            .control
            .as_ref()
            .ok_or_else(|| error(ErrorKind::Denied, "admission-managed-store-required"))?;
        if control.index != profile.index_identity
            || control.store != profile.scope.store
            || source.source.store != profile.scope.store
            || std::fs::canonicalize(&control.root).map_err(crate::io_error)?
                != std::fs::canonicalize(&profile.index_root).map_err(crate::io_error)?
        {
            return Err(error(
                ErrorKind::Conflict,
                "admission-store-controller-binding",
            ));
        }
        if source.source.pool.git != source.snapshot.git.path
            || source.source.pool.executable != source.annex.path
        {
            return Err(error(
                ErrorKind::Conflict,
                "admission-provider-tool-binding",
            ));
        }
        if source.source.maximum_bytes == 0
            || source.source.maximum_bytes > 64 * 1024 * 1024 * 1024
            || packet
                .task
                .inputs
                .iter()
                .any(|input| input.bytes > source.source.maximum_bytes)
        {
            return Err(error(ErrorKind::Budget, "admission-native-reader-bound"));
        }
        let snapshot = SnapshotInputs::open(packet, &source.snapshot)?;
        let mut runtime: BTreeMap<String, Object> = BTreeMap::new();
        for (name, tool) in [("git", &snapshot.git), ("annex", &source.annex)] {
            if tool.bytes == 0 || tool.bytes > 256 * 1024 * 1024 {
                return Err(error(ErrorKind::Budget, "admission-runtime-pin-bytes"));
            }
            runtime.insert(
                name.to_owned(),
                Object {
                    sha256: Digest::parse(&tool.sha256)
                        .map_err(|_| error(ErrorKind::Invalid, "admission-runtime-pin"))?,
                    bytes: tool.bytes,
                },
            );
        }
        if let Some(tool) = &packet.task.tool {
            runtime.insert(
                "producer".to_owned(),
                Object {
                    sha256: Digest::parse(&tool.executable.sha256)
                        .map_err(|_| error(ErrorKind::Invalid, "admission-producer-pin"))?,
                    bytes: tool.executable.bytes,
                },
            );
            for (name, executable) in &tool.auxiliary_executables {
                let role = format!("auxiliary/{name}");
                LogicalPath::parse(&format!("runtime/{role}"))
                    .map_err(|_| error(ErrorKind::Invalid, "admission-runtime-path"))?;
                runtime.insert(
                    role,
                    Object {
                        sha256: Digest::parse(&executable.sha256).map_err(|_| {
                            error(ErrorKind::Invalid, "admission-auxiliary-runtime-pin")
                        })?,
                        bytes: executable.bytes,
                    },
                );
            }
        }
        let mut memory = MemoryNodes::default();
        for (_, bytes) in snapshot.metadata.objects() {
            put(&mut memory, bytes)?;
        }
        let packet_object = put(&mut memory, &encode(packet)?)?;
        let declaration = put(&mut memory, &source.snapshot.declaration_bytes()?)?;
        let descriptor = encode(
            &serde_json::json!({"schema":"reel.workflow-input-projection.v1","task_key":packet.task_key,"attempt_id":attempt,
            "source_checkpoint":snapshot.checkpoint,"source_archive":snapshot.archive,"packet":packet_object,"native_declaration":declaration,
            "index":profile.index_identity,"actor":profile.scope.actor,"owner":profile.scope.owner,"operation":profile.scope.operation,
            "store":profile.scope.store,"policy":profile.scope.policy,"runtime":runtime}),
        )?;
        let candidate = put(&mut memory, &descriptor)?;
        let mut entries = BTreeMap::from([
            (LogicalPath::parse("control/packet").unwrap(), packet_object),
            (
                LogicalPath::parse("control/native-declaration").unwrap(),
                declaration,
            ),
            (
                LogicalPath::parse("control/source-archive").unwrap(),
                snapshot.archive.clone(),
            ),
            (
                LogicalPath::parse("control/admission").unwrap(),
                candidate.clone(),
            ),
        ]);
        for (id, object) in &snapshot.objects {
            entries.insert(
                LogicalPath::parse(&format!("inputs/{id}"))
                    .map_err(|_| error(ErrorKind::Invalid, "admission-input-path"))?,
                object.clone(),
            );
        }
        for (name, object) in &runtime {
            entries.insert(
                LogicalPath::parse(&format!("runtime/{name}"))
                    .map_err(|_| error(ErrorKind::Invalid, "admission-runtime-path"))?,
                object.clone(),
            );
        }
        let input = Selection {
            root: Tree::build(&entries, &mut memory, TreeLimits::default())
                .map_err(|_| error(ErrorKind::Budget, "admission-input-tree"))?
                .0
                .root()
                .clone(),
            context: candidate.sha256.clone(),
        };
        let mut objects: BTreeSet<_> = memory
            .objects()
            .map(|(_, b)| Object::from_bytes(b))
            .collect();
        objects.extend(snapshot.declared_objects.iter().cloned());
        objects.extend(runtime.values().cloned());
        let resources = Resources::admitted(&input, objects);
        // Conservative admission bound; never split one transition into batches.
        if resources.objects().len() + 4 > protection::MAX_CHANGES {
            return Err(error(ErrorKind::Budget, "admission-atomic-claim-bound"));
        }
        Ok(Self {
            reader_limit: source.source.maximum_bytes,
            snapshot,
            memory,
            input,
            resources,
            candidate,
            source_binding: Digest::blob(&descriptor),
            attempt: attempt.into(),
            profile: profile.clone(),
            control_minimum: control.minimum.clone(),
        })
    }
    fn stores(&self, receipt: Option<&Receipt>) -> Result<(Index, MetadataStore)> {
        self.stores_with_pending(receipt, false)
    }
    fn stores_with_pending(
        &self,
        receipt: Option<&Receipt>,
        allow_pending: bool,
    ) -> Result<(Index, MetadataStore)> {
        let p = &self.profile;
        let check = Index::open(&p.index_root, &self.control_minimum)
            .map_err(|e| native_error(e.into()))?;
        if check.identity() != &p.index_identity || !check.supports_effect_fences() {
            return Err(error(ErrorKind::Denied, "admission-index-controller"));
        }
        drop(check);
        let mut index =
            Index::open(&p.index_root, &p.index_minimum).map_err(|e| native_error(e.into()))?;
        let mut nodes = MetadataStore::open(&p.metadata_root, &p.metadata_minimum)
            .map_err(|e| native_error(e.into()))?;
        if let Some(r) = receipt {
            drop(index);
            drop(nodes);
            index = Index::open(&p.index_root, &r.index_checkpoint)
                .map_err(|e| native_error(e.into()))?;
            nodes = MetadataStore::open(&p.metadata_root, &r.metadata_checkpoint)
                .map_err(|e| native_error(e.into()))?;
        }
        let observed = Index::observe(&p.index_root, &index.checkpoint(), &nodes)
            .map_err(|e| native_error(e.into()))?;
        // Original qualified work may need to reconcile its native Prepare.
        // Do not block that acceptance retry; SDK work owns exact reconciliation.
        // A live ClaimsGuard, however, requires a settled observed index.
        if observed.index != p.index_identity
            || observed.cancelled
            || (observed.pending && receipt.is_some() && !allow_pending)
        {
            return Err(error(ErrorKind::NeedsRecovery, "admission-index-not-ready"));
        }
        let enrollment = index.enrollment_profile();
        let actor = enrollment
            .actors
            .iter()
            .find(|a| a.id == p.scope.actor)
            .ok_or_else(|| error(ErrorKind::Denied, "admission-unenrolled-actor"))?;
        if actor.owner != p.scope.owner
            || !actor.stores.contains(&p.scope.store)
            || !enrollment.policies.contains(&p.scope.policy)
            || index.routing().is_none_or(|r| r.actor == p.scope.actor)
        {
            return Err(error(ErrorKind::Denied, "admission-actor-scope"));
        }
        Ok((index, nodes))
    }
    pub fn preview(&self) -> Result<Preview> {
        let (mut index, mut nodes) = self.stores(None)?;
        let mut memory = MemoryNodes::default();
        let prepared = envelope::create(
            &self.resources,
            &self.profile.scope.policy,
            &mut memory,
            limits(),
        )
        .map_err(|e| native_error(e.into()))?;
        let marker = slots::activation(
            &index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &self.candidate,
        )
        .map_err(native_error)?;
        slots::accept(
            &mut index,
            &mut nodes,
            &self.profile.scope,
            &prepared,
            &marker,
            limits(),
            false,
        )
        .map_err(native_error)?;
        let logical_bytes = self
            .resources
            .objects()
            .iter()
            .try_fold(0u64, |n, o| n.checked_add(o.bytes))
            .ok_or_else(|| error(ErrorKind::Budget, "admission-byte-total"))?;
        Ok(Preview {
            declared_objects: self.resources.objects().len(),
            logical_bytes,
            total_actor_limit: 16,
            changed_path_limit: protection::MAX_CHANGES,
            response_close_paths_upper_bound: self.response_close_paths()?,
            response_close_bound_ok: self.response_close_paths()? <= protection::MAX_CHANGES,
            response_objects_upper_bound: self.response_objects(),
            response_object_limit: 32,
            response_bytes_upper_bound: self.response_bytes()?,
            response_byte_limit: 128 * 1024 * 1024,
            response_budget_ok: self.response_budget_ok()?,
        })
    }
    /// Explicitly apply original qualified slot work. On uncertainty retry this
    /// exact profile/task/attempt/operation; never choose a replacement identity.
    pub fn accept(&self) -> Result<Receipt> {
        if !self.preview()?.response_close_bound_ok {
            return Err(error(
                ErrorKind::Budget,
                "admission-response-close-path-bound",
            ));
        }
        if !self.response_budget_ok()? {
            return Err(error(
                ErrorKind::Budget,
                "admission-response-envelope-bound",
            ));
        }
        let (mut index, mut nodes) = self.stores(None)?;
        for (_, bytes) in self.memory.objects() {
            nodes
                .put(bytes)
                .map_err(|_| error(ErrorKind::NeedsRecovery, "admission-metadata-write"))?;
        }
        let prepared = envelope::create(
            &self.resources,
            &self.profile.scope.policy,
            &mut nodes,
            limits(),
        )
        .map_err(|e| native_error(e.into()))?;
        let minimum = nodes.finish().map_err(|e| native_error(e.into()))?;
        drop(nodes);
        let mut nodes = MetadataStore::open(&self.profile.metadata_root, &minimum)
            .map_err(|e| native_error(e.into()))?;
        // SDK restoration checks exact declared resources before admission.
        let prepared = envelope::restore(
            prepared.selection(),
            &self.resources,
            &self.profile.scope.policy,
            &mut nodes,
            limits(),
        )
        .map_err(|e| native_error(e.into()))?;
        let marker = slots::activation(
            &index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &self.candidate,
        )
        .map_err(native_error)?;
        let accepted = slots::accept(
            &mut index,
            &mut nodes,
            &self.profile.scope,
            &prepared,
            &marker,
            limits(),
            true,
        )
        .map_err(native_error)?;
        let work = accepted
            .work
            .ok_or_else(|| error(ErrorKind::NeedsRecovery, "admission-original-work-missing"))?;
        Ok(Receipt {
            schema: "reel.workflow-native-admission-receipt.v1".into(),
            task_key: self.snapshot.packet.task_key.clone(),
            attempt_id: self.attempt.clone(),
            scope: self.profile.scope.clone(),
            input: self.input.clone(),
            source_archive: self.snapshot.archive.clone(),
            candidate: self.candidate.clone(),
            activation: accepted.activation,
            work,
            index_checkpoint: index.checkpoint(),
            metadata_checkpoint: nodes.finish().map_err(|e| native_error(e.into()))?,
        })
    }
    /// Actual readback of original completed WorkRef, activation and positive
    /// coverage. The returned fence must outlive any operation relying on it.
    pub fn reopen(self, receipt: &Receipt) -> Result<ClaimsGuard> {
        self.check_receipt(receipt)?;
        self.reopen_checked(receipt, None)
    }
    fn acceptance_operation(&self, index: &Index) -> Result<Key> {
        Key::parse(&format!(
            "slot-accept-{}",
            Digest::blob(&encode(&(
                index.identity(),
                &self.profile.scope.actor,
                &self.profile.scope.operation
            ))?)
            .as_str()
        ))
        .map_err(|_| error(ErrorKind::Invalid, "admission-operation-key"))
    }
    fn response_close_paths(&self) -> Result<usize> {
        // Slot acceptance adds at most four control paths. Registration adds
        // output objects plus evidence/root/envelope identity. Count unknown
        // hashes separately; deduplication is not an admission guarantee.
        self.resources
            .objects()
            .len()
            .checked_add(4)
            .and_then(|count| count.checked_add(self.response_objects()))
            .and_then(|count| count.checked_add(3))
            .ok_or_else(|| error(ErrorKind::Budget, "admission-response-close-overflow"))
    }
    fn response_bytes(&self) -> Result<u64> {
        self.snapshot
            .packet
            .task
            .outputs
            .values()
            .try_fold(0u64, |total, output| total.checked_add(output.max_bytes))
            .and_then(|n| n.checked_add(self.worker_bytes()))
            .ok_or_else(|| error(ErrorKind::Budget, "admission-response-byte-overflow"))
    }
    fn worker_bytes(&self) -> u64 {
        if self.snapshot.packet.task.tool.is_some() {
            metadata::MAX_NODE_BYTES as u64
        } else {
            0
        }
    }
    fn response_objects(&self) -> usize {
        self.snapshot.packet.task.outputs.len()
            + usize::from(self.snapshot.packet.task.tool.is_some())
    }
    fn response_budget_ok(&self) -> Result<bool> {
        let outputs = self.response_bytes()?;
        let inputs = self
            .resources
            .objects()
            .iter()
            .try_fold(0u64, |total, object| total.checked_add(object.bytes));
        // Reserve worst-case metadata-node overhead inside the same envelope.
        let reserve = protection::MAX_CHANGES as u64 * metadata::MAX_NODE_BYTES as u64;
        Ok(self.response_objects() <= 32
            && self.worker_bytes() <= self.reader_limit
            && self
                .snapshot
                .packet
                .task
                .outputs
                .values()
                .all(|output| output.max_bytes <= self.reader_limit)
            && outputs <= 128 * 1024 * 1024
            && inputs
                .and_then(|n| n.checked_add(outputs))
                .and_then(|n| n.checked_add(reserve))
                .is_some_and(|n| n <= limits().logical_bytes))
    }
    fn check_receipt(&self, receipt: &Receipt) -> Result<()> {
        if receipt.schema != "reel.workflow-native-admission-receipt.v1"
            || receipt.task_key != self.snapshot.packet.task_key
            || receipt.attempt_id != self.attempt
            || receipt.scope != self.profile.scope
            || receipt.input != self.input
            || receipt.source_archive != self.snapshot.archive
            || receipt.candidate != self.candidate
        {
            return Err(error(
                ErrorKind::Conflict,
                "admission-original-receipt-binding",
            ));
        }
        Ok(())
    }
    fn reopen_checked(self, receipt: &Receipt, companion: Option<&Key>) -> Result<ClaimsGuard> {
        let (mut index, mut nodes) = self.stores(Some(receipt))?;
        let fences = self.actor_fences(&index, companion)?;
        self.verify_live(receipt, &mut index, &mut nodes)?;
        Ok(ClaimsGuard {
            _fences: fences,
            _prepared: self,
        })
    }
    pub(crate) fn recovery_guard(self, receipt: &Receipt) -> Result<RecoveryGuard> {
        self.check_receipt(receipt)?;
        let (mut index, mut nodes) = self.stores_with_pending(Some(receipt), true)?;
        let fences = self.actor_fences(&index, None)?;
        self.verify_live(receipt, &mut index, &mut nodes)?;
        Ok(RecoveryGuard {
            _fences: fences,
            _prepared: self,
        })
    }
    fn verify_live(
        &self,
        receipt: &Receipt,
        index: &mut Index,
        nodes: &mut MetadataStore,
    ) -> Result<()> {
        let expected = slots::activation(
            index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &self.candidate,
        )
        .map_err(native_error)?;
        if Object::from_bytes(&expected) != receipt.activation {
            return Err(error(ErrorKind::Conflict, "admission-activation-candidate"));
        }
        slots::verify_activation(
            index,
            &self.profile.scope,
            &self.input,
            &self.source_binding,
            &receipt.activation,
            nodes,
        )
        .map_err(native_error)?;
        let operation = Key::parse(&format!(
            "slot-accept-{}",
            Digest::blob(&encode(&(
                index.identity(),
                &self.profile.scope.actor,
                &self.profile.scope.operation
            ))?)
            .as_str()
        ))
        .map_err(|_| error(ErrorKind::Invalid, "admission-operation-key"))?;
        if work::lookup(index, &operation)
            .map_err(|e| native_error(e.into()))?
            .as_ref()
            != Some(&receipt.work)
        {
            return Err(error(
                ErrorKind::Conflict,
                "admission-original-work-reference",
            ));
        }
        let record = work::read(index, &receipt.work, nodes).map_err(|e| native_error(e.into()))?;
        if !work::complete(index, &receipt.work).map_err(|e| native_error(e.into()))?
            || record.evidence.as_ref() != Some(&receipt.activation)
            || record.update.actor != self.profile.scope.actor
            || record.update.owner != self.profile.scope.owner
            || !record.update.ready
            || record.additional.is_some()
            || record.adoption.is_some()
        {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "admission-original-work-not-complete",
            ));
        }
        let current = index
            .actor(nodes, &self.profile.scope.actor)
            .map_err(|e| native_error(e.into()))?;
        let binding = envelope::read_binding(&current.selection, nodes, limits())
            .map_err(|e| native_error(e.into()))?;
        let entries = Tree::from_root(current.selection.root)
            .entries(nodes, TreeLimits::default())
            .map_err(|_| error(ErrorKind::Integrity, "admission-current-tree"))?
            .0;
        let present: BTreeSet<_> = entries.into_values().collect();
        if !current.ready
            || binding.native != self.input
            || binding.policy != self.profile.scope.policy
            || !present.contains(&receipt.activation)
            || !self.resources.objects().is_subset(&present)
            || !present.contains(&self.input.root)
        {
            return Err(error(ErrorKind::Stale, "admission-generation-or-coverage"));
        }
        Ok(())
    }
    fn actor_fences(
        &self,
        index: &Index,
        companion: Option<&Key>,
    ) -> Result<Vec<reel_assets::admission::MetadataLock>> {
        let mut actors = vec![&self.profile.scope.actor];
        if let Some(actor) = companion {
            actors.push(actor);
        }
        actors.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        actors.dedup();
        actors
            .into_iter()
            .map(|actor| {
                qualified::prepublication::actor_fence(
                    index.identity(),
                    &self.profile.scope.coordination_root,
                    actor,
                )
                .map_err(native_error)
            })
            .collect()
    }
}
impl Receipt {
    /// Stable original operation identity, independent of later witness minima.
    pub fn operation_identity(&self) -> Result<String> {
        Ok(reel_workflow::digest(&encode(
            &serde_json::json!({"schema":"reel.workflow-native-operation.v1",
            "task_key":self.task_key,"attempt_id":self.attempt_id,"scope":self.scope,"input":self.input,
            "source_archive":self.source_archive,"candidate":self.candidate,"activation":self.activation,"work":self.work}),
        )?))
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    /// Exact private operational packet pin for a handoff ticket. This descriptor
    /// alone is not a guard; callers must reopen actual native work before use.
    pub fn artifact(&self) -> Result<Artifact> {
        let bytes = self.canonical_bytes()?;
        Ok(Artifact {
            logical_id: format!("workflow/admission/{}/{}", self.task_key, self.attempt_id),
            sha256: reel_workflow::digest(&bytes),
            bytes: bytes.len() as u64,
            media_type: "application/json".into(),
        })
    }
}
