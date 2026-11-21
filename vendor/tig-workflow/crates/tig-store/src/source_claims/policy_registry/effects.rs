//! Explicit provider-neutral policy admission over the existing registry/budget.
//! No native source-head receipt or payload capability is fabricated.
use super::*;
use crate::retirement_intent::policy::GrantRef;
use tig_core::retirement::review::{
    Review,
    decision::Decision,
    routine_policy::{self, Run},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(in crate::source_claims) struct Source {
    pub index: Digest,
    pub actor: Key,
}
pub(super) fn read_source<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> std::result::Result<Option<Source>, D::Error> {
    Source::deserialize(d).map(Some)
}
impl Binding {
    pub fn from_effect_index(
        index: &protection::Index,
        actor: Key,
        mut slots: Vec<Key>,
    ) -> RegistryResult<Self> {
        if !index.supports_effect_fences() {
            return Err(RegistryError::Scope);
        }
        slots.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        let profile = index.enrollment_profile();
        let id = index.identity().clone();
        let binding = Self {
            owner: profile.owner.clone(),
            subject: profile.subject.clone(),
            namespace: id.clone(),
            source: index
                .policy_source(&actor)
                .map_err(|_| RegistryError::Scope)?,
            authority: id.clone(),
            epoch: id.clone(),
            stores: profile.stores.iter().cloned().collect(),
            placements: profile.policies.iter().cloned().collect(),
            slots,
            accounted: true,
            effect_index: Some(Source { index: id, actor }),
        };
        binding.validate().map_err(RegistryError::Journal)?;
        Ok(binding)
    }
}
/// Caller-supplied artifacts are recompiled against the actual held policy and
/// exact local index frame. This is metadata admission, not provider authority.
pub struct Admission<'a> {
    pub run: &'a Run,
    pub review: &'a Review,
    pub decision: &'a Decision,
    pub context: &'a [u8],
}
pub struct HeldRun {
    _budget: SlotGuard,
}
impl Registry {
    pub fn require_effect_index(&self, index: &protection::Index) -> RegistryResult<()> {
        if self.binding.effect_index.as_ref().map(|s| &s.index) != Some(index.identity())
            || !index.supports_effect_fences()
        {
            return Err(RegistryError::Scope);
        }
        Ok(())
    }
    pub fn effect_frame(
        &self,
        index: &mut protection::Index,
        nodes: &impl NodeStore,
    ) -> RegistryResult<routine_policy::Frame> {
        self.require_effect_index(index)?;
        index
            .policy_frame(
                nodes,
                &self
                    .binding
                    .effect_index
                    .as_ref()
                    .ok_or(RegistryError::Scope)?
                    .actor,
            )
            .map_err(|_| RegistryError::Stale)
    }
    fn effect_admission(
        &self,
        held: &HeldPolicy,
        index: &mut protection::Index,
        input: &Admission<'_>,
        nodes: &impl NodeStore,
    ) -> RegistryResult<GrantRef> {
        self.require_effect_index(index)?;
        if input.run.config().namespace != self.binding.namespace
            || input.run.config().frame != self.effect_frame(index, nodes)?
            || !input.run.config().native_sources.is_empty()
            || input.decision.approval().owner != self.binding.subject
            || !input.decision.consent().release_owned_claims.is_empty()
            || !input.decision.consent().accept_intentional_loss.is_empty()
            || routine_policy::compile(held.policy(), input.run, input.review)
                .map_err(|_| RegistryError::Scope)?
                .proposed_consent()
                != input.decision.consent()
        {
            return Err(RegistryError::Scope);
        }
        let grant = GrantRef {
            registry: self.identity().clone(),
            slot: held.slot().clone(),
            version: held.version().clone(),
        };
        if input.run.config().admission
            != grant.fingerprint().map_err(|_| RegistryError::Encoding)?
        {
            return Err(RegistryError::Scope);
        }
        self.budget_scope(held, &grant, held.policy())?;
        Ok(grant)
    }
    pub fn reserve_effect_run(
        &mut self,
        held: &HeldPolicy,
        index: &mut protection::Index,
        input: &Admission<'_>,
        nodes: &mut MetadataStore,
    ) -> RegistryResult<()> {
        let grant = self.effect_admission(held, index, input, nodes)?;
        let artifacts = run_budget::reservation_bytes([
            input.context.to_vec(),
            input
                .review
                .to_json()
                .map_err(|_| RegistryError::Encoding)?,
            input
                .decision
                .to_json()
                .map_err(|_| RegistryError::Encoding)?,
        ])?;
        self.reserve_budget(
            held,
            &grant,
            &input.run.config().frame,
            artifacts,
            input.decision,
            nodes,
            |_| Ok(()),
        )
    }
    /// Hold the budget scope through the short provider effect; a concurrent
    /// admission cannot reset this slot's frame while the old worker can act.
    pub fn hold_effect_run(
        &mut self,
        held: &HeldPolicy,
        index: &mut protection::Index,
        input: &Admission<'_>,
        nodes: &impl NodeStore,
    ) -> RegistryResult<HeldRun> {
        let grant = self.effect_admission(held, index, input, nodes)?;
        let expected = run_budget::reservation_bytes([
            input.context.to_vec(),
            input
                .review
                .to_json()
                .map_err(|_| RegistryError::Encoding)?,
            input
                .decision
                .to_json()
                .map_err(|_| RegistryError::Encoding)?,
        ])?
        .0;
        Ok(HeldRun {
            _budget: self.check_budget(
                held,
                &grant,
                &input.run.config().frame,
                expected,
                input.decision,
                nodes,
            )?,
        })
    }
}
