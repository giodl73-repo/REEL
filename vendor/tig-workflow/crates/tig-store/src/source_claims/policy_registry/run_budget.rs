//! Cumulative admission for one exact registry grant and accepted source frame.
//! Catalogue selections and operation nonces cannot reset this scope. Existing
//! journal rows select compact headers; expanding sets stay in asset-side trees.
use super::*;
use crate::retirement_intent::{
    execution_profile::Context,
    policy::{GrantRef, Proof},
};
use tig_core::{
    LogicalPath,
    retirement::review::{Review, decision::Decision, routine_policy::Frame},
    tree::{Tree, TreeError, TreeLimits, TreeResult},
};

const SCHEMA: &str = "tig.native-policy-run-budget.experimental-1";
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    schema: String,
    grant: GrantRef,
    frame: Frame,
    objects: Object,
    reservations: Object,
    object_count: u32,
    object_bytes: u64,
    actions: u32,
}
#[derive(Serialize)]
pub(super) struct Reservation {
    schema: &'static str,
    context: Object,
    review: Object,
    decision: Object,
}
pub(super) fn source_key(slot: &Key) -> super::super::Result<Key> {
    Key::parse(&format!(
        "policy-budget-{}",
        Digest::blob(&encode(&(SCHEMA, slot))?).as_str()
    ))
    .map_err(|_| Error::InvalidReference)
}
fn budget<T>(r: TreeResult<T>) -> RegistryResult<T> {
    r.map_err(|_| RegistryError::Metadata)
}
fn path(d: &Digest) -> RegistryResult<LogicalPath> {
    LogicalPath::parse(d.as_str()).map_err(|_| RegistryError::Encoding)
}
fn limits() -> TreeLimits {
    TreeLimits {
        max_entries: 256,
        max_reads: 1024,
        max_writes: 1024,
        max_read_bytes: 1_048_576,
        max_write_bytes: 1_048_576,
        max_map_depth: 64,
    }
}
fn serialize(v: &impl Serialize) -> RegistryResult<Vec<u8>> {
    encode(v).map_err(RegistryError::Journal)
}
fn save(nodes: &mut impl NodeStore, v: &impl Serialize) -> RegistryResult<Object> {
    budget(nodes.put(&serialize(v)?))
}
fn header(nodes: &impl NodeStore, root: &Object) -> RegistryResult<Header> {
    let bytes = storage::read(nodes, root, 4096)?;
    let h: Header =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| RegistryError::Encoding)?)
            .map_err(|_| RegistryError::Encoding)?;
    if h.schema != SCHEMA || serialize(&h)? != bytes || h.object_count > 256 || h.actions > 256 {
        return Err(RegistryError::Encoding);
    }
    Ok(h)
}
fn reservation(
    context: &Context,
    review: &Review,
    decision: &Decision,
) -> RegistryResult<(Reservation, [Vec<u8>; 3])> {
    reservation_bytes([
        context.to_json().map_err(|_| RegistryError::Encoding)?,
        review.to_json().map_err(|_| RegistryError::Encoding)?,
        decision.to_json().map_err(|_| RegistryError::Encoding)?,
    ])
}
pub(super) fn reservation_bytes(
    bytes: [Vec<u8>; 3],
) -> RegistryResult<(Reservation, [Vec<u8>; 3])> {
    if bytes.iter().any(|b| b.is_empty() || b.len() > 65536) {
        return Err(RegistryError::Encoding);
    }
    Ok((
        Reservation {
            schema: SCHEMA,
            context: Object::from_bytes(&bytes[0]),
            review: Object::from_bytes(&bytes[1]),
            decision: Object::from_bytes(&bytes[2]),
        },
        bytes,
    ))
}
// A single operation shares this budget across all tree point reads and edits.
struct Work<'a, S> {
    inner: &'a mut S,
    reads: std::cell::Cell<usize>,
    bytes: std::cell::Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl<S> Work<'_, S> {
    fn new(inner: &mut S) -> Work<'_, S> {
        Work {
            inner,
            reads: 0.into(),
            bytes: 0.into(),
            writes: 0,
            write_bytes: 0,
        }
    }
}
impl<S: NodeStore> NodeStore for Work<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        if self.reads.get() >= 16384
            || o.bytes > 8_388_608u64.saturating_sub(self.bytes.get() as u64)
        {
            return Err(TreeError::Budget);
        }
        self.reads.set(self.reads.get() + 1);
        self.bytes.set(self.bytes.get() + o.bytes as usize);
        self.inner.get(o)
    }
    fn put(&mut self, b: &[u8]) -> TreeResult<Object> {
        if self.writes >= 16384 || b.len() > 8_388_608usize.saturating_sub(self.write_bytes) {
            return Err(TreeError::Budget);
        }
        self.writes += 1;
        self.write_bytes += b.len();
        self.inner.put(b)
    }
}
struct Reader<'a, S>(&'a S);
impl<S: NodeStore> NodeStore for Reader<'_, S> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.0.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Budget)
    }
}
impl Registry {
    pub(super) fn budget_scope(
        &self,
        held: &HeldPolicy,
        grant: &GrantRef,
        policy: &Policy,
    ) -> RegistryResult<Key> {
        if !self.binding.accounted
            || held.registry() != self.identity()
            || &grant.registry != self.identity()
            || held.version() != &grant.version
            || held.slot() != &grant.slot
            || held.policy() != policy
        {
            return Err(RegistryError::Scope);
        }
        self.binding.source_key(held.slot())?;
        source_key(held.slot()).map_err(RegistryError::Journal)
    }
    fn settle_budget(&mut self, key: &Key) -> RegistryResult<Option<Source>> {
        let mut state = self.journal.source(key).map_err(RegistryError::Journal)?;
        if let Some(s) = &state {
            if s.cancelled || s.owner != self.binding.owner {
                return Err(RegistryError::Scope);
            }
            if let Some(p) = &s.pending {
                let op = Key::parse(&format!(
                    "policy-budget-adopt-{}",
                    Digest::blob(&serialize(&p.reference)?).as_str()
                ))
                .map_err(|_| RegistryError::Encoding)?;
                self.issue(
                    op,
                    Command::Adopt {
                        source: key.clone(),
                        owner: self.binding.owner.clone(),
                        expected_revision: s.revision,
                        prepared: p.reference.clone(),
                    },
                )?;
                state = self.journal.source(key).map_err(RegistryError::Journal)?;
            }
        }
        Ok(state)
    }
    /// Reserve the entire exact decision once. Repeated operations are charged
    /// separately; retries of that same decision are free. Original progress
    /// permits at most one completed effect per decision/object/action target.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reserve_run(
        &mut self,
        held: &HeldPolicy,
        head: &coordinated::accepted_head::HeldHead,
        proof: &Proof,
        context: &Context,
        review: &Review,
        decision: &Decision,
        nodes: &mut MetadataStore,
    ) -> RegistryResult<()> {
        self.reserve_run_hook(held, head, proof, context, review, decision, nodes, |_| {
            Ok(())
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reserve_run_hook(
        &mut self,
        held: &HeldPolicy,
        head: &coordinated::accepted_head::HeldHead,
        proof: &Proof,
        context: &Context,
        review: &Review,
        decision: &Decision,
        nodes: &mut MetadataStore,
        hook: impl FnMut(UpdateStage) -> RegistryResult<()>,
    ) -> RegistryResult<()> {
        if self.binding.effect_index.is_some() {
            return Err(RegistryError::Scope);
        }
        let restored =
            Proof::load(nodes, context, review, decision).map_err(|_| RegistryError::Encoding)?;
        if restored.grant() != proof.grant() || restored.run() != proof.run() {
            return Err(RegistryError::Scope);
        }
        head.require_run(proof.run())
            .map_err(RegistryError::Journal)?;
        self.reserve_budget(
            held,
            proof.grant(),
            &proof.run().config().frame,
            reservation(context, review, decision)?,
            decision,
            nodes,
            hook,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn reserve_budget(
        &mut self,
        held: &HeldPolicy,
        grant: &GrantRef,
        frame: &Frame,
        artifacts: (Reservation, [Vec<u8>; 3]),
        decision: &Decision,
        nodes: &mut MetadataStore,
        mut hook: impl FnMut(UpdateStage) -> RegistryResult<()>,
    ) -> RegistryResult<()> {
        let key = self.budget_scope(held, grant, held.policy())?;
        let _writer = SlotGuard::acquire(&self.root, &key, false)?;
        // Previously acknowledged pending metadata is reconciled before admitting
        // a new scope. This grants no permission for its old payload effects.
        let state = self.settle_budget(&key)?;
        let mut w = Work::new(nodes);
        let existing = state
            .as_ref()
            .and_then(|s| s.current.as_ref())
            .map(|v| header(&w, &v.selection.root))
            .transpose()?;
        let mut h = if let Some(h) = existing.filter(|h| h.grant == *grant && h.frame == *frame) {
            h
        } else {
            let empty = budget(Tree::empty(&mut w))?.root().clone();
            Header {
                schema: SCHEMA.into(),
                grant: grant.clone(),
                frame: frame.clone(),
                objects: empty.clone(),
                reservations: empty,
                object_count: 0,
                object_bytes: 0,
                actions: 0,
            }
        };
        let (record, bytes) = artifacts;
        let expected = Object::from_bytes(&serialize(&record)?);
        let id = decision.digest().map_err(|_| RegistryError::Encoding)?;
        let reservations = Tree::from_root(h.reservations.clone());
        if let Some(old) = budget(reservations.get(&path(&id)?, &mut w, limits()))?.0 {
            if old != expected || storage::read(&w, &old, 1024)? != serialize(&record)? {
                return Err(RegistryError::Encoding);
            }
            return Ok(());
        }
        let consent = decision.consent();
        let actions = consent
            .remove_copies
            .len()
            .checked_add(consent.release_owned_claims.len())
            .ok_or(RegistryError::Scope)?;
        h.actions = h
            .actions
            .checked_add(u32::try_from(actions).map_err(|_| RegistryError::Scope)?)
            .ok_or(RegistryError::Scope)?;
        let mut changes = BTreeMap::new();
        let objects = Tree::from_root(h.objects.clone());
        let unique: BTreeSet<_> = consent
            .remove_copies
            .iter()
            .map(|t| t.object.clone())
            .collect();
        for object in unique {
            let path = path(&object.sha256)?;
            if let Some(old) = budget(objects.get(&path, &mut w, limits()))?.0 {
                if old != object {
                    return Err(RegistryError::Encoding);
                }
            } else {
                h.object_count = h.object_count.checked_add(1).ok_or(RegistryError::Scope)?;
                h.object_bytes = h
                    .object_bytes
                    .checked_add(object.bytes)
                    .ok_or(RegistryError::Scope)?;
                changes.insert(path, Some(object));
            }
        }
        let l = &held.policy().config().limits;
        if h.object_count > l.maximum_objects
            || h.object_bytes > l.maximum_total_object_bytes
            || h.actions > l.maximum_actions
        {
            return Err(RegistryError::Journal(Error::Budget));
        }
        for bytes in bytes {
            budget(w.put(&bytes))?;
        }
        let reservation = save(&mut w, &record)?;
        h.objects = budget(objects.edit(objects.root(), &changes, &mut w, limits()))?
            .0
            .root()
            .clone();
        h.reservations = budget(reservations.edit(
            reservations.root(),
            &[(path(&id)?, Some(reservation))].into(),
            &mut w,
            limits(),
        ))?
        .0
        .root()
        .clone();
        let root = save(&mut w, &h)?;
        nodes.finish().map_err(|_| RegistryError::Metadata)?;
        hook(UpdateStage::MetadataReady)?;
        let revision = state.as_ref().map_or(0, |s| s.revision);
        let op = Key::parse(&format!(
            "policy-budget-prepare-{}",
            Digest::blob(&serialize(&(self.identity(), &key, revision, &root))?).as_str()
        ))
        .map_err(|_| RegistryError::Encoding)?;
        self.issue(
            op,
            Command::Prepare {
                source: key.clone(),
                owner: self.binding.owner.clone(),
                expected_revision: revision,
                selection: Selection {
                    context: Digest::blob(&serialize(&(SCHEMA, &root))?),
                    root,
                },
            },
        )?;
        hook(UpdateStage::Prepared)?;
        self.settle_budget(&key)?;
        hook(UpdateStage::Adopted)?;
        Ok(())
    }
    /// Point lookup, no scan or new reservation. The caller holds the exact live
    /// grant and source frame, preventing a concurrent scope reset until effect.
    pub(crate) fn check_run(
        &mut self,
        held: &HeldPolicy,
        proof: &Proof,
        context: &Context,
        review: &Review,
        decision: &Decision,
        nodes: &impl NodeStore,
    ) -> RegistryResult<()> {
        if self.binding.effect_index.is_some() {
            return Err(RegistryError::Scope);
        }
        self.check_budget(
            held,
            proof.grant(),
            &proof.run().config().frame,
            reservation(context, review, decision)?.0,
            decision,
            nodes,
        )
        .map(drop)
    }
    pub(super) fn check_budget(
        &mut self,
        held: &HeldPolicy,
        grant: &GrantRef,
        frame: &Frame,
        expected: Reservation,
        decision: &Decision,
        nodes: &impl NodeStore,
    ) -> RegistryResult<SlotGuard> {
        let key = self.budget_scope(held, grant, held.policy())?;
        let reader_hold = SlotGuard::acquire(&self.root, &key, true)?;
        let state = self
            .journal
            .source(&key)
            .map_err(RegistryError::Journal)?
            .ok_or(RegistryError::Missing)?;
        if state.pending.is_some() || state.cancelled {
            return Err(RegistryError::Pending);
        }
        let h = header(
            nodes,
            &state.current.ok_or(RegistryError::Missing)?.selection.root,
        )?;
        if h.grant != *grant || h.frame != *frame {
            return Err(RegistryError::Stale);
        }
        let mut reader = Reader(nodes);
        let root = budget(Tree::from_root(h.reservations).get(
            &path(&decision.digest().map_err(|_| RegistryError::Encoding)?)?,
            &mut reader,
            limits(),
        ))?
        .0
        .ok_or(RegistryError::Missing)?;
        if storage::read(nodes, &root, 1024)? != serialize(&expected)? {
            return Err(RegistryError::Encoding);
        }
        Ok(reader_hold)
    }
}
