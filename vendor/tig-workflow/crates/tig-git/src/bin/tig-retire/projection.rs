//! Exact negative index work, admitted only after selected history acknowledgements.
use super::{
    codec, history,
    model::{Projection, Recipe},
    session::Session,
    steps,
};
use tig_core::{
    Digest, Object,
    tree::{Tree, TreeLimits},
};
use tig_store::{
    Key,
    retirement_intent::ObservedIntent,
    source_claims::{
        Selection,
        protection::{self, Update, work},
    },
};

fn identity(projection: &Projection) -> Result<Digest, String> {
    Ok(Digest::blob(&codec::encode(&(
        "tig.native-retirement-projection.experimental-1",
        projection,
    ))?))
}
fn update(projection: &Projection, root: &Object, session: &mut Session) -> Result<Update, String> {
    if projection.remove.is_empty() || projection.remove.len() > protection::MAX_CHANGES {
        return Err("invalid projection change bound".into());
    }
    let tree = Tree::from_root(projection.prior.root.clone());
    let mut reads = super::read_scope::ReadScope::new(&session.nodes, TreeLimits::default());
    for (path, object) in &projection.remove {
        if tree
            .get(path, &mut reads, TreeLimits::default())
            .map_err(|_| "projection prior path unavailable")?
            .0
            .as_ref()
            != Some(object)
        {
            return Err("projection prior binding changed".into());
        }
    }
    let changes = projection
        .remove
        .keys()
        .map(|p| (p.clone(), None))
        .collect();
    let (next, _) = tree
        .edit(
            tree.root(),
            &changes,
            &mut session.nodes,
            TreeLimits::default(),
        )
        .map_err(|_| "bounded exact projection edit refused")?;
    let context = Digest::blob(&codec::encode(&(
        "tig.native-retirement-projection-context.experimental-1",
        root,
        projection,
    ))?);
    let operation = Key::parse(&format!("retire-projection-{}", context.as_str()))
        .map_err(|_| "invalid projection operation")?;
    Ok(Update {
        operation,
        actor: projection.actor.clone(),
        owner: projection.owner.clone(),
        expected_revision: projection.expected_revision,
        selection: Selection {
            root: next.root().clone(),
            context,
        },
        changes,
        ready: true,
    })
}
pub fn apply(
    session: &mut Session,
    recipe: &Recipe,
    root: &Object,
    ctx: &ObservedIntent,
) -> Result<(), String> {
    if !history::complete(session, recipe, ctx)? {
        return Err("history outcomes must be acknowledged before projection admission".into());
    }
    for projection in &recipe.projections {
        let input = identity(projection)?;
        let saved = steps::load::<Update>(session, recipe, root, &input)?;
        if saved.is_none() {
            let parent =
                Key::parse(recipe.operation.as_str()).map_err(|_| "invalid parent intent")?;
            if session
                .journal
                .source(&parent)
                .map_err(|_| "parent intent unavailable")?
                .is_none_or(|state| state.cancelled)
            {
                return Err("cancelled intent stops new projection preparation".into());
            }
        }
        let expected = update(projection, root, session)?;
        let update = if let Some(saved) = saved {
            if codec::encode(&saved)? != codec::encode(&expected)? {
                return Err("saved projection differs from exact recipe".into());
            }
            saved
        } else {
            // Check the original actor snapshot before admitting this exact
            // controller action. Never rebase onto a newer actor root/revision.
            let state = session
                .index
                .actor(&mut session.nodes, &projection.actor)
                .map_err(|_| "projection actor unavailable")?;
            if !state.ready
                || state.revision != projection.expected_revision
                || state.selection != projection.prior
            {
                return Err("projection actor changed since review".into());
            }
            expected
        };
        // Parent cancellation and this exact action admission share the same
        // original journal's atomic AdoptGuarded operation. Exact replay after
        // cancellation may finish this already-admitted action, never new work.
        steps::save(session, recipe, root, &input, &update)?;
        let reference = if let Some(reference) = work::lookup(&mut session.index, &update.operation)
            .map_err(|_| "projection work lookup unavailable")?
        {
            let record = work::read(&mut session.index, &reference, &session.nodes)
                .map_err(|_| "saved projection work unavailable")?;
            if record.additional.is_some()
                || record.adoption.is_some()
                || record.evidence.as_ref() != Some(root)
                || codec::encode(&record.update)? != codec::encode(&update)?
            {
                return Err("foreign projection work under original operation".into());
            }
            reference
        } else {
            work::stage(
                &mut session.index,
                &update,
                Some(root.clone()),
                &mut session.nodes,
            )
            .map_err(|_| "native projection staging refused")?
        };
        work::execute(&mut session.index, &reference, &mut session.nodes)
            .map_err(|_| "native projection pending; reconcile exact saved work")?;
        if !work::complete(&mut session.index, &reference)
            .map_err(|_| "projection acknowledgement unavailable")?
        {
            return Err("projection not acknowledged".into());
        }
    }
    Ok(())
}
