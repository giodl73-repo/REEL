//! Historical matching data, never fresh observation or provider authority.
use super::checkpoint::{Budget, Error, Limits, Meter, Metrics};
use super::*;
use crate::{
    LogicalPath, Object, identity,
    tree::{NodeStore, Tree},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, Error>;
const SAFE_MAX: u64 = 9_007_199_254_740_991;
const MAX_RETIRED_LEAVES: usize = 1 + MAX_EVENTS;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationRecord {
    domain: String,
    epoch: String,
    id: String,
    owner: String,
    generation: u64,
}
impl ReservationRecord {
    pub fn matches_context(&self, domain: &str, epoch: &str) -> bool {
        self.domain == domain && self.epoch == epoch
    }
    pub fn capture(t: &ReservationToken) -> Self {
        Self {
            domain: t.domain.clone(),
            epoch: t.epoch.clone(),
            id: t.id.clone(),
            owner: t.owner.clone(),
            generation: t.generation,
        }
    }
    pub fn token(&self) -> Result<ReservationToken> {
        if !crate::token(&self.domain)
            || !crate::token(&self.epoch)
            || !crate::token(&self.id)
            || !crate::token(&self.owner)
            || self.generation == 0
            || self.generation > SAFE_MAX
        {
            return Err(Error::Invalid);
        }
        Ok(ReservationToken {
            domain: self.domain.clone(),
            epoch: self.epoch.clone(),
            id: self.id.clone(),
            owner: self.owner.clone(),
            generation: self.generation,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AllocationRecord {
    reservation: ReservationRecord,
    event: String,
    limit: u64,
}
impl AllocationRecord {
    pub fn reservation(&self) -> &ReservationRecord {
        &self.reservation
    }
    pub fn event(&self) -> &str {
        &self.event
    }
    pub fn limit(&self) -> u64 {
        self.limit
    }
    pub fn capture(t: &AllocationTicket) -> Self {
        Self {
            reservation: ReservationRecord::capture(&t.reservation),
            event: t.event.clone(),
            limit: t.limit,
        }
    }
    pub fn ticket(&self) -> Result<AllocationTicket> {
        if !crate::token(&self.event) || self.limit == 0 || self.limit > SAFE_MAX {
            return Err(Error::Invalid);
        }
        Ok(AllocationTicket {
            reservation: self.reservation.token()?,
            event: self.event.clone(),
            limit: self.limit,
        })
    }
}
/// Serializable audit evidence with deliberately no conversion to a live probe.
/// ```
/// use git_asset_core::capacity::history::ProbeEvidence;
/// let evidence: ProbeEvidence = serde_json::from_str(
///     r#"{"domain":"volume","epoch":"epoch","nonce":1,"watermark":0}"#
/// ).unwrap();
/// evidence.validate().unwrap();
/// ```
/// ```compile_fail
/// use git_asset_core::capacity::{history::ProbeEvidence, ObservationTicket};
/// let evidence: ProbeEvidence = serde_json::from_str(
///     r#"{"domain":"volume","epoch":"epoch","nonce":1,"watermark":0}"#
/// ).unwrap();
/// let live: ObservationTicket = evidence.into();
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProbeEvidence {
    domain: String,
    epoch: String,
    nonce: u64,
    watermark: u64,
}
impl ProbeEvidence {
    pub fn matches_context(&self, domain: &str, epoch: &str) -> bool {
        self.domain == domain && self.epoch == epoch
    }
    pub fn capture(t: &ObservationTicket) -> Self {
        Self {
            domain: t.domain.clone(),
            epoch: t.epoch.clone(),
            nonce: t.nonce,
            watermark: t.watermark,
        }
    }
    pub fn validate(&self) -> Result<()> {
        if !crate::token(&self.domain)
            || !crate::token(&self.epoch)
            || self.nonce == 0
            || self.nonce > SAFE_MAX
            || self.watermark > SAFE_MAX
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetiredArchive {
    schema: String,
    root: Object,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub charge: u64,
    pub sequence: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    schema: String,
    entry: Entry,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Entry {
    Header {
        reservation: ReservationRecord,
        peak: u64,
        event_count: usize,
    },
    Allocation {
        event: String,
        limit: u64,
        charge: u64,
        sequence: u64,
    },
}
impl Entry {
    fn path(&self) -> Result<LogicalPath> {
        path(&match self {
            Self::Header { .. } => "header".into(),
            Self::Allocation { event, .. } => {
                format!("a/{}", crate::Digest::blob(event.as_bytes()).as_str())
            }
        })
    }
}
fn path(s: &str) -> Result<LogicalPath> {
    LogicalPath::parse(s).map_err(|_| Error::Invalid)
}
fn canonical(v: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}
fn put(
    entry: Entry,
    nodes: &mut impl NodeStore,
    entries: &mut BTreeMap<LogicalPath, Object>,
) -> Result<()> {
    let p = entry.path()?;
    let bytes = canonical(&Document {
        schema: "tig.capacity-retired-node.design-1".into(),
        entry,
    })?;
    let o = nodes.put(&bytes)?;
    if entries.insert(p, o).is_some() {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn get(p: &LogicalPath, o: &Object, nodes: &impl NodeStore) -> Result<Entry> {
    let raw = nodes.get(o)?.ok_or_else(|| Error::Missing(o.clone()))?;
    let v = identity::parse_json(&raw).map_err(|_| Error::Invalid)?;
    let d: Document = serde_json::from_value(v).map_err(|_| Error::Invalid)?;
    if d.schema != "tig.capacity-retired-node.design-1"
        || d.entry.path()? != *p
        || canonical(&d)? != raw
    {
        return Err(Error::Invalid);
    }
    Ok(d.entry)
}
fn retired_limits(mut limits: Limits) -> Result<Limits> {
    limits.validate()?;
    limits.max_leaves = limits.max_leaves.min(MAX_RETIRED_LEAVES);
    Ok(limits)
}
fn validate_event(event: &str, limit: u64, charge: u64, sequence: u64) -> Result<()> {
    if !crate::token(event)
        || limit == 0
        || limit > SAFE_MAX
        || charge > limit
        || sequence == 0
        || sequence > SAFE_MAX
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn validate_header(
    reservation: &ReservationRecord,
    peak: u64,
    event_count: usize,
) -> Result<ReservationToken> {
    if peak == 0 || peak > SAFE_MAX || event_count > MAX_EVENTS {
        return Err(Error::Invalid);
    }
    reservation.token()
}
impl RetiredReservation {
    /// Writes only caller-admitted immutable metadata; it installs no journal root.
    pub fn export_archive(
        &self,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(RetiredArchive, Metrics)> {
        let limits = retired_limits(limits)?;
        let r = &self.record;
        let reservation = ReservationRecord::capture(&r.token);
        validate_header(&reservation, r.peak, r.events.len())?;
        if !r.closed || r.remaining != 0 {
            return Err(Error::Invalid);
        }
        if 1 + r.events.len() > limits.max_leaves {
            return Err(Error::Budget);
        }
        let mut sequences = BTreeSet::new();
        let mut total = 0u64;
        for (event, e) in &r.events {
            let (charge, sequence) = e.completed.ok_or(Error::Invalid)?;
            validate_event(event, e.limit, charge, sequence)?;
            if !sequences.insert(sequence) {
                return Err(Error::Invalid);
            }
            total = total.checked_add(charge).ok_or(Error::Invalid)?;
        }
        if total > r.peak {
            return Err(Error::Invalid);
        }
        let mut meter = Meter {
            store: nodes,
            budget: Budget::new(limits),
        };
        let mut entries = BTreeMap::new();
        put(
            Entry::Header {
                reservation,
                peak: r.peak,
                event_count: r.events.len(),
            },
            &mut meter,
            &mut entries,
        )?;
        for (event, e) in &r.events {
            let (charge, sequence) = e.completed.ok_or(Error::Invalid)?;
            put(
                Entry::Allocation {
                    event: event.clone(),
                    limit: e.limit,
                    charge,
                    sequence,
                },
                &mut meter,
                &mut entries,
            )?;
        }
        let (tree, _) = Tree::build(&entries, &mut meter, limits.tree())?;
        Ok((
            RetiredArchive {
                schema: "tig.capacity-retired.design-1".into(),
                root: tree.root().clone(),
            },
            meter.budget.metrics(),
        ))
    }
}
impl RetiredArchive {
    pub fn root(&self) -> &Object {
        &self.root
    }
    pub fn validate(&self) -> Result<()> {
        if self.schema != "tig.capacity-retired.design-1"
            || self.root.bytes == 0
            || self.root.bytes > 65536
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
    /// Reads only the historical header of an exact admitted root.
    pub fn reservation(
        &self,
        nodes: &mut impl NodeStore,
        mut limits: Limits,
    ) -> Result<(ReservationRecord, Metrics)> {
        self.validate()?;
        limits = retired_limits(limits)?;
        limits.max_nodes = limits.max_nodes.min(512);
        limits.max_bytes = limits.max_bytes.min(4 * 1024 * 1024);
        if limits.max_leaves == 0 {
            return Err(Error::Budget);
        }
        let mut meter = Meter {
            store: nodes,
            budget: Budget::new(limits),
        };
        let p = path("header")?;
        let (o, _) = Tree::from_root(self.root.clone()).get(&p, &mut meter, limits.tree())?;
        let o = o.ok_or(Error::Invalid)?;
        let Entry::Header {
            reservation,
            peak,
            event_count,
        } = get(&p, &o, &meter)?
        else {
            return Err(Error::Invalid);
        };
        validate_header(&reservation, peak, event_count)?;
        Ok((reservation, meter.budget.metrics()))
    }
    /// Complete bounded validation restores only the existing read-only history type.
    pub fn read(
        &self,
        nodes: &mut impl NodeStore,
        limits: Limits,
    ) -> Result<(RetiredReservation, Metrics)> {
        self.validate()?;
        let limits = retired_limits(limits)?;
        let mut meter = Meter {
            store: nodes,
            budget: Budget::new(limits),
        };
        let (mut entries, _) =
            Tree::from_root(self.root.clone()).entries(&mut meter, limits.tree())?;
        let header = path("header")?;
        let o = entries.remove(&header).ok_or(Error::Invalid)?;
        let Entry::Header {
            reservation,
            peak,
            event_count,
        } = get(&header, &o, &meter)?
        else {
            return Err(Error::Invalid);
        };
        let token = validate_header(&reservation, peak, event_count)?;
        if entries.len() != event_count {
            return Err(Error::Invalid);
        }
        let mut events = BTreeMap::new();
        let mut sequences = BTreeSet::new();
        let mut charged = 0u64;
        for (p, o) in entries {
            let Entry::Allocation {
                event,
                limit,
                charge,
                sequence,
            } = get(&p, &o, &meter)?
            else {
                return Err(Error::Invalid);
            };
            validate_event(&event, limit, charge, sequence)?;
            if !sequences.insert(sequence) {
                return Err(Error::Invalid);
            }
            charged = charged.checked_add(charge).ok_or(Error::Invalid)?;
            if events
                .insert(
                    event,
                    Event {
                        limit,
                        completed: Some((charge, sequence)),
                    },
                )
                .is_some()
            {
                return Err(Error::Invalid);
            }
        }
        if charged > peak {
            return Err(Error::Invalid);
        }
        Ok((
            RetiredReservation {
                record: Reservation {
                    token,
                    peak,
                    remaining: 0,
                    closed: true,
                    events,
                },
            },
            meter.budget.metrics(),
        ))
    }
    /// Point inspection of an exact admitted root, not complete coverage or authority.
    pub fn outcome(
        &self,
        nodes: &mut impl NodeStore,
        ticket: &AllocationTicket,
        mut limits: Limits,
    ) -> Result<(Outcome, Metrics)> {
        self.validate()?;
        limits = retired_limits(limits)?;
        limits.max_nodes = limits.max_nodes.min(512);
        limits.max_bytes = limits.max_bytes.min(4 * 1024 * 1024);
        if limits.max_leaves < 2 {
            return Err(Error::Budget);
        }
        let mut meter = Meter {
            store: nodes,
            budget: Budget::new(limits),
        };
        let tree = Tree::from_root(self.root.clone());
        let header = path("header")?;
        let (o, _) = tree.get(&header, &mut meter, limits.tree())?;
        let o = o.ok_or(Error::Invalid)?;
        let Entry::Header {
            reservation,
            peak,
            event_count,
        } = get(&header, &o, &meter)?
        else {
            return Err(Error::Invalid);
        };
        if validate_header(&reservation, peak, event_count)? != ticket.reservation {
            return Err(Error::Invalid);
        }
        let p = path(&format!(
            "a/{}",
            crate::Digest::blob(ticket.event.as_bytes()).as_str()
        ))?;
        let (o, _) = tree.get(&p, &mut meter, limits.tree())?;
        let o = o.ok_or(Error::Invalid)?;
        let Entry::Allocation {
            event,
            limit,
            charge,
            sequence,
        } = get(&p, &o, &meter)?
        else {
            return Err(Error::Invalid);
        };
        validate_event(&event, limit, charge, sequence)?;
        if event != ticket.event || limit != ticket.limit || charge > peak || event_count == 0 {
            return Err(Error::Invalid);
        }
        Ok((Outcome { charge, sequence }, meter.budget.metrics()))
    }
}
#[cfg(test)]
mod tests;
