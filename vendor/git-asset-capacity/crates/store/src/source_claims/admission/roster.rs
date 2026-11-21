//! Explicit roster-wide advisory precedence; no authority or independent journal.
use super::*;

fn raw(
    ns: &mut Namespace,
    work: &mut Work<'_, impl NodeStore>,
) -> Result<(Checkpoint, BTreeMap<Digest, Demand>)> {
    ns.journal.check_guarded_marker()?;
    ns.journal.sync()?;
    let checkpoint = ns.journal.checkpoint();
    let mut all = BTreeMap::<Digest, Demand>::new();
    for source in ns.journal.state.sources.values() {
        for (id, demand) in work.source(source, ns.policy())? {
            match all.entry(id) {
                std::collections::btree_map::Entry::Vacant(e) => {
                    e.insert(demand);
                }
                std::collections::btree_map::Entry::Occupied(mut e) => {
                    e.get_mut().extend(&demand)?;
                }
            }
        }
    }
    ns.journal.check_guarded_marker()?;
    work.timed()?;
    Ok((checkpoint, all))
}

fn required(demand: &Demand, policy: &Policy, place: &str, active: bool) -> bool {
    demand.retained.contains(place)
        || demand.active.contains(place)
        || ((!policy.prefer_active || !active) && demand.history.contains(place))
}

/// Read-only planning. Callers separately admit the exact complete roster.
pub(crate) fn requirements(
    namespaces: &mut [Namespace],
    work: &mut Work<'_, impl NodeStore>,
) -> Result<Vec<Requirements>> {
    let mut snapshots = Vec::new();
    let mut active = BTreeSet::new();
    let mut sizes = BTreeMap::new();
    for ns in namespaces.iter_mut() {
        let (checkpoint, demands) = raw(ns, work)?;
        for (id, demand) in &demands {
            if sizes
                .insert(id.clone(), demand.bytes)
                .is_some_and(|n| n != demand.bytes)
            {
                return Err(Error::ConflictingSize);
            }
            if !demand.active.is_empty() {
                active.insert(id.clone());
            }
        }
        snapshots.push((checkpoint, demands));
    }
    let mut result = Vec::new();
    for (ns, (checkpoint, demands)) in namespaces.iter_mut().zip(snapshots) {
        ns.journal.check_guarded_marker()?;
        ns.journal.sync()?;
        if ns.journal.checkpoint() != checkpoint {
            return Err(Error::WrongAuthority);
        }
        let policy = ns.policy();
        let keys = policy
            .classes
            .values()
            .flat_map(|p| p.current.iter().chain(&p.pending).chain(&p.predecessor))
            .map(|p| p.as_str())
            .collect::<BTreeSet<_>>();
        let mut entries = Vec::new();
        for (id, demand) in demands {
            let places = keys
                .iter()
                .filter(|p| required(&demand, policy, p, active.contains(&id)))
                .map(|p| Key::parse(p).map_err(Error::from))
                .collect::<Result<Vec<_>>>()?;
            entries.push(Assignment {
                object: Object {
                    sha256: id,
                    bytes: demand.bytes,
                },
                places,
            });
        }
        result.push(Requirements {
            namespace: ns.identity().clone(),
            checkpoint,
            entries,
        });
    }
    work.timed()?;
    Ok(result)
}

/// All subject fences must already be held. Admission fences raw bucket changes,
/// including loss of the other namespace's active use that justifies suppression.
pub(crate) fn absent(
    namespaces: &mut [Namespace],
    order: &[usize],
    subjects: &[SubjectFence],
    places: &[Vec<Key>],
    work: &mut Work<'_, impl NodeStore>,
) -> Result<Option<Vec<Checkpoint>>> {
    if order.len() != subjects.len() || order.len() != places.len() {
        return Err(Error::WrongAuthority);
    }
    let mut snapshots = Vec::new();
    let mut active = false;
    for ((index, fence), keys) in order.iter().zip(subjects).zip(places) {
        let ns = namespaces.get_mut(*index).ok_or(Error::WrongAuthority)?;
        if fence.authority != *ns.identity()
            || keys.is_empty()
            || keys.len() > 16
            || !keys.iter().all(|key| {
                ns.policy().classes.values().any(|p| {
                    p.current
                        .iter()
                        .chain(&p.pending)
                        .chain(&p.predecessor)
                        .any(|k| k == key)
                })
            })
        {
            return Err(Error::WrongAuthority);
        }
        let (checkpoint, mut demands) = raw(ns, work)?;
        let demand = demands
            .remove(&fence.object.sha256)
            .unwrap_or_else(|| Demand {
                bytes: fence.object.bytes,
                ..Demand::default()
            });
        if demand.bytes != fence.object.bytes {
            return Err(Error::ConflictingSize);
        }
        active |= !demand.active.is_empty();
        snapshots.push((checkpoint, demand));
    }
    for ((index, keys), (_, demand)) in order.iter().zip(places).zip(&snapshots) {
        let ns = &namespaces[*index];
        ns.journal.check_guarded_marker()?;
        if keys
            .iter()
            .any(|p| required(demand, ns.policy(), p.as_str(), active))
        {
            return Ok(None);
        }
    }
    work.timed()?;
    Ok(Some(
        snapshots
            .into_iter()
            .map(|(checkpoint, _)| checkpoint)
            .collect(),
    ))
}
