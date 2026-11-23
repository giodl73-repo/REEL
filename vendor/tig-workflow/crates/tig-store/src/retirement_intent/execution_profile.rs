//! Immutable admitted execution context, reachable from the native intent.
//! Evidence is caller-admitted; this is not an authenticated provider registry.
use super::*;
use crate::provider::profile::Profile;
use std::collections::{BTreeMap, BTreeSet};
use tig_core::retirement::review::{
    Loss,
    decision::{Approval, Consent},
};

const SCHEMA: &str = "tig.retirement-execution-context.experimental-1";
const EXACT_SCHEMA: &str = "tig.retirement-execution-context.experimental-2";
const POLICY_SCHEMA: &str = "tig.retirement-execution-context.experimental-3";
const MAX_BYTES: usize = 49_152;
mod exact_handoffs;

/// Removal/custody keys are places; release keys are exact old claim IDs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Removal,
    Custody,
    RetirementRelease,
    HandoffRelease,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Context {
    schema: &'static str,
    logical_operation: Digest,
    profiles: BTreeMap<Digest, Profile>,
    bindings: BTreeMap<Role, BTreeMap<Digest, Digest>>,
    history_releases: BTreeSet<Digest>,
    #[serde(skip_serializing_if = "Option::is_none")]
    exact_handoffs: Option<BTreeMap<Digest, Digest>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    policy_proof: Option<Object>,
}
impl Context {
    pub fn new(
        logical_operation: Digest,
        profiles: BTreeMap<Digest, Profile>,
        bindings: BTreeMap<Role, BTreeMap<Digest, Digest>>,
        history_releases: BTreeSet<Digest>,
    ) -> Result<Self> {
        let context = Self {
            schema: SCHEMA,
            logical_operation,
            profiles,
            bindings,
            history_releases,
            exact_handoffs: None,
            policy_proof: None,
        };
        context.to_json()?;
        Ok(context)
    }
    pub fn profiles(&self) -> &BTreeMap<Digest, Profile> {
        &self.profiles
    }
    pub fn logical_operation(&self) -> &Digest {
        &self.logical_operation
    }
    pub fn history_releases(&self) -> &BTreeSet<Digest> {
        &self.history_releases
    }
    pub fn binding(&self, role: Role, key: &Digest) -> Result<(&Digest, &Profile)> {
        let id = self
            .bindings
            .get(&role)
            .and_then(|refs| refs.get(key))
            .ok_or(Error::Missing)?;
        Ok((id, self.profiles.get(id).ok_or(Error::Missing)?))
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        match (self.schema, &self.policy_proof) {
            (POLICY_SCHEMA, Some(root)) if root.bytes > 0 && root.bytes <= 4096 => (),
            (SCHEMA | EXACT_SCHEMA, None) => (),
            _ => return Err(Error::Invalid),
        }
        self.validate_exact_handoffs()?;
        if self.profiles.is_empty() || self.profiles.len() > 16 {
            return Err(Error::Budget);
        }
        for (id, p) in &self.profiles {
            if p.fingerprint().map_err(|_| Error::Invalid)? != *id {
                return Err(Error::Invalid);
            }
        }
        let mut used = BTreeSet::new();
        let mut count = self.history_releases.len();
        for (role, refs) in &self.bindings {
            if refs.is_empty() {
                return Err(Error::Invalid);
            }
            count = count.checked_add(refs.len()).ok_or(Error::Budget)?;
            for (key, id) in refs {
                let profile = self.profiles.get(id).ok_or(Error::Missing)?;
                if matches!(role, Role::Removal | Role::Custody) && profile.binding.place != *key {
                    return Err(Error::Invalid);
                }
                used.insert(id);
            }
        }
        if count > 256 {
            return Err(Error::Budget);
        }
        if used.len() != self.profiles.len() {
            return Err(Error::Invalid);
        }
        let bytes = encode(self)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::Budget);
        }
        Ok(bytes)
    }
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Record {
            schema: String,
            logical_operation: Digest,
            profiles: BTreeMap<Digest, Profile>,
            bindings: BTreeMap<Role, BTreeMap<Digest, Digest>>,
            history_releases: BTreeSet<Digest>,
            exact_handoffs: Option<BTreeMap<Digest, Digest>>,
            policy_proof: Option<Object>,
        }
        if bytes.len() > MAX_BYTES {
            return Err(Error::Budget);
        }
        let record: Record =
            serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        let schema = match record.schema.as_str() {
            SCHEMA => SCHEMA,
            EXACT_SCHEMA => EXACT_SCHEMA,
            POLICY_SCHEMA => POLICY_SCHEMA,
            _ => return Err(Error::Invalid),
        };
        let context = Self {
            schema,
            logical_operation: record.logical_operation,
            profiles: record.profiles,
            bindings: record.bindings,
            history_releases: record.history_releases,
            exact_handoffs: record.exact_handoffs,
            policy_proof: record.policy_proof,
        };
        if context.to_json()? != bytes {
            return Err(Error::Integrity);
        }
        Ok(context)
    }
    pub fn fingerprint(&self) -> Result<Digest> {
        // Validate the complete shared record before deriving any native ID.
        self.to_json()?;
        Ok(Digest::blob(&encode(&(
            "tig.retirement-execution-context-fingerprint.experimental-1",
            self,
        ))?))
    }
    fn operation(&self, review: &Review, approval: &Approval, consent: &Consent) -> Result<Digest> {
        if (self.exact_handoffs.is_some() || self.policy_proof.is_some())
            && !consent.accept_intentional_loss.is_empty()
        {
            return Err(Error::Invalid);
        }
        let selected: BTreeSet<_> = consent.remove_copies.iter().map(|t| &t.object).collect();
        let mut targets = BTreeSet::new();
        let mut survivors = BTreeSet::new();
        for entry in review
            .objects()
            .iter()
            .filter(|o| selected.contains(o.object()))
        {
            targets.extend(entry.targets().iter().cloned());
            if let Loss::ObservedSurvivors(places) = entry.loss() {
                survivors.extend(places.iter().cloned());
            }
        }
        let keys = |role| {
            self.bindings
                .get(&role)
                .map(|refs| refs.keys().cloned().collect::<BTreeSet<_>>())
                .unwrap_or_default()
        };
        let retirement = keys(Role::RetirementRelease);
        let handoff = keys(Role::HandoffRelease);
        let releases: BTreeSet<_> = consent
            .release_owned_claims
            .iter()
            .map(|r| r.claim.clone())
            .collect();
        if keys(Role::Removal) != targets
            || keys(Role::Custody) != survivors
            || !retirement.is_disjoint(&handoff)
            || !retirement.is_disjoint(&self.history_releases)
            || !handoff.is_disjoint(&self.history_releases)
            || retirement
                .union(&handoff)
                .chain(self.history_releases.iter())
                .cloned()
                .collect::<BTreeSet<_>>()
                != releases
        {
            return Err(Error::Invalid);
        }
        Ok(Digest::blob(&encode(&(
            "tig.profiled-retirement-operation.experimental-1",
            &self.logical_operation,
            &approval.namespace,
            &approval.owner,
            &approval.evidence,
            review.digest().map_err(Error::Review)?,
            consent,
            self.fingerprint()?,
        ))?))
    }
    /// Explicit caller authorization only, with profiles bound before admission.
    /// The original Decision validates exact consent/loss/owner/review semantics.
    pub fn authorize(
        &self,
        review: &Review,
        approval: Approval,
        consent: Consent,
    ) -> Result<Decision> {
        if self.policy_proof.is_some() {
            return Err(Error::Invalid);
        }
        self.authorize_inner(review, approval, consent)
    }
    pub(crate) fn authorize_registered(
        &self,
        review: &Review,
        approval: Approval,
        consent: Consent,
    ) -> Result<Decision> {
        if self.policy_proof.is_none() || !review.is_routine_eviction() {
            return Err(Error::Invalid);
        }
        self.authorize_inner(review, approval, consent)
    }
    fn authorize_inner(
        &self,
        review: &Review,
        mut approval: Approval,
        consent: Consent,
    ) -> Result<Decision> {
        if approval.operation != self.logical_operation {
            return Err(Error::Invalid);
        }
        approval.operation = self.operation(review, &approval, &consent)?;
        if self.policy_proof.is_some() {
            Decision::authorize_guarded(review, approval, consent, self.fingerprint()?)
                .map_err(Error::Decision)
        } else {
            Decision::authorize(review, approval, consent).map_err(Error::Decision)
        }
    }
    pub fn policy_proof(&self) -> Option<&Object> {
        self.policy_proof.as_ref()
    }
    pub(crate) fn bind_policy(mut self, root: Object) -> Result<Self> {
        self.schema = POLICY_SCHEMA;
        self.policy_proof = Some(root);
        self.to_json()?;
        Ok(self)
    }
    pub fn verify(&self, review: &Review, decision: &Decision) -> Result<()> {
        let expected = self
            .policy_proof
            .as_ref()
            .map(|_| self.fingerprint())
            .transpose()?;
        if decision.execution_context() != expected.as_ref() {
            return Err(Error::Invalid);
        }
        if self.operation(review, decision.approval(), decision.consent())?
            != decision.approval().operation
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
