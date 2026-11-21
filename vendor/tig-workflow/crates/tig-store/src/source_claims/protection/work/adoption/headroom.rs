//! Pre-admission room for every exact post-admission journal command.
use super::*;
use crate::source_claims::{Source, outcome_budget as room};
fn allocation(index: &Index, reference: &WorkRef) -> Result<Key> {
    Ok(key(&format!(
        "outcomes-{}",
        Digest::blob(&encode(&(DOMAIN, index.identity(), &reference.root))?).as_str()
    )))
}
fn plan(
    mut steps: Vec<room::Step>,
    admissions: Vec<room::Admission>,
    mut endings: Vec<Vec<Key>>,
) -> room::Plan {
    steps.sort_by(|a, b| a.operation.as_str().cmp(b.operation.as_str()));
    for end in &mut endings {
        end.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    }
    room::Plan {
        steps,
        admissions,
        endings,
    }
}
fn step(operation: Key, command: Command) -> room::Step {
    room::Step {
        operation,
        command: Box::new(command),
    }
}
pub(super) fn reserve_source(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
    operation: &Key,
) -> Result<()> {
    if !index.requires_outcome_room() {
        return Ok(());
    }
    let id = allocation(index, reference)?;
    if let Some(r) = room::lookup(journal, &id, &link.binding.owner)?
        && !room::active(journal, &r)?
    {
        return Err(Error::Conflict);
    }
    let (sp, spc, sa, sac) =
        slot::recipe(index, journal, reference, link, operation, true)?.ok_or(Error::Invalid)?;
    let (p, pc, a, ac) = commands(index, link, operation, false)?;
    let (stop, stopc) = slot::abandon_command(index, link, operation, &sp, &spc)?;
    let endings = vec![
        vec![sp.clone(), p.clone(), a.clone(), sa.clone()],
        vec![sp.clone(), stop.clone()],
    ];
    room::reserve(
        journal,
        id,
        link.binding.owner.clone(),
        plan(
            vec![
                step(sp, spc),
                step(p, pc),
                step(a, ac),
                step(sa, sac),
                step(stop, stopc),
            ],
            vec![],
            endings,
        ),
    )?;
    Ok(())
}
pub(in crate::source_claims::protection) fn reserve_index(
    index: &mut Index,
    operation: &Key,
    source: &Source,
    nodes: &impl NodeStore,
) -> Result<()> {
    let reference = lookup(index, operation)?.ok_or(Error::Unknown)?;
    let record = read(index, &reference, nodes)?;
    record.adoption.as_ref().ok_or(Error::Invalid)?;
    let id = allocation(index, &reference)?;
    let owner = index.enrollment.owner.clone();
    let p = op(index.identity(), operation, "prepare")?;
    let a = op(index.identity(), operation, "adopt")?;
    let old = source.current.as_ref().ok_or(Error::Unknown)?;
    if !source.predecessors.is_empty() {
        return Err(Error::Invalid);
    }
    let trim = key(&format!("trim-{}-{}", source.revision + 2, old.revision));
    let complete = op(index.identity(), &reference.source, "work-adopt")?;
    let native = step(
        a.clone(),
        Command::Adopt {
            source: key("authority"),
            owner: owner.clone(),
            expected_revision: source.revision + 1,
            prepared: Reference {
                journal: index.identity().clone(),
                operation: p.clone(),
            },
        },
    );
    let retire = step(
        trim.clone(),
        Command::Retire {
            source: key("authority"),
            owner: owner.clone(),
            expected_revision: source.revision + 2,
            predecessor_revision: old.revision,
        },
    );
    let marker = step(
        complete.clone(),
        Command::Adopt {
            source: reference.source.clone(),
            owner: owner.clone(),
            expected_revision: 1,
            prepared: Reference {
                journal: index.identity().clone(),
                operation: op(index.identity(), &reference.source, "work-prepare")?,
            },
        },
    );
    let admission = room::Admission {
        operation: p,
        source: key("authority"),
        owner: owner.clone(),
        revision: source.revision,
    };
    room::reserve_mode(
        &mut index.journal,
        id,
        owner,
        plan(
            vec![native, retire, marker],
            vec![admission],
            vec![vec![a, trim, complete]],
        ),
        true,
    )?;
    Ok(())
}
pub(super) fn release_source(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
) -> Result<()> {
    if index.requires_outcome_room()
        && let Some(r) = room::lookup(journal, &allocation(index, reference)?, &link.binding.owner)?
    {
        room::release(journal, &r)?;
    }
    Ok(())
}
pub(super) fn release_index(index: &mut Index, reference: &WorkRef) -> Result<()> {
    if !index.requires_outcome_room() {
        return Ok(());
    }
    let id = allocation(index, reference)?;
    if let Some(r) = room::lookup(&mut index.journal, &id, &index.enrollment.owner)? {
        room::release_mode(&mut index.journal, &r, true)?;
    }
    Ok(())
}
