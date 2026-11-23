//! Pure caller-admitted policy proposals. No approval, current authority or IO.
use super::{
    Release, Review,
    decision::{Consent, Target},
};
use crate::{Digest, Object, identity};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, BTreeSet};

const POLICY: &str = "tig.routine-policy.design-1";
const RUN: &str = "tig.routine-policy-run.design-1";
const PROPOSAL: &str = "tig.routine-policy-proposal.design-1";
const ADMITTED: &str = "caller-admitted-not-authenticated-authority";
const PROPOSED: &str = "proposal-only-not-admission-or-execution";
// This experimental proposal profile fits the current native controller.
const MAX_ACTIONS: u32 = 256;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Budget,
    Scope,
    Kind,
    Encoding,
}
type Result<T> = std::result::Result<T, Error>;
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(value).map_err(|_| Error::Encoding)?)
        .map_err(|_| Error::Encoding)
}
fn decode<T: DeserializeOwned>(bytes: &[u8], schema: &str) -> Result<T> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record<T> {
        schema: String,
        scope: String,
        config: T,
    }
    let record: Record<T> =
        serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Encoding)?)
            .map_err(|_| Error::Encoding)?;
    if record.schema != schema || record.scope != ADMITTED {
        return Err(Error::Encoding);
    }
    Ok(record.config)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub maximum_objects: u32,
    pub maximum_actions: u32,
    pub maximum_object_bytes: u64,
    pub maximum_total_object_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    pub namespace: Digest,
    pub owner: Digest,
    pub source: Digest,
    pub authority: Digest,
    pub epoch: Digest,
    pub version: u64,
    pub placement_policy: Digest,
    pub provider_bindings: Digest,
    pub removable: BTreeSet<Digest>,
    pub required_survivors: BTreeSet<Digest>,
    pub limits: Limits,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Policy {
    schema: &'static str,
    scope: &'static str,
    config: PolicyConfig,
}
impl Policy {
    pub fn new(config: PolicyConfig) -> Result<Self> {
        if config.removable.is_empty()
            || config.required_survivors.is_empty()
            || config.removable.len() > super::MAX_STORES
            || config.required_survivors.len() > super::MAX_STORES
            || config.removable.union(&config.required_survivors).count() > super::MAX_STORES
            || config.limits.maximum_objects == 0
            || config.limits.maximum_objects as usize > crate::retirement::MAX_SELECTED
            || config.limits.maximum_actions == 0
            || config.limits.maximum_actions > MAX_ACTIONS
            || config.limits.maximum_object_bytes == 0
            || config.limits.maximum_total_object_bytes == 0
        {
            return Err(Error::Budget);
        }
        if config.version == 0 || !config.removable.is_disjoint(&config.required_survivors) {
            return Err(Error::Scope);
        }
        let result = Self {
            schema: POLICY,
            scope: ADMITTED,
            config,
        };
        result.to_json()?;
        Ok(result)
    }
    pub fn config(&self) -> &PolicyConfig {
        &self.config
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let result = Self::new(decode(bytes, POLICY)?)?;
        if result.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(result)
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub source: Digest,
    pub authority: Digest,
    pub epoch: Digest,
    pub generation: u64,
    pub selection: Digest,
}
/// A declaration to be validated against an actual native handoff grant later.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSource {
    pub release: Release,
    pub source: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunConfig {
    pub namespace: Digest,
    pub frame: Frame,
    pub catalogue: Digest,
    pub provider_bindings: Digest,
    pub admission: Digest,
    pub native_sources: BTreeSet<NativeSource>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Run {
    schema: &'static str,
    scope: &'static str,
    config: RunConfig,
}
impl Run {
    pub fn new(config: RunConfig) -> Result<Self> {
        if config.native_sources.len() > super::MAX_OBLIGATIONS {
            return Err(Error::Budget);
        }
        if config.frame.generation == 0 {
            return Err(Error::Scope);
        }
        let mut seen = BTreeSet::new();
        for source in &config.native_sources {
            if !seen.insert(&source.release) {
                return Err(Error::Scope);
            }
        }
        let result = Self {
            schema: RUN,
            scope: ADMITTED,
            config,
        };
        result.to_json()?;
        Ok(result)
    }
    pub fn config(&self) -> &RunConfig {
        &self.config
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        let result = Self::new(decode(bytes, RUN)?)?;
        if result.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(result)
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Reason {
    ReviewedHold,
    RequiredSurvivorsUnobserved(BTreeSet<Digest>),
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub object: Object,
    pub reason: Reason,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Proposal {
    schema: &'static str,
    scope: &'static str,
    policy: Digest,
    run: Digest,
    reviewed: Digest,
    proposed_consent: Consent,
    excluded: BTreeSet<Excluded>,
    has_candidates: bool,
    proposed_logical_object_bytes: u64,
}
impl Proposal {
    pub fn proposed_consent(&self) -> &Consent {
        &self.proposed_consent
    }
    pub fn excluded(&self) -> &BTreeSet<Excluded> {
        &self.excluded
    }
    pub fn has_candidates(&self) -> bool {
        self.has_candidates
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        encode(self)
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        Ok(Digest::blob(&self.to_json()?))
    }
    /// Restoring a proposal is still neither approval nor a current-authority check.
    pub fn from_json(bytes: &[u8], policy: &Policy, run: &Run, review: &Review) -> Result<Self> {
        identity::parse_json(bytes).map_err(|_| Error::Encoding)?;
        let result = compile(policy, run, review)?;
        if result.to_json()? != bytes {
            return Err(Error::Encoding);
        }
        Ok(result)
    }
}

/// Derive exact proposed targets and releases; this never constructs Approval or Decision.
pub fn compile(policy: &Policy, run: &Run, review: &Review) -> Result<Proposal> {
    let super::Basis::Eviction { eviction } = &review.basis else {
        return Err(Error::Kind);
    };
    let p = &policy.config;
    let r = &run.config;
    if p.provider_bindings != r.provider_bindings
        || p.namespace != r.namespace
        || &p.owner != review.owner()
        || &p.placement_policy != review.policy_identity()
        || &r.catalogue != eviction.snapshot()
        || p.source != r.frame.source
        || p.authority != r.frame.authority
        || p.epoch != r.frame.epoch
    {
        return Err(Error::Scope);
    }
    let objects: BTreeMap<_, _> = review
        .objects()
        .iter()
        .map(|o| (o.object().clone(), o))
        .collect();
    let sources: BTreeMap<_, _> = r
        .native_sources
        .iter()
        .map(|s| (&s.release, &s.source))
        .collect();
    if sources.keys().copied().ne(review.requested_releases.iter()) {
        return Err(Error::Scope);
    }
    for (release, source) in &sources {
        let object = objects.get(&release.object).ok_or(Error::Scope)?;
        if !object.targets().contains(*source) || !p.removable.contains(*source) {
            return Err(Error::Scope);
        }
    }
    let mut removals = BTreeSet::new();
    let mut excluded = BTreeSet::new();
    let mut selected = BTreeSet::new();
    let mut logical = 0u64;
    for entry in review.objects() {
        if !entry.targets().is_subset(&p.removable) {
            return Err(Error::Scope);
        }
        if !entry.eligible() {
            excluded.insert(Excluded {
                object: entry.object().clone(),
                reason: Reason::ReviewedHold,
            });
            continue;
        }
        let super::Loss::ObservedSurvivors(survivors) = entry.loss() else {
            return Err(Error::Kind);
        };
        let missing: BTreeSet<_> = p
            .required_survivors
            .difference(survivors)
            .cloned()
            .collect();
        if !missing.is_empty() {
            excluded.insert(Excluded {
                object: entry.object().clone(),
                reason: Reason::RequiredSurvivorsUnobserved(missing),
            });
            continue;
        }
        if entry.object().bytes > p.limits.maximum_object_bytes {
            return Err(Error::Budget);
        }
        logical = logical
            .checked_add(entry.object().bytes)
            .ok_or(Error::Budget)?;
        selected.insert(entry.object().clone());
        removals.extend(entry.targets().iter().map(|store| Target {
            object: entry.object().clone(),
            store: store.clone(),
        }));
    }
    let releases: BTreeSet<_> = review
        .requested_releases
        .iter()
        .filter(|r| selected.contains(&r.object))
        .cloned()
        .collect();
    let actions = removals
        .len()
        .checked_add(releases.len())
        .ok_or(Error::Budget)?;
    if selected.len() > p.limits.maximum_objects as usize
        || actions > p.limits.maximum_actions as usize
        || logical > p.limits.maximum_total_object_bytes
    {
        return Err(Error::Budget);
    }
    let result = Proposal {
        schema: PROPOSAL,
        scope: PROPOSED,
        policy: policy.fingerprint()?,
        run: run.fingerprint()?,
        reviewed: review.digest().map_err(|_| Error::Encoding)?,
        has_candidates: !removals.is_empty(),
        proposed_consent: Consent {
            release_owned_claims: releases,
            remove_copies: removals,
            accept_intentional_loss: BTreeSet::new(),
        },
        excluded,
        proposed_logical_object_bytes: logical,
    };
    result.to_json()?;
    Ok(result)
}
