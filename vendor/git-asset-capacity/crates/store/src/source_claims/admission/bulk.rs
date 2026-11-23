//! Read-only hypothetical history projection and exact owned-command validation.
use super::*;

pub(crate) struct Exclusion {
    pub source: Key,
    pub owner: Key,
    pub predecessor: u64,
    pub original: super::super::Selection,
    pub paths: BTreeSet<LogicalPath>,
}

pub(crate) fn snapshot(namespace: &mut Namespace) -> Result<Checkpoint> {
    namespace.journal.check_guarded_marker()?;
    namespace.journal.sync()?;
    Ok(namespace.checkpoint())
}

/// Computes remaining membership in one captured journal snapshot, not a lease.
/// All original closure IO is charged, including hypothetically removed bindings.
pub(crate) fn remaining(
    namespace: &mut Namespace,
    exclusions: &[Exclusion],
    candidates: &BTreeMap<Digest, Object>,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<(Checkpoint, BTreeSet<Digest>)> {
    remaining_with_limit(namespace, exclusions, candidates, work, 1024)
}

/// Separate larger-selection preview. Its caller bounds the aggregate selected
/// binding inventory. This is hypothetical protection, never removal authority.
pub(crate) fn remaining_selected(
    namespace: &mut Namespace,
    exclusions: &[Exclusion],
    candidates: &BTreeMap<Digest, Object>,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<(Checkpoint, BTreeSet<Digest>)> {
    let max_paths = work.limits.bindings;
    remaining_with_limit(namespace, exclusions, candidates, work, max_paths)
}
fn remaining_with_limit(
    namespace: &mut Namespace,
    exclusions: &[Exclusion],
    candidates: &BTreeMap<Digest, Object>,
    work: &mut Work<'_, impl NodeStore>,
    max_paths: usize,
) -> Result<(Checkpoint, BTreeSet<Digest>)> {
    let checkpoint = snapshot(namespace)?;
    let sources = namespace.journal.state.sources.clone();
    let mut removed = BTreeMap::<String, BTreeMap<u64, BTreeSet<LogicalPath>>>::new();
    for exclusion in exclusions {
        let source = sources
            .get(exclusion.source.as_str())
            .ok_or(Error::WrongAuthority)?;
        if source.owner != exclusion.owner
            || source.pending.is_some()
            || !matches!(source.kind, Some(Kind::Branch | Kind::Draft))
            || !source
                .predecessors
                .iter()
                .any(|v| v.revision == exclusion.predecessor && v.selection == exclusion.original)
            || exclusion.paths.is_empty()
            || exclusion.paths.len() > max_paths
            || removed
                .entry(exclusion.source.as_str().into())
                .or_default()
                .insert(exclusion.predecessor, exclusion.paths.clone())
                .is_some()
        {
            return Err(Error::WrongAuthority);
        }
    }
    let mut protected = BTreeSet::new();
    for (key, source) in &sources {
        let empty = BTreeMap::new();
        for (id, demand) in work.source_excluding(
            source,
            namespace.policy(),
            removed.get(key).unwrap_or(&empty),
        )? {
            if let Some(object) = candidates.get(&id) {
                if object.bytes != demand.bytes {
                    return Err(Error::ConflictingSize);
                }
                protected.insert(id);
            }
        }
    }
    namespace.journal.check_guarded_marker()?;
    work.timed()?;
    Ok((checkpoint, protected))
}

/// Call with a refreshed namespace under final clearance's live subject fence.
/// Known history through that snapshot may contain only these exact commands.
pub(crate) fn only_owned_since(
    namespace: &Namespace,
    minimum: &Checkpoint,
    allowed: &[(Key, Command)],
) -> Result<bool> {
    namespace.verify_minimum(minimum)?;
    let head = namespace.checkpoint();
    let span = head
        .sequence()
        .checked_sub(minimum.sequence())
        .ok_or(Error::WrongAuthority)?;
    if span > allowed.len() as u64 {
        return Ok(false);
    }
    let mut query = namespace
        .journal
        .conn
        .prepare(
            "SELECT length(body),body FROM events WHERE sequence>?1 AND sequence<=?2 ORDER BY sequence LIMIT ?3",
        )
        .map_err(super::super::Error::from)?;
    let mut rows = query
        .query(rusqlite::params![
            i64::try_from(minimum.sequence()).map_err(|_| Error::Budget)?,
            i64::try_from(head.sequence()).map_err(|_| Error::Budget)?,
            i64::try_from(super::super::MAX_COMMANDS).map_err(|_| Error::Budget)?
        ])
        .map_err(super::super::Error::from)?;
    let mut count = 0;
    let mut bytes = 0u64;
    while let Some(row) = rows.next().map_err(super::super::Error::from)? {
        let length: i64 = row.get(0).map_err(super::super::Error::from)?;
        if !(0..=65_536).contains(&length) {
            return Err(Error::Budget);
        }
        bytes = bytes.checked_add(length as u64).ok_or(Error::Budget)?;
        if bytes > super::super::MAX_COMMAND_BYTES {
            return Err(Error::Budget);
        }
        let raw: Vec<u8> = row.get(1).map_err(super::super::Error::from)?;
        if raw.len() as i64 != length {
            return Err(Error::WrongAuthority);
        }
        let request: super::super::Request = super::super::decode(&raw)?;
        if !allowed
            .iter()
            .any(|(op, command)| op == &request.operation && command == &request.command)
        {
            return Ok(false);
        }
        count += 1;
    }
    if count != span {
        return Err(Error::WrongAuthority);
    }
    Ok(true)
}
