//! Exact object release from a predecessor, preserving every other binding.
pub mod block;
use super::*;
use std::cell::Cell;
use tig_core::tree::{Tree, TreeLimits, TreeResult};
pub(super) const SCHEMA: &str = "tig.source-history-work.design-1";
const MAX_READS: usize = 131_072;
const MAX_READ_BYTES: usize = 64 * 1024 * 1024;
const MAX_WRITES: usize = 65_536;
const MAX_WRITE_BYTES: usize = 32 * 1024 * 1024;

/// One budget shared across traversals/edits, including failed operations.
struct BudgetNodes<'a, N> {
    nodes: &'a mut N,
    reads: Cell<usize>,
    read_bytes: Cell<usize>,
    writes: usize,
    write_bytes: usize,
}
impl<'a, N> BudgetNodes<'a, N> {
    fn new(nodes: &'a mut N) -> Self {
        Self {
            nodes,
            reads: Cell::new(0),
            read_bytes: Cell::new(0),
            writes: 0,
            write_bytes: 0,
        }
    }
}
impl<N: NodeStore> NodeStore for BudgetNodes<'_, N> {
    fn get(&self, object: &Object) -> TreeResult<Option<Vec<u8>>> {
        let bytes = usize::try_from(object.bytes).map_err(|_| TreeError::Budget)?;
        let reads = self.reads.get().checked_add(1).ok_or(TreeError::Budget)?;
        let read_bytes = self
            .read_bytes
            .get()
            .checked_add(bytes)
            .ok_or(TreeError::Budget)?;
        if reads > MAX_READS || read_bytes > MAX_READ_BYTES {
            return Err(TreeError::Budget);
        }
        self.reads.set(reads);
        self.read_bytes.set(read_bytes);
        self.nodes.get(object)
    }
    fn put(&mut self, bytes: &[u8]) -> TreeResult<Object> {
        self.writes = self.writes.checked_add(1).ok_or(TreeError::Budget)?;
        self.write_bytes = self
            .write_bytes
            .checked_add(bytes.len())
            .ok_or(TreeError::Budget)?;
        if self.writes > MAX_WRITES || self.write_bytes > MAX_WRITE_BYTES {
            return Err(TreeError::Budget);
        }
        self.nodes.put(bytes)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryWork {
    journal: Digest,
    source: Key,
    owner: Key,
    subject: Digest,
    expected_revision: u64,
    predecessor: Version,
    object: Object,
    replacement: Option<Selection>,
}
impl HistoryWork {
    pub fn replacement(&self) -> Option<&Selection> {
        self.replacement.as_ref()
    }
    pub fn predecessor(&self) -> &Version {
        &self.predecessor
    }
    pub fn source(&self) -> &Key {
        &self.source
    }
    pub fn claim_id(&self) -> Result<Digest> {
        claim_id(
            &self.journal,
            &self.source,
            &self.owner,
            &self.predecessor,
            &self.object,
        )
    }
}
/// Exact object-scoped claim identity; computing it does not prove membership.
pub fn claim_id(
    journal: &Digest,
    source: &Key,
    owner: &Key,
    version: &Version,
    object: &Object,
) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        "tig.source-predecessor-object-claim.design-1",
        journal,
        source,
        owner,
        version,
        object,
    ))?))
}
fn projection_context(w: &HistoryWork) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        "tig.filtered-history-context.design-1",
        w.claim_id()?,
    ))?))
}
/// Trusted source-owner mapping, not authentication. `state` is an observed
/// same-journal source snapshot; execution rechecks its exact owner/revision/root.
pub fn plan(
    journal: &Digest,
    source: &Key,
    state: &Source,
    predecessor_revision: u64,
    object: &Object,
    enrollment: &Enrollment,
    nodes: &mut impl NodeStore,
) -> Result<HistoryWork> {
    if state.owner != enrollment.owner {
        return Err(Error::Owner);
    }
    if state.cancelled {
        return Err(Error::Cancelled);
    }
    if state.pending.is_some() {
        return Err(Error::Pending);
    }
    let predecessor = state
        .predecessors
        .iter()
        .find(|v| v.revision == predecessor_revision)
        .ok_or(Error::Missing)?
        .clone();
    let mut w = HistoryWork {
        journal: journal.clone(),
        source: source.clone(),
        owner: enrollment.owner.clone(),
        subject: enrollment.subject.clone(),
        expected_revision: state.revision,
        predecessor,
        object: object.clone(),
        replacement: None,
    };
    let tree = Tree::from_root(w.predecessor.selection.root.clone());
    let mut budget = BudgetNodes::new(nodes);
    let bindings = tree
        .entries(&mut budget, TreeLimits::default())
        .map_err(Error::Metadata)?
        .0;
    let changes = bindings
        .iter()
        .filter(|(_, o)| *o == object)
        .map(|(p, _)| (p.clone(), None))
        .collect::<BTreeMap<_, _>>();
    if changes.is_empty() {
        return Err(Error::Invalid);
    }
    if changes.len() != bindings.len() {
        let filtered = tree
            .edit(tree.root(), &changes, &mut budget, TreeLimits::default())
            .map_err(Error::Metadata)?
            .0;
        w.replacement = Some(Selection {
            root: filtered.root().clone(),
            context: projection_context(&w)?,
        });
    }
    Ok(w)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    attempt: u64,
    work: HistoryWork,
}
fn validate(ctx: &ObservedIntent, frame: &Frame, r: &Record) -> Result<()> {
    if r.schema != SCHEMA
        || r.attempt != frame.attempt
        || r.work.journal != ctx.journal
        || r.work.subject != ctx.decision().approval().owner
        || r.work.object != frame.object
        || r.work.source == ctx.source
        || r.work.source == step(ctx, &frame.object)?
        || r.work.expected_revision <= r.work.predecessor.revision
        || frame.action
            != (Action::Release {
                claim: r.work.claim_id()?,
            })
        || r.work
            .replacement
            .as_ref()
            .is_some_and(|s| projection_context(&r.work).as_ref() != Ok(&s.context))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn decode(bytes: &[u8]) -> Result<Record> {
    let r: Record =
        serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if r.schema != SCHEMA || encode(&r)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(r)
}
pub(super) fn verify_bytes(ctx: &ObservedIntent, frame: &Frame, bytes: &[u8]) -> Result<()> {
    validate(ctx, frame, &decode(bytes)?)
}
/// Factory may construct immutable projection metadata, never apply a source command.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    claim: &Digest,
    prepare: impl FnOnce(&mut MetadataStore, u64) -> Result<HistoryWork>,
) -> Result<Admission> {
    start_hook(j, nodes, ctx, object, claim, prepare, |_| Ok(()))
}
#[allow(clippy::too_many_arguments)]
fn start_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    object: &Object,
    claim: &Digest,
    prepare: impl FnOnce(&mut MetadataStore, u64) -> Result<HistoryWork>,
    before_adopt: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Admission> {
    start_bound(
        j,
        nodes,
        ctx,
        object,
        Action::Release {
            claim: claim.clone(),
        },
        SCHEMA,
        |nodes, frame| {
            let r = Record {
                schema: SCHEMA.into(),
                attempt: frame.attempt,
                work: prepare(nodes, frame.attempt)?,
            };
            validate(ctx, frame, &r)?;
            let bytes = encode(&r)?;
            if bytes.len() > 65_536 {
                return Err(Error::Budget);
            }
            put(nodes, &bytes)
        },
        before_adopt,
    )
}
pub fn current(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
) -> Result<Option<HistoryWork>> {
    let loaded = load(j, nodes, ctx, object)?;
    if loaded.pending.is_some() {
        return Err(Error::Pending);
    }
    let Some((_, f)) = loaded.frames.last() else {
        return Ok(None);
    };
    if f.outcome.is_some() || !matches!(f.action, Action::Release { .. }) {
        return Ok(None);
    }
    typed_execution(nodes, f, SCHEMA)?
        .map(|b| Ok(decode(&b)?.work))
        .transpose()
}
struct ReadOnly<'a, N>(&'a N);
impl<N: NodeStore> NodeStore for ReadOnly<'_, N> {
    fn get(&self, o: &Object) -> TreeResult<Option<Vec<u8>>> {
        self.0.get(o)
    }
    fn put(&mut self, _: &[u8]) -> TreeResult<Object> {
        Err(TreeError::Store("read-only history check".into()))
    }
}
fn projection(nodes: &impl NodeStore, w: &HistoryWork) -> Result<()> {
    let mut read_only = ReadOnly(nodes);
    let mut budget = BudgetNodes::new(&mut read_only);
    let mut expected = Tree::from_root(w.predecessor.selection.root.clone())
        .entries(&mut budget, TreeLimits::default())
        .map_err(Error::Metadata)?
        .0;
    let old_len = expected.len();
    expected.retain(|_, o| *o != w.object);
    if expected.len() == old_len {
        return Err(Error::Invalid);
    }
    if let Some(s) = &w.replacement {
        if expected.is_empty()
            || Tree::from_root(s.root.clone())
                .entries(&mut budget, TreeLimits::default())
                .map_err(Error::Metadata)?
                .0
                != expected
        {
            return Err(Error::Integrity);
        }
    } else if !expected.is_empty() {
        return Err(Error::Integrity);
    }
    Ok(())
}
/// Atomically retire only the approved object's predecessor obligation. All
/// other bindings remain predecessor obligations under a distinct projection.
pub fn apply(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
) -> Result<Digest> {
    apply_hook(j, nodes, ctx, object, attempt, |_| Ok(()))
}
fn apply_hook(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    object: &Object,
    attempt: u64,
    before_apply: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Digest> {
    let loaded = load(j, nodes, ctx, object)?;
    if loaded.pending.is_some() {
        return Err(Error::Pending);
    }
    let (_, frame) = loaded.frames.last().ok_or(Error::Missing)?;
    if frame.attempt != attempt || frame.outcome.is_some() {
        return Err(Error::Stale);
    }
    let bytes = typed_execution(nodes, frame, SCHEMA)?.ok_or(Error::Occupied)?;
    let w = decode(&bytes)?.work;
    let claim = w.claim_id()?;
    let command = Command::RetireGuarded {
        source: w.source.clone(),
        owner: w.owner.clone(),
        expected_revision: w.expected_revision,
        predecessor_revision: w.predecessor.revision,
        replacement: w.replacement.clone(),
        guards: vec![Guard {
            source: step(ctx, object)?,
            owner: ctx.owner().clone(),
            expected_revision: loaded.frames.len() as u64 * 2,
        }],
    };
    let op = Key::parse(&format!(
        "history-work-{}",
        Digest::blob(&encode(&(
            SCHEMA,
            &ctx.journal,
            ctx.decision().digest().map_err(Error::Decision)?,
            object,
            attempt
        ))?)
        .as_str()
    ))
    .map_err(|_| Error::Invalid)?;
    let evidence = |r: Receipt| -> Result<Digest> {
        Ok(Digest::blob(&encode(&(
            SCHEMA,
            &claim,
            r.reference,
            r.checkpoint,
            r.source_revision,
        ))?))
    };
    if let Some(r) = j
        .replay(op.clone(), command.clone())
        .map_err(Error::Journal)?
    {
        return evidence(r);
    }
    let source = j
        .source(&w.source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if source.owner != w.owner
        || source.revision != w.expected_revision
        || source.cancelled
        || source.pending.is_some()
        || !source.predecessors.contains(&w.predecessor)
    {
        return Err(Error::Stale);
    }
    projection(nodes, &w)?;
    before_apply(j)?;
    evidence(j.apply(op, command).map_err(Error::Journal)?)
}

#[cfg(test)]
#[path = "history_work_tests.rs"]
pub(crate) mod tests;
