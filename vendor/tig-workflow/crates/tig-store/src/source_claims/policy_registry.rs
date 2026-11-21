//! Explicit native owner-admitted standing policy. Original journal engine,
//! bounded pre-enrolled slots and per-slot effective holds; no payload IO.
//! This is neither cryptographic owner authentication nor automatic execution.
use super::*;
use crate::{metadata::MetadataStore, provider::profile::Profile};
use std::{collections::BTreeSet, path::PathBuf};
use tig_core::{retirement::review::routine_policy::Policy, tree::NodeStore};
mod catalogue;
pub mod effects;
mod run_budget;
mod storage;
pub use catalogue::{PlaceScope, ProviderCatalogue};
use storage::*;

const PROJECT: &str = "routine-policy-registry";
const RECORD: &str = "tig.native-standing-policy.experimental-1";
pub const MAX_SLOTS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    owner: Key,
    subject: Digest,
    namespace: Digest,
    source: Digest,
    authority: Digest,
    epoch: Digest,
    stores: BTreeSet<Digest>,
    placements: BTreeSet<Digest>,
    slots: Vec<Key>,
    #[serde(default, skip_serializing_if = "super::unbound")]
    pub(super) accounted: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "effects::read_source"
    )]
    pub(super) effect_index: Option<effects::Source>,
}
impl Binding {
    pub fn from_head(
        index: &protection::Index,
        head: &coordinated::accepted_head::HeldHead,
        mut slots: Vec<Key>,
    ) -> RegistryResult<Self> {
        if slots.is_empty() || slots.len() > MAX_SLOTS {
            return Err(RegistryError::Journal(Error::Budget));
        }
        slots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        if &head.frame().authority != index.identity() {
            return Err(RegistryError::Scope);
        }
        let profile = index.enrollment_profile();
        let binding = Self {
            owner: profile.owner.clone(),
            subject: profile.subject.clone(),
            namespace: head.journal().clone(),
            source: head.frame().source.clone(),
            authority: head.frame().authority.clone(),
            epoch: head.frame().epoch.clone(),
            stores: profile.stores.iter().cloned().collect(),
            placements: profile.policies.iter().cloned().collect(),
            slots,
            accounted: false,
            effect_index: None,
        };
        binding.validate().map_err(RegistryError::Journal)?;
        Ok(binding)
    }
    pub(super) fn validate(&self) -> super::Result<()> {
        if self.effect_index.is_some() && !self.accounted {
            return Err(Error::Corrupt);
        }
        if let Some(source) = &self.effect_index
            && (source.index != self.namespace
                || source.index != self.authority
                || source.index != self.epoch)
        {
            return Err(Error::Corrupt);
        }
        if self.slots.is_empty()
            || self.slots.len() > MAX_SLOTS
            || self.stores.is_empty()
            || self.stores.len() > 16
            || self.placements.is_empty()
            || self.placements.len() > 16
        {
            return Err(Error::Budget);
        }
        if slots_not_ordered(&self.slots) {
            return Err(Error::Corrupt);
        }
        Ok(())
    }
    pub fn owner(&self) -> &Key {
        &self.owner
    }
    pub fn namespace(&self) -> &Digest {
        &self.namespace
    }
    fn source_key(&self, slot: &Key) -> RegistryResult<Key> {
        if !self.slots.contains(slot) {
            return Err(RegistryError::Scope);
        }
        slot_key(slot).map_err(RegistryError::Journal)
    }
    fn check_policy(&self, policy: &Policy) -> RegistryResult<()> {
        let p = policy.config();
        if p.owner != self.subject
            || p.namespace != self.namespace
            || p.source != self.source
            || p.authority != self.authority
            || p.epoch != self.epoch
            || !p.removable.is_subset(&self.stores)
            || !p.required_survivors.is_subset(&self.stores)
            || !self.placements.contains(&p.placement_policy)
        {
            return Err(RegistryError::Scope);
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum RegistryError {
    Journal(Error),
    Scope,
    Encoding,
    Metadata,
    Missing,
    Pending,
    Revoked,
    Stale,
    Injected,
}
pub type RegistryResult<T> = std::result::Result<T, RegistryError>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    schema: String,
    policy: Object,
    providers: Object,
}
impl Registration {
    pub fn new(policy: Object, providers: Object) -> RegistryResult<Self> {
        check_ref(&policy, 65_536)?;
        check_ref(&providers, 65_536)?;
        Ok(Self {
            schema: RECORD.into(),
            policy,
            providers,
        })
    }
    fn bytes(&self) -> RegistryResult<Vec<u8>> {
        if self.schema != RECORD {
            return Err(RegistryError::Encoding);
        }
        check_ref(&self.policy, 65_536)?;
        check_ref(&self.providers, 65_536)?;
        encode(self).map_err(RegistryError::Journal)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub slot: Key,
    pub owner: Key,
    pub operation: Key,
    pub expected_revision: u64,
    pub registration: Registration,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateStage {
    MetadataReady,
    Prepared,
    Adopted,
}

pub struct Registry {
    root: PathBuf,
    journal: SourceJournal,
    binding: Binding,
}
impl Registry {
    /// Explicit new control metadata only. No attach, migration or cache setup.
    pub fn create_accounted(root: &Path, epoch: Key, mut binding: Binding) -> RegistryResult<Self> {
        binding.accounted = true;
        Self::create(root, epoch, binding)
    }
    pub fn create(root: &Path, epoch: Key, binding: Binding) -> RegistryResult<Self> {
        binding.validate().map_err(RegistryError::Journal)?;
        let journal = SourceJournal::create_profiles(
            root,
            Key::parse(PROJECT).unwrap(),
            epoch,
            Profiles {
                registry: Some(binding.clone()),
                ..Profiles::default()
            },
        )
        .map_err(RegistryError::Journal)?;
        Ok(Self {
            root: fs::canonicalize(root).map_err(|_| RegistryError::Metadata)?,
            journal,
            binding,
        })
    }
    pub fn open(root: &Path, minimum: &Checkpoint) -> RegistryResult<Self> {
        let journal = SourceJournal::open(root, minimum).map_err(RegistryError::Journal)?;
        let binding = journal.registry.clone().ok_or(RegistryError::Scope)?;
        Ok(Self {
            root: fs::canonicalize(root).map_err(|_| RegistryError::Metadata)?,
            journal,
            binding,
        })
    }
    pub fn identity(&self) -> &Digest {
        self.journal.identity()
    }
    pub fn checkpoint(&self) -> Checkpoint {
        self.journal.checkpoint()
    }
    pub fn binding(&self) -> &Binding {
        &self.binding
    }
    /// Point control observation only. Current metadata is no permission lease.
    pub fn state(&mut self, slot: &Key) -> RegistryResult<Option<Source>> {
        self.journal
            .source(&self.binding.source_key(slot)?)
            .map_err(RegistryError::Journal)
    }
    fn issue(&mut self, operation: Key, command: Command) -> RegistryResult<Receipt> {
        let result = self
            .journal
            .apply_scoped(operation, command, |_| Ok(()), false, true);
        self.journal.mark_uncertain(&result);
        result.map_err(RegistryError::Journal)
    }
    pub fn update(
        &mut self,
        request: &Update,
        nodes: &mut MetadataStore,
    ) -> RegistryResult<Receipt> {
        self.update_hook(request, nodes, |_| Ok(()))
    }
    /// Hooks delimit acknowledged metadata and original committed journal phases.
    pub fn update_hook(
        &mut self,
        request: &Update,
        nodes: &mut MetadataStore,
        mut hook: impl FnMut(UpdateStage) -> RegistryResult<()>,
    ) -> RegistryResult<Receipt> {
        if request.owner != self.binding.owner {
            return Err(RegistryError::Scope);
        }
        let source = self.binding.source_key(&request.slot)?;
        let _writer = SlotGuard::acquire(&self.root, &source, false)?;
        let bytes = request.registration.bytes()?;
        let root = Object::from_bytes(&bytes);
        let selection = Selection {
            root: root.clone(),
            context: Digest::blob(
                &encode(&(
                    RECORD,
                    self.identity(),
                    &source,
                    &request.operation,
                    request.expected_revision,
                    &root,
                ))
                .map_err(RegistryError::Journal)?,
            ),
        };
        let next = request
            .expected_revision
            .checked_add(1)
            .ok_or(RegistryError::Scope)?;
        let prepare = command_key(
            self.identity(),
            &request.slot,
            &request.operation,
            "prepare",
        )?;
        let adopt = command_key(self.identity(), &request.slot, &request.operation, "adopt")?;
        let reference = Reference {
            journal: self.identity().clone(),
            operation: prepare.clone(),
        };
        let adoption = Command::Adopt {
            source: source.clone(),
            owner: request.owner.clone(),
            expected_revision: next,
            prepared: reference.clone(),
        };
        let preparation = Command::Prepare {
            source: source.clone(),
            owner: request.owner.clone(),
            expected_revision: request.expected_revision,
            selection: selection.clone(),
        };
        let prepared_before = self
            .journal
            .replay(prepare.clone(), preparation.clone())
            .map_err(RegistryError::Journal)?;
        // Historical exact acceptance is not current permission and reads no node.
        if let Some(receipt) = self
            .journal
            .replay(adopt.clone(), adoption.clone())
            .map_err(RegistryError::Journal)?
        {
            if prepared_before.is_none() {
                return Err(RegistryError::Encoding);
            }
            return Ok(receipt);
        }
        let state = self
            .journal
            .source(&source)
            .map_err(RegistryError::Journal)?;
        if let Some(s) = &state {
            if s.cancelled {
                return Err(RegistryError::Revoked);
            }
            if s.owner != request.owner {
                return Err(RegistryError::Scope);
            }
            if s.pending
                .as_ref()
                .is_some_and(|p| p.reference != reference || p.selection != selection)
            {
                return Err(RegistryError::Pending);
            }
            if s.revision != request.expected_revision + u64::from(s.pending.is_some()) {
                return Err(RegistryError::Stale);
            }
        } else if request.expected_revision != 0 {
            return Err(RegistryError::Stale);
        }
        let (policy, _) = load_registration(nodes, &self.binding, &request.registration)?;
        if let Some(current) = state.as_ref().and_then(|s| s.current.as_ref()) {
            let previous = read_registration(nodes, &current.selection.root)?;
            let (old, _) = load_registration(nodes, &self.binding, &previous)?;
            if previous != request.registration && policy.config().version <= old.config().version {
                return Err(RegistryError::Scope);
            }
        }
        if nodes.put(&bytes).map_err(|_| RegistryError::Metadata)? != root {
            return Err(RegistryError::Metadata);
        }
        nodes.finish().map_err(|_| RegistryError::Metadata)?;
        hook(UpdateStage::MetadataReady)?;
        self.issue(prepare, preparation)?;
        hook(UpdateStage::Prepared)?;
        let receipt = self.issue(adopt, adoption)?;
        hook(UpdateStage::Adopted)?;
        Ok(receipt)
    }
    /// Permanent stop for this enrolled slot, using original reserved cancellation
    /// headroom. No policy/catalogue/metadata nodes are needed. New enrollment is
    /// required to enable a revoked slot; historical receipts remain replayable.
    pub fn revoke(
        &mut self,
        slot: &Key,
        owner: &Key,
        expected_revision: u64,
        operation: Key,
    ) -> RegistryResult<Receipt> {
        if owner != &self.binding.owner {
            return Err(RegistryError::Scope);
        }
        let source = self.binding.source_key(slot)?;
        let _writer = SlotGuard::acquire(&self.root, &source, false)?;
        self.issue(
            operation,
            Command::Cancel {
                source,
                owner: owner.clone(),
                expected_revision,
            },
        )
    }
    pub fn hold(
        &mut self,
        slot: &Key,
        expected: Option<&Version>,
        nodes: &impl NodeStore,
    ) -> RegistryResult<HeldPolicy> {
        let source = self.binding.source_key(slot)?;
        let lease = SlotGuard::acquire(&self.root, &source, true)?;
        let state = self
            .journal
            .source(&source)
            .map_err(RegistryError::Journal)?
            .ok_or(RegistryError::Missing)?;
        if state.owner != self.binding.owner {
            return Err(RegistryError::Scope);
        }
        if state.cancelled {
            return Err(RegistryError::Revoked);
        }
        if state.pending.is_some() {
            return Err(RegistryError::Pending);
        }
        let version = state.current.ok_or(RegistryError::Missing)?;
        if expected.is_some_and(|e| e != &version) {
            return Err(RegistryError::Stale);
        }
        let record = read_registration(nodes, &version.selection.root)?;
        let (policy, providers) = load_registration(nodes, &self.binding, &record)?;
        Ok(HeldPolicy {
            _lease: lease,
            registry: self.identity().clone(),
            slot: slot.clone(),
            version,
            policy,
            owner: self.binding.owner.clone(),
            catalogue: providers,
        })
    }
}

/// Effective native registration read hold only. It grants neither Decision nor
/// provider custody/current source/restore truth. No serialization or cloning.
pub struct HeldPolicy {
    _lease: SlotGuard,
    registry: Digest,
    slot: Key,
    version: Version,
    policy: Policy,
    owner: Key,
    catalogue: ProviderCatalogue,
}
impl HeldPolicy {
    pub fn owner(&self) -> &Key {
        &self.owner
    }
    pub fn registry(&self) -> &Digest {
        &self.registry
    }
    pub fn slot(&self) -> &Key {
        &self.slot
    }
    pub fn version(&self) -> &Version {
        &self.version
    }
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
    pub fn providers(&self) -> &BTreeMap<Digest, Profile> {
        self.catalogue.profiles()
    }
    pub fn catalogue(&self) -> &ProviderCatalogue {
        &self.catalogue
    }
}

pub(super) fn command_scope(binding: &Binding, command: &Command) -> super::Result<()> {
    let (source, owner) = match command {
        Command::Prepare { source, owner, .. }
        | Command::Adopt { source, owner, .. }
        | Command::Cancel { source, owner, .. } => (source, owner),
        _ => return Err(Error::Restricted),
    };
    if owner != &binding.owner
        || !binding.slots.iter().any(|s| {
            slot_key(s).as_ref() == Ok(source)
                || (binding.accounted && run_budget::source_key(s).as_ref() == Ok(source))
        })
    {
        return Err(Error::Restricted);
    }
    Ok(())
}

fn slots_not_ordered(slots: &[Key]) -> bool {
    slots
        .windows(2)
        .any(|pair| pair[0].as_str() >= pair[1].as_str())
}
