//! One immutable block, guarded member admission, one history projection.
use super::*;
pub(in crate::retirement_intent::progress) const SCHEMA: &str = "tig.history-block-member.design-1";
const BLOCK_SCHEMA: &str = "tig.history-block.design-1";
pub const MAX_MEMBERS: usize = crate::source_claims::MAX_GUARDS - 1;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Member {
    pub object: Object,
    pub claim: Digest,
    pub attempt: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Block {
    journal: Digest,
    decision: Digest,
    source: Key,
    owner: Key,
    subject: Digest,
    expected_revision: u64,
    predecessor: Version,
    members: Vec<Member>,
    replacement: Option<Selection>,
}
impl Block {
    pub fn members(&self) -> &[Member] {
        &self.members
    }
    pub fn replacement(&self) -> Option<&Selection> {
        self.replacement.as_ref()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BlockRecord {
    schema: String,
    block: Block,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberRecord {
    schema: String,
    block: Object,
}
fn revision(attempt: u64) -> Result<u64> {
    let revision = attempt
        .checked_mul(4)
        .and_then(|n| n.checked_sub(2))
        .ok_or(Error::Budget)?;
    if revision > 2 * (MAX_PREDECESSORS as u64 + 1) {
        return Err(Error::Budget);
    }
    Ok(revision)
}
fn context(b: &Block) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        BLOCK_SCHEMA,
        &b.journal,
        &b.decision,
        &b.source,
        &b.owner,
        &b.predecessor,
        &b.members,
    ))?))
}
fn validate(ctx: &ObservedIntent, b: &Block) -> Result<()> {
    if b.journal != ctx.journal
        || b.decision != ctx.decision().digest().map_err(Error::Decision)?
        || b.subject != ctx.decision().approval().owner
        || b.source == ctx.source
        || b.expected_revision <= b.predecessor.revision
        || b.members.is_empty()
        || b.members.len() > MAX_MEMBERS
        || b.members.windows(2).any(|w| w[0].object >= w[1].object)
        || b.replacement
            .as_ref()
            .is_some_and(|s| context(b).as_ref() != Ok(&s.context))
    {
        return Err(Error::Invalid);
    }
    for m in &b.members {
        revision(m.attempt)?;
        if b.source == step(ctx, &m.object)?
            || ctx
                .execution_profile()
                .is_some_and(|p| !p.history_releases().contains(&m.claim))
            || m.claim != claim_id(&b.journal, &b.source, &b.owner, &b.predecessor, &m.object)?
            || !allowed(
                ctx,
                &m.object,
                &Action::Release {
                    claim: m.claim.clone(),
                },
            )
        {
            return Err(Error::Invalid);
        }
    }
    Ok(())
}
/// Plan an exact approved set against one observed predecessor. Tree IO is
/// metadata only; no source transition, grant release or payload removal occurs.
pub fn plan(
    j: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    ctx: &ObservedIntent,
    source: &Key,
    predecessor_revision: u64,
    objects: &BTreeSet<Object>,
    enrollment: &Enrollment,
) -> Result<Block> {
    retained(j, ctx, true)?;
    if objects.is_empty() || objects.len() > MAX_MEMBERS {
        return Err(Error::Budget);
    }
    let state = j
        .source(source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
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
    let mut b = Block {
        journal: j.identity().clone(),
        decision: ctx.decision().digest().map_err(Error::Decision)?,
        source: source.clone(),
        owner: enrollment.owner.clone(),
        subject: enrollment.subject.clone(),
        expected_revision: state.revision,
        predecessor,
        members: Vec::new(),
        replacement: None,
    };
    for object in objects {
        let claim = claim_id(&b.journal, source, &b.owner, &b.predecessor, object)?;
        let loaded = load(j, nodes, ctx, object)?;
        if loaded.pending.is_some() {
            return Err(Error::Pending);
        }
        let attempt = if let Some((_, f)) = loaded.frames.last() {
            if f.outcome.is_none() {
                return Err(Error::Pending);
            }
            if f.released.contains(&claim) {
                return Err(Error::Occupied);
            }
            f.attempt.checked_add(1).ok_or(Error::Budget)?
        } else {
            1
        };
        b.members.push(Member {
            object: object.clone(),
            claim,
            attempt,
        });
    }
    validate(ctx, &b)?;
    let mut budget = BudgetNodes::new(nodes);
    let tree = Tree::from_root(b.predecessor.selection.root.clone());
    let entries = tree
        .entries(&mut budget, TreeLimits::default())
        .map_err(Error::Metadata)?
        .0;
    if objects.iter().any(|o| !entries.values().any(|v| v == o)) {
        return Err(Error::Invalid);
    }
    let changes = entries
        .iter()
        .filter(|(_, o)| objects.contains(o))
        .map(|(p, _)| (p.clone(), None))
        .collect::<BTreeMap<_, _>>();
    if changes.len() != entries.len() {
        let projected = tree
            .edit(tree.root(), &changes, &mut budget, TreeLimits::default())
            .map_err(Error::Metadata)?
            .0;
        b.replacement = Some(Selection {
            root: projected.root().clone(),
            context: context(&b)?,
        });
    }
    Ok(b)
}
fn restore(nodes: &impl NodeStore, root: &Object) -> Result<Block> {
    let bytes = read(nodes, root)?;
    let record: BlockRecord =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if record.schema != BLOCK_SCHEMA || encode(&record)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(record.block)
}
fn member_record(bytes: &[u8]) -> Result<MemberRecord> {
    let r: MemberRecord =
        serde_json::from_value(identity::parse_json(bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if r.schema != SCHEMA || encode(&r)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(r)
}
pub(in crate::retirement_intent::progress) fn verify_bytes(
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    f: &Frame,
    bytes: &[u8],
) -> Result<()> {
    let r = member_record(bytes)?;
    let b = restore(nodes, &r.block)?;
    validate(ctx, &b)?;
    let m = b
        .members
        .iter()
        .find(|m| m.object == f.object)
        .ok_or(Error::Invalid)?;
    if m.attempt != f.attempt
        || f.action
            != (Action::Release {
                claim: m.claim.clone(),
            })
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn locate(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
) -> Result<(Object, Block)> {
    let loaded = load(j, nodes, ctx, anchor)?;
    let (_, f) = loaded
        .frames
        .iter()
        .find(|(_, f)| f.attempt == attempt && f.outcome.is_none())
        .ok_or(Error::Missing)?;
    let bytes = typed_execution(nodes, f, SCHEMA)?.ok_or(Error::Occupied)?;
    let root = member_record(&bytes)?.block;
    let b = restore(nodes, &root)?;
    validate(ctx, &b)?;
    Ok((root, b))
}
/// Discover the latest admitted block for this object, including after its
/// outcome, so another member's unfinished report can still be recovered.
pub fn current(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    anchor: &Object,
) -> Result<Option<(u64, Block)>> {
    let loaded = load(j, nodes, ctx, anchor)?;
    for (_, f) in loaded
        .frames
        .iter()
        .rev()
        .filter(|(_, f)| f.outcome.is_none())
    {
        if let Some(bytes) = typed_execution(nodes, f, SCHEMA)? {
            let b = restore(nodes, &member_record(&bytes)?.block)?;
            return Ok(Some((f.attempt, b)));
        }
    }
    Ok(None)
}
fn group(root: &Object) -> Result<Key> {
    key("hbg", root)
}
fn key(prefix: &str, root: &Object) -> Result<Key> {
    Key::parse(&format!("{prefix}-{}", root.sha256.as_str())).map_err(|_| Error::Invalid)
}
fn guards(ctx: &ObservedIntent, b: &Block) -> Result<Vec<Guard>> {
    let mut guards = b
        .members
        .iter()
        .map(|m| {
            Ok(Guard {
                source: step(ctx, &m.object)?,
                owner: ctx.owner().clone(),
                expected_revision: revision(m.attempt)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    guards.sort_by(|a, b| a.source.as_str().cmp(b.source.as_str()));
    Ok(guards)
}
fn prepare_group(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    root: &Object,
    b: &Block,
) -> Result<Receipt> {
    let source = group(root)?;
    let prepare = Command::Prepare {
        source: source.clone(),
        owner: ctx.owner().clone(),
        expected_revision: 0,
        selection: Selection {
            root: root.clone(),
            context: context(b)?,
        },
    };
    let p = if let Some(p) = j
        .replay(key("hbp", root)?, prepare.clone())
        .map_err(Error::Journal)?
    {
        p
    } else {
        retained(j, ctx, true)?;
        nodes.finish().map_err(Error::Provider)?;
        if restore(nodes, root)? != *b {
            return Err(Error::Integrity);
        }
        j.apply(key("hbp", root)?, prepare)
            .map_err(Error::Journal)?
    };
    Ok(p)
}
fn seal(j: &mut SourceJournal, ctx: &ObservedIntent, root: &Object, b: &Block) -> Result<()> {
    let source = group(root)?;
    let prepare = Command::Prepare {
        source: source.clone(),
        owner: ctx.owner().clone(),
        expected_revision: 0,
        selection: Selection {
            root: root.clone(),
            context: context(b)?,
        },
    };
    let p = j
        .replay(key("hbp", root)?, prepare)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    let mut gs = guards(ctx, b)?;
    gs.push(guard(ctx));
    gs.sort_by(|a, b| a.source.as_str().cmp(b.source.as_str()));
    let adopt = Command::AdoptGuarded {
        source,
        owner: ctx.owner().clone(),
        expected_revision: 1,
        prepared: p.reference,
        guards: gs,
    };
    if j.replay(key("hba", root)?, adopt.clone())
        .map_err(Error::Journal)?
        .is_none()
    {
        j.apply(key("hba", root)?, adopt).map_err(Error::Journal)?;
    }
    Ok(())
}
fn admit(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    root: &Object,
    b: &Block,
    mut after_member: impl FnMut(&mut SourceJournal, usize) -> Result<()>,
) -> Result<()> {
    validate(ctx, b)?;
    if j.source(&group(root)?)
        .map_err(Error::Journal)?
        .is_some_and(|s| s.cancelled)
    {
        return Err(Error::Cancelled);
    }
    prepare_group(j, nodes, ctx, root, b)?;
    for (index, m) in b.members.iter().enumerate() {
        if j.source(&group(root)?)
            .map_err(Error::Journal)?
            .is_some_and(|s| s.cancelled)
        {
            return Err(Error::Cancelled);
        }
        let loaded = load(j, nodes, ctx, &m.object)?;
        if let Some((_, f)) = loaded
            .frames
            .iter()
            .find(|(_, f)| f.attempt == m.attempt && f.outcome.is_none())
        {
            let bytes = typed_execution(nodes, f, SCHEMA)?.ok_or(Error::Occupied)?;
            if member_record(&bytes)?.block != *root {
                return Err(Error::Occupied);
            }
        } else {
            start_bound(
                j,
                nodes,
                ctx,
                &m.object,
                Action::Release {
                    claim: m.claim.clone(),
                },
                SCHEMA,
                |nodes, frame| {
                    if frame.attempt != m.attempt {
                        return Err(Error::Stale);
                    }
                    put(
                        nodes,
                        &encode(&MemberRecord {
                            schema: SCHEMA.into(),
                            block: root.clone(),
                        })?,
                    )
                },
                |_| Ok(()),
            )?;
        }
        after_member(j, index)?;
    }
    seal(j, ctx, root, b)
}
/// Initial admission. Reopen uses `resume`/`current` with an anchor object,
/// rather than retaining this executable plan or its metadata root.
pub fn start(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    b: Block,
) -> Result<()> {
    start_hook(j, nodes, ctx, b, |_, _| Ok(()))
}
fn start_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    b: Block,
    after_member: impl FnMut(&mut SourceJournal, usize) -> Result<()>,
) -> Result<()> {
    retained(j, ctx, true)?;
    validate(ctx, &b)?;
    let bytes = encode(&BlockRecord {
        schema: BLOCK_SCHEMA.into(),
        block: b.clone(),
    })?;
    if bytes.len() > 65_536 {
        return Err(Error::Budget);
    }
    let root = put(nodes, &bytes)?;
    admit(j, nodes, ctx, &root, &b, after_member)
}
pub fn resume(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
) -> Result<()> {
    let (root, b) = locate(j, nodes, ctx, anchor, attempt)?;
    admit(j, nodes, ctx, &root, &b, |_, _| Ok(()))
}
fn command(ctx: &ObservedIntent, root: &Object, b: &Block) -> Result<Command> {
    let mut gs = guards(ctx, b)?;
    gs.push(Guard {
        source: group(root)?,
        owner: ctx.owner().clone(),
        expected_revision: 2,
    });
    gs.sort_by(|a, b| a.source.as_str().cmp(b.source.as_str()));
    Ok(Command::RetireGuarded {
        source: b.source.clone(),
        owner: b.owner.clone(),
        expected_revision: b.expected_revision,
        predecessor_revision: b.predecessor.revision,
        replacement: b.replacement.clone(),
        guards: gs,
    })
}
fn projection(nodes: &impl NodeStore, b: &Block) -> Result<()> {
    let mut ro = ReadOnly(nodes);
    let mut budget = BudgetNodes::new(&mut ro);
    let mut expected = Tree::from_root(b.predecessor.selection.root.clone())
        .entries(&mut budget, TreeLimits::default())
        .map_err(Error::Metadata)?
        .0;
    if b.members
        .iter()
        .any(|m| !expected.values().any(|o| *o == m.object))
    {
        return Err(Error::Invalid);
    }
    expected.retain(|_, o| !b.members.iter().any(|m| m.object == *o));
    if let Some(s) = &b.replacement {
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
fn effect_receipt(
    j: &mut SourceJournal,
    ctx: &ObservedIntent,
    root: &Object,
    b: &Block,
) -> Result<Option<Receipt>> {
    j.replay(key("hbe", root)?, command(ctx, root, b)?)
        .map_err(Error::Journal)
}
/// One native effect for every member. Each current member Begin and the group
/// seal are guarded atomically; replay remains historical after partial reporting.
pub fn apply(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
) -> Result<Digest> {
    apply_hook(j, nodes, ctx, anchor, attempt, |_| Ok(()))
}
fn apply_hook(
    j: &mut SourceJournal,
    nodes: &impl NodeStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
    before_apply: impl FnOnce(&mut SourceJournal) -> Result<()>,
) -> Result<Digest> {
    let (root, b) = locate(j, nodes, ctx, anchor, attempt)?;
    let receipt = if let Some(r) = effect_receipt(j, ctx, &root, &b)? {
        r
    } else {
        let state = j
            .source(&b.source)
            .map_err(Error::Journal)?
            .ok_or(Error::Missing)?;
        if state.owner != b.owner
            || state.revision != b.expected_revision
            || state.cancelled
            || state.pending.is_some()
            || !state.predecessors.contains(&b.predecessor)
        {
            return Err(Error::Stale);
        }
        projection(nodes, &b)?;
        before_apply(j)?;
        j.apply(key("hbe", &root)?, command(ctx, &root, &b)?)
            .map_err(Error::Journal)?
    };
    effect_evidence(&root, &receipt)
}
fn effect_evidence(root: &Object, r: &Receipt) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        BLOCK_SCHEMA,
        root,
        &r.reference,
        &r.checkpoint,
        r.source_revision,
    ))?))
}
fn report_hook(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    root: &Object,
    b: &Block,
    effect: Option<&Receipt>,
    mut after_member: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    for (index, m) in b.members.iter().enumerate() {
        let loaded = load(j, nodes, ctx, &m.object)?;
        let Some((_, frame)) = loaded
            .frames
            .iter()
            .rev()
            .find(|(_, f)| f.attempt == m.attempt)
        else {
            continue;
        };
        // Partial admission may have stopped on a member already owned by a
        // different block or protocol. This group's stop/effect receipt cannot
        // justify any outcome for that independently admitted execution.
        let Some(bytes) = typed_execution(nodes, frame, SCHEMA)? else {
            continue;
        };
        if member_record(&bytes)?.block != *root {
            continue;
        }
        let evidence = if let Some(r) = effect {
            Digest::blob(&encode(&(effect_evidence(root, r)?, m))?)
        } else {
            Digest::blob(&encode(&(BLOCK_SCHEMA, "stopped-no-effect", root, m))?)
        };
        finish(
            j,
            nodes,
            ctx,
            &m.object,
            m.attempt,
            Action::Release {
                claim: m.claim.clone(),
            },
            if effect.is_some() {
                Outcome::Released { evidence }
            } else {
                Outcome::NoEffect { evidence }
            },
        )?;
        after_member(index)?;
    }
    Ok(())
}
/// Record the committed outcome for each original member. Partial reports resume
/// even after other members advance or the parent intent is cancelled.
pub fn report(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
) -> Result<()> {
    let (root, b) = locate(j, nodes, ctx, anchor, attempt)?;
    let r = effect_receipt(j, ctx, &root, &b)?.ok_or(Error::Pending)?;
    report_hook(j, nodes, ctx, &root, &b, Some(&r), |_| Ok(()))
}
/// Positive journal references only. No manifest read and no completeness or
/// effect permission follows from this discovery result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupReference {
    journal: Digest,
    source: Key,
    block: Object,
    pub revision: u64,
    pub cancelled: bool,
}
impl GroupReference {
    pub fn source(&self) -> &Key {
        &self.source
    }
    pub fn block(&self) -> &Object {
        &self.block
    }
}
/// Find selected or pending group slots, including before any member Begin.
/// This inspects bounded journal state, not providers, directories or asset trees.
pub fn groups(j: &mut SourceJournal, owner: &Key, max: usize) -> Result<Vec<GroupReference>> {
    let found = j
        .control_sources(owner, "hbg-", max)
        .map_err(Error::Journal)?;
    let mut result = Vec::new();
    for f in found {
        let selection = match (f.current, f.pending) {
            (Some(current), None) => current.selection,
            (None, Some(pending)) => pending.selection,
            _ => return Err(Error::Integrity),
        };
        if f.source != group(&selection.root)? {
            return Err(Error::Integrity);
        }
        result.push(GroupReference {
            journal: j.identity().clone(),
            source: f.source,
            block: selection.root,
            revision: f.revision,
            cancelled: f.cancelled,
        });
    }
    Ok(result)
}
/// Resume the exact group after a pre-member interruption. Membership
/// and current intent are checked before any new admission; scope is not rebuilt.
pub fn resume_group(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    prepared: &GroupReference,
) -> Result<Block> {
    if prepared.journal != *j.identity() || prepared.source != group(&prepared.block)? {
        return Err(Error::Invalid);
    }
    let b = restore(nodes, &prepared.block)?;
    validate(ctx, &b)?;
    let s = j
        .source(&prepared.source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if s.owner != *ctx.owner() {
        return Err(Error::Owner);
    }
    if s.cancelled {
        return Err(Error::Cancelled);
    }
    let expected = Selection {
        root: prepared.block.clone(),
        context: context(&b)?,
    };
    if !s.pending.as_ref().is_some_and(|p| p.selection == expected)
        && !s.current.as_ref().is_some_and(|v| v.selection == expected)
    {
        return Err(Error::Stale);
    }
    admit(j, nodes, ctx, &prepared.block, &b, |_, _| Ok(()))?;
    Ok(b)
}
#[derive(Clone, Debug)]
pub struct GroupStop {
    pub source: Key,
    pub revision: u64,
    pub checkpoint: crate::source_claims::Checkpoint,
}
fn stop_root(j: &mut SourceJournal, owner: &Key, root: &Object) -> Result<GroupStop> {
    let id = group(root)?;
    let state = j.source(&id).map_err(Error::Journal)?;
    if state.as_ref().is_some_and(|s| s.owner != *owner) {
        return Err(Error::Owner);
    }
    if state.as_ref().is_none_or(|s| !s.cancelled) {
        let rev = state.as_ref().map_or(0, |s| s.revision);
        let op = Key::parse(&format!("hbc-{rev}-{}", root.sha256.as_str()))
            .map_err(|_| Error::Invalid)?;
        j.apply(
            op,
            Command::Cancel {
                source: id.clone(),
                owner: owner.clone(),
                expected_revision: rev,
            },
        )
        .map_err(Error::Journal)?;
    }
    let state = j
        .source(&id)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if !state.cancelled {
        return Err(Error::Stale);
    }
    Ok(GroupStop {
        source: id,
        revision: state.revision,
        checkpoint: j.checkpoint(),
    })
}
/// Stop an existing prepared/admitted group using journal references only. This fences
/// native work but does not assert NoEffect, release claims or complete cleanup.
pub fn stop_group(
    j: &mut SourceJournal,
    owner: &Key,
    prepared: &GroupReference,
) -> Result<GroupStop> {
    if prepared.journal != *j.identity() || prepared.source != group(&prepared.block)? {
        return Err(Error::Invalid);
    }
    let s = j
        .source(&prepared.source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if s.owner != *owner {
        return Err(Error::Owner);
    }
    if !s
        .pending
        .as_ref()
        .is_some_and(|p| p.selection.root == prepared.block)
        && !s
            .current
            .as_ref()
            .is_some_and(|v| v.selection.root == prepared.block)
    {
        return Err(Error::Stale);
    }
    stop_root(j, owner, &prepared.block)
}
/// Terminally fence this group before deciding NoEffect. A commit that won the
/// race is reported Released; otherwise stopped native guards exclude old workers.
pub fn cancel(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    anchor: &Object,
    attempt: u64,
) -> Result<()> {
    let (_, b) = locate(j, nodes, ctx, anchor, attempt)?;
    cancel_plan(j, nodes, ctx, &b).map(|_| ())
}

fn exact_root(ctx: &ObservedIntent, b: &Block) -> Result<Object> {
    validate(ctx, b)?;
    let bytes = encode(&BlockRecord {
        schema: BLOCK_SCHEMA.into(),
        block: b.clone(),
    })?;
    if bytes.len() > 65_536 {
        return Err(Error::Budget);
    }
    Ok(Object::from_bytes(&bytes))
}
/// Stop an exact saved native plan, including before its first group/member
/// acknowledgement. No manifest reads, new member admission or effect assertion.
/// The canonical group identity fences only this plan; source slots remain bounded.
pub fn stop_plan(j: &mut SourceJournal, ctx: &ObservedIntent, b: &Block) -> Result<GroupStop> {
    retained(j, ctx, false)?;
    let root = exact_root(ctx, b)?;
    if let Some(state) = j.source(&group(&root)?).map_err(Error::Journal)? {
        if state.owner != *ctx.owner() {
            return Err(Error::Owner);
        }
        if !state.predecessors.is_empty() {
            return Err(Error::Stale);
        }
        let selected = match (&state.current, &state.pending) {
            (Some(current), None) => Some(&current.selection),
            (None, Some(pending)) => Some(&pending.selection),
            (None, None) => None,
            _ => return Err(Error::Stale),
        };
        match selected {
            Some(selection) if selection.root == root && selection.context == context(b)? => (),
            None if state.cancelled && state.revision == 1 && state.predecessors.is_empty() => (),
            _ => return Err(Error::Stale),
        }
    }
    stop_root(j, ctx.owner(), &root)
}
/// Fence exact saved work before consulting the native effect receipt. Report
/// only this block's adopted members. Foreign work and unadopted prepared members
/// remain untouched; a committed effect wins stop. Errors may follow fencing.
pub fn cancel_plan(
    j: &mut SourceJournal,
    nodes: &mut MetadataStore,
    ctx: &ObservedIntent,
    b: &Block,
) -> Result<GroupStop> {
    let root = exact_root(ctx, b)?;
    let mut stopped = stop_plan(j, ctx, b)?;
    let effect = effect_receipt(j, ctx, &root, b)?;
    report_hook(j, nodes, ctx, &root, b, effect.as_ref(), |_| Ok(()))?;
    stopped.checkpoint = j.checkpoint();
    Ok(stopped)
}

#[cfg(test)]
#[path = "block_tests.rs"]
mod tests;
