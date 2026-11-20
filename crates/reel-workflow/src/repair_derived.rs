//! Bounded dependency-edge correspondence. No payload I/O, custody or effects.
use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResultWitness {
    pub packet: Packet,
    pub result: ReturnedResult,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ParentChange {
    pub before: ResultWitness,
    pub after: ResultWitness,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConsumerBindings {
    pub before: upstream::Bindings,
    pub after: upstream::Bindings,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Derivation {
    pub parents: BTreeMap<String, ParentChange>,
    pub consumers: BTreeMap<String, ConsumerBindings>,
}
fn task<'a>(plan: &'a Plan, id: &str) -> Result<&'a Task> {
    plan.tasks.iter().find(|t| t.id == id).ok_or(Error {
        kind: ErrorKind::Missing,
        code: "repair-derived-task".into(),
    })
}
fn dependencies(
    task: &Task,
    parents: &BTreeMap<String, ParentChange>,
    after: bool,
) -> Result<BTreeMap<String, DependencyResult>> {
    task.depends_on
        .iter()
        .map(|id| {
            let pair = parents.get(id).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "repair-derived-parent".into(),
            })?;
            let witness = if after { &pair.after } else { &pair.before };
            Ok((
                id.clone(),
                DependencyResult {
                    result_sha256: canonical_digest(&witness.result)?,
                    artifacts: witness.result.artifacts.clone(),
                },
            ))
        })
        .collect()
}
fn check_parents(plan: &Plan, derivation: &Derivation, after: bool) -> Result<()> {
    for (id, pair) in &derivation.parents {
        let witness = if after { &pair.after } else { &pair.before };
        check_result(&witness.packet, &witness.result)?;
        let expected = make_packet(
            plan,
            id,
            dependencies(task(plan, id)?, &derivation.parents, after)?,
        )?;
        if expected != witness.packet {
            return fail(ErrorKind::Stale, "repair-derived-parent-packet");
        }
    }
    Ok(())
}
fn bound(
    plan: &Plan,
    id: &str,
    bindings: &upstream::Bindings,
    derivation: &Derivation,
    after: bool,
) -> Result<BTreeMap<String, upstream::BoundInput>> {
    let packet = make_packet(
        plan,
        id,
        dependencies(task(plan, id)?, &derivation.parents, after)?,
    )?;
    let ids: BTreeSet<_> = bindings.inputs.values().map(|r| &r.task_id).collect();
    let parents = ids
        .into_iter()
        .map(|id| {
            let pair = derivation.parents.get(id).ok_or(Error {
                kind: ErrorKind::Missing,
                code: "repair-derived-parent".into(),
            })?;
            let witness = if after { &pair.after } else { &pair.before };
            Ok((
                id.clone(),
                upstream::Parent {
                    packet: &witness.packet,
                    result: &witness.result,
                },
            ))
        })
        .collect::<Result<_>>()?;
    upstream::bind(&packet, bindings, &parents)
}
fn replace_sources(task: &mut Task, replacements: &[(Artifact, Artifact)]) -> Result<()> {
    let matches = |pin: &Artifact| replacements.iter().any(|(before, _)| before == pin);
    if task.native_inputs.as_ref().is_some_and(matches)
        || task.upstream_inputs.as_ref().is_some_and(matches)
    {
        return fail(ErrorKind::Unsupported, "repair-derived-control-input");
    }
    let replace = |pin: &mut Artifact| {
        if let Some((_, after)) = replacements.iter().find(|(before, _)| before == pin) {
            *pin = after.clone();
        }
    };
    for input in &mut task.inputs {
        replace(input);
    }
    if let Some(tool) = &mut task.tool {
        if matches(&tool.executable) || tool.auxiliary_executables.values().any(matches) {
            return fail(ErrorKind::Unsupported, "repair-derived-runtime-input");
        }
        for pin in [&mut tool.publication, &mut tool.request] {
            replace(pin);
        }
        if let Some(part) = &mut tool.part {
            replace(&mut part.plan);
        }
        for id in tool.source_bindings.values_mut() {
            if let Some((_, after)) = replacements
                .iter()
                .find(|(before, _)| *id == before.logical_id)
            {
                *id = after.logical_id.clone();
            }
        }
    }
    Ok(())
}
pub(super) fn apply(
    baseline: &Plan,
    candidate: &mut Plan,
    impact: &review::Impact,
    repair: &review::Repair,
    derivation: &Derivation,
) -> Result<()> {
    if derivation.parents.is_empty()
        || derivation.parents.len() > MAX_TASKS
        || derivation.consumers.is_empty()
        || derivation.consumers.len() > MAX_TASKS
    {
        return fail(ErrorKind::Budget, "repair-derived-count");
    }
    let mut required = BTreeSet::new();
    let mut pending = Vec::new();
    for id in derivation.consumers.keys() {
        if !impact.rebuild.contains(id) {
            return fail(ErrorKind::Conflict, "repair-derived-consumer-scope");
        }
        pending.extend(task(baseline, id)?.depends_on.iter().cloned());
    }
    while let Some(id) = pending.pop() {
        if required.insert(id.clone()) {
            pending.extend(task(baseline, &id)?.depends_on.iter().cloned());
        }
    }
    if required.iter().ne(derivation.parents.keys()) {
        return fail(ErrorKind::Conflict, "repair-derived-parent-set");
    }
    check_parents(baseline, derivation, false)?;
    for (id, bindings) in &derivation.consumers {
        let original = task(baseline, id)?;
        let before = bound(baseline, id, &bindings.before, derivation, false)?;
        // One exact producer/slot per input, with identical selector sets on
        // both sides. Different bytes/IDs cannot change their declared origin.
        let refs: Vec<_> = bindings.before.inputs.values().collect();
        if bindings.after.inputs.len() != refs.len()
            || refs.iter().any(|r| {
                refs.iter().filter(|other| *other == r).count() != 1
                    || bindings
                        .after
                        .inputs
                        .values()
                        .filter(|other| *other == *r)
                        .count()
                        != 1
            })
        {
            return fail(ErrorKind::Conflict, "repair-derived-selector-set");
        }
        bindings.after.allowance()?;
        let before_control = original.upstream_inputs.as_ref().ok_or(Error {
            kind: ErrorKind::Missing,
            code: "repair-derived-binding-pin".into(),
        })?;
        if repair.edits.iter().any(|edit| {
            &edit.before == before_control
                || before.values().any(|input| edit.before == input.artifact)
        }) {
            return fail(ErrorKind::Conflict, "repair-derived-asset-edit-overlap");
        }
        let changed = candidate
            .tasks
            .iter_mut()
            .find(|t| &t.id == id)
            .expect("checked task scope");
        if changed.upstream_inputs.as_ref() != Some(before_control) {
            return fail(ErrorKind::Stale, "repair-derived-binding-baseline");
        }
        let mut replacements = Vec::new();
        for input in before.values() {
            if original.native_inputs.as_ref() == Some(&input.artifact) {
                return fail(ErrorKind::Unsupported, "repair-derived-control-input");
            }
            let pair = &derivation.parents[&input.reference.task_id];
            let after = match &input.reference.artifact {
                upstream::Selector::Output { name } => pair.after.result.artifacts.get(name),
                upstream::Selector::WorkerReceipt => pair.after.result.worker_receipt.as_ref(),
            }
            .ok_or(Error {
                kind: ErrorKind::Missing,
                code: "repair-derived-output".into(),
            })?;
            replacements.push((input.artifact.clone(), after.clone()));
        }
        replace_sources(changed, &replacements)?;
        let raw = bindings.after.canonical_bytes()?;
        changed.upstream_inputs = Some(Artifact {
            logical_id: before_control.logical_id.clone(),
            sha256: digest(&raw),
            bytes: raw.len() as u64,
            media_type: before_control.media_type.clone(),
        });
    }
    check_plan(candidate)?;
    check_parents(candidate, derivation, true)?;
    for (id, bindings) in &derivation.consumers {
        bound(candidate, id, &bindings.after, derivation, true)?;
    }
    Ok(())
}
