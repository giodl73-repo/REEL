//! Durable explicit retirement intent. No payload execution or native authentication.
pub mod execution_profile;
pub mod policy;
pub mod progress;
use crate::{
    Key,
    metadata::{Checkpoint as MetadataCheckpoint, MetadataStore},
    source_claims::{self, Command, Receipt, Reference, Selection, Source, SourceJournal},
};
use serde::{Deserialize, Serialize};
use tig_core::{
    Digest, Object, identity,
    retirement::review::{
        Review,
        decision::{self, Decision},
    },
    tree::{NodeStore, TreeError},
};

#[derive(Clone, Debug)]
pub struct Enrollment {
    pub owner: Key,
    pub subject: Digest,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Journal(source_claims::Error),
    Metadata(TreeError),
    Provider(crate::metadata::Error),
    Review(tig_core::retirement::review::Error),
    Decision(decision::Error),
    Invalid,
    Integrity,
    Missing,
    Owner,
    Cancelled,
    Occupied,
    Stale,
    Pending,
    Budget,
}
type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: String,
    journal: Digest,
    source: Key,
    owner: Key,
    subject: Digest,
    review: Object,
    decision: Object,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    execution_profile: Option<Object>,
}
#[derive(Clone, Debug)]
pub struct Candidate {
    record: Record,
    selection: Selection,
}
impl Candidate {
    pub fn selection(&self) -> Selection {
        self.selection.clone()
    }
}
#[derive(Clone, Debug)]
pub struct Ready {
    candidate: Candidate,
    checkpoint: MetadataCheckpoint,
}
impl Ready {
    pub fn provider_checkpoint(&self) -> MetadataCheckpoint {
        self.checkpoint.clone()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    New,
    Historical,
    Recovered,
}
#[derive(Clone, Debug)]
pub struct Prepared {
    candidate: Candidate,
    reference: Reference,
    origin: Origin,
}
impl Prepared {
    pub fn reference(&self) -> Reference {
        self.reference.clone()
    }
    pub fn origin(&self) -> &Origin {
        &self.origin
    }
    /// Read the exact retained decision before resuming adoption. This grants
    /// neither current authority nor permission to perform a payload effect.
    pub fn decision(&self, nodes: &impl NodeStore) -> Result<Decision> {
        let c = &self.candidate;
        let (_, _, decision, _) = load(
            &c.record.journal,
            &enrollment(&c.record),
            &c.record.source,
            &c.selection,
            nodes,
        )?;
        Ok(decision)
    }
}
#[derive(Clone, Debug)]
pub struct ObservedIntent {
    journal: Digest,
    source: Key,
    state: Source,
    review: Review,
    decision: Decision,
    execution_profile: Option<std::sync::Arc<execution_profile::Context>>,
    policy_authority: Option<std::sync::Arc<policy::NativeAuthority>>,
}
impl ObservedIntent {
    pub fn owner(&self) -> &Key {
        &self.state.owner
    }
    pub fn review(&self) -> &Review {
        &self.review
    }
    pub fn decision(&self) -> &Decision {
        &self.decision
    }
    /// Saved immutable admitted context, not current provider authority.
    pub fn execution_profile(&self) -> Option<&execution_profile::Context> {
        self.execution_profile.as_deref()
    }
    pub fn revision(&self) -> u64 {
        self.state.revision
    }
    /// Optimistic state check, not a physical-work lease or current-owner proof.
    pub fn revalidate(&self, journal: &mut SourceJournal) -> Result<()> {
        if journal.identity() != &self.journal
            || journal
                .source(&self.source)
                .map_err(Error::Journal)?
                .as_ref()
                != Some(&self.state)
        {
            return Err(Error::Stale);
        }
        Ok(())
    }
}
fn encode(v: &impl Serialize) -> Result<Vec<u8>> {
    identity::canonical_json(&serde_json::to_value(v).map_err(|_| Error::Invalid)?)
        .map_err(|_| Error::Invalid)
}
fn source(operation: &Digest) -> Key {
    Key::parse(operation.as_str()).expect("digest is a bounded key")
}
fn context(r: &Record) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(
        "tig.retirement-intent-context.design-1",
        r,
    ))?))
}
fn read(nodes: &impl NodeStore, object: &Object) -> Result<Vec<u8>> {
    if object.bytes == 0 || object.bytes > 65_536 {
        return Err(Error::Invalid);
    }
    let bytes = nodes
        .get(object)
        .map_err(Error::Metadata)?
        .ok_or(Error::Missing)?;
    if Object::from_bytes(&bytes) != *object {
        return Err(Error::Integrity);
    }
    Ok(bytes)
}
fn put(nodes: &mut impl NodeStore, bytes: &[u8]) -> Result<Object> {
    let expected = Object::from_bytes(bytes);
    if nodes.put(bytes).map_err(Error::Metadata)? != expected || read(nodes, &expected)? != bytes {
        return Err(Error::Integrity);
    }
    Ok(expected)
}
fn state(journal: &mut SourceJournal, enrollment: &Enrollment, id: &Key) -> Result<Option<Source>> {
    let state = journal.source(id).map_err(Error::Journal)?;
    if let Some(s) = &state {
        if s.owner != enrollment.owner {
            return Err(Error::Owner);
        }
        if s.cancelled {
            return Err(Error::Cancelled);
        }
    }
    Ok(state)
}
fn unused(journal: &mut SourceJournal, enrollment: &Enrollment, id: &Key) -> Result<()> {
    if state(journal, enrollment, id)?.is_some() {
        return Err(Error::Occupied);
    }
    Ok(())
}
fn enrollment(r: &Record) -> Enrollment {
    Enrollment {
        owner: r.owner.clone(),
        subject: r.subject.clone(),
    }
}
fn load(
    journal: &Digest,
    enrollment: &Enrollment,
    id: &Key,
    selection: &Selection,
    nodes: &impl NodeStore,
) -> Result<(Record, Review, Decision, Option<execution_profile::Context>)> {
    let bytes = read(nodes, &selection.root)?;
    let record: Record =
        serde_json::from_value(identity::parse_json(&bytes).map_err(|_| Error::Invalid)?)
            .map_err(|_| Error::Invalid)?;
    if encode(&record)? != bytes
        || !matches!(
            (record.schema.as_str(), record.execution_profile.is_some()),
            ("tig.retirement-intent.design-1", false)
                | ("tig.retirement-intent.experimental-2", true)
        )
        || &record.journal != journal
        || &record.source != id
        || record.owner != enrollment.owner
        || record.subject != enrollment.subject
        || context(&record)? != selection.context
    {
        return Err(Error::Invalid);
    }
    let review = Review::from_json(&read(nodes, &record.review)?).map_err(Error::Review)?;
    let decision =
        Decision::from_json(&read(nodes, &record.decision)?, &review).map_err(Error::Decision)?;
    let a = decision.approval();
    if &a.namespace != journal || source(&a.operation) != *id || a.owner != enrollment.subject {
        return Err(Error::Invalid);
    }
    if decision.execution_context().is_some() && record.execution_profile.is_none() {
        return Err(Error::Invalid);
    }
    let execution_profile = record
        .execution_profile
        .as_ref()
        .map(|root| {
            let context = execution_profile::Context::from_json(&read(nodes, root)?)?;
            context.verify(&review, &decision)?;
            if context.policy_proof().is_some() {
                let proof = policy::Proof::load(nodes, &context, &review, &decision)?;
                if proof.owner() != &enrollment.owner {
                    return Err(Error::Owner);
                }
            }
            Ok::<_, Error>(context)
        })
        .transpose()?;
    Ok((record, review, decision, execution_profile))
}
pub fn stage(
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    enrollment: &Enrollment,
    review: &Review,
    decision: &Decision,
) -> Result<Candidate> {
    if decision.execution_context().is_some() {
        return Err(Error::Invalid);
    }
    stage_inner(journal, nodes, enrollment, review, decision, None)
}
/// Save the exact execution context before original intent admission.
/// No provider action, work budget or deletion permission is created here.
pub fn stage_profiled(
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    enrollment: &Enrollment,
    review: &Review,
    decision: &Decision,
    profile: &execution_profile::Context,
) -> Result<Candidate> {
    if profile.policy_proof().is_some() {
        return Err(Error::Invalid);
    }
    profile.verify(review, decision)?;
    stage_inner(journal, nodes, enrollment, review, decision, Some(profile))
}
fn stage_inner(
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    enrollment: &Enrollment,
    review: &Review,
    decision: &Decision,
    profile: Option<&execution_profile::Context>,
) -> Result<Candidate> {
    let a = decision.approval();
    if &a.namespace != journal.identity() || a.owner != enrollment.subject {
        return Err(Error::Owner);
    }
    Decision::from_json(&decision.to_json().map_err(Error::Decision)?, review)
        .map_err(Error::Decision)?;
    let id = source(&a.operation);
    unused(journal, enrollment, &id)?;
    let r = Record {
        schema: if profile.is_some() {
            "tig.retirement-intent.experimental-2"
        } else {
            "tig.retirement-intent.design-1"
        }
        .into(),
        journal: journal.identity().clone(),
        source: id,
        owner: enrollment.owner.clone(),
        subject: enrollment.subject.clone(),
        review: put(nodes, &review.to_json().map_err(Error::Review)?)?,
        decision: put(nodes, &decision.to_json().map_err(Error::Decision)?)?,
        execution_profile: profile.map(|p| put(nodes, &p.to_json()?)).transpose()?,
    };
    let selection = Selection {
        root: put(nodes, &encode(&r)?)?,
        context: context(&r)?,
    };
    Ok(Candidate {
        record: r,
        selection,
    })
}
pub fn finish(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    candidate: Candidate,
) -> Result<Ready> {
    if &candidate.record.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    unused(
        journal,
        &enrollment(&candidate.record),
        &candidate.record.source,
    )?;
    let checkpoint = nodes.finish().map_err(Error::Provider)?;
    load(
        journal.identity(),
        &enrollment(&candidate.record),
        &candidate.record.source,
        &candidate.selection,
        nodes,
    )?;
    unused(
        journal,
        &enrollment(&candidate.record),
        &candidate.record.source,
    )?;
    Ok(Ready {
        candidate,
        checkpoint,
    })
}
pub fn prepare(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    ready: Ready,
    command: Key,
) -> Result<Prepared> {
    let candidate = ready.candidate;
    if &candidate.record.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let request = Command::Prepare {
        source: candidate.record.source.clone(),
        owner: candidate.record.owner.clone(),
        expected_revision: 0,
        selection: candidate.selection.clone(),
    };
    if let Some(old) = journal
        .replay(command.clone(), request.clone())
        .map_err(Error::Journal)?
    {
        return Ok(Prepared {
            candidate,
            reference: old.reference,
            origin: Origin::Historical,
        });
    }
    unused(
        journal,
        &enrollment(&candidate.record),
        &candidate.record.source,
    )?;
    load(
        journal.identity(),
        &enrollment(&candidate.record),
        &candidate.record.source,
        &candidate.selection,
        nodes,
    )?;
    let receipt = journal.apply(command, request).map_err(Error::Journal)?;
    Ok(Prepared {
        candidate,
        reference: receipt.reference,
        origin: if receipt.replayed {
            Origin::Historical
        } else {
            Origin::New
        },
    })
}
pub fn adopt(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    prepared: &Prepared,
    command: Key,
) -> Result<Receipt> {
    let c = &prepared.candidate;
    if &c.record.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let request = Command::Adopt {
        source: c.record.source.clone(),
        owner: c.record.owner.clone(),
        expected_revision: 1,
        prepared: prepared.reference.clone(),
    };
    if let Some(old) = journal
        .replay(command.clone(), request.clone())
        .map_err(Error::Journal)?
    {
        return Ok(old);
    }
    let current =
        state(journal, &enrollment(&c.record), &c.record.source)?.ok_or(Error::Missing)?;
    let pending = current.pending.as_ref().ok_or(Error::Pending)?;
    if current.revision != 1
        || current.current.is_some()
        || !current.predecessors.is_empty()
        || pending.reference != prepared.reference
        || pending.selection != c.selection
    {
        return Err(Error::Stale);
    }
    load(
        journal.identity(),
        &enrollment(&c.record),
        &c.record.source,
        &c.selection,
        nodes,
    )?;
    journal.apply(command, request).map_err(Error::Journal)
}
pub fn resume_pending(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    enrollment: &Enrollment,
    operation: &Digest,
) -> Result<Prepared> {
    let id = source(operation);
    let current = state(journal, enrollment, &id)?.ok_or(Error::Missing)?;
    let pending = current.pending.as_ref().ok_or(Error::Pending)?;
    if current.revision != 1 || current.current.is_some() || !current.predecessors.is_empty() {
        return Err(Error::Stale);
    }
    let (record, _, _, _) = load(
        journal.identity(),
        enrollment,
        &id,
        &pending.selection,
        nodes,
    )?;
    nodes.finish().map_err(Error::Provider)?;
    load(
        journal.identity(),
        enrollment,
        &id,
        &pending.selection,
        nodes,
    )?;
    if journal.source(&id).map_err(Error::Journal)?.as_ref() != Some(&current) {
        return Err(Error::Stale);
    }
    Ok(Prepared {
        candidate: Candidate {
            record,
            selection: pending.selection.clone(),
        },
        reference: pending.reference.clone(),
        origin: Origin::Recovered,
    })
}
pub fn observe(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    enrollment: &Enrollment,
    operation: &Digest,
) -> Result<ObservedIntent> {
    observe_mode(journal, nodes, enrollment, operation, false)
}
/// Read retained intent data after cancellation for reconciliation, never new work.
pub fn inspect_retained(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    enrollment: &Enrollment,
    operation: &Digest,
) -> Result<ObservedIntent> {
    observe_mode(journal, nodes, enrollment, operation, true)
}
fn observe_mode(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    enrollment: &Enrollment,
    operation: &Digest,
    allow_cancelled: bool,
) -> Result<ObservedIntent> {
    let id = source(operation);
    let current = journal
        .source(&id)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if current.owner != enrollment.owner {
        return Err(Error::Owner);
    }
    if current.cancelled && !allow_cancelled {
        return Err(Error::Cancelled);
    }
    if current.pending.is_some() {
        return Err(Error::Pending);
    }
    if current.revision != if current.cancelled { 3 } else { 2 } || !current.predecessors.is_empty()
    {
        return Err(Error::Stale);
    }
    let selection = &current.current.as_ref().ok_or(Error::Missing)?.selection;
    let (_, review, decision, execution_profile) =
        load(journal.identity(), enrollment, &id, selection, nodes)?;
    let observed = ObservedIntent {
        journal: journal.identity().clone(),
        source: id,
        state: current,
        review,
        decision,
        execution_profile: execution_profile.map(std::sync::Arc::new),
        policy_authority: None,
    };
    observed.revalidate(journal)?;
    Ok(observed)
}
pub fn cancel(
    journal: &mut SourceJournal,
    owner: &Key,
    operation: &Digest,
    expected_revision: u64,
    command: Key,
) -> Result<Receipt> {
    journal
        .apply(
            command,
            Command::Cancel {
                source: source(operation),
                owner: owner.clone(),
                expected_revision,
            },
        )
        .map_err(Error::Journal)
}
