use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
use tig_core::{
    Digest, LogicalPath, Object,
    retirement::{
        Binding, ClockPoint, Policy,
        review::{
            Inventory, Protection, Release,
            decision::{Approval, Consent},
        },
    },
};
use tig_store::{
    Key, ledger, metadata,
    provider::custody,
    source_claims::{self, Selection, Version},
};

pub const REVIEW_INPUT: &str = "tig.retirement-review-profile.experimental-1";
pub const EVICTION_REVIEW_INPUT: &str = "tig.retained-eviction-review-profile.experimental-1";
pub const STAGE_INPUT: &str = "tig.native-retirement-stage.experimental-1";
pub const POLICY_RECIPE: &str = "tig.native-retirement-recipe.experimental-2";
pub const RECIPE: &str = "tig.native-retirement-recipe.experimental-1";
pub const POLICY_HANDLE: &str = "tig.native-retirement-work.experimental-2";
pub const HANDLE: &str = "tig.native-retirement-work.experimental-1";
pub const MAX_INPUT: usize = 61_440;
pub const MAX_ARTIFACT: u64 = 65_536;
pub const MAX_CONTROL_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authority {
    pub root: PathBuf,
    pub identity: Digest,
    pub minimum_checkpoint: source_claims::Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub root: PathBuf,
    pub minimum_checkpoint: metadata::Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capacity {
    pub root: PathBuf,
    pub ledger: Digest,
    pub minimum_checkpoint: ledger::Checkpoint,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Place {
    pub root: PathBuf,
    pub capacity: Capacity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Authorities {
    pub index: Authority,
    pub journal: Authority,
    pub metadata: Metadata,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub owner: Key,
    pub subject: Digest,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub object: Object,
    pub file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalogue {
    pub snapshot: Digest,
    pub bindings: BTreeMap<LogicalPath, Binding>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewInput {
    pub schema: String,
    pub catalogue: Catalogue,
    pub selected: BTreeSet<LogicalPath>,
    pub policy: Policy,
    pub at: ClockPoint,
    pub owner: Digest,
    pub places: BTreeMap<String, Digest>,
    pub protection: Protection,
    pub inventory: Inventory,
    pub releases: BTreeSet<Release>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvictionReviewInput {
    pub schema: String,
    pub selection: serde_json::Value,
    pub owner: Digest,
    pub places: BTreeMap<String, Digest>,
    pub protection: Protection,
    pub inventory: Inventory,
    pub releases: BTreeSet<Release>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    RetainedEviction,
    FinalRetirement,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preservation {
    pub object: Object,
    pub source: Digest,
    pub profile: custody::Profile,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    pub claim: Digest,
    pub successor: Digest,
    pub source_domain: Key,
    pub successor_domain: Key,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub source: Key,
    pub predecessor: Version,
    pub enrollment: Enrollment,
    pub actor: Key,
    pub actor_owner: Key,
    pub expected_actor_revision: u64,
    pub objects: BTreeSet<Object>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub maximum_object_bytes: u64,
    pub maximum_peak_per_action: u64,
    pub maximum_actions: u32,
    pub allow_cooperative: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageInput {
    pub schema: String,
    pub mode: Mode,
    pub enrollment: Enrollment,
    pub authorities: Authorities,
    pub places: BTreeMap<Digest, Place>,
    pub review: Artifact,
    pub context: Artifact,
    pub approval: Approval,
    pub consent: Consent,
    pub grants: Vec<Artifact>,
    pub preservations: Vec<Preservation>,
    pub handoffs: Vec<Handoff>,
    pub history: Vec<History>,
    pub limits: Limits,
}

/// Native private control recipe, never a portable provider or credential format.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_authority: Option<tig_store::retirement_intent::policy::AuthorityBindings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub controller_operation: Option<Digest>,
    pub schema: String,
    pub input: Digest,
    pub mode: Mode,
    pub operation: Digest,
    pub enrollment: Enrollment,
    pub authorities: Authorities,
    pub places: BTreeMap<Digest, Place>,
    pub review: Object,
    pub decision: Object,
    pub context: Object,
    pub grants: Object,
    pub preservations: Vec<Preservation>,
    pub handoffs: Vec<Handoff>,
    pub history: Vec<History>,
    pub projections: Vec<Projection>,
    pub limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Projection {
    pub actor: Key,
    pub owner: Key,
    pub expected_revision: u64,
    pub prior: Selection,
    pub remove: BTreeMap<LogicalPath, Object>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handle {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_minimum: Option<source_claims::Checkpoint>,
    pub schema: String,
    pub operation: Digest,
    pub recipe: Object,
    pub enrollment: Enrollment,
    pub authorities: Authorities,
    pub capacity_minimums: BTreeMap<Digest, ledger::Checkpoint>,
    pub digest: Digest,
}
impl Authorities {
    pub fn same_binding(&self, other: &Self) -> bool {
        self.index.root == other.index.root
            && self.index.identity == other.index.identity
            && self.journal.root == other.journal.root
            && self.journal.identity == other.journal.identity
            && self.metadata.root == other.metadata.root
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.index.root.is_absolute()
            || !self.journal.root.is_absolute()
            || !self.metadata.root.is_absolute()
            || self.index.identity == self.journal.identity
        {
            return Err("absolute distinct native authorities required".into());
        }
        Ok(())
    }
}
impl Limits {
    pub fn validate(&self) -> Result<(), String> {
        if !self.allow_cooperative
            || self.maximum_object_bytes == 0
            || self.maximum_peak_per_action == 0
            || !(1..=256).contains(&self.maximum_actions)
        {
            return Err("unsupported native controller limits/enforcement".into());
        }
        Ok(())
    }
}
impl StageInput {
    pub fn validate(&self) -> Result<(), String> {
        self.authorities.validate()?;
        self.limits.validate()?;
        let total = std::iter::once(&self.review)
            .chain(std::iter::once(&self.context))
            .chain(&self.grants)
            .try_fold(0u64, |sum, artifact| sum.checked_add(artifact.object.bytes))
            .ok_or("native artifact aggregate budget")?;
        if total > MAX_CONTROL_BYTES {
            return Err("native artifact aggregate budget".into());
        }
        capacity_minimums(self.places.values())?;
        if self.schema != STAGE_INPUT
            || self.places.is_empty()
            || self.places.len() > 16
            || self.grants.len() > 256
            || self.history.len() > 16
            || self.handoffs.len() > 256
            || self.preservations.len() > 256
            || self
                .places
                .values()
                .any(|p| !p.root.is_absolute() || !p.capacity.root.is_absolute())
            || std::iter::once(&self.review)
                .chain(std::iter::once(&self.context))
                .chain(&self.grants)
                .any(|a| {
                    !a.file.is_absolute() || a.object.bytes == 0 || a.object.bytes > MAX_ARTIFACT
                })
            || (self.mode == Mode::RetainedEviction
                && !self.consent.accept_intentional_loss.is_empty())
        {
            return Err("invalid native controller schema/scope/bindings".into());
        }
        Ok(())
    }
}

/// Preserve the strongest known witness for shared native capacity bindings.
/// Sequence ordering does not prove ancestry; execution must validate supplied
/// witnesses through the original ledger before reserving or mutating anything.
pub fn capacity_minimums<'a>(
    places: impl Iterator<Item = &'a Place>,
) -> Result<BTreeMap<Digest, ledger::Checkpoint>, String> {
    let mut bindings: BTreeMap<Digest, (&PathBuf, ledger::Checkpoint)> = BTreeMap::new();
    let mut roots = BTreeMap::new();
    for place in places {
        let capacity = &place.capacity;
        let encoded = serde_json::to_value(&capacity.minimum_checkpoint)
            .map_err(|_| "invalid capacity witness")?;
        if encoded.get("genesis")
            != Some(&serde_json::to_value(&capacity.ledger).map_err(|_| "invalid ledger identity")?)
        {
            return Err("capacity witness belongs to another ledger".into());
        }
        if let Some(prior) = roots.insert(&capacity.root, &capacity.ledger)
            && prior != &capacity.ledger
        {
            return Err("capacity root has conflicting identities".into());
        }
        if let Some((root, minimum)) = bindings.get_mut(&capacity.ledger) {
            if *root != &capacity.root {
                return Err("shared ledger has conflicting roots".into());
            }
            if minimum.sequence() == capacity.minimum_checkpoint.sequence()
                && *minimum != capacity.minimum_checkpoint
            {
                return Err("conflicting shared-ledger witnesses".into());
            }
            if capacity.minimum_checkpoint.sequence() > minimum.sequence() {
                *minimum = capacity.minimum_checkpoint.clone();
            }
        } else {
            bindings.insert(
                capacity.ledger.clone(),
                (&capacity.root, capacity.minimum_checkpoint.clone()),
            );
        }
    }
    Ok(bindings
        .into_iter()
        .map(|(id, (_, minimum))| (id, minimum))
        .collect())
}
