//! Original typed history blocks, followed only by acknowledged exact outcomes.
use super::{
    codec,
    model::{History, Recipe},
    session::Session,
    steps,
};
use tig_core::{Digest, Object};
use tig_store::retirement_intent::{
    ObservedIntent,
    progress::{
        self,
        history_work::{self, block},
    },
};

pub(super) fn identity(history: &History) -> Result<Digest, String> {
    Ok(Digest::blob(&codec::encode(&(
        "tig.native-retirement-history.experimental-1",
        history,
    ))?))
}
fn validate(history: &History, recipe: &Recipe, plan: &block::Block) -> Result<(), String> {
    let expected = history
        .objects
        .iter()
        .map(|object| {
            history_work::claim_id(
                &recipe.authorities.journal.identity,
                &history.source,
                &history.enrollment.owner,
                &history.predecessor,
                object,
            )
            .map(|claim| (object.clone(), claim))
            .map_err(|_| "invalid saved history claim")
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let actual: std::collections::BTreeMap<_, _> = plan
        .members()
        .iter()
        .map(|m| (m.object.clone(), m.claim.clone()))
        .collect();
    if expected != actual || actual.len() != plan.members().len() {
        return Err("saved history plan differs from selected exact group".into());
    }
    Ok(())
}
pub fn complete(
    session: &mut Session,
    recipe: &Recipe,
    ctx: &ObservedIntent,
) -> Result<bool, String> {
    for history in &recipe.history {
        for object in &history.objects {
            let claim = history_work::claim_id(
                &recipe.authorities.journal.identity,
                &history.source,
                &history.enrollment.owner,
                &history.predecessor,
                object,
            )
            .map_err(|_| "invalid history prerequisite")?;
            if !progress::inspect(&mut session.journal, &session.nodes, ctx, object)
                .map_err(|_| "history prerequisite unavailable")?
                .released
                .contains(&claim)
            {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

pub fn apply(
    session: &mut Session,
    recipe: &Recipe,
    root: &Object,
    ctx: &ObservedIntent,
) -> Result<(), String> {
    let mut stopped = false;
    for history in &recipe.history {
        let mut released = true;
        let mut cancelled = false;
        for object in &history.objects {
            let snapshot = progress::inspect(&mut session.journal, &session.nodes, ctx, object)
                .map_err(|_| "history progress unavailable")?;
            let claim = history_work::claim_id(
                &recipe.authorities.journal.identity,
                &history.source,
                &history.enrollment.owner,
                &history.predecessor,
                object,
            )
            .map_err(|_| "invalid exact history claim")?;
            released &= snapshot.released.contains(&claim);
            cancelled |= snapshot.cancelled;
        }
        if released {
            continue;
        }
        let input = identity(history)?;
        let plan = if let Some(plan) = steps::load::<block::Block>(session, recipe, root, &input)? {
            plan
        } else {
            if cancelled {
                // The controller acknowledges its immutable plan before any
                // native group admission. No saved plan means no such work may
                // be newly admitted after the parent's guarded cancellation.
                stopped = true;
                continue;
            }
            let plan = block::plan(
                &mut session.journal,
                &mut session.nodes,
                ctx,
                &history.source,
                history.predecessor.revision,
                &history.objects,
                &Session::enrollment(&history.enrollment),
            )
            .map_err(|_| "native history planning refused")?;
            validate(history, recipe, &plan)?;
            steps::save(session, recipe, root, &input, &plan)?;
            plan
        };
        validate(history, recipe, &plan)?;
        if cancelled {
            block::cancel_plan(&mut session.journal, &mut session.nodes, ctx, &plan).map_err(
                |_| "native history stop/report pending; exact group may already be fenced",
            )?;
            stopped = true;
            continue;
        }
        // Finish an uncertain action-record acknowledgement before native work.
        steps::save(session, recipe, root, &input, &plan)?;
        block::start(&mut session.journal, &mut session.nodes, ctx, plan.clone())
            .map_err(|_| "native history admission pending; resume saved plan")?;
        let anchor = plan.members().first().ok_or("empty saved history block")?;
        block::apply(
            &mut session.journal,
            &session.nodes,
            ctx,
            &anchor.object,
            anchor.attempt,
        )
        .map_err(|_| "native history effect pending; reconcile saved plan")?;
        block::report(
            &mut session.journal,
            &mut session.nodes,
            ctx,
            &anchor.object,
            anchor.attempt,
        )
        .map_err(|_| "native history reporting pending; reconcile saved plan")?;
    }
    if stopped {
        return Err(
            "cancelled history actions fenced and owned outcomes reconciled; no new work admitted"
                .into(),
        );
    }
    if !complete(session, recipe, ctx)? {
        return Err("history prerequisites not acknowledged".into());
    }
    Ok(())
}
