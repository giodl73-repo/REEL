//! Bounded single-authority accounting model. Not a durable reservation service
//! or filesystem observation. Physical proofs and transaction persistence belong
//! to admitted provider/engine adapters.
use std::collections::BTreeMap;

pub const MAX_RESERVATIONS: usize = 1024;
pub const MAX_EVENTS: usize = 4096;
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CapacityError {
    Invalid,
    UnknownObservation,
    Insufficient,
    Conflict,
    Stale,
    Closed,
    InFlight,
    Budget,
    Overflow,
}
impl std::fmt::Display for CapacityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CapacityError {}
pub type Result<T> = std::result::Result<T, CapacityError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReservationToken {
    domain: String,
    epoch: String,
    id: String,
    owner: String,
    generation: u64,
}
impl ReservationToken {
    pub fn owner(&self) -> &str {
        &self.owner
    }
}
#[derive(Clone, Debug)]
pub struct AllocationTicket {
    reservation: ReservationToken,
    event: String,
    limit: u64,
}
#[derive(Clone, Debug)]
pub struct ObservationTicket {
    domain: String,
    epoch: String,
    nonce: u64,
    watermark: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapacitySummary {
    pub sampled_free: u64,
    pub reserved_remaining: u64,
    pub allocated_after_watermark: u64,
    pub available: u64,
}
#[derive(Clone, Debug)]
struct Event {
    limit: u64,
    completed: Option<(u64, u64)>,
}
#[derive(Clone, Debug)]
struct Reservation {
    token: ReservationToken,
    peak: u64,
    remaining: u64,
    closed: bool,
    events: BTreeMap<String, Event>,
}

#[derive(Debug)]
pub struct CapacityDomain {
    domain: String,
    epoch: String,
    floor: u64,
    next_generation: u64,
    allocation_sequence: u64,
    observation_nonce: u64,
    pending_observation: Option<u64>,
    sampled_free: Option<u64>,
    records: BTreeMap<String, Reservation>,
    debits: BTreeMap<u64, u64>,
    event_count: usize,
}
impl CapacityDomain {
    /// Stage a bounded pure transition and adopt only if the caller's metadata
    /// commit succeeds. This is not persistence or a distributed authority fork.
    /// Do not use staged tokens/results for physical work before successful return.
    pub fn try_transition<T, E>(
        &mut self,
        action: impl FnOnce(&mut Self) -> std::result::Result<T, E>,
    ) -> std::result::Result<T, E> {
        let mut staged = Self {
            domain: self.domain.clone(),
            epoch: self.epoch.clone(),
            floor: self.floor,
            next_generation: self.next_generation,
            allocation_sequence: self.allocation_sequence,
            observation_nonce: self.observation_nonce,
            pending_observation: self.pending_observation,
            sampled_free: self.sampled_free,
            records: self.records.clone(),
            debits: self.debits.clone(),
            event_count: self.event_count,
        };
        let result = action(&mut staged)?;
        *self = staged;
        Ok(result)
    }
    pub fn new(domain: &str, epoch: &str, floor: u64) -> Result<Self> {
        if !crate::token(domain) || !crate::token(epoch) {
            return Err(CapacityError::Invalid);
        }
        Ok(Self {
            domain: domain.into(),
            epoch: epoch.into(),
            floor,
            next_generation: 0,
            allocation_sequence: 0,
            observation_nonce: 0,
            pending_observation: None,
            sampled_free: None,
            records: BTreeMap::new(),
            debits: BTreeMap::new(),
            event_count: 0,
        })
    }
    pub fn domain(&self) -> &str {
        &self.domain
    }
    /// Capture BEFORE sampling free bytes. Allocations confirmed afterward stay
    /// charged even when a racing sample might already include them.
    pub fn begin_observation(&mut self) -> Result<ObservationTicket> {
        let nonce = self
            .observation_nonce
            .checked_add(1)
            .ok_or(CapacityError::Overflow)?;
        self.observation_nonce = nonce;
        self.pending_observation = Some(nonce);
        Ok(ObservationTicket {
            domain: self.domain.clone(),
            epoch: self.epoch.clone(),
            nonce,
            watermark: self.allocation_sequence,
        })
    }
    /// sampled_free must come from the admitted probe after this ticket was issued.
    /// Timestamp/freshness, provider visibility and persistence are not proved here.
    pub fn observe(&mut self, ticket: ObservationTicket, sampled_free: u64) -> Result<()> {
        if ticket.domain != self.domain
            || ticket.epoch != self.epoch
            || self.pending_observation != Some(ticket.nonce)
        {
            return Err(CapacityError::Stale);
        }
        self.debits
            .retain(|sequence, _| *sequence > ticket.watermark);
        self.sampled_free = Some(sampled_free);
        self.pending_observation = None;
        Ok(())
    }
    pub fn invalidate_observation(&mut self) {
        self.sampled_free = None;
        self.pending_observation = None;
    }
    pub fn summary(&self) -> Result<CapacitySummary> {
        let sampled_free = self.sampled_free.ok_or(CapacityError::UnknownObservation)?;
        let remaining = sum(self.records.values().map(|r| r.remaining))?;
        let allocated = sum(self.debits.values().copied())?;
        let held = remaining
            .checked_add(allocated)
            .ok_or(CapacityError::Overflow)?;
        Ok(CapacitySummary {
            sampled_free,
            reserved_remaining: remaining,
            allocated_after_watermark: allocated,
            available: sampled_free.saturating_sub(self.floor).saturating_sub(held),
        })
    }
    pub fn reserve(&mut self, id: &str, owner: &str, peak: u64) -> Result<ReservationToken> {
        if !crate::token(id) || !crate::token(owner) || peak == 0 {
            return Err(CapacityError::Invalid);
        }
        if let Some(r) = self.records.get(id) {
            if r.token.owner != owner || r.peak != peak {
                return Err(CapacityError::Conflict);
            }
            if r.closed {
                return Err(CapacityError::Closed);
            }
            return Ok(r.token.clone());
        }
        if self.records.len() >= MAX_RESERVATIONS {
            return Err(CapacityError::Budget);
        }
        if peak > self.summary()?.available {
            return Err(CapacityError::Insufficient);
        }
        let generation = self
            .next_generation
            .checked_add(1)
            .ok_or(CapacityError::Overflow)?;
        let token = ReservationToken {
            domain: self.domain.clone(),
            epoch: self.epoch.clone(),
            id: id.into(),
            owner: owner.into(),
            generation,
        };
        self.records.insert(
            id.into(),
            Reservation {
                token: token.clone(),
                peak,
                remaining: peak,
                closed: false,
                events: BTreeMap::new(),
            },
        );
        self.next_generation = generation;
        Ok(token)
    }
    fn record(&self, token: &ReservationToken) -> Result<&Reservation> {
        if token.domain != self.domain || token.epoch != self.epoch {
            return Err(CapacityError::Stale);
        }
        let r = self.records.get(&token.id).ok_or(CapacityError::Stale)?;
        if r.token != *token {
            return Err(CapacityError::Stale);
        }
        Ok(r)
    }
    pub fn allocation_status(&self, ticket: &AllocationTicket) -> Result<Option<u64>> {
        let r = self.record(&ticket.reservation)?;
        let event = r.events.get(&ticket.event).ok_or(CapacityError::Stale)?;
        if event.limit != ticket.limit {
            return Err(CapacityError::Stale);
        }
        Ok(event.completed.map(|(charge, _)| charge))
    }
    /// Bound a write before it starts. In-flight space remains fully reserved.
    pub fn begin_allocation(
        &mut self,
        token: &ReservationToken,
        event: &str,
        limit: u64,
    ) -> Result<AllocationTicket> {
        if !crate::token(event) || limit == 0 {
            return Err(CapacityError::Invalid);
        }
        let r = self.record(token)?;
        if let Some(old) = r.events.get(event) {
            if old.limit != limit {
                return Err(CapacityError::Conflict);
            }
            return Ok(AllocationTicket {
                reservation: token.clone(),
                event: event.into(),
                limit,
            });
        }
        if r.closed {
            return Err(CapacityError::Closed);
        }
        if self.event_count >= MAX_EVENTS {
            return Err(CapacityError::Budget);
        }
        let in_flight = sum(r
            .events
            .values()
            .filter(|e| e.completed.is_none())
            .map(|e| e.limit))?;
        if limit
            > r.remaining
                .checked_sub(in_flight)
                .ok_or(CapacityError::Overflow)?
        {
            return Err(CapacityError::Insufficient);
        }
        self.records
            .get_mut(&token.id)
            .ok_or(CapacityError::Stale)?
            .events
            .insert(
                event.into(),
                Event {
                    limit,
                    completed: None,
                },
            );
        self.event_count += 1;
        Ok(AllocationTicket {
            reservation: token.clone(),
            event: event.into(),
            limit,
        })
    }
    /// Call only AFTER actual allocation is visible, or admitted/fenced proof of
    /// zero remaining allocation. Unknown partial writes must stay in flight.
    /// An explicitly labelled conservative accounting adapter may charge a
    /// scoped footprint upper bound; this is not measured net volume growth.
    pub fn complete_allocation(&mut self, ticket: &AllocationTicket, actual: u64) -> Result<u64> {
        let r = self.record(&ticket.reservation)?;
        let e = r.events.get(&ticket.event).ok_or(CapacityError::Stale)?;
        if ticket.limit != e.limit || actual > e.limit {
            return Err(CapacityError::Conflict);
        }
        if let Some((old, sequence)) = e.completed {
            return if old == actual {
                Ok(sequence)
            } else {
                Err(CapacityError::Conflict)
            };
        }
        if r.closed {
            return Err(CapacityError::Closed);
        }
        let remaining = r
            .remaining
            .checked_sub(actual)
            .ok_or(CapacityError::Overflow)?;
        let sequence = self
            .allocation_sequence
            .checked_add(1)
            .ok_or(CapacityError::Overflow)?;
        let r = self
            .records
            .get_mut(&ticket.reservation.id)
            .ok_or(CapacityError::Stale)?;
        r.remaining = remaining;
        r.events
            .get_mut(&ticket.event)
            .ok_or(CapacityError::Stale)?
            .completed = Some((actual, sequence));
        if actual > 0 {
            self.debits.insert(sequence, actual);
        }
        self.allocation_sequence = sequence;
        Ok(sequence)
    }
    /// Releases unallocated headroom only after all writes have a disposition.
    /// Does not free actual files or authorize disposal of anything on disk.
    pub fn finish(&mut self, token: &ReservationToken) -> Result<()> {
        let r = self.record(token)?;
        if r.closed {
            return Ok(());
        }
        if r.events.values().any(|e| e.completed.is_none()) {
            return Err(CapacityError::InFlight);
        }
        let r = self
            .records
            .get_mut(&token.id)
            .ok_or(CapacityError::Stale)?;
        r.closed = true;
        r.remaining = 0;
        Ok(())
    }
}
fn sum(mut values: impl Iterator<Item = u64>) -> Result<u64> {
    values.try_fold(0u64, |a, b| a.checked_add(b).ok_or(CapacityError::Overflow))
}
