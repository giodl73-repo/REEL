//! Private successor framing/base proofs. No SQL seal, activation or write grant.
use super::*;
use crate::metadata::{Checkpoint as MetadataMinimum, MetadataReader};
use git_asset_core::{
    LogicalPath, Object,
    capacity::checkpoint as state,
    tree::{NodeStore, Tree, TreeLimits},
};
use history::{Error, Limits as ReadLimits, Meter, Result, index::Index};

const MAX_NUMBER: u64 = 9_007_199_254_740_991;

fn object_bound(object: &Object) -> Result<()> {
    if object.bytes == 0 || object.bytes > 65536 {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn checkpoint_bound(checkpoint: &Checkpoint) -> Result<()> {
    if checkpoint.sequence > MAX_NUMBER
        || checkpoint.sequence == 0 && checkpoint.hash != checkpoint.genesis
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn record<T: DeserializeOwned + Serialize>(raw: &[u8]) -> Result<T> {
    if raw.is_empty() || raw.len() > 65536 {
        return Err(Error::Budget);
    }
    Ok(decode(raw)?)
}
fn bytes(value: &impl Serialize) -> Result<Vec<u8>> {
    let raw = encode(value)?;
    if raw.len() > 65536 {
        return Err(Error::Budget);
    }
    Ok(raw)
}
fn read_record<T: DeserializeOwned + Serialize>(
    nodes: &mut impl NodeStore,
    object: &Object,
) -> Result<T> {
    object_bound(object)?;
    let raw = nodes
        .get(object)?
        .ok_or_else(|| Error::Missing(object.clone()))?;
    if Object::from_bytes(&raw) != *object {
        return Err(Error::Integrity);
    }
    record(&raw)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ledger) struct ActivationIdentity {
    pub(in crate::ledger) generation: u64,
    pub(in crate::ledger) digest: Digest,
}
impl ActivationIdentity {
    fn validate(&self) -> Result<()> {
        if self.generation == 0 || self.generation > MAX_NUMBER {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreparedData {
    schema: String,
    config: Config,
    generation: u64,
    source: Checkpoint,
    working_state: state::Checkpoint,
    operations: Object,
    sequences: Object,
    activations: Object,
    operation_count: u64,
    clock_high: Option<u64>,
    predecessor: Option<ActivationIdentity>,
}
impl PreparedData {
    fn validate(&self) -> Result<()> {
        let clock = match (self.config.schema.as_str(), &self.config.clock) {
            ("tig.capacity-ledger.design-1", None) => false,
            ("tig.capacity-ledger.design-2", Some(p)) if p.id().is_ok() => true,
            _ => return Err(Error::Invalid),
        };
        checkpoint_bound(&self.source)?;
        if self.schema != "tig.capacity-successor-data.design-1"
            || Digest::blob(&encode(&self.config)?) != self.source.genesis
            || self.generation == 0
            || self.generation > MAX_NUMBER
            || self.operation_count != self.source.sequence
            || self.operation_count > u32::MAX as u64
            || self.clock_high.is_some_and(|n| n == 0 || n > MAX_NUMBER)
            || self.clock_high.is_some() && (!clock || self.operation_count == 0)
        {
            return Err(Error::Invalid);
        }
        match &self.predecessor {
            None if self.generation == 1 => (),
            Some(previous) => {
                previous.validate()?;
                if previous.generation.checked_add(1) != Some(self.generation) {
                    return Err(Error::Invalid);
                }
            }
            _ => return Err(Error::Invalid),
        }
        for object in [
            self.working_state.root(),
            &self.operations,
            &self.sequences,
            &self.activations,
        ] {
            object_bound(object)?;
        }
        Ok(())
    }
    fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TailIdentity {
    authority: Digest,
    generation: u64,
    operation: Key,
    base: Checkpoint,
}
impl TailIdentity {
    fn validate(&self) -> Result<()> {
        checkpoint_bound(&self.base)?;
        if self.authority != self.base.genesis
            || self.generation == 0
            || self.generation > MAX_NUMBER
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn relative_name(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "segments/{:020}-{}.sqlite",
            self.generation,
            Digest::blob(self.operation.as_str().as_bytes()).as_str()
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Preparation {
    schema: String,
    operation: Key,
    data: Object,
    metadata: MetadataMinimum,
    tail: TailIdentity,
}
impl Preparation {
    fn validate(&self) -> Result<()> {
        self.tail.validate()?;
        object_bound(&self.data)?;
        if self.schema != "tig.capacity-successor-preparation.design-1"
            || self.operation != self.tail.operation
            || self.metadata.generation() == 0
            || self.metadata.generation() > MAX_NUMBER
            || self.metadata.nodes() == 0
            || self.metadata.nodes() > MAX_NUMBER
            || self.metadata.bytes() < self.data.bytes
            || self.metadata.bytes() > MAX_NUMBER
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
    fn relative_name(&self) -> Result<String> {
        self.validate()?;
        Ok(format!(
            "activations/{}.json",
            Digest::blob(self.operation.as_str().as_bytes()).as_str()
        ))
    }
}

// Shared canonical intent. Decoding alone never observes or establishes a SQL seal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::ledger) struct SealIntent {
    pub(in crate::ledger) schema: String,
    pub(in crate::ledger) operation: Key,
    pub(in crate::ledger) authority: Digest,
    pub(in crate::ledger) generation: u64,
    pub(in crate::ledger) source: Checkpoint,
    pub(in crate::ledger) prepared: Object,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(in crate::ledger) predecessor: Option<ActivationIdentity>,
}
impl SealIntent {
    fn validate(&self) -> Result<()> {
        checkpoint_bound(&self.source)?;
        object_bound(&self.prepared)?;
        if self.authority != self.source.genesis
            || self.generation == 0
            || self.generation > MAX_NUMBER
        {
            return Err(Error::Invalid);
        }
        match (self.schema.as_str(), &self.predecessor) {
            ("tig.capacity-activation-intent.design-1", None)
                if self.source.sequence <= MAX_COMMANDS => {}
            ("tig.capacity-activation-intent.design-2", Some(previous))
                if self.source.sequence <= u32::MAX as u64 =>
            {
                previous.validate()?;
                if previous.generation.checked_add(1) != Some(self.generation) {
                    return Err(Error::Invalid);
                }
            }
            _ => return Err(Error::Invalid),
        }
        Ok(())
    }
    pub(in crate::ledger) fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealReceipt {
    intent: SealIntent,
    hash: Digest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ActiveControl {
    schema: String,
    preparation: Preparation,
    seal: SealReceipt,
}
impl ActiveControl {
    fn validate(&self) -> Result<()> {
        self.preparation.validate()?;
        let intent = &self.seal.intent;
        intent.validate()?;
        if self.schema != "tig.capacity-ledger.design-3"
            || self.seal.hash != Digest::blob(&intent.to_json()?)
            || intent.operation != self.preparation.operation
            || intent.generation != self.preparation.tail.generation
            || intent.authority != self.preparation.tail.authority
            || intent.source != self.preparation.tail.base
            || intent.prepared != Object::from_bytes(&self.preparation.to_json()?)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        bytes(self)
    }
    fn from_json(raw: &[u8]) -> Result<Self> {
        let control: Self = record(raw)?;
        control.validate()?;
        Ok(control)
    }
    fn identity(&self) -> Result<ActivationIdentity> {
        Ok(ActivationIdentity {
            generation: self.preparation.tail.generation,
            digest: Digest::blob(&self.to_json()?),
        })
    }
    fn relative_name(&self) -> Result<String> {
        Ok(format!(
            "controls/{}.json",
            self.identity()?.digest.as_str()
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Minimum {
    schema: String,
    authority: Digest,
    activation: ActivationIdentity,
    checkpoint: Checkpoint,
}
impl Minimum {
    fn validate(&self) -> Result<()> {
        self.activation.validate()?;
        checkpoint_bound(&self.checkpoint)?;
        if self.schema != "tig.capacity-successor-minimum.design-1"
            || self.authority != self.checkpoint.genesis
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Ancestor {
    schema: String,
    authority: Digest,
    generation: u64,
    digest: Digest,
}

fn ancestor_path(generation: u64) -> Result<LogicalPath> {
    LogicalPath::parse(&format!("activation/{generation:020}")).map_err(|_| Error::Invalid)
}

fn verify_ancestor(
    tree: &Tree,
    expected: &ActivationIdentity,
    authority: &Digest,
    nodes: &mut impl NodeStore,
    limits: TreeLimits,
) -> Result<()> {
    let object = tree
        .get(&ancestor_path(expected.generation)?, nodes, limits)?
        .0
        .ok_or(Error::Invalid)?;
    let ancestor: Ancestor = read_record(nodes, &object)?;
    if ancestor.schema != "tig.capacity-successor-ancestor.design-1"
        || &ancestor.authority != authority
        || ancestor.generation != expected.generation
        || ancestor.digest != expected.digest
    {
        return Err(Error::Invalid);
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
enum TailRequirement {
    BaseVerified,
    AtLeast(Checkpoint),
}
struct Base {
    data: PreparedData,
    model: CapacityDomain,
    tail: TailRequirement,
    metrics: state::Metrics,
}

/// Read-only base consistency/minimum proof, never an active journal handle.
/// SQL seal/tail prefix/physical admission/current authority remain unverified.
fn read_base(
    control: &ActiveControl,
    expected_authority: &Digest,
    minimum: &Minimum,
    reader: &mut MetadataReader,
    limits: ReadLimits,
) -> Result<Base> {
    control.validate()?;
    minimum.validate()?;
    if &minimum.authority != expected_authority
        || &control.seal.intent.authority != expected_authority
    {
        return Err(Error::Invalid);
    }
    let requested = &control.preparation.metadata;
    let observed = reader.checkpoint();
    if observed.identity() != requested.identity()
        || observed.generation() < requested.generation()
        || observed.nodes() < requested.nodes()
        || observed.bytes() < requested.bytes()
    {
        return Err(Error::Invalid);
    }
    let mut meter = Meter::new(reader, limits)?;
    let data: PreparedData = read_record(&mut meter, &control.preparation.data)?;
    data.validate()?;
    if data.source != control.seal.intent.source
        || data.generation != control.preparation.tail.generation
        || data.predecessor != control.seal.intent.predecessor
    {
        return Err(Error::Invalid);
    }
    let current = control.identity()?;
    if minimum.activation.generation > current.generation {
        return Err(Error::Journal(LedgerError::Rollback));
    }
    let tree_limits = TreeLimits {
        max_entries: u32::MAX as usize,
        ..TreeLimits::default()
    };
    let ancestry = Tree::from_root(data.activations.clone());
    if ancestry
        .get(&ancestor_path(current.generation)?, &mut meter, tree_limits)?
        .0
        .is_some()
    {
        return Err(Error::Invalid);
    }
    if let Some(previous) = &data.predecessor {
        verify_ancestor(
            &ancestry,
            previous,
            expected_authority,
            &mut meter,
            tree_limits,
        )?;
    }
    if minimum.activation.generation == current.generation {
        if minimum.activation != current {
            return Err(Error::Invalid);
        }
    } else {
        verify_ancestor(
            &ancestry,
            &minimum.activation,
            expected_authority,
            &mut meter,
            tree_limits,
        )?;
    }
    let tail = if minimum.checkpoint.sequence <= data.source.sequence {
        let index = Index::from_roots(
            data.config.clone(),
            data.source.clone(),
            data.operations.clone(),
            data.sequences.clone(),
        )?;
        if index
            .prefix(
                minimum.checkpoint.sequence,
                &mut meter,
                ReadLimits::default(),
            )?
            .0
            != minimum.checkpoint
        {
            return Err(Error::Journal(LedgerError::Rollback));
        }
        TailRequirement::BaseVerified
    } else {
        if minimum.checkpoint.sequence - data.source.sequence > MAX_COMMANDS {
            return Err(Error::Journal(LedgerError::Rollback));
        }
        TailRequirement::AtLeast(minimum.checkpoint.clone())
    };
    let model = data
        .working_state
        .read(&mut meter, state::Limits::default())?
        .into_domain_for_context(
            data.config.domain.as_str(),
            data.config.instance.as_str(),
            data.config.floor,
        )?;
    Ok(Base {
        data,
        model,
        tail,
        metrics: meter.metrics(),
    })
}

mod preparation;
pub(super) mod tail;
#[cfg(test)]
mod tests;
