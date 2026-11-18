//! Ordered source-head adoption while the native old/new protection union holds.
use super::*;
use crate::source_claims::{Version, coordinated};
const DOMAIN: &str = "tig.coordinated-adoption.design-1";
pub mod enrollment;
pub(in crate::source_claims::protection) mod headroom;
mod slot;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub binding: coordinated::Binding,
    pub journal: Digest,
    pub expected_revision: u64,
    pub prior: Version,
    pub target: Selection,
}
impl Link {
    pub(super) fn validate(&self, index: &Index, record: &Record) -> Result<()> {
        let b = &self.binding;
        let update = record
            .updates()
            .find(|r| r.actor == b.actor)
            .ok_or(Error::Invalid)?;
        if index
            .routing()
            .is_some_and(|r| r.source != b.source || r.actor != b.actor)
            || b.index != *index.identity()
            || index.enrollment.actor(&b.actor)?.owner != b.owner
            || update.owner != b.owner
            || update.selection != self.target
            || !update.ready
            || self.prior.revision > self.expected_revision
            || self.expected_revision.checked_add(2).is_none()
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
fn candidate_binding(index: &Index, journal: &SourceJournal) -> Result<coordinated::Binding> {
    let b = journal.coordinator().ok_or(Error::Invalid)?.clone();
    if index
        .routing()
        .is_some_and(|r| r.source != b.source || r.actor != b.actor)
        || b.index != *index.identity()
        || index.enrollment.actor(&b.actor)?.owner != b.owner
    {
        return Err(Error::Invalid);
    }
    Ok(b)
}
fn binding(index: &mut Index, journal: &SourceJournal) -> Result<coordinated::Binding> {
    let b = candidate_binding(index, journal)?;
    enrollment::verify(index, journal, &b)?;
    Ok(b)
}
fn command(journal: &mut SourceJournal, operation: Key, command: Command) -> Result<Receipt> {
    let result = journal.apply_once(operation, command, |_| Ok(()), true);
    journal.mark_uncertain(&result);
    Ok(result?)
}
fn id(index: &Digest, journal: &Digest, source: &Key, operation: &Key, phase: &str) -> Result<Key> {
    Ok(key(&format!(
        "adopt-{}",
        Digest::blob(&encode(&(
            DOMAIN, index, journal, source, operation, phase
        ))?)
        .as_str()
    )))
}
pub(in crate::source_claims::protection) fn commands(
    index: &Index,
    link: &Link,
    operation: &Key,
    initial: bool,
) -> Result<(Key, Command, Key, Command)> {
    let prepare = id(
        index.identity(),
        &link.journal,
        &link.binding.source,
        operation,
        if initial {
            "initial-prepare"
        } else {
            "prepare"
        },
    )?;
    let adopt = id(
        index.identity(),
        &link.journal,
        &link.binding.source,
        operation,
        if initial { "initial-select" } else { "select" },
    )?;
    Ok((
        prepare.clone(),
        Command::Prepare {
            source: link.binding.source.clone(),
            owner: link.binding.owner.clone(),
            expected_revision: link.expected_revision,
            selection: link.target.clone(),
        },
        adopt,
        Command::Adopt {
            source: link.binding.source.clone(),
            owner: link.binding.owner.clone(),
            expected_revision: link.expected_revision + 1,
            prepared: Reference {
                journal: link.journal.clone(),
                operation: prepare,
            },
        },
    ))
}
pub fn initialize(
    index: &mut Index,
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
) -> Result<Receipt> {
    let b = binding(index, journal)?;
    let _fence = coordinated::lock(journal)?;
    initialize_head(index, journal, nodes, &b, |_| Ok(()))
}
fn initialize_head(
    index: &mut Index,
    journal: &mut SourceJournal,
    nodes: &mut impl NodeStore,
    b: &coordinated::Binding,
    mut hook: impl FnMut(enrollment::Stage) -> Result<()>,
) -> Result<Receipt> {
    let link = Link {
        binding: b.clone(),
        journal: journal.identity().clone(),
        expected_revision: 0,
        prior: Version {
            revision: 0,
            selection: b.initial.clone(),
        },
        target: b.initial.clone(),
    };
    let (p, pc, a, ac) = commands(index, &link, &key("initialize"), true)?;
    if let Some(receipt) = journal.replay(a.clone(), ac.clone())? {
        hook(enrollment::Stage::SourceAdopted)?;
        return Ok(receipt);
    }
    let state = index.actor(nodes, &b.actor)?;
    if state.revision != 1 || !state.ready || state.selection != b.initial {
        return Err(Error::Stale);
    }
    command(journal, p, pc)?;
    hook(enrollment::Stage::SourcePrepared)?;
    let receipt = command(journal, a, ac)?;
    hook(enrollment::Stage::SourceAdopted)?;
    Ok(receipt)
}
pub fn verify_binding(index: &mut Index, journal: &SourceJournal, link: &Link) -> Result<()> {
    if candidate_binding(index, journal)? != link.binding || *journal.identity() != link.journal {
        return Err(Error::Invalid);
    }
    enrollment::verify(index, journal, &link.binding)
}
pub fn stage(
    index: &mut Index,
    journal: &mut SourceJournal,
    batch: &batch::Batch,
    evidence: Option<Object>,
    nodes: &mut metadata::MetadataStore,
) -> Result<WorkRef> {
    batch.validate(index)?;
    let b = binding(index, journal)?;
    let _fence = coordinated::lock(journal)?;
    if let Some(reference) = lookup(index, &batch.operation)? {
        let record = read(index, &reference, nodes)?;
        let link = record.adoption.as_ref().ok_or(Error::Conflict)?;
        verify_binding(index, journal, link)?;
        if record
            .batch()
            .as_ref()
            .map(batch::Batch::digest)
            .transpose()?
            != Some(batch.digest()?)
            || record.evidence != evidence
        {
            return Err(Error::Conflict);
        }
        return Ok(reference);
    }
    let source = journal.source(&b.source)?.ok_or(Error::Unknown)?;
    if source.cancelled {
        return Err(Error::Journal(super::super::super::Error::Cancelled));
    }
    if source.owner != b.owner || source.pending.is_some() {
        return Err(Error::Stale);
    }
    let prior = source.current.ok_or(Error::Unknown)?;
    if index.actor(nodes, &b.actor)?.selection != prior.selection {
        return Err(Error::Stale);
    }
    let update = batch
        .updates
        .iter()
        .find(|r| r.actor == b.actor)
        .ok_or(Error::Invalid)?;
    let link = Link {
        binding: b,
        journal: journal.identity().clone(),
        expected_revision: source.revision,
        prior,
        target: update.selection.clone(),
    };
    stage_record(
        index,
        Record {
            schema: LINKED_WORK.into(),
            index: index.identity().clone(),
            update: batch.updates[0].clone(),
            additional: Some(batch.updates[1..].to_vec()),
            adoption: Some(link),
            evidence,
        },
        nodes,
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    HeadroomReserved,
    SourceRoomReleased,
    IndexRoomReleased,
    Reserved,
    MetadataReady,
    ProtectionPrepared,
    SourcePrepared,
    SourceAdopted,
    ProtectionSelected,
    ReservationReleased,
    Complete,
}
pub fn execute(
    index: &mut Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
) -> Result<Receipt> {
    execute_validated(index, journal, reference, nodes, |_| Ok(()), |_| Ok(()))
}
pub fn resume_admitted(
    index: &mut Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
) -> Result<Receipt> {
    let record = read(index, reference, nodes)?;
    if index.prepared(nodes, &record.update.operation)?.is_none() {
        return Err(Error::Pending);
    }
    execute_validated(
        index,
        journal,
        reference,
        nodes,
        |_| Err(Error::Unknown),
        |_| Ok(()),
    )
}
fn current(
    journal: &mut SourceJournal,
    link: &Link,
    selected: bool,
    prepared: bool,
    prepare: &Key,
) -> Result<()> {
    let s = journal
        .source(&link.binding.source)?
        .ok_or(Error::Unknown)?;
    if s.cancelled {
        return Err(Error::Journal(super::super::super::Error::Cancelled));
    }
    if s.owner != link.binding.owner {
        return Err(Error::Invalid);
    }
    if selected {
        if s.pending.is_some()
            || s.current
                != Some(Version {
                    revision: link.expected_revision + 2,
                    selection: link.target.clone(),
                })
        {
            return Err(Error::Stale);
        }
    } else if s.current != Some(link.prior.clone())
        || s.revision != link.expected_revision + u64::from(prepared)
        || if prepared {
            s.pending
                != Some(super::super::super::Pending {
                    reference: Reference {
                        journal: link.journal.clone(),
                        operation: prepare.clone(),
                    },
                    selection: link.target.clone(),
                })
        } else {
            s.pending.is_some()
        }
    {
        return Err(Error::Stale);
    }
    Ok(())
}
pub fn execute_validated(
    index: &mut Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    nodes: &mut metadata::MetadataStore,
    mut validate: impl FnMut(&Record) -> Result<()>,
    mut hook: impl FnMut(Stage) -> Result<()>,
) -> Result<Receipt> {
    let record = read(index, reference, nodes)?;
    let link = record.adoption.as_ref().ok_or(Error::Invalid)?;
    verify_binding(index, journal, link)?;
    let _fence = coordinated::lock(journal)?;
    let batch = record.batch().ok_or(Error::Invalid)?;
    let (p, pc, a, ac) = commands(index, link, &batch.operation, false)?;
    let logical_selected = journal.replay(a.clone(), ac.clone())?.is_some();
    let native = index.prepared(nodes, &batch.operation)?;
    if let Some(header) = &native {
        if header.request != batch.digest()? {
            return Err(Error::Conflict);
        }
        let cmd = Command::Adopt {
            source: key("authority"),
            owner: index.enrollment.owner.clone(),
            expected_revision: header.base_revision + 1,
            prepared: Reference {
                journal: index.identity().clone(),
                operation: op(index.identity(), &batch.operation, "prepare")?,
            },
        };
        if let Some(receipt) = index
            .journal
            .replay(op(index.identity(), &batch.operation, "adopt")?, cmd)?
        {
            if !logical_selected {
                return Err(Error::Invalid);
            }
            slot::finish(index, journal, reference, link, &batch.operation)?;
            hook(Stage::ReservationReleased)?;
            headroom::release_source(index, journal, reference, link)?;
            if index.requires_outcome_room() {
                hook(Stage::SourceRoomReleased)?;
            }
            mark_complete(index, reference)?;
            headroom::release_index(index, reference)?;
            if index.requires_outcome_room() {
                hook(Stage::IndexRoomReleased)?;
            }
            hook(Stage::Complete)?;
            return Ok(receipt);
        }
    }
    let logical_prepared = journal.replay(p.clone(), pc.clone())?.is_some();
    current(journal, link, logical_selected, logical_prepared, &p)?;
    let admitted = native.is_some();
    if !admitted && let Err(error) = validate(&record) {
        if !logical_prepared && !logical_selected {
            slot::abandon(index, journal, reference, link, &batch.operation)?;
            headroom::release_source(index, journal, reference, link)?;
            headroom::release_index(index, reference)?;
        }
        return Err(error);
    }
    headroom::reserve_source(index, journal, reference, link, &batch.operation)?;
    if index.requires_outcome_room() {
        hook(Stage::HeadroomReserved)?;
    }
    slot::reserve(index, journal, reference, link, &batch.operation)?;
    hook(Stage::Reserved)?;
    let mut may_prepare = false;
    let prepared = index.prepare_coordinated_hook(&batch, nodes, |s| {
        if !admitted && s == super::super::Stage::MetadataReady {
            validate(&record)?;
        }
        if s == super::super::Stage::MetadataReady {
            hook(Stage::MetadataReady)?;
            may_prepare = true;
        }
        Ok(())
    });
    if let Err(error) = prepared {
        if !admitted
            && !may_prepare
            && !logical_prepared
            && !logical_selected
            && matches!(index.prepared(nodes, &batch.operation), Ok(None))
        {
            slot::abandon(index, journal, reference, link, &batch.operation)?;
            headroom::release_source(index, journal, reference, link)?;
            headroom::release_index(index, reference)?;
        }
        return Err(error);
    }
    hook(Stage::ProtectionPrepared)?;
    command(journal, p, pc)?;
    hook(Stage::SourcePrepared)?;
    command(journal, a, ac)?;
    hook(Stage::SourceAdopted)?;
    current(journal, link, true, false, &key("unused"))?;
    let header = index
        .prepared(nodes, &batch.operation)?
        .ok_or(Error::Unknown)?;
    let receipt = index.resume_header_authorized(nodes, &header, &mut |_| Ok(()))?;
    hook(Stage::ProtectionSelected)?;
    slot::finish(index, journal, reference, link, &batch.operation)?;
    hook(Stage::ReservationReleased)?;
    headroom::release_source(index, journal, reference, link)?;
    if index.requires_outcome_room() {
        hook(Stage::SourceRoomReleased)?;
    }
    mark_complete(index, reference)?;
    headroom::release_index(index, reference)?;
    if index.requires_outcome_room() {
        hook(Stage::IndexRoomReleased)?;
    }
    hook(Stage::Complete)?;
    Ok(receipt)
}
