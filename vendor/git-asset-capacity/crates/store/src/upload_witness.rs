//! Two small upload witnesses composed through existing accounted publication.
//! No transport, credentials, payload cache, deletion or independent journal.
use crate::{
    AckLevel, Key, Request, accounted,
    ledger::{CapacityLedger, JournalRef, LedgerError},
    local::{LocalAttempt, LocalStore},
};
use git_asset_core::{Digest, Object, identity};
use std::io::Cursor;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidPlan,
    Budget,
    Conflict,
    Busy,
    NeedsRecovery,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
impl From<crate::Error> for Error {
    fn from(e: crate::Error) -> Self {
        match e {
            crate::Error::Busy => Self::Busy,
            crate::Error::OperationConflict => Self::Conflict,
            crate::Error::Budget => Self::Budget,
            _ => Self::NeedsRecovery,
        }
    }
}
impl From<LedgerError> for Error {
    fn from(e: LedgerError) -> Self {
        match e {
            LedgerError::Busy => Self::Busy,
            LedgerError::Conflict => Self::Conflict,
            LedgerError::Budget
            | LedgerError::Capacity(
                git_asset_core::capacity::CapacityError::Budget
                | git_asset_core::capacity::CapacityError::Insufficient,
            ) => Self::Budget,
            _ => Self::NeedsRecovery,
        }
    }
}
impl From<accounted::AccountedError> for Error {
    fn from(e: accounted::AccountedError) -> Self {
        match e {
            accounted::AccountedError::Store(e) => e.into(),
            accounted::AccountedError::Ledger(e) => e.into(),
            _ => Self::NeedsRecovery,
        }
    }
}
pub struct Document {
    pub operation: Key,
    pub bytes: Vec<u8>,
}
struct Entry {
    request: Request,
    bytes: Vec<u8>,
    plan: accounted::PublicationPlan,
}
pub struct Packet {
    store: Digest,
    entries: [Entry; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct State {
    pub intent: bool,
    pub completion: bool,
    pub settled: bool,
}
fn digest(v: &impl serde::Serialize) -> Result<Digest> {
    let bytes = identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::InvalidPlan)?)
        .map_err(|_| Error::InvalidPlan)?;
    if bytes.len() > 16384 {
        return Err(Error::Budget);
    }
    Ok(Digest::blob(&bytes))
}
fn evidence(
    store: &LocalStore,
    ledger: &mut CapacityLedger,
    job: &JournalRef,
    entry: &Entry,
) -> Result<(bool, bool)> {
    let mut recorded = store.optional_completed_receipt(&entry.request)?.is_some();
    let id = digest(&(store.identity(), &entry.request.operation))?;
    let op = Key::parse(&format!("publish-{}", id.as_str()))?;
    let allocation = ledger.allocation_reference(&op, false)?;
    let settled = match &allocation {
        Some(a) => ledger
            .publication_status(a, &digest(&(&entry.plan, job))?)?
            .is_some(),
        None => false,
    };
    if settled && !recorded {
        recorded = store.optional_completed_receipt(&entry.request)?.is_some();
    }
    if settled && !recorded || recorded && allocation.is_none() {
        return Err(Error::NeedsRecovery);
    }
    Ok((recorded, settled))
}
impl Packet {
    pub fn new(
        store: &LocalStore,
        context: &Digest,
        owner: &Key,
        documents: [Document; 2],
    ) -> Result<Self> {
        if store.supports_retirement() || documents[0].operation == documents[1].operation {
            return Err(Error::InvalidPlan);
        }
        let mut entries = Vec::new();
        for doc in documents {
            if doc.bytes.len() > 16384 {
                return Err(Error::Budget);
            }
            let v = identity::parse_json(&doc.bytes).map_err(|_| Error::InvalidPlan)?;
            if identity::canonical_json(&v).map_err(|_| Error::InvalidPlan)? != doc.bytes {
                return Err(Error::InvalidPlan);
            }
            let request = Request {
                operation: doc.operation,
                owner: owner.clone(),
                object: Object::from_bytes(&doc.bytes),
                required: AckLevel::ProcessRestart,
            };
            let plan = accounted::plan_bound(store, &request, context.clone())?;
            entries.push(Entry {
                request,
                bytes: doc.bytes,
                plan,
            });
        }
        Ok(Self {
            store: store.identity().clone(),
            entries: entries.try_into().map_err(|_| Error::InvalidPlan)?,
        })
    }
    pub fn required_peak(&self) -> Result<u64> {
        self.entries[0]
            .plan
            .required_peak()
            .checked_add(self.entries[1].plan.required_peak())
            .ok_or(Error::Budget)
    }
    pub fn inspect(
        &self,
        store: &LocalStore,
        ledger: &mut CapacityLedger,
        job: &JournalRef,
    ) -> Result<State> {
        if store.identity() != &self.store {
            return Err(Error::InvalidPlan);
        }
        let (done, ds) = evidence(store, ledger, job, &self.entries[1])?;
        let (intent, is) = evidence(store, ledger, job, &self.entries[0])?;
        if done && !intent {
            return Err(Error::NeedsRecovery);
        }
        Ok(State {
            intent,
            completion: done,
            settled: ds && is,
        })
    }
    pub fn begin<'a>(
        &'a self,
        store: &'a LocalStore,
        ledger: &'a mut CapacityLedger,
        job: &'a JournalRef,
    ) -> Result<Pending<'a>> {
        self.inspect(store, ledger, job)?;
        let a = accounted::admit(ledger, store, job, &self.entries[0].plan)?;
        let b = accounted::admit(ledger, store, job, &self.entries[1].plan)?;
        let guard = accounted::prepare(ledger, store, job, &self.entries[1].plan, &b)?;
        if guard.has_partials()? {
            return Err(Error::NeedsRecovery);
        }
        {
            let intent = accounted::prepare(ledger, store, job, &self.entries[0].plan, &a)?;
            if intent.has_partials()? {
                return Err(Error::NeedsRecovery);
            }
        }
        let state = self.inspect(store, ledger, job)?;
        accounted::publish_accounted(
            ledger,
            store,
            job,
            &self.entries[0].plan,
            &mut Cursor::new(&self.entries[0].bytes),
        )?;
        Ok(Pending {
            packet: self,
            store,
            ledger,
            job,
            guard,
            state,
        })
    }
}
pub struct Pending<'a> {
    packet: &'a Packet,
    store: &'a LocalStore,
    ledger: &'a mut CapacityLedger,
    job: &'a JournalRef,
    guard: LocalAttempt,
    pub state: State,
}
impl Pending<'_> {
    pub fn complete(self) -> Result<()> {
        drop(self.guard);
        let done = &self.packet.entries[1];
        accounted::publish_accounted(
            self.ledger,
            self.store,
            self.job,
            &done.plan,
            &mut Cursor::new(&done.bytes),
        )?;
        Ok(())
    }
}
