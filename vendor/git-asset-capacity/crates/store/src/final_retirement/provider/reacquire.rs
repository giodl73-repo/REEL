//! Separately authorized fresh final reacquisition, reusing accounted streaming.
use super::*;
use crate::accounted::{self, AccountedReceipt, PublicationPlan};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub publication: Publication,
    pub capacity: Digest,
    pub context: Digest,
    pub window: Window,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    schema: String,
    parent: super::Plan,
    request: Request,
    birth: Birth,
    previous: Digest,
    payload: PublicationPlan,
    limit: u64,
}
impl Plan {
    pub fn id(&self) -> Result<Digest> {
        self.parent.id()?;
        if self.schema != "tig.final-reacquisition-plan.design-1"
            || self.request.publication.object != self.parent.birth.object
            || self.request.publication.operation == self.parent.request.publication.operation
            || self.request.publication.required != AckLevel::ProcessRestart
            || !self
                .parent
                .policy
                .owners
                .contains(&self.request.publication.owner)
            || self.request.capacity != self.parent.request.capacity
            || self.request.window.expires <= self.request.window.not_before
            || self.request.window.expires - self.request.window.not_before
                > self.parent.policy.max_window
            || self.birth.index != self.parent.birth.index + 1
            || self.birth.final_context.is_none()
        {
            return Err(Error::Invalid);
        }
        digest(self)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.id()?;
        encode(self)
    }
    pub fn from_json(raw: &[u8]) -> Result<Self> {
        if raw.len() > 65536 {
            return Err(Error::Invalid);
        }
        let p: Self = serde_json::from_value(
            git_asset_core::identity::parse_json(raw).map_err(|_| Error::Invalid)?,
        )
        .map_err(|_| Error::Invalid)?;
        if p.to_json()? != raw {
            return Err(Error::Invalid);
        }
        Ok(p)
    }
    pub fn authorize(&self) -> Result<Authorization> {
        Ok(Authorization { plan: self.id()? })
    }
    pub fn required_peak(&self) -> Result<u64> {
        self.limit
            .checked_add(self.payload.required_peak())
            .ok_or(Error::Invalid)
    }
    fn state(&self, store: &LocalStore) -> Result<State> {
        self.id()?;
        self.parent.validate(store)?;
        if self.limit != peak(store)? {
            return Err(Error::Invalid);
        }
        let state = store
            .load_final_state(&self.parent.birth)?
            .ok_or(Error::Recovery)?;
        if !state.retired()
            || digest(&state.intent.plan)? != self.parent.id()?
            || digest(state.terminal.as_ref().ok_or(Error::Recovery)?)? != self.previous
        {
            return Err(Error::Invalid);
        }
        let context = digest(&(
            "tig.final-reacquisition.design-1",
            self.parent.id()?,
            &self.request,
        ))?;
        let expected = store.next_final_birth(
            &state.intent,
            &state,
            Some(self.request.publication.clone()),
            Some(context.clone()),
        )?;
        let payload = accounted::plan_bound(store, &self.request.publication, context)
            .map_err(|_| Error::Invalid)?;
        if digest(&expected)? != digest(&self.birth)? || encode(&payload)? != encode(&self.payload)?
        {
            return Err(Error::Invalid);
        }
        Ok(state)
    }
}
pub fn plan(store: &LocalStore, parent: &super::Plan, request: Request) -> Result<Plan> {
    parent.validate(store)?;
    let state = store
        .load_final_state(&parent.birth)?
        .ok_or(Error::Recovery)?;
    if !state.retired() || digest(&state.intent.plan)? != parent.id()? {
        return Err(Error::Invalid);
    }
    let previous = digest(state.terminal.as_ref().ok_or(Error::Recovery)?)?;
    let context = digest(&("tig.final-reacquisition.design-1", parent.id()?, &request))?;
    let birth = store.next_final_birth(
        &state.intent,
        &state,
        Some(request.publication.clone()),
        Some(context.clone()),
    )?;
    let payload =
        accounted::plan_bound(store, &request.publication, context).map_err(|_| Error::Invalid)?;
    let p = Plan {
        schema: "tig.final-reacquisition-plan.design-1".into(),
        parent: parent.clone(),
        request,
        birth,
        previous,
        payload,
        limit: peak(store)?,
    };
    p.state(store)?;
    Ok(p)
}
pub struct Execution<'a> {
    pub store: &'a LocalStore,
    pub ledger: &'a mut CapacityLedger,
    pub job: &'a JournalRef,
    pub clock: &'a dyn Clock,
    pub admitted_context: &'a Digest,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Admitted,
    Born,
    MetadataSettled,
    Published,
}
impl Execution<'_> {
    pub fn apply(
        &mut self,
        p: &Plan,
        a: &Authorization,
        source: &mut dyn std::io::Read,
    ) -> Result<AccountedReceipt> {
        self.apply_hook(p, a, source, |_| Ok(()))
    }
    pub fn apply_hook(
        &mut self,
        p: &Plan,
        a: &Authorization,
        source: &mut dyn std::io::Read,
        mut hook: impl FnMut(Stage) -> Result<()>,
    ) -> Result<AccountedReceipt> {
        let state = p.state(self.store)?;
        if a.plan != p.id()?
            || self.admitted_context != &p.request.context
            || self.ledger.identity() != &p.request.capacity
        {
            return Err(Error::WrongAuthority);
        }
        let parent_binding = digest(&(
            "tig.final-allocation.design-1",
            &p.parent,
            &state.intent.job,
        ))?;
        if self
            .ledger
            .retention_status(&state.intent.allocation, &parent_binding)?
            .is_none()
        {
            return Err(Error::Recovery);
        }
        let id = digest(&(self.store.identity(), &p.request.publication.operation))?;
        let operation = Key::parse(&format!("final-reacq-{}", id.as_str()))?;
        let settle = Key::parse(&format!("settle-final-reacq-{}", id.as_str()))?;
        if operation == p.request.publication.operation {
            return Err(Error::Invalid);
        }
        let binding = digest(&(p, self.job))?;
        let allocation = self
            .ledger
            .begin_retention(
                operation.clone(),
                self.job,
                p.request.publication.owner.clone(),
                binding.clone(),
                p.limit,
            )?
            .reference()
            .clone();
        let payload_allocation = accounted::admit(self.ledger, self.store, self.job, &p.payload)
            .map_err(|_| Error::Recovery)?;
        hook(Stage::Admitted)?;
        if self
            .ledger
            .retention_status(&allocation, &binding)?
            .is_some()
            && self
                .ledger
                .publication_status(&payload_allocation, &digest(&(&p.payload, self.job))?)?
                .is_some()
        {
            return accounted::publish_accounted(
                self.ledger,
                self.store,
                self.job,
                &p.payload,
                source,
            )
            .map_err(|_| Error::Recovery);
        }
        let payload_attempt = accounted::prepare(
            self.ledger,
            self.store,
            self.job,
            &p.payload,
            &payload_allocation,
        )
        .map_err(|_| Error::Recovery)?;
        let scope = Publication {
            operation,
            owner: p.request.publication.owner.clone(),
            object: p.birth.object.clone(),
            required: AckLevel::ProcessRestart,
        };
        let attempt = self.store.attempt(&scope)?;
        attempt.bind(&(p, self.job, &allocation))?;
        if attempt.has_partials()? || payload_attempt.has_partials()? {
            return Err(Error::Partial);
        }
        let fence = self.store.handoff_fence(&p.birth.object)?;
        let current = self
            .store
            .life_head(&p.birth.object)?
            .ok_or(Error::Recovery)?;
        if digest(&current.birth)? != digest(&p.birth)? {
            p.request.window.check_clock(
                self.clock,
                &p.parent.policy.clock,
                state.release.as_ref().ok_or(Error::Recovery)?.time,
            )?;
            attempt
                .managed_store()
                .begin_final_reacquisition(&state.intent, &p.birth, &fence)?;
        }
        hook(Stage::Born)?;
        let charged = match self.ledger.retention_status(&allocation, &binding)? {
            Some(n) => n,
            None => attempt.lifecycle_footprint(&p.birth.object)?,
        };
        if charged > p.limit {
            return Err(Error::Recovery);
        }
        self.ledger
            .settle_retention(settle, &allocation, binding, charged)?;
        hook(Stage::MetadataSettled)?;
        drop(fence);
        drop(payload_attempt);
        let receipt =
            accounted::publish_accounted(self.ledger, self.store, self.job, &p.payload, source)
                .map_err(|_| Error::Recovery)?;
        hook(Stage::Published)?;
        Ok(receipt)
    }
}
