//! Selected native claim metadata for a routine proposal. Never an authority or
//! custody lease. Stores/ledgers are explicitly supplied; no discovery or IO on
//! excluded claims. The caller retains and reuses its scoped native handles.
use super::{ClaimMetadata, RetentionError, RetentionGrant, observe_claim};
use crate::{Key, ledger::CapacityLedger, local::LocalStore};
use std::collections::BTreeMap;
use tig_core::{
    Digest,
    retirement::review::{
        Review,
        routine_policy::{self, NativeSource, Policy, Proposal, Run},
    },
};

#[derive(Debug)]
pub enum Error {
    Proposal(routine_policy::Error),
    Native(RetentionError),
    Scope,
    Budget,
    Incomplete,
}
pub type Result<T> = std::result::Result<T, Error>;

/// One caller-owned operation budget. Failed validation/metadata attempts consume
/// it; retrying a failed source cannot reset the budget of this batch.
pub struct Validation {
    policy: Digest,
    run: Digest,
    review: Digest,
    owner: Key,
    proposal: Proposal,
    pending: BTreeMap<Digest, NativeSource>,
    observed: BTreeMap<Digest, ClaimMetadata>,
    maximum_queries: u32,
    queries: u32,
}
impl Validation {
    /// Compile the original pure proposal, then select only its native releases.
    /// The owner Key mapping is caller-admitted, not derived from Policy.owner.
    pub fn new(
        policy: &Policy,
        run: &Run,
        review: &Review,
        owner: Key,
        maximum_queries: u32,
    ) -> Result<Self> {
        if maximum_queries > 256 {
            return Err(Error::Budget);
        }
        let proposal = routine_policy::compile(policy, run, review).map_err(Error::Proposal)?;
        let mut pending = BTreeMap::new();
        for source in &run.config().native_sources {
            if proposal
                .proposed_consent()
                .release_owned_claims
                .contains(&source.release)
                && pending
                    .insert(source.release.claim.clone(), source.clone())
                    .is_some()
            {
                return Err(Error::Scope);
            }
        }
        if pending.len() > 256
            || pending.len() > maximum_queries as usize
            || pending.len() != proposal.proposed_consent().release_owned_claims.len()
        {
            return Err(Error::Budget);
        }
        Ok(Self {
            policy: policy.fingerprint().map_err(Error::Proposal)?,
            run: run.fingerprint().map_err(Error::Proposal)?,
            review: review.digest().map_err(|_| Error::Scope)?,
            owner,
            proposal,
            pending,
            observed: BTreeMap::new(),
            maximum_queries,
            queries: 0,
        })
    }
    pub fn pending(&self) -> &BTreeMap<Digest, NativeSource> {
        &self.pending
    }
    pub fn queries(&self) -> u32 {
        self.queries
    }
    /// Exact native point observation. No payload verification, claim creation,
    /// release, reservation or removal. Failure keeps the declaration pending.
    pub fn observe(
        &mut self,
        claim: &Digest,
        grant: &RetentionGrant,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
    ) -> Result<()> {
        if self.queries >= self.maximum_queries {
            return Err(Error::Budget);
        }
        self.queries += 1;
        let declared = self.pending.get(claim).ok_or(Error::Scope)?;
        if grant.claim_id() != claim
            || grant.store() != &declared.source
            || store.identity() != &declared.source
            || grant.request().owner != self.owner
            || grant.request().publication.object != declared.release.object
        {
            return Err(Error::Scope);
        }
        let metadata = observe_claim(ledger, store, grant).map_err(Error::Native)?;
        self.observed.insert(claim.clone(), metadata);
        self.pending.remove(claim);
        Ok(())
    }
    pub fn finish(self) -> Result<Observed> {
        if !self.pending.is_empty() {
            return Err(Error::Incomplete);
        }
        Ok(Observed {
            policy: self.policy,
            run: self.run,
            review: self.review,
            owner: self.owner,
            proposal: self.proposal,
            metadata: self.observed,
            queries: self.queries,
        })
    }
}

/// Complete selected metadata observations at the time they were queried. Not
/// serializable or cloneable: this is no persistent authorization, freshness
/// guarantee, effective hold, failure independence or proof of current source.
pub struct Observed {
    policy: Digest,
    run: Digest,
    review: Digest,
    owner: Key,
    proposal: Proposal,
    metadata: BTreeMap<Digest, ClaimMetadata>,
    queries: u32,
}
impl Observed {
    pub fn inputs(&self) -> (&Digest, &Digest, &Digest, &Key) {
        (&self.policy, &self.run, &self.review, &self.owner)
    }
    pub fn proposal(&self) -> &Proposal {
        &self.proposal
    }
    pub fn metadata(&self) -> &BTreeMap<Digest, ClaimMetadata> {
        &self.metadata
    }
    pub fn queries(&self) -> u32 {
        self.queries
    }
}
