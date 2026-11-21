//! Exact asset-side provenance. Restoring it grants no execution authority.
use super::*;
use crate::retention::routine_sources::Observed;
use crate::source_claims::{
    Version,
    coordinated::accepted_head::HeldHead,
    policy_registry::{HeldPolicy, ProviderCatalogue, Registration},
};
use tig_core::retirement::review::{
    decision::Approval,
    routine_policy::{Policy, Proposal, Run},
};

const SCHEMA: &str = "tig.native-policy-intent-proof.experimental-1";
mod authority;
pub use authority::{AuthorityBindings, EffectHold, NativeAuthority};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRef {
    pub registry: Digest,
    pub slot: Key,
    pub version: Version,
}
impl GrantRef {
    pub fn fingerprint(&self) -> Result<Digest> {
        Ok(Digest::blob(&encode(&(SCHEMA, self))?))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    grant: GrantRef,
    owner: Key,
    policy: Object,
    providers: Object,
    run: Object,
    proposal: Object,
}
pub struct Proof {
    record: Record,
    policy: Policy,
    providers: ProviderCatalogue,
    run: Run,
    proposal: Proposal,
}
impl Proof {
    pub fn grant(&self) -> &GrantRef {
        &self.record.grant
    }
    pub fn policy(&self) -> &Policy {
        &self.policy
    }
    pub fn providers(&self) -> &ProviderCatalogue {
        &self.providers
    }
    pub fn run(&self) -> &Run {
        &self.run
    }
    pub fn proposal(&self) -> &Proposal {
        &self.proposal
    }
    pub fn owner(&self) -> &Key {
        &self.record.owner
    }
    pub(crate) fn check_handoff(
        &self,
        request: &crate::retention::handoff::HandoffRequest,
    ) -> Result<()> {
        let source = request.previous.store();
        let successor = request.successor.store();
        if !self.policy.config().removable.contains(source)
            || request.owner != self.record.owner
            || self
                .providers
                .places()
                .get(source)
                .is_none_or(|p| p.failure_domain != request.source_domain)
            || self
                .providers
                .places()
                .get(successor)
                .is_none_or(|p| p.failure_domain != request.successor_domain)
            || !self.run.config().native_sources.iter().any(|s| {
                s.source == *source
                    && s.release.claim == *request.previous.claim_id()
                    && s.release.object == request.previous.request().publication.object
            })
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub(crate) fn check_capacity(&self, place: &Digest, capacity: &Digest) -> Result<()> {
        if self
            .providers
            .places()
            .get(place)
            .is_none_or(|p| &p.capacity_domain != capacity)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub(crate) fn check_removal(
        &self,
        store: &Digest,
        object: &Object,
        capacity: &Digest,
        profile: Option<&crate::provider::custody::Profile>,
    ) -> Result<()> {
        let place = self.providers.places().get(store).ok_or(Error::Missing)?;
        let profile = profile.ok_or(Error::Missing)?;
        let places = profile.places(store, object).map_err(|_| Error::Invalid)?;
        if !self.policy.config().removable.contains(store)
            || &place.capacity_domain != capacity
            || profile.source_domain != place.failure_domain
            || !self.policy.config().required_survivors.is_subset(&places)
            || profile.retained.iter().any(|s| {
                self.providers
                    .places()
                    .get(&s.commitment.place)
                    .is_none_or(|p| p.failure_domain != s.failure_domain)
            })
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    pub fn load(
        nodes: &impl NodeStore,
        context: &execution_profile::Context,
        review: &Review,
        decision: &Decision,
    ) -> Result<Self> {
        let root = context.policy_proof().ok_or(Error::Missing)?;
        if root.bytes > 4096 {
            return Err(Error::Budget);
        }
        let bytes = read(nodes, root)?;
        let record: Record =
            serde_json::from_value(identity::parse_json(&bytes).map_err(|_| Error::Invalid)?)
                .map_err(|_| Error::Invalid)?;
        if record.schema != SCHEMA
            || encode(&record)? != bytes
            || record.grant.version.revision == 0
            || !review.is_routine_eviction()
        {
            return Err(Error::Invalid);
        }
        let policy =
            Policy::from_json(&read(nodes, &record.policy)?).map_err(|_| Error::Invalid)?;
        let providers = ProviderCatalogue::from_json(&read(nodes, &record.providers)?)
            .map_err(|_| Error::Invalid)?;
        let run = Run::from_json(&read(nodes, &record.run)?).map_err(|_| Error::Invalid)?;
        let proposal = Proposal::from_json(&read(nodes, &record.proposal)?, &policy, &run, review)
            .map_err(|_| Error::Invalid)?;
        let registration = Registration::new(record.policy.clone(), record.providers.clone())
            .map_err(|_| Error::Invalid)?;
        if Object::from_bytes(&encode(&registration)?) != record.grant.version.selection.root
            || policy.config().provider_bindings != record.providers.sha256
            || proposal.proposed_consent() != decision.consent()
            || decision.approval().owner != policy.config().owner
            || decision.approval().namespace != run.config().namespace
            || decision.approval().evidence != record.grant.fingerprint()?
            || run.config().admission != record.grant.fingerprint()?
            || context
                .profiles()
                .iter()
                .any(|(id, p)| providers.profiles().get(id) != Some(p))
        {
            return Err(Error::Invalid);
        }
        context.verify(review, decision)?;
        Ok(Self {
            record,
            policy,
            providers,
            run,
            proposal,
        })
    }
}

/// Save a registered-policy decision under actual registration/head holds and
/// complete selected native observations. No byte work or effect authority.
pub fn stage_proof(
    nodes: &mut impl NodeStore,
    held: &HeldPolicy,
    head: &HeldHead,
    observed: &Observed,
    run: &Run,
    review: &Review,
    base: execution_profile::Context,
) -> Result<(execution_profile::Context, Decision)> {
    let grant = GrantRef {
        registry: held.registry().clone(),
        slot: held.slot().clone(),
        version: held.version().clone(),
    };
    let policy = held.policy();
    let (p, r, v, owner) = observed.inputs();
    if p != &policy.fingerprint().map_err(|_| Error::Invalid)?
        || r != &run.fingerprint().map_err(|_| Error::Invalid)?
        || v != &review.digest().map_err(Error::Review)?
        || owner != held.owner()
        || run.config().admission != grant.fingerprint()?
        || head.require_run(run).is_err()
        || !review.is_routine_eviction()
        || base
            .profiles()
            .iter()
            .any(|(id, p)| held.providers().get(id) != Some(p))
    {
        return Err(Error::Invalid);
    }
    let record = Record {
        schema: SCHEMA.into(),
        grant,
        owner: owner.clone(),
        policy: put(nodes, &policy.to_json().map_err(|_| Error::Invalid)?)?,
        providers: put(
            nodes,
            &held.catalogue().to_json().map_err(|_| Error::Invalid)?,
        )?,
        run: put(nodes, &run.to_json().map_err(|_| Error::Invalid)?)?,
        proposal: put(
            nodes,
            &observed.proposal().to_json().map_err(|_| Error::Invalid)?,
        )?,
    };
    let evidence = record.grant.fingerprint()?;
    let root = put(nodes, &encode(&record)?)?;
    let context = base.bind_policy(root)?;
    let approval = Approval {
        namespace: run.config().namespace.clone(),
        owner: policy.config().owner.clone(),
        operation: context.logical_operation().clone(),
        evidence,
    };
    let decision = context.authorize_registered(
        review,
        approval,
        observed.proposal().proposed_consent().clone(),
    )?;
    Proof::load(nodes, &context, review, &decision)?;
    Ok((context, decision))
}

/// Fresh native intent admission from actual registration/source holds. Original
/// metadata finish/prepare/adopt follow; no physical work authority is supplied.
#[allow(clippy::too_many_arguments)]
pub fn stage_registered(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    registry: &mut crate::source_claims::policy_registry::Registry,
    enrollment: &Enrollment,
    held: &HeldPolicy,
    head: &HeldHead,
    observed: &Observed,
    run: &Run,
    review: &Review,
    base: execution_profile::Context,
) -> Result<(Candidate, execution_profile::Context, Decision)> {
    if enrollment.owner != *held.owner()
        || enrollment.subject != held.policy().config().owner
        || journal.identity() != head.journal()
    {
        return Err(Error::Owner);
    }
    let (context, decision) = stage_proof(nodes, held, head, observed, run, review, base)?;
    let proof = Proof::load(nodes, &context, review, &decision)?;
    registry
        .reserve_run(held, head, &proof, &context, review, &decision, nodes)
        .map_err(|_| Error::Budget)?;
    let candidate = stage_inner(
        journal,
        nodes,
        enrollment,
        review,
        &decision,
        Some(&context),
    )?;
    Ok((candidate, context, decision))
}

/// Fail closed until a runtime authority adapter is attached. Restored data and
/// historical grant receipts never provide fresh permission by themselves.
pub(crate) fn require_effect_authority(
    ctx: &ObservedIntent,
    nodes: &impl NodeStore,
) -> Result<Option<EffectHold>> {
    if !ctx
        .execution_profile()
        .is_some_and(|p| p.policy_proof().is_some())
    {
        return Ok(None);
    }
    let authority = ctx.policy_authority.as_ref().ok_or(Error::Missing)?;
    Ok(Some(authority.acquire(ctx, nodes)?))
}

/// Resume saved admission after a controller recipe was acknowledged but the
/// intent was not prepared. The exact saved decision is never rebased.
#[allow(clippy::too_many_arguments)]
pub fn stage_saved_registered(
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    enrollment: &Enrollment,
    registry: &mut crate::source_claims::policy_registry::Registry,
    held: &HeldPolicy,
    head: &HeldHead,
    review: &Review,
    decision: &Decision,
    context: &execution_profile::Context,
) -> Result<Candidate> {
    let proof = Proof::load(nodes, context, review, decision)?;
    if journal.identity() != head.journal()
        || proof.owner() != &enrollment.owner
        || decision.approval().owner != enrollment.subject
    {
        return Err(Error::Owner);
    }
    head.require_run(proof.run()).map_err(Error::Journal)?;
    registry
        .check_run(held, &proof, context, review, decision, nodes)
        .map_err(|_| Error::Budget)?;
    stage_inner(journal, nodes, enrollment, review, decision, Some(context))
}
