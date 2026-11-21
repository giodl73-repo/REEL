//! Durable, discoverable exact index work. Staging is not protection admission.
use super::*;
const WORK: &str = "tig.protection-work.design-1";
const BATCH_WORK: &str = "tig.protection-work.design-2";
const LINKED_WORK: &str = "tig.protection-work.design-3";
pub mod adoption;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRef {
    index: Digest,
    source: Key,
    root: Object,
}
impl WorkRef {
    pub fn source(&self) -> &Key {
        &self.source
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    schema: String,
    index: Digest,
    pub update: Update,
    pub evidence: Option<Object>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional: Option<Vec<Update>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adoption: Option<adoption::Link>,
}
impl Record {
    pub fn updates(&self) -> impl Iterator<Item = &Update> {
        std::iter::once(&self.update).chain(self.additional.iter().flatten())
    }
    pub fn batch(&self) -> Option<batch::Batch> {
        self.additional.as_ref().map(|_| batch::Batch {
            operation: self.update.operation.clone(),
            updates: self.updates().cloned().collect(),
        })
    }
    fn validate(&self, index: &Index) -> Result<()> {
        if self.adoption.is_none()
            && index
                .routing()
                .is_some_and(|route| self.updates().any(|u| u.actor == route.actor))
        {
            return Err(Error::Pending);
        }
        if self.additional.as_ref().is_some_and(|a| a.len() > 15) {
            return Err(Error::Budget);
        }
        if self.adoption.is_some() && self.schema != LINKED_WORK {
            return Err(Error::Invalid);
        }
        match (&self.schema[..], self.batch()) {
            (WORK, None) => {
                if index.enrollment.actor(&self.update.actor)?.owner != self.update.owner
                    || self.update.changes.len() > MAX_CHANGES
                {
                    return Err(Error::Invalid);
                }
                Ok(())
            }
            (BATCH_WORK, Some(batch)) => batch.validate(index),
            (LINKED_WORK, Some(batch)) => {
                batch.validate(index)?;
                self.adoption
                    .as_ref()
                    .ok_or(Error::Invalid)?
                    .validate(index, self)
            }
            _ => Err(Error::Invalid),
        }
    }
}
fn source(index: &Index, operation: &Key) -> Result<Key> {
    Ok(key(&format!(
        "work-{}",
        Digest::blob(&encode(&(index.identity(), operation))?).as_str()
    )))
}
fn context(root: &Object) -> Result<Digest> {
    Ok(Digest::blob(&encode(&(WORK, root))?))
}

/// Idempotent exact-body staging. The record and optional evidence are finished
/// before the native prepare acknowledges its root. No actor claims change.
pub fn stage(
    index: &mut Index,
    update: &Update,
    evidence: Option<Object>,
    nodes: &mut metadata::MetadataStore,
) -> Result<WorkRef> {
    stage_record(
        index,
        Record {
            schema: WORK.into(),
            index: index.identity().clone(),
            update: update.clone(),
            evidence,
            additional: None,
            adoption: None,
        },
        nodes,
    )
}
pub fn stage_batch(
    index: &mut Index,
    batch: &batch::Batch,
    evidence: Option<Object>,
    nodes: &mut metadata::MetadataStore,
) -> Result<WorkRef> {
    batch.validate(index)?;
    stage_record(
        index,
        Record {
            schema: BATCH_WORK.into(),
            index: index.identity().clone(),
            update: batch.updates[0].clone(),
            evidence,
            additional: Some(batch.updates[1..].to_vec()),
            adoption: None,
        },
        nodes,
    )
}
fn stage_record(
    index: &mut Index,
    record: Record,
    nodes: &mut metadata::MetadataStore,
) -> Result<WorkRef> {
    record.validate(index)?;
    if let Some(old) = lookup(index, &record.update.operation)? {
        let prior = read(index, &old, nodes)?;
        if encode(&prior)? != encode(&record)? {
            return Err(Error::Conflict);
        }
        return Ok(old);
    }
    if let Some(evidence) = &record.evidence {
        if evidence.bytes > 65536 {
            return Err(Error::Budget);
        }
        let bytes = nodes.get(evidence)?.ok_or(Error::Unknown)?;
        if Object::from_bytes(&bytes) != *evidence {
            return Err(Error::Invalid);
        }
    }
    for update in record.updates() {
        let state = index.actor(nodes, &update.actor)?;
        if state.revision != update.expected_revision {
            return Err(Error::Stale);
        }
    }
    let root = put(nodes, &record)?;
    nodes.finish()?;
    let _: Record = load(nodes, &root)?;
    let id = source(index, &record.update.operation)?;
    index.command(
        op(index.identity(), &id, "work-prepare")?,
        Command::Prepare {
            source: id.clone(),
            owner: index.enrollment.owner.clone(),
            expected_revision: 0,
            selection: Selection {
                root: root.clone(),
                context: context(&root)?,
            },
        },
    )?;
    Ok(WorkRef {
        index: index.identity().clone(),
        source: id,
        root,
    })
}
pub fn lookup(index: &mut Index, operation: &Key) -> Result<Option<WorkRef>> {
    let id = source(index, operation)?;
    let Some(state) = index.journal.source(&id)? else {
        return Ok(None);
    };
    reference(index, id, state).map(Some)
}
fn reference(index: &Index, id: Key, state: super::super::Source) -> Result<WorkRef> {
    if state.owner != index.enrollment.owner || state.cancelled {
        return Err(Error::Invalid);
    }
    let selection = match (state.current, state.pending) {
        (Some(current), None) if state.revision == 2 => current.selection,
        (None, Some(pending)) if state.revision == 1 => pending.selection,
        _ => return Err(Error::Invalid),
    };
    if selection.context != context(&selection.root)? {
        return Err(Error::Invalid);
    }
    Ok(WorkRef {
        index: index.identity().clone(),
        source: id,
        root: selection.root,
    })
}
/// Bounded positive references; no metadata reads or inferred queue truncation.
pub fn discover(index: &mut Index, maximum: usize) -> Result<Vec<WorkRef>> {
    let found = index
        .journal
        .control_sources(&index.enrollment.owner, "work-", maximum)?;
    found
        .into_iter()
        .map(|state| {
            let id = state.source.clone();
            let source = super::super::Source {
                owner: index.enrollment.owner.clone(),
                revision: state.revision,
                cancelled: state.cancelled,
                current: state.current,
                pending: state.pending,
                predecessors: Vec::new(),
            };
            reference(index, id, source)
        })
        .collect()
}
pub fn read(index: &mut Index, reference: &WorkRef, nodes: &impl NodeStore) -> Result<Record> {
    if reference.index != *index.identity() {
        return Err(Error::Invalid);
    }
    let state = index
        .journal
        .source(&reference.source)?
        .ok_or(Error::Unknown)?;
    if self::reference(index, reference.source.clone(), state)? != *reference {
        return Err(Error::Invalid);
    }
    let record: Record = load(nodes, &reference.root)?;
    if record.index != *index.identity()
        || source(index, &record.update.operation)? != reference.source
    {
        return Err(Error::Invalid);
    }
    record.validate(index)?;
    Ok(record)
}
pub fn complete(index: &mut Index, reference: &WorkRef) -> Result<bool> {
    if reference.index != *index.identity() {
        return Err(Error::Invalid);
    }
    let state = index
        .journal
        .source(&reference.source)?
        .ok_or(Error::Unknown)?;
    let complete = state.current.is_some();
    if self::reference(index, reference.source.clone(), state)? != *reference {
        return Err(Error::Invalid);
    }
    Ok(complete)
}
pub fn execute(
    index: &mut Index,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
) -> Result<Receipt> {
    execute_validated(index, reference, nodes, |_| Ok(()))
}

/// Recovery for an index admission that already crossed native Prepare. It does
/// not require the original Git/job producer to be online and cannot start a
/// never-admitted update.
pub fn resume_admitted(
    index: &mut Index,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
) -> Result<Receipt> {
    let record = read(index, reference, nodes)?;
    if index.prepared(nodes, &record.update.operation)?.is_none() {
        return Err(Error::Pending);
    }
    execute_validated(index, reference, nodes, |_| Err(Error::Unknown))
}
/// Validation applies only before NEW index admission, and is repeated after
/// metadata IO. Already prepared/adopted index work reconciles its exact receipt.
/// This callback is an admitted source check, not a universal authority lease.
pub fn execute_validated(
    index: &mut Index,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
    mut validate: impl FnMut(&Record) -> Result<()>,
) -> Result<Receipt> {
    let record = read(index, reference, nodes)?;
    if record.adoption.is_some() {
        return Err(Error::Pending);
    }
    let admitted = index.prepared(nodes, &record.update.operation)?.is_some();
    if !admitted {
        validate(&record)?;
    }
    let check = |stage| {
        if !admitted && stage == Stage::MetadataReady {
            validate(&record)?;
        }
        Ok(())
    };
    let receipt = match record.batch() {
        Some(batch) => index.update_batch_hook(&batch, nodes, check)?,
        None => index.update_hook(&record.update, nodes, check)?,
    };
    mark_complete(index, reference)?;
    Ok(receipt)
}
fn mark_complete(index: &mut Index, reference: &WorkRef) -> Result<()> {
    if !complete(index, reference)? {
        index.command(
            op(index.identity(), &reference.source, "work-adopt")?,
            Command::Adopt {
                source: reference.source.clone(),
                owner: index.enrollment.owner.clone(),
                expected_revision: 1,
                prepared: Reference {
                    journal: index.identity().clone(),
                    operation: op(index.identity(), &reference.source, "work-prepare")?,
                },
            },
        )?;
    }
    Ok(())
}
pub(super) fn is_linked(
    index: &mut Index,
    nodes: &impl NodeStore,
    operation: &Key,
) -> Result<bool> {
    match lookup(index, operation)? {
        Some(reference) => Ok(read(index, &reference, nodes)?.adoption.is_some()),
        None => Ok(false),
    }
}
