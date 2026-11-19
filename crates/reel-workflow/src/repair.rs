//! Materialize an exact-pin repair as a new candidate plan. This does not admit
//! resources, carry old results across commits, select assets or close findings.
use crate::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub schema: String,
    pub source_commit: String,
    /// Every native task needs a fresh commit-bound declaration, including
    /// creatively unaffected tasks. These are pointers, not custody evidence.
    pub native_inputs: BTreeMap<String, review::Edit>,
}

#[derive(Debug, Serialize)]
pub struct Candidate {
    pub schema: String,
    pub revision_sha256: String,
    pub plan_sha256: String,
    pub plan: Plan,
    pub impact: review::Impact,
}

fn replace(pin: &mut Artifact, edits: &[review::Edit]) {
    if let Some(edit) = edits.iter().find(|e| e.before == *pin) {
        *pin = edit.after.clone();
    }
}

/// Rewrites exact matching inputs and their producer references. Prompts,
/// languages, output contracts, dependency edges and producer choices stay
/// authored. Changed control JSON must be supplied as another explicit edit;
/// this function never guesses or rewrites bytes inside an asset.
pub fn materialize(
    baseline: &Plan,
    report: &review::CheckedReport,
    repair: &review::Repair,
    revision: &Revision,
) -> Result<Candidate> {
    let impact = review::repair_impact(baseline, report, repair)?;
    let revision_sha256 = canonical_digest(revision)?;
    if revision.schema != "reel.workflow-repair-revision.v1"
        || !hex(&revision.source_commit, &[40, 64])
        || revision.source_commit.bytes().all(|b| b == b'0')
        || revision.source_commit == baseline.source_commit
    {
        return fail(ErrorKind::Invalid, "repair-revision-commit");
    }
    let required: BTreeSet<_> = baseline
        .tasks
        .iter()
        .filter(|t| t.native_inputs.is_some())
        .map(|t| &t.id)
        .collect();
    if revision.native_inputs.keys().collect::<BTreeSet<_>>() != required {
        return fail(ErrorKind::Conflict, "repair-revision-native-task-set");
    }
    let mut plan = baseline.clone();
    plan.source_commit = revision.source_commit.clone();
    for task in &mut plan.tasks {
        for pin in &mut task.inputs {
            replace(pin, &repair.edits);
        }
        if let Some(tool) = &mut task.tool {
            replace(&mut tool.publication, &repair.edits);
            replace(&mut tool.request, &repair.edits);
            replace(&mut tool.executable, &repair.edits);
            for pin in tool.auxiliary_executables.values_mut() {
                replace(pin, &repair.edits);
            }
            if let Some(part) = &mut tool.part {
                replace(&mut part.plan, &repair.edits);
            }
        }
        if let Some(pin) = &mut task.upstream_inputs {
            replace(pin, &repair.edits);
        }
        if let Some(pin) = &mut task.native_inputs {
            let edit = &revision.native_inputs[&task.id];
            artifact(&edit.before)?;
            artifact(&edit.after)?;
            if edit.before != *pin
                || edit.before.logical_id != edit.after.logical_id
                || edit.before == edit.after
            {
                return fail(ErrorKind::Stale, "repair-revision-native-pin");
            }
            if repair
                .edits
                .iter()
                .any(|e| e.before == edit.before && e.after != edit.after)
            {
                return fail(ErrorKind::Conflict, "repair-revision-native-edit");
            }
            *pin = edit.after.clone();
        }
    }
    check_plan(&plan)?;
    let candidate = Candidate {
        schema: "reel.workflow-repair-candidate.v1".into(),
        revision_sha256,
        plan_sha256: canonical_digest(&plan)?,
        plan,
        impact,
    };
    canonical_digest(&candidate)?;
    Ok(candidate)
}
