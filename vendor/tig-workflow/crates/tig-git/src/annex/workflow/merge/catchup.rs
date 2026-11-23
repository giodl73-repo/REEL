//! Settle one superseded operation, then drive the exact current local commit.
//! The existing source journal and effect reconciler own all durable state.
use super::*;
fn scope(s: &Session) -> io::Result<Digest> {
    Ok(Digest::blob(&encode(&(
        SCHEMA,
        &s.handle.journal,
        "source-controller",
    ))?))
}
fn blocked(
    s: &Session,
    r: &Request,
    old: &Active,
    outcomes: Vec<retirement::Report>,
) -> io::Result<Report> {
    let recipe: Recipe = read(&s.nodes, &old.recipe)?;
    let mut result = report(s, r, Status::Pending, outcomes, &recipe.cleanup)?;
    result.blocked_by = Some(old.operation.clone());
    Ok(result)
}
pub(super) fn execute(request: &Request, mode: Mode) -> io::Result<Report> {
    let _lock = root_lock(&request.handle.root)?;
    let mut s = Session::open(&request.handle)?;
    // Completed replay precedes every provider/ref/controller operation.
    if let Some(saved) = s.saved(&request.operation)? {
        let recipe: Recipe = read(&s.nodes, &saved.recipe)?;
        if recipe.after != request.commit {
            return Err(invalid());
        }
        if saved.terminal.is_some() {
            return finalize(&mut s, request, &saved, true);
        }
        if s.head
            .active
            .as_ref()
            .is_none_or(|a| a.operation != request.operation)
        {
            return Err(invalid());
        }
    }
    if matches!(mode, Mode::Inspect) {
        staging::advance(&mut s, request)?;
        if let Some(old) = s
            .head
            .active
            .clone()
            .filter(|a| a.operation != request.operation)
        {
            let original = Request {
                handle: s.handle(),
                operation: old.operation.clone(),
                commit: old.commit.clone(),
            };
            if let Some(saved) = s.saved(&old.operation)? {
                s.handle.index_minimum = stronger(&s.handle.index_minimum, &saved.index_minimum)?;
                s.handle.policy_minimum =
                    stronger(&s.handle.policy_minimum, &saved.policy_minimum)?;
            }
            staging::advance(&mut s, &original)?;
            return blocked(&s, request, &old, vec![]);
        }
        return report(&s, request, Status::Pending, vec![], &s.head.deferred);
    }
    let prior = s
        .head
        .active
        .clone()
        .filter(|a| a.operation != request.operation);
    let lease = if let Some(old) = &prior {
        if old.commit == request.commit || matches!(mode, Mode::Cancel) {
            return Err(invalid());
        }
        // Refuse a delayed/foreign event before stopping or canceling old work.
        // The held exact new local ref spans old-work settlement, not bulk copy.
        Some(hold_ref(&s.enrolled.config, &request.commit)?)
    } else {
        if s.head
            .active
            .as_ref()
            .is_some_and(|a| a.commit != request.commit)
        {
            return Err(invalid());
        }
        None
    };
    // Recover the earlier per-operation containment name for existing roots.
    if let Some(active) = &s.head.active {
        let legacy = Digest::blob(&encode(&(SCHEMA, &s.handle.journal, &active.operation))?);
        Controller::stop(&legacy, 5000)?;
    }
    // One source-scoped controller covers both old reconciliation and new work.
    // Stable scope also fences a failed catch-up whose new operation wasn't saved.
    let scope = scope(&s)?;
    Controller::stop(&scope, 5000)?;
    let controller = Controller::enter(&scope)?;
    if let Some(old) = prior {
        let original = Request {
            handle: s.handle(),
            operation: old.operation.clone(),
            commit: old.commit.clone(),
        };
        let result = drive(&mut s, &original, Mode::Cancel, &controller);
        if controller.children_stopped(5000).is_err() {
            std::mem::forget(lease);
            return blocked(
                &s,
                request,
                &old,
                result.map(|v| v.outcomes).unwrap_or_default(),
            );
        }
        // A valid current request cannot proceed while old settlement is
        // unproved, including provider/control IO errors. This is no success
        // claim about the older operation and creates no new execution work.
        let result = match result {
            Ok(value) => value,
            Err(_) => return blocked(&s, request, &old, vec![]),
        };
        if matches!(result.status, Status::Pending) || s.head.active.is_some() {
            return blocked(&s, request, &old, result.outcomes);
        }
    }
    drop(lease);
    drive(&mut s, request, mode, &controller)
}
