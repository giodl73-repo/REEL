//! Bounded consumer slot lifecycle through original protection work, never payload deletion.
use super::prepublication::{Scope, actor_fence};
use super::*;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tig_core::tree::MemoryNodes;
use tig_store::source_claims::{
    projection::{Limits, Resources, envelope},
    protection::{
        Actor, ActorState, Index, MAX_CHANGES, Update,
        work::{self, WorkRef},
    },
};
const ACTIVATION: &str = "reel.job-actor-activation.experimental-1";
#[cfg(test)]
mod tests;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    schema: String,
    index: Digest,
    actor: Key,
    owner: Key,
    operation: Key,
    input: Selection,
    policy: Digest,
    store: Digest,
    source_binding: Digest,
    candidate: Object,
}
fn canonical(value: &impl Serialize) -> Result<Vec<u8>> {
    Ok(identity::canonical_json(&serde_json::to_value(value)?)?)
}
fn actor(index: &Index, key: &Key) -> Result<Actor> {
    index
        .enrollment_profile()
        .actors
        .iter()
        .find(|a| &a.id == key)
        .cloned()
        .context("slot actor is not enrolled")
}
fn scope(index: &Index, s: &Scope) -> Result<Actor> {
    let a = actor(index, &s.actor)?;
    if index.routing().is_none_or(|r| r.actor == s.actor)
        || a.owner != s.owner
        || !a.stores.contains(&s.store)
        || !index.enrollment_profile().policies.contains(&s.policy)
    {
        bail!("slot scope differs from admitted enrollment");
    }
    Ok(a)
}
/// Typed empty slot; empty trees with other contexts are not implicitly free.
pub fn idle(index: &Index, key: &Key) -> Result<Selection> {
    let a = actor(index, key)?;
    let mut nodes = MemoryNodes::default();
    Ok(Selection {
        root: Tree::empty(&mut nodes)?.root().clone(),
        context: Digest::blob(&canonical(&serde_json::json!([
            "reel.job-actor-idle.experimental-1",
            index.identity(),
            a.id,
            a.owner,
            a.stores
        ]))?),
    })
}
pub fn activation(
    index: &Index,
    s: &Scope,
    input: &Selection,
    source_binding: &Digest,
    candidate: &Object,
) -> Result<Vec<u8>> {
    scope(index, s)?;
    canonical(&Activation {
        schema: ACTIVATION.into(),
        index: index.identity().clone(),
        actor: s.actor.clone(),
        owner: s.owner.clone(),
        operation: s.operation.clone(),
        input: input.clone(),
        policy: s.policy.clone(),
        store: s.store.clone(),
        source_binding: source_binding.clone(),
        candidate: candidate.clone(),
    })
}
pub fn verify_activation(
    index: &Index,
    s: &Scope,
    input: &Selection,
    source_binding: &Digest,
    object: &Object,
    nodes: &impl NodeStore,
) -> Result<()> {
    if object.bytes > 65536 {
        bail!("slot activation exceeds bound");
    }
    let raw = nodes.get(object)?.context("slot activation missing")?;
    let value = identity::parse_json(&raw)?;
    let a: Activation = serde_json::from_value(value)?;
    if Object::from_bytes(&raw) != *object
        || canonical(&a)? != raw
        || a.schema != ACTIVATION
        || a.index != *index.identity()
        || a.actor != s.actor
        || a.owner != s.owner
        || a.operation != s.operation
        || a.input != *input
        || a.policy != s.policy
        || a.store != s.store
        || a.source_binding != *source_binding
    {
        bail!("slot activation admission differs");
    }
    Ok(())
}
fn operation(domain: &str, index: &Index, s: &Scope, op: &Key) -> Result<Key> {
    Ok(Key::parse(&format!(
        "{domain}-{}",
        Digest::blob(&canonical(&(index.identity(), &s.actor, op))?).as_str()
    ))?)
}
fn entries(
    selection: &Selection,
    nodes: &mut impl NodeStore,
    limits: Limits,
) -> Result<BTreeMap<LogicalPath, Object>> {
    Ok(Tree::from_root(selection.root.clone())
        .entries(nodes, limits.tree)?
        .0)
}
fn delta(
    a: &BTreeMap<LogicalPath, Object>,
    b: &BTreeMap<LogicalPath, Object>,
) -> BTreeMap<LogicalPath, Option<Object>> {
    a.keys()
        .chain(b.keys())
        .filter(|k| a.get(*k) != b.get(*k))
        .map(|k| (k.clone(), b.get(k).cloned()))
        .collect()
}
fn active(
    index: &mut Index,
    nodes: &mut MetadataStore,
    s: &Scope,
    input: &Selection,
    marker: &Object,
    limits: Limits,
) -> Result<()> {
    let current = index.actor(nodes, &s.actor)?;
    let binding = envelope::read_binding(&current.selection, nodes, limits)?;
    if !current.ready
        || binding.native != *input
        || binding.policy != s.policy
        || !entries(&current.selection, nodes, limits)?
            .values()
            .any(|o| o == marker)
    {
        bail!("slot activation is no longer current");
    }
    Ok(())
}
#[derive(Serialize)]
pub struct Accepted {
    pub applied: bool,
    pub activation: Object,
    pub selection: Selection,
    pub work: Option<WorkRef>,
}
/// Candidate closure must be independently admitted by the caller. No actor is
/// added to enrollment; only the exact typed idle slot can acquire a new job.
pub fn accept(
    index: &mut Index,
    nodes: &mut MetadataStore,
    s: &Scope,
    prepared: &envelope::Envelope,
    marker: &[u8],
    limits: Limits,
    apply: bool,
) -> Result<Accepted> {
    scope(index, s)?;
    let _lease = actor_fence(index.identity(), &s.coordination_root, &s.actor)?;
    let value: Activation = serde_json::from_value(identity::parse_json(marker)?)?;
    let evidence = Object::from_bytes(marker);
    let expected = activation(
        index,
        s,
        &prepared.binding().native,
        &value.source_binding,
        &value.candidate,
    )?;
    if marker != expected || prepared.binding().policy != s.policy {
        bail!("slot candidate/activation differs");
    }
    let op = operation("slot-accept", index, s, &s.operation)?;
    if let Some(reference) = work::lookup(index, &op)? {
        let record = work::read(index, &reference, nodes)?;
        if record.evidence.as_ref() != Some(&evidence)
            || record.additional.is_some()
            || record.adoption.is_some()
            || record.update.actor != s.actor
            || record.update.owner != s.owner
            || !record.update.ready
        {
            bail!("slot acceptance retry conflicts");
        }
        if apply {
            work::execute(index, &reference, nodes)?;
            active(
                index,
                nodes,
                s,
                &prepared.binding().native,
                &evidence,
                limits,
            )?;
        }
        return Ok(Accepted {
            applied: apply,
            activation: evidence,
            selection: record.update.selection,
            work: Some(reference),
        });
    }
    let current = index.actor(nodes, &s.actor)?;
    if !current.ready || current.selection != idle(index, &s.actor)? {
        bail!("slot is not explicitly idle");
    }
    let mut memory = MemoryNodes::default();
    memory.put(marker)?;
    let mut objects = prepared.resources().objects().clone();
    objects.insert(evidence.clone());
    let target = envelope::create(
        &Resources::admitted(&prepared.binding().native, objects),
        &s.policy,
        &mut memory,
        limits,
    )?;
    let changes = entries(target.selection(), &mut memory, limits)?
        .into_iter()
        .map(|(k, o)| (k, Some(o)))
        .collect::<BTreeMap<_, _>>();
    if changes.len() > MAX_CHANGES {
        bail!("slot acceptance exceeds change bound");
    }
    if !apply {
        return Ok(Accepted {
            applied: false,
            activation: evidence,
            selection: target.selection().clone(),
            work: None,
        });
    }
    for (_, raw) in memory.objects() {
        if nodes.put(raw)? != Object::from_bytes(raw) {
            bail!("slot metadata substituted");
        }
    }
    let update = Update {
        operation: op,
        actor: s.actor.clone(),
        owner: s.owner.clone(),
        expected_revision: current.revision,
        selection: target.selection().clone(),
        changes,
        ready: true,
    };
    let reference = work::stage(index, &update, Some(evidence.clone()), nodes)?;
    work::execute(index, &reference, nodes)?;
    active(
        index,
        nodes,
        s,
        &prepared.binding().native,
        &evidence,
        limits,
    )?;
    Ok(Accepted {
        applied: true,
        activation: evidence,
        selection: update.selection,
        work: Some(reference),
    })
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handoff {
    schema: String,
    index: Digest,
    slot: Key,
    slot_owner: Key,
    activation: Object,
    operation: Key,
    retained: Key,
    retained_owner: Key,
    source: ActorState,
}
pub struct Close<'a> {
    pub activation: &'a Object,
    pub operation: &'a Key,
    pub retained: &'a Key,
    pub retained_owner: &'a Key,
    pub index_root: &'a Path,
    pub metadata_root: &'a Path,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    HandoffStaged,
    HandoffAdopted,
    ClearStaged,
    Cleared,
}
#[derive(Serialize)]
pub struct Closed {
    pub applied: bool,
    pub slot_idle: bool,
    pub handoff: Option<WorkRef>,
    pub clear: Option<WorkRef>,
}
fn flat(
    objects: impl IntoIterator<Item = Object>,
    nodes: &mut impl NodeStore,
    limits: Limits,
    context: Digest,
) -> Result<Selection> {
    let mut map = BTreeMap::new();
    let mut bytes = 0u64;
    for object in objects {
        let name = LogicalPath::parse(&format!("objects/{}", object.sha256.as_str()))?;
        if let Some(previous) = map.insert(name, object.clone()) {
            if previous != object {
                bail!("handoff size conflict");
            }
            continue;
        }
        bytes = bytes
            .checked_add(object.bytes)
            .context("handoff byte overflow")?;
        if map.len() > limits.objects || bytes > limits.logical_bytes {
            bail!("handoff budget exceeded");
        }
    }
    Ok(Selection {
        root: Tree::build(&map, nodes, limits.tree)?.0.root().clone(),
        context,
    })
}
/// All stores and references survive positive handoff BEFORE clearing the slot.
/// Hook failures model restart boundaries; original work records own recovery.
pub fn close(
    index: &mut Index,
    nodes: &mut MetadataStore,
    s: &Scope,
    c: &Close<'_>,
    limits: Limits,
    apply: bool,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Closed> {
    let a = scope(index, s)?;
    let retained = actor(index, c.retained)?;
    if retained.id == s.actor
        || index.routing().is_some_and(|r| r.actor == retained.id)
        || retained.owner != *c.retained_owner
        || a.stores
            .iter()
            .any(|store| !retained.stores.contains(store))
    {
        bail!("retained actor cannot preserve slot scope");
    }
    let mut keys = [&s.actor, c.retained];
    keys.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    let _first = actor_fence(index.identity(), &s.coordination_root, keys[0])?;
    let _second = actor_fence(index.identity(), &s.coordination_root, keys[1])?;
    let op = operation("slot-handoff", index, s, c.operation)?;
    let clear_op = operation("slot-clear", index, s, c.operation)?;
    let reference = work::lookup(index, &op)?;
    let (plan, record) = if let Some(reference) = &reference {
        let record = work::read(index, reference, nodes)?;
        let evidence = record
            .evidence
            .as_ref()
            .context("handoff evidence missing")?;
        if evidence.bytes > 65536 {
            bail!("handoff evidence bound");
        }
        let raw = nodes.get(evidence)?.context("handoff plan missing")?;
        let plan: Handoff = serde_json::from_value(identity::parse_json(&raw)?)?;
        if canonical(&plan)? != raw || Object::from_bytes(&raw) != *evidence {
            bail!("handoff evidence differs");
        }
        (plan, Some(record))
    } else {
        let current = index.actor(nodes, &s.actor)?;
        if !current.ready
            || !entries(&current.selection, nodes, limits)?
                .values()
                .any(|o| o == c.activation)
        {
            bail!("slot generation not current");
        }
        (
            Handoff {
                schema: "reel.job-actor-handoff.experimental-1".into(),
                index: index.identity().clone(),
                slot: s.actor.clone(),
                slot_owner: s.owner.clone(),
                activation: c.activation.clone(),
                operation: c.operation.clone(),
                retained: c.retained.clone(),
                retained_owner: c.retained_owner.clone(),
                source: current,
            },
            None,
        )
    };
    if plan.schema != "reel.job-actor-handoff.experimental-1"
        || plan.index != *index.identity()
        || plan.slot != s.actor
        || plan.slot_owner != s.owner
        || plan.activation != *c.activation
        || plan.operation != *c.operation
        || plan.retained != *c.retained
        || plan.retained_owner != *c.retained_owner
    {
        bail!("handoff retry conflicts");
    }
    let source = entries(&plan.source.selection, nodes, limits)?;
    let required: BTreeSet<_> = source
        .values()
        .cloned()
        .chain([plan.source.selection.root.clone()])
        .collect();
    let idle = idle(index, &s.actor)?;
    let clear_changes = source
        .keys()
        .cloned()
        .map(|k| (k, None))
        .collect::<BTreeMap<_, _>>();
    if clear_changes.len() > MAX_CHANGES {
        bail!("slot clear exceeds change bound");
    }
    let current_retained = index.actor(nodes, c.retained)?;
    if !current_retained.ready {
        bail!("retained actor unready");
    }
    let (target, changes) = if let Some(record) = &record {
        if record.additional.is_some()
            || record.adoption.is_some()
            || record.update.actor != *c.retained
            || record.update.owner != *c.retained_owner
            || !record.update.ready
        {
            bail!("handoff record differs");
        }
        (
            record.update.selection.clone(),
            record.update.changes.clone(),
        )
    } else {
        let prior = entries(&current_retained.selection, nodes, limits)?;
        let mut memory = MemoryNodes::default();
        let target = flat(
            prior
                .values()
                .cloned()
                .chain([current_retained.selection.root.clone()])
                .chain(required.iter().cloned()),
            &mut memory,
            limits,
            Digest::blob(&canonical(&plan)?),
        )?;
        let changes = delta(&prior, &entries(&target, &mut memory, limits)?);
        if changes.len() > MAX_CHANGES {
            bail!("handoff exceeds change bound");
        }
        if apply {
            for (_, raw) in memory.objects() {
                if nodes.put(raw)? != Object::from_bytes(raw) {
                    bail!("handoff metadata substituted");
                }
            }
        }
        (target, changes)
    };
    if !apply {
        return Ok(Closed {
            applied: false,
            slot_idle: index.actor(nodes, &s.actor)?.selection == idle,
            handoff: reference,
            clear: work::lookup(index, &clear_op)?,
        });
    }
    let evidence = nodes.put(&canonical(&plan)?)?;
    let reference = if let Some(reference) = reference {
        reference
    } else {
        work::stage(
            index,
            &Update {
                operation: op,
                actor: c.retained.clone(),
                owner: c.retained_owner.clone(),
                expected_revision: current_retained.revision,
                selection: target,
                changes,
                ready: true,
            },
            Some(evidence.clone()),
            nodes,
        )?
    };
    hook(Stage::HandoffStaged)?;
    work::execute(index, &reference, nodes)?;
    hook(Stage::HandoffAdopted)?;
    Tree::empty(nodes)?;
    let clear = work::stage(
        index,
        &Update {
            operation: clear_op,
            actor: s.actor.clone(),
            owner: s.owner.clone(),
            expected_revision: plan.source.revision,
            selection: idle.clone(),
            changes: clear_changes,
            ready: true,
        },
        Some(evidence),
        nodes,
    )?;
    hook(Stage::ClearStaged)?;
    let index_minimum = index.checkpoint();
    let metadata_minimum = nodes.finish()?;
    let expected_index = index.identity().clone();
    let validate = |_: &work::Record| -> tig_store::source_claims::protection::Result<()> {
        let check = || -> Result<()> {
            let mut reader = MetadataReader::open(c.metadata_root, &metadata_minimum)?;
            let observed = Index::observe(c.index_root, &index_minimum, &reader)?;
            if observed.index != expected_index || observed.cancelled {
                bail!("handoff authority changed");
            }
            let slot = observed
                .actors
                .get(s.actor.as_str())
                .context("slot missing")?;
            if slot.selection != plan.source.selection
                || slot.revision != plan.source.revision
                || !slot.ready
            {
                bail!("slot changed before clear");
            }
            let held = observed
                .actors
                .get(c.retained.as_str())
                .context("retained actor missing")?;
            let present: BTreeSet<_> = entries(&held.selection, &mut reader, limits)?
                .into_values()
                .collect();
            if !held.ready || !required.is_subset(&present) {
                bail!("handoff protection incomplete");
            }
            Ok(())
        };
        check().map_err(|_| tig_store::source_claims::protection::Error::Stale)
    };
    work::execute_validated(index, &clear, nodes, validate)?;
    hook(Stage::Cleared)?;
    Ok(Closed {
        applied: true,
        slot_idle: index.actor(nodes, &s.actor)?.selection == idle,
        handoff: Some(reference),
        clear: Some(clear),
    })
}
