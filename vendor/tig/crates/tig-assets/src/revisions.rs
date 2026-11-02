//! Optional common revision history. Checks metadata, never production or custody.
use crate::{native::BoundInstance, schema::Checked};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use tig_core::{Digest, Object, identity};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Schema,
    Attachment,
    Pin,
    Missing,
    Budget,
    Identity,
    Conflict,
    Cycle,
    Selection,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

/// Reader must bound acquisition itself; errors carry no provider/parser details.
pub trait Reader {
    fn read(&mut self, expected: &Object, maximum: usize) -> Result<Vec<u8>>;
}

#[derive(Clone, Copy)]
pub struct Limits {
    pub documents: usize,
    pub bytes: usize,
    pub references: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            documents: 4096,
            bytes: 64 * 1024 * 1024,
            references: 65_536,
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Attachment {
    schema: String,
    history_slot: String,
    selection: Option<Selection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    slot: String,
    representation: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Coverage {
    Complete,
    Partial,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Page {
    schema: String,
    asset: String,
    coverage: Coverage,
    parents: Vec<Object>,
    revisions: Vec<Object>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    schema: String,
    asset: String,
    revision: String,
    parents: Vec<Object>,
    representations: Vec<Representation>,
    provenance: Provenance,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Representation {
    key: String,
    object: Object,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
enum Provenance {
    Imported { source: Object },
    Produced { claim: Object, context: Object },
}

/// No Debug/Serialize: inventories, run keys and provider details stay private.
pub struct History {
    coverage: Coverage,
    documents: BTreeSet<Object>,
    payloads: BTreeSet<Object>,
    provenance: BTreeSet<Object>,
    revisions: BTreeMap<String, (Object, Revision)>,
    selected: Option<Object>,
}
impl History {
    pub(crate) fn contains_revision(&self, object: &Object) -> bool {
        self.revisions.values().any(|(pin, _)| pin == object)
    }
    pub fn coverage(&self) -> Coverage {
        self.coverage
    }
    pub fn documents(&self) -> &BTreeSet<Object> {
        &self.documents
    }
    pub fn payloads(&self) -> &BTreeSet<Object> {
        &self.payloads
    }
    /// These witnesses are not traversed or certified by this metadata checker.
    pub fn provenance(&self) -> &BTreeSet<Object> {
        &self.provenance
    }
    pub fn revision_count(&self) -> usize {
        self.revisions.len()
    }
    pub fn selected_revision(&self) -> Option<&Object> {
        self.selected.as_ref()
    }
    pub fn representation(&self, revision: &str, key: &str) -> Option<&Object> {
        self.revisions
            .get(revision)?
            .1
            .representations
            .iter()
            .find(|r| r.key == key)
            .map(|r| &r.object)
    }
    pub(crate) fn representation_pin(&self, revision: &Object, key: &str) -> Option<&Object> {
        self.revisions
            .values()
            .find(|(pin, _)| pin == revision)?
            .1
            .representations
            .iter()
            .find(|r| r.key == key)
            .map(|r| &r.object)
    }
    pub fn complete_semantic_coverage(&self) -> bool {
        false
    }
    pub fn authorizes_eviction(&self) -> bool {
        false
    }
}

pub(crate) struct Budget {
    limits: Limits,
    bytes: usize,
    references: usize,
    sizes: BTreeMap<Digest, u64>,
    pub(crate) documents: BTreeSet<Object>,
    cache: BTreeMap<Object, Vec<u8>>,
}
impl Budget {
    pub(crate) fn new(bound: &BoundInstance, checked: &Checked, limits: Limits) -> Result<Self> {
        if checked.instance() != bound.instance().object()
            || checked.definition() != bound.definition().object()
            || *checked.authored_schema() != bound.definition().value().authored_schema.object()
        {
            return Err(Error::Schema);
        }
        let hard = Limits::default();
        if limits.documents == 0
            || limits.documents > hard.documents
            || limits.bytes == 0
            || limits.bytes > hard.bytes
            || limits.references == 0
            || limits.references > hard.references
        {
            return Err(Error::Budget);
        }
        let instance = bound.instance().value();
        let mut budget = Self {
            limits,
            bytes: 0,
            references: 0,
            sizes: BTreeMap::new(),
            documents: BTreeSet::new(),
            cache: BTreeMap::new(),
        };
        for object in std::iter::once(bound.instance().object().clone())
            .chain(std::iter::once(instance.definition.object()))
            .chain(std::iter::once(
                bound.definition().value().authored_schema.object(),
            ))
            .chain(instance.streams.iter().map(|s| s.object.object()))
            .chain(instance.selected.iter().map(|s| s.object()))
        {
            budget.pin(&object)?;
        }
        Ok(budget)
    }

    pub(crate) fn pin(&mut self, object: &Object) -> Result<()> {
        if object.bytes > 9_007_199_254_740_991 {
            return Err(Error::Pin);
        }
        if self
            .sizes
            .insert(object.sha256.clone(), object.bytes)
            .is_some_and(|n| n != object.bytes)
        {
            return Err(Error::Pin);
        }
        self.references += 1;
        if self.references > self.limits.references {
            return Err(Error::Budget);
        }
        Ok(())
    }
    pub(crate) fn read<T: serde::de::DeserializeOwned>(
        &mut self,
        object: &Object,
        reader: &mut impl Reader,
    ) -> Result<T> {
        let raw = self.blob(object, reader, 65_536)?;
        let value = identity::parse_json(&raw).map_err(|_| Error::Schema)?;
        serde_json::from_value(value).map_err(|_| Error::Schema)
    }
    pub(crate) fn blob(
        &mut self,
        object: &Object,
        reader: &mut impl Reader,
        maximum: usize,
    ) -> Result<Vec<u8>> {
        let size = usize::try_from(object.bytes).map_err(|_| Error::Budget)?;
        if size > maximum {
            return Err(Error::Budget);
        }
        if let Some(raw) = self.cache.get(object) {
            return Ok(raw.clone());
        }
        if self.documents.len() >= self.limits.documents
            || size > self.limits.bytes.saturating_sub(self.bytes)
        {
            return Err(Error::Budget);
        }
        let raw = reader.read(object, size)?;
        if raw.len() != size || Object::from_bytes(&raw) != *object {
            return Err(Error::Pin);
        }
        self.documents.insert(object.clone());
        self.bytes += size;
        self.cache.insert(object.clone(), raw.clone());
        Ok(raw)
    }
}

pub(crate) fn token(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(Error::Identity);
    }
    Ok(())
}
pub(crate) fn unique(objects: &[Object], limit: usize) -> Result<()> {
    if objects.len() > limit {
        return Err(Error::Budget);
    }
    if objects.iter().collect::<BTreeSet<_>>().len() != objects.len() {
        return Err(Error::Conflict);
    }
    Ok(())
}

pub(crate) fn acyclic(graph: &BTreeMap<Object, Vec<Object>>) -> Result<()> {
    let mut done = BTreeSet::new();
    let mut active = BTreeSet::new();
    for root in graph.keys() {
        let mut stack = vec![(root, false)];
        while let Some((node, exit)) = stack.pop() {
            if exit {
                active.remove(node);
                done.insert(node);
            } else if !done.contains(node) {
                if !active.insert(node) {
                    return Err(Error::Cycle);
                }
                stack.push((node, true));
                for parent in graph.get(node).ok_or(Error::Missing)?.iter().rev() {
                    stack.push((parent, false));
                }
            }
        }
    }
    Ok(())
}

/// Requires exact authored-schema success, but grants no domain/production proof.
/// None means unconfigured/unknown history; it is not a complete empty inventory.
pub fn check(
    bound: &BoundInstance,
    checked: &Checked,
    limits: Limits,
    reader: &mut impl Reader,
) -> Result<Option<History>> {
    let mut budget = Budget::new(bound, checked, limits)?;
    check_in(bound, &mut budget, reader)
}

pub(crate) fn check_in(
    bound: &BoundInstance,
    budget: &mut Budget,
    reader: &mut impl Reader,
) -> Result<Option<History>> {
    let instance = bound.instance().value();
    let Some(value) = instance.metadata.get("revisions") else {
        return Ok(None);
    };
    let attachment: Attachment =
        serde_json::from_value(value.clone()).map_err(|_| Error::Attachment)?;
    if attachment.schema != "tig.revision-state.experimental-1" {
        return Err(Error::Attachment);
    }
    let slot = |name: &str, role: &str| -> Result<Object> {
        token(name)?;
        let stream = instance
            .streams
            .iter()
            .find(|s| s.slot == name)
            .ok_or(Error::Attachment)?;
        if stream.role != role {
            return Err(Error::Attachment);
        }
        Ok(stream.object.object())
    };
    let root = slot(&attachment.history_slot, "history/revisions")?;
    let selected = attachment
        .selection
        .as_ref()
        .map(|s| {
            token(&s.representation)?;
            slot(&s.slot, "revision/selected")
        })
        .transpose()?;
    let mut coverage = Coverage::Complete;
    let mut pages = BTreeMap::new();
    let mut page_queue = VecDeque::from([root]);
    let mut revision_queue = VecDeque::new();
    while let Some(object) = page_queue.pop_front() {
        if pages.contains_key(&object) {
            continue;
        }
        let page: Page = budget.read(&object, reader)?;
        if page.schema != "tig.revision-history.experimental-1" {
            return Err(Error::Schema);
        }
        if page.asset != instance.id {
            return Err(Error::Identity);
        }
        unique(&page.parents, 16)?;
        unique(&page.revisions, 64)?;
        if page.coverage == Coverage::Partial {
            coverage = Coverage::Partial
        }
        for pin in page.parents.iter().chain(&page.revisions) {
            budget.pin(pin)?
        }
        page_queue.extend(page.parents.iter().cloned());
        revision_queue.extend(page.revisions);
        pages.insert(object, page.parents);
    }
    acyclic(&pages)?;
    let mut revisions = BTreeMap::new();
    let mut manifests = BTreeMap::new();
    let mut payloads = BTreeSet::new();
    let mut provenance = BTreeSet::new();
    while let Some(object) = revision_queue.pop_front() {
        if manifests.contains_key(&object) {
            continue;
        }
        let revision: Revision = budget.read(&object, reader)?;
        if revision.schema != "tig.asset-revision.experimental-1" {
            return Err(Error::Schema);
        }
        if revision.asset != instance.id {
            return Err(Error::Identity);
        }
        token(&revision.revision)?;
        unique(&revision.parents, 16)?;
        if revision.representations.is_empty() || revision.representations.len() > 256 {
            return Err(Error::Budget);
        }
        let mut keys = BTreeSet::new();
        for rep in &revision.representations {
            token(&rep.key)?;
            if !keys.insert(&rep.key) {
                return Err(Error::Conflict);
            }
            budget.pin(&rep.object)?;
            payloads.insert(rep.object.clone());
        }
        let witnesses = match &revision.provenance {
            Provenance::Imported { source } => vec![source],
            Provenance::Produced { claim, context } => vec![claim, context],
        };
        for pin in witnesses {
            budget.pin(pin)?;
            provenance.insert(pin.clone());
        }
        for pin in &revision.parents {
            budget.pin(pin)?
        }
        revision_queue.extend(revision.parents.iter().cloned());
        manifests.insert(object.clone(), revision.parents.clone());
        if revisions
            .insert(revision.revision.clone(), (object, revision))
            .is_some()
        {
            return Err(Error::Conflict);
        }
    }
    acyclic(&manifests)?;
    if let (Some(selected), Some(selection)) = (&selected, attachment.selection) {
        let revision = revisions
            .values()
            .find(|(object, _)| object == selected)
            .map(|(_, revision)| revision)
            .ok_or(Error::Selection)?;
        let subject = revision
            .representations
            .iter()
            .find(|r| r.key == selection.representation)
            .ok_or(Error::Selection)?;
        if instance.selected.as_ref().map(|s| s.object()) != Some(subject.object.clone()) {
            return Err(Error::Selection);
        }
    }
    Ok(Some(History {
        coverage,
        documents: budget.documents.clone(),
        payloads,
        provenance,
        revisions,
        selected,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_walk_rejects_cycles_and_accepts_shared_diamond_without_recursion() {
        let a = Object::from_bytes(b"a");
        let b = Object::from_bytes(b"b");
        let c = Object::from_bytes(b"c");
        let d = Object::from_bytes(b"d");
        let mut graph = BTreeMap::from([
            (a.clone(), vec![b.clone(), c.clone()]),
            (b, vec![d.clone()]),
            (c, vec![d.clone()]),
            (d.clone(), vec![]),
        ]);
        assert_eq!(acyclic(&graph), Ok(()));
        graph.insert(d.clone(), vec![a]);
        assert_eq!(acyclic(&graph), Err(Error::Cycle));
        graph.insert(d.clone(), vec![d]);
        assert_eq!(acyclic(&graph), Err(Error::Cycle));
    }
}
