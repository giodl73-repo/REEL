//! Checked metadata publication only. No source authentication or payload authority.
use super::*;
use crate::{
    metadata::{Checkpoint as MetadataCheckpoint, MetadataStore},
    source_claims::{Command, Receipt, Reference},
};

#[derive(Clone, Debug)]
pub struct Ready {
    candidate: Candidate,
    history: History,
    provider_checkpoint: MetadataCheckpoint,
}
impl Ready {
    pub fn provider_checkpoint(&self) -> MetadataCheckpoint {
        self.provider_checkpoint.clone()
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preparation {
    New,
    Historical,
    Recovered,
}
#[derive(Clone, Debug)]
pub struct Prepared {
    ready: Ready,
    reference: Reference,
    revision: u64,
    origin: Preparation,
}
impl Prepared {
    pub fn reference(&self) -> Reference {
        self.reference.clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn origin(&self) -> &Preparation {
        &self.origin
    }
}
pub fn finish(
    candidate: Candidate,
    journal: &SourceJournal,
    nodes: &mut MetadataStore,
) -> Result<Ready> {
    if &candidate.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let provider_checkpoint = nodes.finish().map_err(Error::Provider)?;
    let history = load(
        journal.identity(),
        nodes,
        &candidate.source,
        &candidate.owner,
        &candidate.selection,
    )?;
    if history.checkpoint() != candidate.checkpoint {
        return Err(Error::Integrity);
    }
    Ok(Ready {
        candidate,
        history,
        provider_checkpoint,
    })
}
fn state(
    journal: &mut SourceJournal,
    candidate: &Candidate,
    revision: u64,
) -> Result<Option<Source>> {
    if &candidate.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let current = journal.source(&candidate.source).map_err(Error::Journal)?;
    if let Some(s) = &current {
        if s.owner != candidate.owner {
            return Err(Error::Owner);
        }
        if s.cancelled {
            return Err(Error::Cancelled);
        }
        if s.revision != revision {
            return Err(Error::Stale);
        }
    } else if revision != 0 {
        return Err(Error::Stale);
    }
    Ok(current)
}
fn lineage(
    journal: &SourceJournal,
    nodes: &impl NodeStore,
    ready: &Ready,
    source: Option<&Source>,
) -> Result<()> {
    if let Some(source) = source {
        let current = source.current.as_ref().ok_or(Error::Missing)?;
        let previous = load(
            journal.identity(),
            nodes,
            &ready.candidate.source,
            &ready.candidate.owner,
            &current.selection,
        )?;
        if ready.history.previous_checkpoint() != Some(previous.checkpoint())
            || !ready.history.same_stream(&previous)
            || ready.history.at().tick < previous.at().tick
        {
            return Err(Error::Lineage);
        }
    } else if ready.history.previous_checkpoint().is_some()
        || ready.history.checkpoint().sequence() != 0
    {
        return Err(Error::Lineage);
    }
    Ok(())
}
pub fn prepare(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    ready: Ready,
    expected_revision: u64,
    operation: Key,
) -> Result<Prepared> {
    if &ready.candidate.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let command = Command::Prepare {
        source: ready.candidate.source.clone(),
        owner: ready.candidate.owner.clone(),
        expected_revision,
        selection: ready.candidate.selection.clone(),
    };
    if let Some(receipt) = journal
        .replay(operation.clone(), command.clone())
        .map_err(Error::Journal)?
    {
        return Ok(Prepared {
            ready,
            reference: receipt.reference,
            revision: receipt.source_revision,
            origin: Preparation::Historical,
        });
    }
    let current = state(journal, &ready.candidate, expected_revision)?;
    if current.as_ref().is_some_and(|s| s.pending.is_some()) {
        return Err(Error::Pending);
    }
    load(
        journal.identity(),
        nodes,
        &ready.candidate.source,
        &ready.candidate.owner,
        &ready.candidate.selection,
    )?;
    lineage(journal, nodes, &ready, current.as_ref())?;
    let receipt = journal.apply(operation, command).map_err(Error::Journal)?;
    Ok(Prepared {
        ready,
        reference: receipt.reference,
        revision: receipt.source_revision,
        origin: if receipt.replayed {
            Preparation::Historical
        } else {
            Preparation::New
        },
    })
}
pub fn adopt(
    journal: &mut SourceJournal,
    nodes: &impl NodeStore,
    prepared: &Prepared,
    operation: Key,
) -> Result<Receipt> {
    let candidate = &prepared.ready.candidate;
    if &candidate.journal != journal.identity() {
        return Err(Error::Invalid);
    }
    let command = Command::Adopt {
        source: candidate.source.clone(),
        owner: candidate.owner.clone(),
        expected_revision: prepared.revision,
        prepared: prepared.reference.clone(),
    };
    if let Some(receipt) = journal
        .replay(operation.clone(), command.clone())
        .map_err(Error::Journal)?
    {
        return Ok(receipt);
    }
    let current = state(journal, candidate, prepared.revision)?.ok_or(Error::Missing)?;
    let pending = current.pending.as_ref().ok_or(Error::Pending)?;
    if pending.reference != prepared.reference || pending.selection != candidate.selection {
        return Err(Error::Stale);
    }
    load(
        journal.identity(),
        nodes,
        &candidate.source,
        &candidate.owner,
        &candidate.selection,
    )?;
    // A first publication has no selected predecessor, but must be initial.
    if current.current.is_some() {
        lineage(journal, nodes, &prepared.ready, Some(&current))?;
    } else {
        if current.revision != 1 || !current.predecessors.is_empty() {
            return Err(Error::Lineage);
        }
        lineage(journal, nodes, &prepared.ready, None)?;
    }
    journal.apply(operation, command).map_err(Error::Journal)
}
pub fn resume_pending(
    journal: &mut SourceJournal,
    nodes: &mut MetadataStore,
    source: &Key,
    owner: &Key,
    expected_revision: u64,
) -> Result<Prepared> {
    let current = journal
        .source(source)
        .map_err(Error::Journal)?
        .ok_or(Error::Missing)?;
    if &current.owner != owner {
        return Err(Error::Owner);
    }
    if current.cancelled {
        return Err(Error::Cancelled);
    }
    if current.revision != expected_revision {
        return Err(Error::Stale);
    }
    let pending = current.pending.as_ref().ok_or(Error::Pending)?;
    let history = load(journal.identity(), nodes, source, owner, &pending.selection)?;
    let candidate = Candidate {
        journal: journal.identity().clone(),
        source: source.clone(),
        owner: owner.clone(),
        selection: pending.selection.clone(),
        checkpoint: history.checkpoint(),
    };
    let ready = finish(candidate, journal, nodes)?;
    if current.current.is_some() {
        lineage(journal, nodes, &ready, Some(&current))?;
    } else {
        if current.revision != 1 || !current.predecessors.is_empty() {
            return Err(Error::Lineage);
        }
        lineage(journal, nodes, &ready, None)?;
    }
    let after = journal
        .source(source)
        .map_err(Error::Journal)?
        .ok_or(Error::Stale)?;
    if after != current {
        return Err(Error::Stale);
    }
    Ok(Prepared {
        ready,
        reference: pending.reference.clone(),
        revision: expected_revision,
        origin: Preparation::Recovered,
    })
}
