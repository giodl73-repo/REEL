//! One bounded pending source reservation; exact commands remain in its journal.
use super::*;
use rusqlite::OptionalExtension;
fn selection(reference: &WorkRef) -> Result<Selection> {
    Ok(Selection {
        root: reference.root.clone(),
        context: Digest::blob(&encode(&(DOMAIN, "source-slot", &reference.source))?),
    })
}
fn stored(journal: &mut SourceJournal, operation: &Key) -> Result<Option<Command>> {
    journal.sync()?;
    let body: Option<Vec<u8>> = journal
        .conn
        .query_row(
            "SELECT body FROM events WHERE operation=?1",
            [operation.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(crate::source_claims::Error::from)?;
    body.map(|body| {
        let request: crate::source_claims::Request = crate::source_claims::decode(&body)?;
        if request.operation != *operation {
            return Err(Error::Invalid);
        }
        Ok(request.command)
    })
    .transpose()
}
pub(super) fn recipe(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
    operation: &Key,
    create: bool,
) -> Result<Option<(Key, Command, Key, Command)>> {
    let source = coordinated::control(&link.binding)?;
    let wanted = selection(reference)?;
    let p = id(
        index.identity(),
        &link.journal,
        &source,
        operation,
        "reserve",
    )?;
    let a = id(
        index.identity(),
        &link.journal,
        &source,
        operation,
        "release",
    )?;
    let expected = if let Some(command) = stored(journal, &p)? {
        match command {
            Command::Prepare {
                source: found,
                owner,
                expected_revision,
                selection,
            } if found == source && owner == link.binding.owner && selection == wanted => {
                expected_revision
            }
            _ => return Err(Error::Conflict),
        }
    } else {
        if !create {
            return Ok(None);
        }
        while let Some(state) = journal.source(&source)? {
            if state.owner != link.binding.owner || state.cancelled {
                return Err(Error::Invalid);
            }
            if state.pending.is_some() {
                return Err(Error::Pending);
            }
            let Some(old) = state.predecessors.first() else {
                break;
            };
            let trim = id(
                index.identity(),
                &link.journal,
                &source,
                &key(&format!("trim-{}-{}", state.revision, old.revision)),
                "trim",
            )?;
            command(
                journal,
                trim,
                Command::Retire {
                    source: source.clone(),
                    owner: link.binding.owner.clone(),
                    expected_revision: state.revision,
                    predecessor_revision: old.revision,
                },
            )?;
        }
        journal.source(&source)?.map_or(0, |s| s.revision)
    };
    let next = expected.checked_add(1).ok_or(Error::Budget)?;
    Ok(Some((
        p.clone(),
        Command::Prepare {
            source: source.clone(),
            owner: link.binding.owner.clone(),
            expected_revision: expected,
            selection: wanted,
        },
        a,
        Command::Adopt {
            source,
            owner: link.binding.owner.clone(),
            expected_revision: next,
            prepared: Reference {
                journal: link.journal.clone(),
                operation: p,
            },
        },
    )))
}
pub(super) fn reserve(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
    operation: &Key,
) -> Result<()> {
    let (p, pc, _, _) =
        recipe(index, journal, reference, link, operation, true)?.ok_or(Error::Invalid)?;
    let receipt = command(journal, p, pc)?;
    let source = coordinated::control(&link.binding)?;
    let s = journal.source(&source)?.ok_or(Error::Unknown)?;
    if s.pending
        != Some(crate::source_claims::Pending {
            reference: receipt.reference,
            selection: selection(reference)?,
        })
    {
        return Err(Error::Stale);
    }
    Ok(())
}
pub(super) fn finish(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
    operation: &Key,
) -> Result<()> {
    let (_, _, a, ac) =
        recipe(index, journal, reference, link, operation, false)?.ok_or(Error::Unknown)?;
    command(journal, a, ac)?;
    Ok(())
}
pub(super) fn abandon(
    index: &Index,
    journal: &mut SourceJournal,
    reference: &WorkRef,
    link: &Link,
    operation: &Key,
) -> Result<()> {
    let Some((p, pc, _, _)) = recipe(index, journal, reference, link, operation, false)? else {
        return Ok(());
    };
    let (operation, cmd) = abandon_command(index, link, operation, &p, &pc)?;
    command(journal, operation, cmd)?;
    Ok(())
}
pub(super) fn abandon_command(
    index: &Index,
    link: &Link,
    operation: &Key,
    p: &Key,
    pc: &Command,
) -> Result<(Key, Command)> {
    let Command::Prepare {
        source,
        expected_revision,
        ..
    } = pc
    else {
        return Err(Error::Invalid);
    };
    let operation = id(
        index.identity(),
        &link.journal,
        source,
        operation,
        "abandon",
    )?;
    Ok((
        operation,
        Command::Abort {
            source: source.clone(),
            owner: link.binding.owner.clone(),
            expected_revision: expected_revision.checked_add(1).ok_or(Error::Budget)?,
            prepared: Reference {
                journal: link.journal.clone(),
                operation: p.clone(),
            },
        },
    ))
}
