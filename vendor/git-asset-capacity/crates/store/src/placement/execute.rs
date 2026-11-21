//! Existing effect adapters; no independent operation journal.
use super::*;
use crate::{
    AckLevel, Request,
    accounted::{self, PublicationPlan},
    existing::ByteSource,
    final_retirement::provider as final_provider,
    replica::ReplicaEnrollment,
    retention::{self, RetentionGrant, RetentionPlan, RetentionRequest, handoff},
    retirement,
};
use std::{
    io::{self, Read},
    time::{Duration, Instant},
};

impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        use crate::Error as E;
        match e {
            E::Busy => Self::Busy,
            E::Budget => Self::Budget,
            E::Integrity => Self::Integrity,
            E::Io(..) => Self::Unavailable,
            E::Retained => Self::Retained,
            E::OperationConflict => Self::WrongEnrollment,
            E::AuthorizedAbsence | E::ReacquisitionRequired => Self::ReacquisitionRequired,
            _ => Self::Recovery,
        }
    }
}
impl From<crate::ledger::LedgerError> for Error {
    fn from(e: crate::ledger::LedgerError) -> Self {
        use crate::ledger::LedgerError as E;
        match e {
            E::Busy => Self::Busy,
            E::Budget | E::Capacity(_) => Self::Budget,
            E::Store(e) => e.into(),
            E::Conflict | E::InvalidReference => Self::WrongEnrollment,
            _ => Self::Recovery,
        }
    }
}
impl From<accounted::AccountedError> for Error {
    fn from(e: accounted::AccountedError) -> Self {
        use accounted::AccountedError as E;
        match e {
            E::Store(e) => e.into(),
            E::Ledger(e) => e.into(),
            E::FootprintExceedsLimit => Self::Budget,
            _ => Self::Recovery,
        }
    }
}
impl From<retention::RetentionError> for Error {
    fn from(e: retention::RetentionError) -> Self {
        use retention::RetentionError as E;
        match e {
            E::Store(e) => e.into(),
            E::Ledger(e) => e.into(),
            E::Released => Self::Retained,
            E::Replica(crate::replica::ReplicaError::Store(e)) => e.into(),
            _ => Self::Recovery,
        }
    }
}
impl From<handoff::HandoffError> for Error {
    fn from(e: handoff::HandoffError) -> Self {
        match e {
            handoff::HandoffError::Retention(e) => e.into(),
            _ => Self::Recovery,
        }
    }
}
impl From<retirement::Error> for Error {
    fn from(e: retirement::Error) -> Self {
        use retirement::Error as E;
        match e {
            E::Store(e) => e.into(),
            E::Ledger(e) => e.into(),
            E::Retention(e) => e.into(),
            E::Publication(e) => e.into(),
            E::Protection(crate::store_protection::Error::Required) => Self::Protected,
            E::Protection(crate::store_protection::Error::Retained) => Self::Retained,
            E::Protection(crate::store_protection::Error::Busy) => Self::Busy,
            E::MissingCustody => Self::Protected,
            _ => Self::Recovery,
        }
    }
}
fn key(kind: &str, value: &impl Serialize) -> Result<Key> {
    Key::parse(&format!("{kind}-{}", digest(value)?.as_str())).map_err(|_| Error::Invalid)
}
fn context(config: &Config, pin: &Provider, object: &Object) -> Result<Digest> {
    digest(&(
        "tig.placement-object.design-1",
        config.id()?,
        &pin.place,
        object,
    ))
}
fn initial(config: &Config, pin: &Provider, object: &Object) -> Result<Request> {
    Ok(Request {
        operation: key("place", &("initial", config.id()?, &pin.place, object))?,
        owner: config.owner.clone(),
        object: object.clone(),
        required: AckLevel::ProcessRestart,
    })
}
/// Resolve publication identity for an exact enrolled provider/object without
/// applying effects. This is not a custody observation or deletion authority.
/// Retirement recipes require their existing recovery/reacquisition path.
pub fn publication(
    config: &Config,
    pin: &Provider,
    store: &LocalStore,
    object: &Object,
) -> Result<Request> {
    config.id()?;
    let identity = digest(pin)?;
    let mut admitted = false;
    for provider in &config.providers {
        admitted |= digest(provider)? == identity;
    }
    if !admitted {
        return Err(Error::WrongEnrollment);
    }
    match recipe(config, pin, store, object)? {
        Recipe::Publication { request, .. } => Ok(request),
        Recipe::Retirement(_) => Err(Error::ReacquisitionRequired),
    }
}
fn retention_plan(store: &LocalStore, publication: &Request) -> Result<RetentionPlan> {
    Ok(retention::plan(
        store,
        &RetentionRequest {
            operation: publication.operation.clone(),
            owner: publication.owner.clone(),
            publication: publication.clone(),
            minimum_ack: AckLevel::ProcessRestart,
        },
    )?)
}
enum Recipe {
    Publication {
        request: Request,
        payload: Box<PublicationPlan>,
        reacquire: Option<Box<retirement::ReacquisitionPlan>>,
    },
    Retirement(Box<retirement::Plan>),
}
fn recipe(config: &Config, pin: &Provider, store: &LocalStore, object: &Object) -> Result<Recipe> {
    if Place::local(store, pin.place.generation).map_err(|_| Error::Unavailable)? != pin.place {
        return Err(Error::WrongEnrollment);
    }
    if store.supports_retirement() {
        if let Some(head) = store.life_head(object)? {
            if head.final_state.is_some() {
                return Err(Error::FinalRetirement);
            }
            if let Some(intent) = head.intent {
                return Ok(Recipe::Retirement(Box::new(intent.plan)));
            }
            if let Some(origin) = store.placement_origin(object)?
                && let Some(request) = &origin.reacquire
            {
                if request.owner != config.owner {
                    return Err(Error::WrongEnrollment);
                }
                if let Some(context) = &origin.final_context {
                    return Ok(Recipe::Publication {
                        request: request.clone(),
                        payload: Box::new(accounted::plan_bound(store, request, context.clone())?),
                        reacquire: None,
                    });
                }
                let reacquire = retirement::resume_reacquisition(
                    store,
                    origin.clone(),
                    context(config, pin, object)?,
                )?;
                return Ok(Recipe::Publication {
                    request: request.clone(),
                    payload: Box::new(reacquire.payload_plan().clone()),
                    reacquire: Some(Box::new(reacquire)),
                });
            }
        } else if store.object_present(object)? {
            return Err(Error::Recovery);
        }
    }
    let request = initial(config, pin, object)?;
    let payload = accounted::plan_bound(store, &request, context(config, pin, object)?)?;
    Ok(Recipe::Publication {
        request,
        payload: Box::new(payload),
        reacquire: None,
    })
}
fn published(
    runtime: &mut Local<'_>,
    request: &Request,
    payload: &PublicationPlan,
) -> Result<bool> {
    let mut provider = runtime.store.optional_completed_receipt(request)?.is_some();
    let operation = key("publish", &(runtime.store.identity(), &request.operation))?;
    let allocation = runtime.ledger.allocation_reference(&operation, false)?;
    let settled = match allocation {
        Some(ref a) => runtime
            .ledger
            .publication_status(a, &digest(&(payload, runtime.job))?)?
            .is_some(),
        None => false,
    };
    if settled && !provider {
        provider = runtime.store.optional_completed_receipt(request)?.is_some();
    }
    if settled && !provider
        || allocation.is_none()
            && (provider
                || runtime.store.supports_retirement()
                    && runtime
                        .store
                        .placement_origin(&request.object)?
                        .is_some_and(|b| b.reacquire.is_none()))
    {
        return Err(Error::Recovery);
    }
    Ok(settled && provider)
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    NotStarted,
    UntrackedPresent,
    Partial,
    Published,
    Retained,
    Released,
    RetirementPending,
    AuthorizedAbsence,
    Cancelled,
    Held(Error),
}
#[derive(Clone, Debug, Serialize)]
pub struct Observed {
    pub object: Object,
    pub provider: Digest,
    pub required: bool,
    pub state: Observation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectKind {
    Published,
    Retained,
    Handoff,
    Retired,
    Reacquired,
    Cancelled,
    Recovered,
}
#[derive(Clone, Debug, Serialize)]
pub struct Effect {
    pub object: Object,
    pub provider: Digest,
    pub kind: EffectKind,
    pub logical_bytes: u64,
    pub file_allocation_before: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub plan: Digest,
    pub observed_basis: Digest,
    pub basis_current: bool,
    pub page_converged: bool,
    pub storage_converged: Option<bool>,
    pub stream_bytes: u64,
    pub effects: Vec<Effect>,
    pub observations: Vec<Observed>,
    pub holds: Vec<Error>,
    pub next: Option<Cursor>,
}
/// Read-only intentional absence; never activation or removal authority.
fn settled_final_absence(pin: &Provider, runtime: &mut Local<'_>, object: &Object) -> Result<bool> {
    if !runtime.store.supports_retirement() || runtime.store.final_policy().is_none() {
        return Ok(false);
    }
    if Place::local(runtime.store, pin.place.generation).map_err(|_| Error::Unavailable)?
        != pin.place
        || runtime.ledger.identity() != &pin.capacity
    {
        return Err(Error::WrongEnrollment);
    }
    let Some(head) = runtime.store.life_head(object)? else {
        return Ok(false);
    };
    let Some(state) = &head.final_state else {
        return Ok(false);
    };
    if !state.retired() {
        return Ok(false);
    }
    let status = final_provider::status::inspect(
        runtime.store,
        runtime.ledger,
        runtime.job,
        &state.intent.plan,
    )
    .map_err(|error| match error {
        final_provider::Error::Store(e) => Error::from(e),
        final_provider::Error::Ledger(e) => Error::from(e),
        final_provider::Error::WrongAuthority => Error::WrongEnrollment,
        _ => Error::Recovery,
    })?;
    if status.phase != final_provider::status::Phase::Deleted
        || !status.grants_released
        || status.charged_metadata.is_none()
    {
        return Err(Error::Recovery);
    }
    match status.observed {
        final_provider::Observation::Absent => (),
        final_provider::Observation::PresentUnverified => return Err(Error::Recovery),
        final_provider::Observation::Unknown => return Err(Error::Unavailable),
    }
    let current = runtime.store.life_head(object)?.ok_or(Error::Recovery)?;
    if current.birth.index != head.birth.index
        || current.birth.placement != head.birth.placement
        || runtime.store.object_present(object)?
    {
        return Err(Error::Recovery);
    }
    Ok(true)
}
fn observe(
    config: &Config,
    pin: &Provider,
    runtime: &mut Local<'_>,
    object: &Object,
    required: bool,
) -> Result<Observation> {
    if !required && settled_final_absence(pin, runtime, object)? {
        return Ok(Observation::AuthorizedAbsence);
    }
    match recipe(config, pin, runtime.store, object)? {
        Recipe::Retirement(plan) => {
            let status =
                retirement::status::inspect(runtime.store, runtime.ledger, runtime.job, &plan)?;
            use retirement::status::Phase as P;
            Ok(match status.phase {
                P::Retired if !runtime.store.object_present(object)? => {
                    Observation::AuthorizedAbsence
                }
                P::Retired => return Err(Error::Recovery),
                P::Cancelled => Observation::Cancelled,
                P::RecoveryRequired => return Err(Error::Recovery),
                _ => Observation::RetirementPending,
            })
        }
        Recipe::Publication {
            request, payload, ..
        } => {
            if !published(runtime, &request, &payload)? {
                let op = key("publish", &(runtime.store.identity(), &request.operation))?;
                return Ok(
                    if runtime.ledger.allocation_reference(&op, false)?.is_some() {
                        Observation::Partial
                    } else if runtime.store.object_present(object)? {
                        Observation::UntrackedPresent
                    } else {
                        Observation::NotStarted
                    },
                );
            }
            let planned = retention_plan(runtime.store, &request)?;
            let Some(grant) =
                retention::recorded(runtime.ledger, runtime.store, runtime.job, &planned)?
            else {
                return Ok(Observation::Published);
            };
            if retention::released(runtime.store, &grant)? {
                return Ok(Observation::Released);
            }
            drop(retention::verify(runtime.ledger, runtime.store, &grant)?);
            Ok(Observation::Retained)
        }
    }
}
struct LazySource<'a, S> {
    stores: &'a [&'a LocalStore],
    target: &'a Digest,
    seed: &'a mut S,
    object: &'a Object,
    reader: Option<Box<dyn Read>>,
    position: u64,
    left: &'a mut u64,
    streamed: &'a mut u64,
    start: Instant,
    millis: u64,
}
impl<S: ByteSource> Read for LazySource<'_, S>
where
    S::Reader: 'static,
{
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        if self.start.elapsed() >= Duration::from_millis(self.millis) {
            return Err(io::Error::other("placement deadline"));
        }
        if self.position == self.object.bytes {
            return Ok(0);
        }
        if self.reader.is_none() {
            for store in self.stores {
                if store.identity() == self.target {
                    continue;
                }
                if store.check_identity().is_ok()
                    && store.object_present(self.object).unwrap_or(false)
                    && let Ok(reader) = store.read_object(self.object)
                {
                    self.reader = Some(Box::new(reader));
                    break;
                }
            }
            if self.reader.is_none() {
                self.reader =
                    Some(Box::new(self.seed.open_verified(self.object).map_err(
                        |_| io::Error::other("verified source unavailable"),
                    )?));
            }
        }
        let cap = b
            .len()
            .min(65536)
            .min((self.object.bytes - self.position) as usize)
            .min((*self.left).try_into().unwrap_or(usize::MAX));
        if cap == 0 && !b.is_empty() {
            return Err(io::Error::other("placement byte budget"));
        }
        let n = self
            .reader
            .as_mut()
            .ok_or_else(|| io::Error::other("source unavailable"))?
            .read(&mut b[..cap])?;
        self.position += n as u64;
        *self.left -= n as u64;
        *self.streamed += n as u64;
        Ok(n)
    }
}
impl<S: ByteSource, N: NodeStore> Execution<'_, '_, S, N>
where
    S::Reader: 'static,
{
    /// Execute one freshly admitted page. Individual holds preserve useful work;
    /// callers explicitly schedule later pages or retry the same page.
    pub fn apply(&mut self, plan: &Plan) -> Result<Report> {
        self.apply_hook(plan, |_, _| Ok(()))
    }
    pub(super) fn apply_hook(
        &mut self,
        plan: &Plan,
        mut hook: impl FnMut(&Object, usize) -> Result<()>,
    ) -> Result<Report> {
        self.admit(plan)?;
        let start = Instant::now();
        let mut left = self.config.budget.max_stream_bytes;
        let mut streamed = 0;
        let mut effects = Vec::new();
        let mut holds = Vec::new();
        for row in &plan.rows {
            if start.elapsed() >= Duration::from_millis(self.config.budget.limits.millis) {
                holds.push(Error::Budget);
                break;
            }
            // Retained floors first. A failed floor still permits useful cache
            // publication with its temporary ingest protection intact.
            let order = (0..self.places.len())
                .filter(|i| row.required[*i])
                .collect::<Vec<_>>();
            let mut order = order;
            order.sort_by_key(|i| self.config.providers[*i].role == Role::Cache);
            for index in order {
                let result = self.activate(
                    index,
                    &row.object,
                    &mut left,
                    &mut streamed,
                    start,
                    &mut effects,
                );
                if let Err(e) = result {
                    holds.push(e);
                }
                hook(&row.object, index)?;
            }
            let mut keepers = (0..self.places.len())
                .filter(|i| self.config.providers[*i].role == Role::Retained)
                .collect::<Vec<_>>();
            keepers.sort_by(|a, b| {
                self.config.providers[*a]
                    .place
                    .instance
                    .cmp(&self.config.providers[*b].place.instance)
            });
            let mut floor = true;
            let mut domains = BTreeSet::new();
            for index in &keepers {
                let pin = &self.config.providers[*index];
                match self.grant(*index, &row.object).and_then(|g| {
                    let runtime = &mut self.places[*index];
                    drop(retention::verify(runtime.ledger, runtime.store, &g)?);
                    Ok(())
                }) {
                    Ok(()) => {
                        domains.insert(pin.failure_domain.as_str().to_owned());
                    }
                    Err(e) => {
                        holds.push(e);
                        floor = false;
                    }
                }
            }
            if domains.len() < self.config.minimum_domains {
                floor = false;
                holds.push(Error::Protected);
            }
            if !floor {
                continue;
            }
            // Claim handoff takes an exclusive provider metadata scope. Finish
            // it before taking cache readers that span later physical removal.
            for index in 0..self.places.len() {
                if self.config.providers[index].role != Role::Cache {
                    continue;
                }
                if matches!(
                    observe(
                        self.config,
                        &self.config.providers[index],
                        &mut self.places[index],
                        &row.object,
                        row.required[index]
                    ),
                    Ok(Observation::Published | Observation::Partial)
                ) {
                    // A superseding selection can arrive after publication but
                    // before ingest retention. Finish that original operation
                    // before its claim handoff; do not invent a new copy key.
                    if let Err(e) = self.ensure(
                        index,
                        &row.object,
                        &mut left,
                        &mut streamed,
                        start,
                        &mut effects,
                    ) {
                        holds.push(e);
                        continue;
                    }
                }
                match observe(
                    self.config,
                    &self.config.providers[index],
                    &mut self.places[index],
                    &row.object,
                    row.required[index],
                ) {
                    Ok(Observation::Retained | Observation::Released) => {
                        let result = if self.allow_eviction {
                            match keepers.iter().find(|k| {
                                self.config.providers[**k].failure_domain
                                    != self.config.providers[index].failure_domain
                            }) {
                                Some(keeper) => {
                                    self.handoff(index, *keeper, &row.object, &mut effects)
                                }
                                None => Err(Error::Protected),
                            }
                        } else {
                            Err(Error::Disabled)
                        };
                        if let Err(e) = result {
                            holds.push(e);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => holds.push(e),
                }
            }
            // The ordinary retirement adapter retains the backup grants itself.
            // Hold every other desired cache reader through any source unlink.
            let mut readers = Vec::new();
            for (index, required) in row.required.iter().enumerate() {
                if *required && self.config.providers[index].role == Role::Cache {
                    match self.places[index].store.read_object(&row.object) {
                        Ok(r) => readers.push(r),
                        Err(e) => {
                            holds.push(e.into());
                            floor = false;
                        }
                    }
                }
            }
            if !floor {
                continue;
            }
            for index in 0..self.places.len() {
                if self.config.providers[index].role != Role::Cache {
                    continue;
                }
                let result = self.release_cache(index, row, &keepers, &mut effects);
                if let Err(e) = result {
                    holds.push(e);
                }
                hook(&row.object, index)?;
            }
            drop(readers);
        }
        let mut report = self.status(plan)?;
        report.stream_bytes = streamed;
        report.effects = effects;
        report.holds.extend(holds);
        report.holds.sort_by_key(|e| format!("{e:?}"));
        report.holds.dedup();
        if !report.holds.is_empty() {
            report.page_converged = false;
            report.storage_converged = Some(false);
        }
        Ok(report)
    }
    fn activate(
        &mut self,
        index: usize,
        object: &Object,
        left: &mut u64,
        streamed: &mut u64,
        start: Instant,
        effects: &mut Vec<Effect>,
    ) -> Result<()> {
        if self.config.providers[index].role == Role::Cache
            && let Recipe::Retirement(plan) = recipe(
                self.config,
                &self.config.providers[index],
                self.places[index].store,
                object,
            )?
        {
            let present = self.places[index].store.object_present(object)?;
            if present {
                if !self.allow_cancellation {
                    return Err(Error::Disabled);
                }
                let before = self.places[index]
                    .store
                    .life_head(object)?
                    .ok_or(Error::Recovery)?
                    .birth
                    .index;
                self.retire(index, &plan, true)?;
                let after = self.places[index]
                    .store
                    .life_head(object)?
                    .ok_or(Error::Recovery)?
                    .birth
                    .index;
                if after != before {
                    effects.push(self.effect(object, index, EffectKind::Cancelled, 0, 0));
                }
            } else {
                if !self.allow_reacquisition || !self.allow_copy {
                    return Err(Error::ReacquisitionRequired);
                }
                // An existing intent with absent bytes can only complete past
                // metadata; it cannot perform another unlink.
                self.retire(index, &plan, false)?;
                if object.bytes > *left {
                    return Err(Error::Budget);
                }
                let pin = &self.config.providers[index];
                let request = Request {
                    operation: key("reacquire", &(self.config.id()?, plan.id()?))?,
                    owner: self.config.owner.clone(),
                    object: object.clone(),
                    required: AckLevel::ProcessRestart,
                };
                let reacquire = retirement::plan_reacquisition(
                    self.places[index].store,
                    &plan,
                    request,
                    context(self.config, pin, object)?,
                )?;
                if reacquire.required_peak()? > self.config.budget.max_peak_bytes {
                    return Err(Error::Budget);
                }
                let auth = reacquire.authorize()?;
                let stores = self.places.iter().map(|p| p.store).collect::<Vec<_>>();
                let runtime = &mut self.places[index];
                let mut source = LazySource {
                    stores: &stores,
                    target: &pin.place.instance,
                    seed: self.seed,
                    object,
                    reader: None,
                    position: 0,
                    left,
                    streamed,
                    start,
                    millis: self.config.budget.limits.millis,
                };
                retirement::Reacquisition {
                    store: runtime.store,
                    ledger: runtime.ledger,
                    job: runtime.job,
                }
                .apply(&reacquire, &auth, &mut source)?;
                drop(source);
                effects.push(self.effect(object, index, EffectKind::Reacquired, object.bytes, 0));
            }
        }
        self.ensure(index, object, left, streamed, start, effects)
    }
    fn release_cache(
        &mut self,
        index: usize,
        row: &Row,
        keepers: &[usize],
        effects: &mut Vec<Effect>,
    ) -> Result<()> {
        let object = &row.object;
        if !row.required[index]
            && settled_final_absence(
                &self.config.providers[index],
                &mut self.places[index],
                object,
            )?
        {
            return Ok(());
        }
        if let Recipe::Retirement(plan) = recipe(
            self.config,
            &self.config.providers[index],
            self.places[index].store,
            object,
        )? {
            if row.required[index] {
                return Err(Error::Recovery);
            }
            if !self.allow_eviction {
                return Err(Error::Disabled);
            }
            let receipt = self.retire(index, &plan, false)?;
            if !receipt.historical_replay {
                effects.push(self.effect(
                    object,
                    index,
                    EffectKind::Recovered,
                    receipt.logical_bytes,
                    receipt.file_allocation_before,
                ));
            }
            return Ok(());
        }
        let observed = observe(
            self.config,
            &self.config.providers[index],
            &mut self.places[index],
            object,
            row.required[index],
        )?;
        if observed == Observation::NotStarted {
            return Ok(());
        }
        if !matches!(
            observed,
            Observation::Published | Observation::Retained | Observation::Released
        ) {
            return Err(Error::Recovery);
        }
        if row.required[index] {
            return Ok(());
        }
        if observed != Observation::Released {
            return Err(Error::Retained);
        }
        if !self.allow_eviction {
            return Err(Error::Disabled);
        }
        let Recipe::Publication { request, .. } = recipe(
            self.config,
            &self.config.providers[index],
            self.places[index].store,
            object,
        )?
        else {
            return Err(Error::Recovery);
        };
        let mut grants = Vec::new();
        for keeper in keepers {
            grants.push(retirement::Keeper {
                grant: self.grant(*keeper, object)?,
                enrollment: self.config.providers[*keeper].replica(),
            });
        }
        let head = self.places[index]
            .store
            .life_head(object)?
            .ok_or(Error::Recovery)?;
        let plan = retirement::plan(
            self.places[index].store,
            retirement::Request {
                operation: key(
                    "placement-retire",
                    &(
                        self.config.id()?,
                        &self.config.providers[index].place,
                        object,
                        head.birth.index,
                    ),
                )?,
                owner: self.config.owner.clone(),
                publication: request,
                context: self.config.id()?,
                source: self.config.providers[index].replica(),
                keepers: grants,
                required_domains: self.config.minimum_domains,
                capacity: self.config.providers[index].capacity.clone(),
            },
        )?;
        let receipt = self.retire(index, &plan, false)?;
        if !receipt.historical_replay {
            effects.push(self.effect(
                object,
                index,
                EffectKind::Retired,
                receipt.logical_bytes,
                receipt.file_allocation_before,
            ));
        }
        Ok(())
    }
    pub fn status(&mut self, p: &Plan) -> Result<Report> {
        self.check()?;
        p.id()?;
        if p.enrollment != self.config.id()?
            || p.providers
                != self
                    .config
                    .providers
                    .iter()
                    .map(|p| p.place.instance.clone())
                    .collect::<Vec<_>>()
            || p.budget != self.config.budget
        {
            return Err(Error::WrongEnrollment);
        }
        let desired = self.desired()?;
        let current = desired.basis == p.basis;
        if current {
            let cursor = p.after.as_ref().map(|after| Cursor {
                basis: p.basis.clone(),
                after: after.clone(),
            });
            if self.page(&desired, cursor.as_ref())?.to_json()? != p.to_json()? {
                return Err(Error::Invalid);
            }
        }
        let mut observations = Vec::new();
        let mut holds = Vec::new();
        let mut converged = current;
        for row in &p.rows {
            if row.required.len() != self.places.len() {
                return Err(Error::Invalid);
            }
            for (index, runtime) in self.places.iter_mut().enumerate() {
                let state = match observe(
                    self.config,
                    &self.config.providers[index],
                    runtime,
                    &row.object,
                    row.required[index],
                ) {
                    Ok(state) => state,
                    Err(e) => {
                        holds.push(e);
                        Observation::Held(e)
                    }
                };
                let satisfied = if row.required[index] {
                    match self.config.providers[index].role {
                        Role::Retained => state == Observation::Retained,
                        Role::Cache => matches!(
                            state,
                            Observation::Published | Observation::Retained | Observation::Released
                        ),
                    }
                } else {
                    matches!(
                        state,
                        Observation::NotStarted | Observation::AuthorizedAbsence
                    )
                };
                converged &= satisfied;
                observations.push(Observed {
                    object: row.object.clone(),
                    provider: self.config.providers[index].place.instance.clone(),
                    required: row.required[index],
                    state,
                });
            }
        }
        Ok(Report {
            schema: "tig.placement-report.design-1",
            plan: p.id()?,
            observed_basis: desired.basis,
            basis_current: current,
            page_converged: converged,
            storage_converged: if converged && (p.after.is_some() || p.next.is_some()) {
                None
            } else {
                Some(converged)
            },
            stream_bytes: 0,
            effects: Vec::new(),
            observations,
            holds,
            next: if current { p.next.clone() } else { None },
        })
    }
    fn effect(
        &self,
        object: &Object,
        index: usize,
        kind: EffectKind,
        logical_bytes: u64,
        file_allocation_before: u64,
    ) -> Effect {
        Effect {
            object: object.clone(),
            provider: self.config.providers[index].place.instance.clone(),
            kind,
            logical_bytes,
            file_allocation_before,
        }
    }
    fn ensure(
        &mut self,
        index: usize,
        object: &Object,
        left: &mut u64,
        streamed: &mut u64,
        start: Instant,
        effects: &mut Vec<Effect>,
    ) -> Result<()> {
        let state = observe(
            self.config,
            &self.config.providers[index],
            &mut self.places[index],
            object,
            true,
        )?;
        if matches!(state, Observation::Retained | Observation::Released) {
            return Ok(());
        }
        if !self.allow_copy {
            return Err(Error::Disabled);
        }
        let pin = &self.config.providers[index];
        let recipe = recipe(self.config, pin, self.places[index].store, object)?;
        let (request, payload, reacquire) = match recipe {
            Recipe::Publication {
                request,
                payload,
                reacquire,
            } => (request, payload, reacquire),
            Recipe::Retirement(_) => return Err(Error::ReacquisitionRequired),
        };
        let retention = retention_plan(self.places[index].store, &request)?;
        let peak = payload
            .required_peak()
            .checked_add(retention.required_peak())
            .ok_or(Error::Budget)?;
        if peak > self.config.budget.max_peak_bytes {
            return Err(Error::Budget);
        }
        if state != Observation::Published {
            if object.bytes > *left {
                return Err(Error::Budget);
            }
            let stores = self.places.iter().map(|p| p.store).collect::<Vec<_>>();
            let runtime = &mut self.places[index];
            let mut reader = LazySource {
                stores: &stores,
                target: &pin.place.instance,
                seed: self.seed,
                object,
                reader: None,
                position: 0,
                left,
                streamed,
                start,
                millis: self.config.budget.limits.millis,
            };
            if let Some(plan) = reacquire {
                if !self.allow_reacquisition {
                    return Err(Error::ReacquisitionRequired);
                }
                let auth = plan.authorize()?;
                retirement::Reacquisition {
                    store: runtime.store,
                    ledger: runtime.ledger,
                    job: runtime.job,
                }
                .apply(&plan, &auth, &mut reader)?;
                drop(reader);
                effects.push(self.effect(object, index, EffectKind::Reacquired, object.bytes, 0));
            } else {
                accounted::publish_accounted(
                    runtime.ledger,
                    runtime.store,
                    runtime.job,
                    &payload,
                    &mut reader,
                )?;
                drop(reader);
                effects.push(self.effect(object, index, EffectKind::Published, object.bytes, 0));
            }
        }
        let runtime = &mut self.places[index];
        if retention::recorded(runtime.ledger, runtime.store, runtime.job, &retention)?.is_none() {
            retention::retain(runtime.ledger, runtime.store, runtime.job, &retention)?;
            effects.push(self.effect(object, index, EffectKind::Retained, 0, 0));
        }
        Ok(())
    }
    fn grant(&mut self, index: usize, object: &Object) -> Result<RetentionGrant> {
        let Recipe::Publication {
            request, payload, ..
        } = recipe(
            self.config,
            &self.config.providers[index],
            self.places[index].store,
            object,
        )?
        else {
            return Err(Error::Recovery);
        };
        let runtime = &mut self.places[index];
        if !published(runtime, &request, &payload)? {
            return Err(Error::Recovery);
        }
        let plan = retention_plan(runtime.store, &request)?;
        retention::recorded(runtime.ledger, runtime.store, runtime.job, &plan)?
            .ok_or(Error::Recovery)
    }
    fn handoff(
        &mut self,
        index: usize,
        keeper: usize,
        object: &Object,
        effects: &mut Vec<Effect>,
    ) -> Result<()> {
        let previous = self.grant(index, object)?;
        let successor = self.grant(keeper, object)?;
        let request = handoff::HandoffRequest {
            operation: key("placement-handoff", &(self.config.id()?, &previous))?,
            owner: self.config.owner.clone(),
            previous,
            successor,
            source_domain: self.config.providers[index].failure_domain.clone(),
            successor_domain: self.config.providers[keeper].failure_domain.clone(),
        };
        let plan = handoff::plan(self.places[index].store, &request)?;
        {
            let source = &mut self.places[index];
            if handoff::inspect(source.ledger, source.store, source.job, &plan)? {
                return Ok(());
            }
        }
        if plan.required_peak() > self.config.budget.max_peak_bytes {
            return Err(Error::Budget);
        }
        let auth = plan.authorize_handoff()?;
        let (source, other) = two(self.places, index, keeper)?;
        handoff::apply(
            source.ledger,
            source.store,
            other.ledger,
            other.store,
            source.job,
            &plan,
            &auth,
        )?;
        effects.push(self.effect(object, index, EffectKind::Handoff, 0, 0));
        Ok(())
    }
    fn retire(
        &mut self,
        index: usize,
        plan: &retirement::Plan,
        cancel: bool,
    ) -> Result<retirement::Receipt> {
        if plan.request().owner != self.config.owner
            || plan.request().context != self.config.id()?
            || plan.request().source != self.config.providers[index].replica()
        {
            return Err(Error::WrongEnrollment);
        }
        if plan.required_peak() > self.config.budget.max_peak_bytes {
            return Err(Error::Budget);
        }
        let wanted = plan
            .request()
            .keepers
            .iter()
            .map(|k| k.grant.store())
            .collect::<BTreeSet<_>>();
        let enrollments = self
            .config
            .providers
            .iter()
            .map(Provider::replica)
            .collect::<Vec<_>>();
        let (left, right) = self.places.split_at_mut(index);
        let (source, tail) = right.split_first_mut().ok_or(Error::Invalid)?;
        let mut backups = left
            .iter_mut()
            .chain(tail)
            .filter(|p| wanted.contains(p.store.identity()))
            .map(|p| retirement::Backup {
                store: p.store,
                ledger: p.ledger,
                enrollment: enrollments
                    .iter()
                    .find(|pin| pin.store == *p.store.identity())
                    .expect("checked provider"),
            })
            .collect::<Vec<_>>();
        let source_enrollment = self.config.providers[index].replica();
        let context = self.config.id()?;
        let mut execution = retirement::Execution {
            store: source.store,
            ledger: source.ledger,
            job: source.job,
            namespaces: self.namespaces,
            nodes: self.nodes,
            limits: &self.config.budget.limits,
            backups: &mut backups,
            admitted_context: &context,
            source_enrollment: &source_enrollment,
        };
        let auth = plan.authorize()?;
        if cancel {
            Ok(execution.cancel(plan, &auth)?)
        } else {
            Ok(execution.apply(plan, &auth)?)
        }
    }
}
impl Provider {
    fn replica(&self) -> ReplicaEnrollment {
        ReplicaEnrollment {
            store: self.place.instance.clone(),
            failure_domain: self.failure_domain.clone(),
        }
    }
}
fn two<'a, 'b>(
    places: &'a mut [Local<'b>],
    a: usize,
    b: usize,
) -> Result<(&'a mut Local<'b>, &'a mut Local<'b>)> {
    if a == b {
        return Err(Error::Invalid);
    }
    if a < b {
        let (left, right) = places.split_at_mut(b);
        Ok((&mut left[a], &mut right[0]))
    } else {
        let (left, right) = places.split_at_mut(a);
        Ok((&mut right[0], &mut left[b]))
    }
}
