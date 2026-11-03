//! Common retained delivery claims; never a remote census or removal authority.
use crate::{
    native::BoundInstance,
    publication::{ParsedBinding, ParsedReceipt, ReceiptChecker, ReceiptReport},
    revisions::{self, Budget, Coverage, Limits, Reader, acyclic, token, unique},
    schema::Checked,
};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tig_core::{Digest, Object};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Metadata(revisions::Error),
    Attachment,
    Policy,
    Event,
    Subject,
    Target,
    Evidence,
    Conflict,
}
impl From<revisions::Error> for Error {
    fn from(error: revisions::Error) -> Self {
        Self::Metadata(error)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachment {
    schema: String,
    policy_slot: String,
    history_slot: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: String,
    targets: Vec<Target>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Target {
    key: String,
    identity: Digest,
    destination: Object,
    representation: String,
    profile: Object,
    gates: Object,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    schema: String,
    asset: String,
    coverage: Coverage,
    parents: Vec<Object>,
    events: Vec<Object>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Attempted,
    Acknowledged,
    Readback,
    Failed,
    Absent,
}
impl Stage {
    fn observed(self) -> bool {
        matches!(self, Self::Acknowledged | Self::Readback | Self::Absent)
    }
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Evidence {
    Receipt { binding: Object },
    Observation { object: Object },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    schema: String,
    asset: String,
    event: String,
    operation: String,
    parents: Vec<Object>,
    policy: Object,
    target: String,
    revision: Object,
    representation: String,
    subject: Object,
    profile: Object,
    stage: Stage,
    evidence: Option<Evidence>,
}
#[derive(Clone, PartialEq, Eq)]
struct Operation {
    target: String,
    identity: Digest,
    revision: Object,
    representation: String,
    subject: Object,
    profile: Object,
    policy: Object,
}

/// Describes retained claims only; not the actual current provider state.
pub enum State {
    Untracked,
    UnknownCoverage,
    Claim {
        event: Object,
        revision: Object,
        subject: Object,
        stage: Stage,
        profile: Object,
        policy: Object,
    },
    Ambiguous {
        events: Vec<Object>,
    },
}

/// Safe aggregates; no target keys, locators or event identifiers.
#[derive(Default, Debug, serde::Serialize)]
pub struct StateCounts {
    pub claims: usize,
    pub absent: usize,
    pub ambiguous: usize,
    pub untracked: usize,
    pub unknown_coverage: usize,
}

/// No Debug/Serialize: IDs, target configuration and inventories stay private.
pub struct History {
    current_policy: Policy,
    revisions: Option<revisions::History>,
    coverage: Option<Coverage>,
    metadata: BTreeSet<Object>,
    receipts: BTreeSet<Object>,
    opaque: BTreeSet<Object>,
    events: BTreeMap<Object, Event>,
    frontiers: BTreeMap<Digest, BTreeSet<Object>>,
    receipt_bindings: usize,
}
impl History {
    pub(crate) fn contains_event(&self, object: &Object) -> bool {
        self.events.contains_key(object)
    }
    pub fn state_counts(&self) -> StateCounts {
        let mut counts = StateCounts::default();
        for target in &self.current_policy.targets {
            match self.state(&target.key) {
                Some(State::Claim {
                    stage: Stage::Absent,
                    ..
                }) => counts.absent += 1,
                Some(State::Claim { .. }) => counts.claims += 1,
                Some(State::Ambiguous { .. }) => counts.ambiguous += 1,
                Some(State::Untracked) => counts.untracked += 1,
                Some(State::UnknownCoverage) => counts.unknown_coverage += 1,
                None => (),
            }
        }
        counts
    }
    pub fn state(&self, key: &str) -> Option<State> {
        let target = self.current_policy.targets.iter().find(|t| t.key == key)?;
        if self.coverage == Some(Coverage::Partial) {
            return Some(State::UnknownCoverage);
        }
        let candidates = self.frontiers.get(&target.identity);
        Some(match candidates {
            None => State::Untracked,
            Some(set) if set.len() == 1 => {
                let pin = set.first()?;
                let event = self.events.get(pin)?;
                State::Claim {
                    event: pin.clone(),
                    revision: event.revision.clone(),
                    subject: event.subject.clone(),
                    stage: event.stage,
                    profile: event.profile.clone(),
                    policy: event.policy.clone(),
                }
            }
            Some(set) => State::Ambiguous {
                events: set.iter().cloned().collect(),
            },
        })
    }
    pub fn revisions(&self) -> Option<&revisions::History> {
        self.revisions.as_ref()
    }
    pub fn coverage(&self) -> Option<Coverage> {
        self.coverage
    }
    pub fn metadata(&self) -> &BTreeSet<Object> {
        &self.metadata
    }
    pub fn receipts(&self) -> &BTreeSet<Object> {
        &self.receipts
    }
    /// Uninterpreted observation/destination/profile/gate references, not custody.
    pub fn opaque(&self) -> &BTreeSet<Object> {
        &self.opaque
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn receipt_binding_count(&self) -> usize {
        self.receipt_bindings
    }
    pub fn current_remote_content_verified(&self) -> bool {
        false
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
    pub fn authorizes_eviction(&self) -> bool {
        false
    }
}

fn policy(pin: &Object, budget: &mut Budget, reader: &mut impl Reader) -> Result<Policy> {
    let policy: Policy = budget.read(pin, reader)?;
    if policy.schema != "tig.delivery-policy.experimental-1"
        || policy.targets.is_empty()
        || policy.targets.len() > 256
    {
        return Err(Error::Policy);
    }
    let mut keys = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for target in &policy.targets {
        token(&target.key)?;
        token(&target.representation)?;
        if !keys.insert(&target.key) || !identities.insert(&target.identity) {
            return Err(Error::Policy);
        }
        for object in [&target.destination, &target.profile, &target.gates] {
            budget.pin(object)?
        }
    }
    Ok(policy)
}

fn receipt(
    pin: &Object,
    budget: &mut Budget,
    reader: &mut impl Reader,
    parsed: &mut BTreeMap<Object, ParsedReceipt>,
) -> Result<ReceiptReport> {
    let binding_raw = budget.blob(pin, reader, 65_536)?;
    let binding = ParsedBinding::parse(&binding_raw).map_err(|_| Error::Evidence)?;
    for object in [binding.mapping(), binding.receipt(), binding.subject()] {
        budget.pin(object)?
    }
    let mapping = budget.blob(binding.mapping(), reader, 65_536)?;
    let checker = ReceiptChecker::from_binding(&binding, &mapping).map_err(|_| Error::Evidence)?;
    if !parsed.contains_key(checker.receipt()) {
        let raw = budget.blob(checker.receipt(), reader, checker.receipt_limit())?;
        let value = checker.parse_receipt(&raw).map_err(|_| Error::Evidence)?;
        parsed.insert(checker.receipt().clone(), value);
    }
    checker
        .check_parsed(parsed.get(checker.receipt()).ok_or(Error::Evidence)?)
        .map_err(|_| Error::Evidence)
}

/// Explicit schema-checked asset, optional common delivery contract and reader.
/// All read objects share revision/evidence byte, count and reference budgets.
pub fn check(
    bound: &BoundInstance,
    checked: &Checked,
    limits: Limits,
    reader: &mut impl Reader,
) -> Result<Option<History>> {
    let mut budget = Budget::new(bound, checked, limits)?;
    let instance = bound.instance().value();
    let Some(value) = instance.metadata.get("delivery") else {
        return Ok(None);
    };
    let attachment: Attachment =
        serde_json::from_value(value.clone()).map_err(|_| Error::Attachment)?;
    if attachment.schema != "tig.delivery-state.experimental-1" {
        return Err(Error::Attachment);
    }
    let stream = |name: &str, role: &str| -> Result<Object> {
        token(name)?;
        let s = instance
            .streams
            .iter()
            .find(|s| s.slot == name && s.role == role)
            .ok_or(Error::Attachment)?;
        Ok(s.object.object())
    };
    let current = stream(&attachment.policy_slot, "policy/delivery")?;
    let root = attachment
        .history_slot
        .as_ref()
        .map(|s| stream(s, "history/delivery"))
        .transpose()?;
    let revisions = revisions::check_in(bound, &mut budget, reader)?;
    let mut policies = BTreeMap::from([(current.clone(), policy(&current, &mut budget, reader)?)]);
    let mut coverage = root.as_ref().map(|_| Coverage::Complete);
    if revisions
        .as_ref()
        .is_some_and(|h| h.coverage() == Coverage::Partial)
        && root.is_some()
    {
        coverage = Some(Coverage::Partial)
    }
    let mut pages = BTreeMap::new();
    let mut queue: VecDeque<_> = root.into_iter().collect();
    let mut event_queue = VecDeque::new();
    while let Some(object) = queue.pop_front() {
        if pages.contains_key(&object) {
            continue;
        }
        let page: Page = budget.read(&object, reader)?;
        if page.schema != "tig.delivery-history.experimental-1" || page.asset != instance.id {
            return Err(Error::Event);
        }
        unique(&page.parents, 16)?;
        unique(&page.events, 64)?;
        if page.coverage == Coverage::Partial {
            coverage = Some(Coverage::Partial)
        }
        for pin in page.parents.iter().chain(&page.events) {
            budget.pin(pin)?
        }
        queue.extend(page.parents.iter().cloned());
        event_queue.extend(page.events);
        pages.insert(object, page.parents);
    }
    acyclic(&pages)?;
    let mut events = BTreeMap::new();
    let mut event_keys = BTreeSet::new();
    let mut operations = BTreeMap::new();
    let mut ancestry = BTreeMap::new();
    let mut identities = BTreeMap::new();
    let mut bindings: BTreeMap<Object, ReceiptReport> = BTreeMap::new();
    let mut parsed = BTreeMap::new();
    let mut receipts = BTreeSet::new();
    let mut opaque = BTreeSet::new();
    while let Some(object) = event_queue.pop_front() {
        if events.contains_key(&object) {
            continue;
        }
        let event: Event = budget.read(&object, reader)?;
        if event.schema != "tig.delivery-event.experimental-1" || event.asset != instance.id {
            return Err(Error::Event);
        }
        for key in [
            &event.event,
            &event.operation,
            &event.target,
            &event.representation,
        ] {
            token(key)?
        }
        if !event_keys.insert(event.event.clone()) {
            return Err(Error::Conflict);
        }
        unique(&event.parents, 16)?;
        for pin in event.parents.iter().chain([
            &event.policy,
            &event.revision,
            &event.subject,
            &event.profile,
        ]) {
            budget.pin(pin)?
        }
        if !policies.contains_key(&event.policy) {
            policies.insert(
                event.policy.clone(),
                policy(&event.policy, &mut budget, reader)?,
            );
        }
        let target = policies
            .get(&event.policy)
            .and_then(|p| p.targets.iter().find(|t| t.key == event.target))
            .ok_or(Error::Target)?;
        if target.representation != event.representation || target.profile != event.profile {
            return Err(Error::Subject);
        }
        if revisions
            .as_ref()
            .and_then(|h| h.representation_pin(&event.revision, &event.representation))
            != Some(&event.subject)
        {
            return Err(Error::Subject);
        }
        let operation = Operation {
            target: event.target.clone(),
            identity: target.identity.clone(),
            revision: event.revision.clone(),
            representation: event.representation.clone(),
            subject: event.subject.clone(),
            profile: event.profile.clone(),
            policy: event.policy.clone(),
        };
        if operations
            .insert(event.operation.clone(), operation.clone())
            .is_some_and(|prior| prior != operation)
        {
            return Err(Error::Conflict);
        }
        match &event.evidence {
            Some(Evidence::Receipt { binding }) => {
                if event.stage == Stage::Absent {
                    return Err(Error::Evidence);
                }
                budget.pin(binding)?;
                if !bindings.contains_key(binding) {
                    bindings.insert(
                        binding.clone(),
                        receipt(binding, &mut budget, reader, &mut parsed)?,
                    );
                }
                let result = bindings.get(binding).ok_or(Error::Evidence)?;
                if result.subject != event.subject
                    || result.declared_target_identity != target.identity
                {
                    return Err(Error::Evidence);
                }
                receipts.insert(result.receipt.clone());
            }
            Some(Evidence::Observation { object }) => {
                if matches!(event.stage, Stage::Acknowledged | Stage::Readback) {
                    return Err(Error::Evidence);
                }
                budget.pin(object)?;
                opaque.insert(object.clone());
            }
            None => {
                if event.stage.observed() {
                    return Err(Error::Evidence);
                }
            }
        }
        ancestry.insert(object.clone(), event.parents.clone());
        identities.insert(object.clone(), target.identity.clone());
        event_queue.extend(event.parents.iter().cloned());
        events.insert(object, event);
    }
    acyclic(&ancestry)?;
    for (object, event) in &events {
        for parent in &event.parents {
            if identities.get(parent) != identities.get(object) {
                return Err(Error::Target);
            }
        }
    }
    let mut frontiers: BTreeMap<Digest, BTreeSet<Object>> = BTreeMap::new();
    let mut ancestors = VecDeque::new();
    for (object, event) in &events {
        if event.stage.observed() {
            frontiers
                .entry(identities[object].clone())
                .or_default()
                .insert(object.clone());
            ancestors.extend(event.parents.iter().cloned());
        }
    }
    let mut seen = BTreeSet::new();
    while let Some(object) = ancestors.pop_front() {
        if !seen.insert(object.clone()) {
            continue;
        }
        let event = events.get(&object).ok_or(Error::Event)?;
        if let Some(set) = frontiers.get_mut(&identities[&object]) {
            set.remove(&object);
        }
        ancestors.extend(event.parents.iter().cloned());
    }
    frontiers.retain(|_, set| !set.is_empty());
    for policy in policies.values() {
        for target in &policy.targets {
            opaque.extend([
                target.destination.clone(),
                target.profile.clone(),
                target.gates.clone(),
            ]);
        }
    }
    if let Some(history) = &revisions {
        opaque.extend(history.provenance().iter().cloned());
    }
    let metadata = budget.documents.difference(&receipts).cloned().collect();
    let current_policy = policies.remove(&current).ok_or(Error::Policy)?;
    Ok(Some(History {
        current_policy,
        revisions,
        coverage,
        metadata,
        receipts,
        opaque,
        events,
        frontiers,
        receipt_bindings: bindings.len(),
    }))
}
