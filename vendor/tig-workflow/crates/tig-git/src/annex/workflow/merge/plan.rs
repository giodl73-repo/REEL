use super::*;
use crate::source::{GitId, Limits as GitLimits, Repository, Scope, protection_work};
use tig_core::retirement::review::{
    Copy, CopyState, Inventory, Kind, Obligation, Protection, Review,
    decision::Target,
    eviction::{Desired, Selection as Eviction},
};
pub(super) fn reader(c: &Config) -> io::Result<Repository> {
    Repository::new(
        c.caller.git.clone(),
        c.caller.repository.clone(),
        GitLimits {
            listing_bytes: 1048576,
            entries: 256,
            pointer_blobs: 128,
            command_millis: 30000,
        },
    )
    .map_err(|_| invalid())
}
pub(super) fn binding(c: &Config) -> protection_work::Binding {
    protection_work::Binding {
        repository: Digest::blob(c.caller.repository.to_string_lossy().as_bytes()),
        reference: c.reference.clone(),
        actor: c.current.clone(),
        owner: Key::parse("observed-local-ref").unwrap(),
        scope: c.scope.clone(),
    }
}
pub(super) fn current(c: &Config, commit: &str) -> io::Result<()> {
    if !c.reference.starts_with("refs/heads/") {
        return Err(invalid());
    }
    protection_work::validate_current(
        &reader(c)?,
        &binding(c),
        &GitId::parse(commit).map_err(|_| invalid())?,
    )
    .map_err(|_| invalid())
}
fn snapshot(
    c: &Config,
    commit: &str,
    nodes: &mut MetadataStore,
) -> io::Result<(Selection, BTreeMap<LogicalPath, Object>)> {
    let r = reader(c)?;
    let e = r
        .extract_annex(
            &r.commit(GitId::parse(commit).map_err(|_| invalid())?)
                .map_err(|_| invalid())?,
            Scope::new(c.scope.clone()).map_err(|_| invalid())?,
        )
        .map_err(|_| invalid())?;
    if !e.complete() || e.bindings().len() > 128 {
        return Err(invalid());
    }
    Ok((
        Selection {
            root: e
                .write_tree(nodes, tree_limits())
                .map_err(|_| invalid())?
                .root()
                .clone(),
            context: e.selection_digest().map_err(|_| invalid())?,
        },
        e.bindings()
            .iter()
            .map(|b| (b.path.clone(), b.object.clone()))
            .collect(),
    ))
}
pub(super) fn validate(c: &Config, index: &mut Index, nodes: &mut MetadataStore) -> io::Result<()> {
    if c.current == c.history
        || c.scope.is_empty()
        || !c.executable.is_absolute()
        || c.destinations.len() > 16
        || c.limits.objects == 0
        || c.limits.objects > 128
        || c.limits.actions == 0
        || c.limits.actions > 128
        || !(1..=3600000).contains(&c.limits.child_millis)
    {
        return Err(invalid());
    }
    let main = index.actor(nodes, &c.current).map_err(|_| invalid())?;
    let history = index.actor(nodes, &c.history).map_err(|_| invalid())?;
    if !main.ready || !history.ready || main.selection != snapshot(c, &c.initial_commit, nodes)?.0 {
        return Err(invalid());
    }
    let mut r = tig_store::source_claims::policy_registry::Registry::open(
        &c.policy.root,
        &c.policy.minimum,
    )
    .map_err(|_| invalid())?;
    if r.identity() != &c.policy.registry {
        return Err(invalid());
    }
    r.require_effect_index(index).map_err(|_| invalid())?;
    if r.effect_frame(index, nodes).map_err(|_| invalid())?.source
        != index.policy_source(&c.current).map_err(|_| invalid())?
    {
        return Err(invalid());
    }
    let held = r
        .hold(&c.policy.slot, Some(&c.policy.version), nodes)
        .map_err(|_| invalid())?;
    let cfg = held.policy().config();
    let main = &index
        .enrollment_profile()
        .actors
        .iter()
        .find(|a| a.id == c.current)
        .ok_or_else(invalid)?
        .stores;
    let history = &index
        .enrollment_profile()
        .actors
        .iter()
        .find(|a| a.id == c.history)
        .ok_or_else(invalid)?
        .stores;
    if !cfg.required_survivors.iter().all(|s| history.contains(s))
        || !cfg
            .removable
            .iter()
            .all(|s| main.contains(s) && !history.contains(s))
    {
        return Err(invalid());
    }
    let mut stores = BTreeSet::new();
    let mut remotes = BTreeSet::new();
    for d in &c.destinations {
        let b = d.enrollment.control.as_ref().ok_or_else(invalid)?;
        Remote::parse(&d.remote)?;
        if !stores.insert(b.store.clone())
            || !remotes.insert(d.remote.clone())
            || b.index != c.package.index
            || b.root != c.package.index_root
        {
            return Err(invalid());
        }
    }
    if stores != index.enrollment_profile().stores.iter().cloned().collect() {
        return Err(invalid());
    }
    current(c, &c.initial_commit)
}
pub(super) fn build(s: &mut Session, request: &Request) -> io::Result<Recipe> {
    let c = s.config()?;
    current(&c, &request.commit)?;
    let mut index = s.index()?;
    let actor = index
        .actor(&mut s.nodes, &c.current)
        .map_err(|_| invalid())?;
    let history = index
        .actor(&mut s.nodes, &c.history)
        .map_err(|_| invalid())?;
    if !actor.ready || !history.ready {
        return Err(invalid());
    }
    let (before, old) = snapshot(&c, &s.head.cursor, &mut s.nodes)?;
    let (after, new) = snapshot(&c, &request.commit, &mut s.nodes)?;
    if actor.selection != before {
        return Err(invalid());
    }
    let new_objects: BTreeSet<_> = new.values().cloned().collect();
    let historical = Tree::from_root(history.selection.root.clone());
    let mut cleanup = BTreeMap::new();
    for (path, object) in entries(&mut s.nodes, &s.head.deferred)? {
        if !new_objects.contains(&object)
            && historical
                .get(&path, &mut s.nodes, tree_limits())
                .map_err(|_| invalid())?
                .0
                == Some(object.clone())
        {
            cleanup.insert(path, object);
        }
    }
    for object in old.values() {
        if !new_objects.contains(object) {
            cleanup.insert(retired(object)?, object.clone());
        }
    }
    if new_objects
        .union(&cleanup.values().cloned().collect())
        .count()
        > c.limits.objects
    {
        return Err(invalid());
    }
    let cleanup_root = Tree::build(&cleanup, &mut s.nodes, tree_limits())
        .map_err(|_| invalid())?
        .0
        .root()
        .clone();
    let mut copies = BTreeSet::new();
    let profile = index.enrollment_profile();
    for (object, actor_id) in new_objects
        .iter()
        .map(|o| (o, &c.current))
        .chain(cleanup.values().map(|o| (o, &c.history)))
    {
        for store in &profile
            .actors
            .iter()
            .find(|a| &a.id == actor_id)
            .ok_or_else(invalid)?
            .stores
        {
            copies.insert((object.clone(), store.clone()));
        }
    }
    if copies.len() > c.limits.actions {
        return Err(invalid());
    }
    let total = copies
        .iter()
        .try_fold(0u64, |n, (o, _)| n.checked_add(o.bytes).ok_or_else(invalid))?;
    if total > c.limits.copy_bytes || total > c.limits.verification_bytes {
        return Err(invalid());
    }
    let tasks: Vec<_> = copies
        .into_iter()
        .map(|(object, store)| CopyTask { object, store })
        .collect();
    let copies = put(&mut s.nodes, &tasks)?;
    let mut changes = BTreeMap::new();
    for path in old.keys().chain(new.keys()) {
        if old.get(path) != new.get(path) {
            changes.insert(path.clone(), new.get(path).cloned());
        }
    }
    let mut history_changes = BTreeMap::new();
    for (path, object) in &cleanup {
        match historical
            .get(path, &mut s.nodes, tree_limits())
            .map_err(|_| invalid())?
            .0
        {
            Some(old) if old != *object => return Err(invalid()),
            None => {
                history_changes.insert(path.clone(), Some(object.clone()));
            }
            _ => (),
        }
    }
    if changes.len() + history_changes.len() > 128 {
        return Err(invalid());
    }
    let projection = if before != after {
        let next = historical
            .edit(
                historical.root(),
                &history_changes,
                &mut s.nodes,
                tree_limits(),
            )
            .map_err(|_| invalid())?
            .0;
        let op = key("projection", &(s.journal.identity(), &request.operation))?;
        let mut updates = vec![
            Update {
                operation: op.clone(),
                actor: c.current.clone(),
                owner: profile
                    .actors
                    .iter()
                    .find(|a| a.id == c.current)
                    .unwrap()
                    .owner
                    .clone(),
                expected_revision: actor.revision,
                selection: after.clone(),
                changes,
                ready: true,
            },
            Update {
                operation: op.clone(),
                actor: c.history.clone(),
                owner: profile
                    .actors
                    .iter()
                    .find(|a| a.id == c.history)
                    .unwrap()
                    .owner
                    .clone(),
                expected_revision: history.revision,
                selection: Selection {
                    root: next.root().clone(),
                    context: Digest::blob(&encode(&(SCHEMA, &request.operation, next.root()))?),
                },
                changes: history_changes,
                ready: true,
            },
        ];
        updates.sort_by(|a, b| a.actor.as_str().cmp(b.actor.as_str()));
        Some(put(
            &mut s.nodes,
            &Batch {
                operation: op,
                updates,
            },
        )?)
    } else {
        if !history_changes.is_empty() {
            return Err(invalid());
        }
        None
    };
    s.nodes.finish().map_err(|_| invalid())?;
    Ok(Recipe {
        before: s.head.cursor.clone(),
        after: request.commit.clone(),
        current_before: before,
        current_after: after,
        projection,
        copies,
        cleanup: cleanup_root,
        copy_bytes: total,
    })
}
pub(super) fn inventory(
    c: &Config,
    nodes: &mut MetadataStore,
    recipe: &Recipe,
) -> io::Result<Inventory> {
    let mut copies = Vec::new();
    let mut complete = true;
    for object in entries(nodes, &recipe.cleanup)?.into_values() {
        for d in &c.destinations {
            let b = d.enrollment.control.as_ref().ok_or_else(invalid)?;
            let state = match super::super::verify_present(
                &Runner::new(d.enrollment.clone())?,
                &AnnexKey::from_object(&object),
                c.limits.verification_bytes,
            ) {
                Ok(true) => CopyState::Verified {
                    evidence: Digest::blob(&encode(&(SCHEMA, &object, &b.store, "verified"))?),
                },
                Ok(false) => CopyState::Absent {
                    evidence: Digest::blob(&encode(&(SCHEMA, &object, &b.store, "absent"))?),
                },
                Err(_) => {
                    complete = false;
                    CopyState::Unknown
                }
            };
            copies.push(Copy {
                object: object.clone(),
                store: b.store.clone(),
                state,
            });
        }
    }
    Ok(Inventory {
        checkpoint: Digest::blob(&encode(&(SCHEMA, &recipe.cleanup, &copies))?),
        complete,
        stores: c
            .destinations
            .iter()
            .filter_map(|d| d.enrollment.control.as_ref().map(|b| b.store.clone()))
            .collect(),
        copies,
    })
}
pub(super) fn review(
    s: &mut Session,
    recipe: &Recipe,
    inventory: Inventory,
) -> io::Result<Option<Review>> {
    let c = s.config()?;
    let mut index = s.index()?;
    if index
        .actor(&mut s.nodes, &c.current)
        .map_err(|_| invalid())?
        .selection
        != recipe.current_after
    {
        return Err(invalid());
    }
    let mut r = tig_store::source_claims::policy_registry::Registry::open(
        &c.policy.root,
        &c.policy.minimum,
    )
    .map_err(|_| invalid())?;
    let held = r
        .hold(&c.policy.slot, Some(&c.policy.version), &s.nodes)
        .map_err(|_| invalid())?;
    let bindings = entries(&mut s.nodes, &recipe.cleanup)?;
    if bindings.is_empty() {
        return Ok(None);
    }
    let mut obligations = Vec::new();
    for object in bindings.values() {
        for target in &held.policy().config().removable {
            for actor in index
                .inspect(&mut s.nodes, object, target)
                .map_err(|_| invalid())?
                .protected_by
            {
                obligations.push(Obligation {
                    claim: Digest::blob(&encode(&(SCHEMA, &actor, object, target))?),
                    object: object.clone(),
                    owner: index.enrollment_profile().subject.clone(),
                    kind: Kind::Ordinary,
                });
            }
        }
    }
    let places: BTreeMap<_, _> = held
        .policy()
        .config()
        .removable
        .iter()
        .map(|d| (d.as_str().to_owned(), d.clone()))
        .collect();
    let desired = Desired {
        checkpoint: Digest::blob(&encode(&(SCHEMA, &recipe.cleanup, "desired"))?),
        complete: true,
        placements: bindings
            .values()
            .flat_map(|o| {
                held.policy()
                    .config()
                    .required_survivors
                    .iter()
                    .map(move |store| Target {
                        object: o.clone(),
                        store: store.clone(),
                    })
            })
            .collect(),
    };
    let selection = Eviction::new(
        recipe.cleanup.sha256.clone(),
        held.policy().config().placement_policy.clone(),
        bindings,
        places.keys().cloned().collect(),
        desired,
    )
    .map_err(|_| invalid())?;
    let review = Review::create_eviction(
        selection,
        index.enrollment_profile().subject.clone(),
        places,
        Protection {
            checkpoint: Digest::blob(&encode(&index.checkpoint())?),
            complete: true,
            obligations,
        },
        inventory,
        BTreeSet::new(),
    )
    .map_err(|_| invalid())?;
    Ok(Some(review))
}
