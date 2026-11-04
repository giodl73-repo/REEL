//! Report reconstruction and disjoint aggregation without new image observations.
use super::*;

/// Construct a subreport after validation. This does not persist it or own a receipt.
pub fn build_subreport(
    task: &Value,
    task_object: &Object,
    evidence: &Value,
    policy: &ObservationPolicy,
    verify: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<Value> {
    let checked = validate_observations(task, evidence, policy, verify)?;
    let mut report = json!({
        "schema":policy.report_schema,"scope":at(task,"scope")?,"episode":at(task,"episode")?,"language":at(task,"language")?,"group_number":at(task,"original_group_number")?,
        "original_frozen_group":at(task,"original_frozen_group")?,"frozen_subtask":task_object,"actual_programme":at(task,"actual_programme")?,
        "frames":checked.observations.iter().map(|o|o["frame"].clone()).collect::<Vec<_>>(),
        "observed_clock_count":checked.observations.len(),"observed_image_occurrences":checked.observations.len()*array(task,"roles")?.len(),
        "raw_reference_view_occurrences":at(evidence,"raw_reference_view_occurrences")?,"raw_source_observations":at(evidence,"raw_source_observations")?,
        "observations":checked.observations,"major_obstruction_found":checked.major_obstruction_found,"finite_scope_qualified_with_holds":checked.finite_scope_qualified_with_holds,
        "holds_preserved":at(evidence,"holds_preserved")?,"all_scope_qualification_pending":true,"human_approval":false
    });
    for field in ["actual_Review", "current_ASS"] {
        if let Some(value) = task.get(field) {
            report[field] = value.clone();
        }
    }
    Ok(report)
}

/// Reopen a completed report for a skip. Assertions are revalidated, never counted
/// as fresh views. The callback verifies bytes without invoking an image viewer.
pub fn validate_subreport(
    task: &Value,
    task_object: &Object,
    report: &Value,
    policy: &ObservationPolicy,
    verify: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<()> {
    ensure!(
        text(report, "schema")? == policy.report_schema,
        "subreport schema differs"
    );
    ensure!(
        pin(at(report, "frozen_subtask")?)? == *task_object,
        "subreport task pin differs"
    );
    let mut observations = array(report, "observations")?.clone();
    for observation in &mut observations {
        let facts = at(observation, "per_role_fresh_observations")?.clone();
        let map = observation
            .as_object_mut()
            .ok_or_else(|| anyhow!("invalid observation"))?;
        map.remove("per_role_fresh_observations");
        map.insert("fresh_role_observations".into(), facts);
    }
    let evidence = json!({"observations":observations,"fresh_observations_not_inherited":true,"finite_scope_qualified_with_holds":at(report,"finite_scope_qualified_with_holds")?,"human_approval":at(report,"human_approval")?,"holds_preserved":at(report,"holds_preserved")?,"raw_source_observations":at(report,"raw_source_observations")?,"raw_reference_view_occurrences":at(report,"raw_reference_view_occurrences")?});
    let expected = build_subreport(task, task_object, &evidence, policy, verify)?;
    for (field, value) in expected
        .as_object()
        .ok_or_else(|| anyhow!("invalid expected report"))?
    {
        if field == "frozen_subtask" {
            continue;
        } // historical path is provenance; pin checked above
        ensure!(
            report.get(field) == Some(value),
            "subreport derived field differs: {field}"
        );
    }
    Ok(())
}

pub struct ReviewPart<'a> {
    pub task_object: &'a Object,
    pub task: &'a Value,
    pub report: &'a Value,
}

/// Aggregate disjoint parts in original order. Incomplete coverage is explicitly
/// marked. This never grants whole-programme qualification or human approval.
pub fn aggregate_parts(
    group: &Value,
    group_object: &Object,
    parts: &[ReviewPart<'_>],
    policy: &ObservationPolicy,
    require_complete: bool,
    verify: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<Value> {
    ensure!(
        !parts.is_empty() && parts.len() <= 512,
        "invalid part count"
    );
    let clocks = array(group, "clocks")?;
    ensure!(
        !clocks.is_empty() && clocks.len() <= 4096,
        "invalid original group size"
    );
    let mut expected = BTreeMap::new();
    for clock in clocks {
        ensure!(
            expected
                .insert(text(at(clock, "sample")?, "sample_id")?, clock)
                .is_none(),
            "duplicate original sample"
        );
    }
    let roles = names(at(&clocks[0], "views")?)?;
    ensure!(
        !roles.is_empty() && roles.len() <= 16,
        "invalid group roles"
    );
    for clock in clocks {
        ensure!(
            names(at(clock, "views")?)? == roles,
            "group clock roles differ"
        );
    }
    let mut observed = BTreeMap::new();
    let mut raw = Vec::new();
    let mut holds = Vec::new();
    let mut qualified = true;
    let mut major = false;
    for part in parts {
        ensure!(
            at(part.task, "original_group_number")? == at(group, "group_number")?,
            "part group number differs"
        );
        ensure!(
            pin(at(part.task, "original_frozen_group")?)? == *group_object,
            "part original group differs"
        );
        ensure!(
            unique_strings(array(part.task, "roles")?)? == roles,
            "part roles differ"
        );
        for field in [
            "episode",
            "language",
            "actual_programme",
            "actual_Review",
            "current_ASS",
        ] {
            ensure!(
                part.task.get(field) == group.get(field),
                "part group context differs: {field}"
            );
        }
        for clock in array(part.task, "clocks")? {
            ensure!(
                expected
                    .get(text(at(clock, "sample")?, "sample_id")?)
                    .copied()
                    == Some(clock),
                "part clock differs from original group"
            );
        }
        validate_subreport(part.task, part.task_object, part.report, policy, verify)?;
        qualified &= boolean(part.report, "finite_scope_qualified_with_holds")?;
        major |= boolean(part.report, "major_obstruction_found")?;
        for observation in array(part.report, "observations")? {
            ensure!(
                observed
                    .insert(text(observation, "sample_id")?, observation.clone())
                    .is_none(),
                "overlapping partial reports"
            );
        }
        raw.extend(
            array(part.report, "raw_source_observations")?
                .iter()
                .cloned(),
        );
        holds.push(at(part.report, "holds_preserved")?.clone());
    }
    let complete = expected.len() == observed.len();
    ensure!(!require_complete || complete, "group coverage incomplete");
    ensure!(!(major && qualified), "major obstruction cannot qualify");
    let ordered = clocks
        .iter()
        .filter_map(|c| observed.get(c["sample"]["sample_id"].as_str()?).cloned())
        .collect::<Vec<_>>();
    Ok(
        json!({"schema":"reel.review-disjoint-aggregation.experimental-1","frozen_group":group_object,"observations":ordered,"observed_clock_count":ordered.len(),"observed_image_occurrences":ordered.len()*roles.len(),"raw_reference_view_occurrences":raw.len(),"raw_source_observations":raw,"holds_preserved":holds,"major_obstruction_found":major,"finite_scope_qualified_with_holds":qualified,"group_coverage_complete":complete,"all_scope_qualification_pending":true,"human_approval":false,"mechanical_disjoint_aggregation_no_new_views_or_outcome_rewrite":true}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Value, Value, ObservationPolicy) {
        let (mut task, evidence, policy) = super::super::tests::fixture();
        task["episode"] = json!("example");
        task["language"] = json!("xx");
        task["original_group_number"] = json!(1);
        task["original_frozen_group"] = json!(Object::from_bytes(b"group"));
        task["actual_programme"] = json!(Object::from_bytes(b"programme"));
        (task, evidence, policy)
    }
    #[test]
    fn completed_retry_revalidates_outcomes_counts_and_source_views() {
        let (task, evidence, policy) = fixture();
        let task_object = Object::from_bytes(&serde_json::to_vec(&task).unwrap());
        let report =
            build_subreport(&task, &task_object, &evidence, &policy, &mut |_| Ok(())).unwrap();
        validate_subreport(&task, &task_object, &report, &policy, &mut |_| Ok(())).unwrap();
        let mut bad = report.clone();
        bad["observations"][0]["views"]["large"]["canonical_path"] = json!("changed");
        assert!(validate_subreport(&task, &task_object, &bad, &policy, &mut |_| Ok(())).is_err());
        let mut bad = report;
        bad["observed_image_occurrences"] = json!(0);
        assert!(validate_subreport(&task, &task_object, &bad, &policy, &mut |_| Ok(())).is_err());
    }
    #[test]
    fn disjoint_partial_continuation_preserves_source_order_and_holds() {
        let (mut group, evidence, policy) = fixture();
        group
            .as_object_mut()
            .unwrap()
            .remove("original_frozen_group");
        group["group_number"] = json!(1);
        group.as_object_mut().unwrap().remove("roles");
        group.as_object_mut().unwrap().remove("scope");
        let mut second = group["clocks"][0].clone();
        second["sample"]["sample_id"] = json!("two");
        second["sample"]["frame"] = json!(13);
        group["clocks"].as_array_mut().unwrap().push(second);
        let group_object = Object::from_bytes(&serde_json::to_vec(&group).unwrap());
        let mut first_task = group.clone();
        first_task["roles"] = json!(["large", "small"]);
        first_task["scope"] = json!("fixture");
        first_task["clocks"].as_array_mut().unwrap().pop();
        first_task["original_frozen_group"] = json!(group_object);
        let first_pin = Object::from_bytes(&serde_json::to_vec(&first_task).unwrap());
        let first_report =
            build_subreport(&first_task, &first_pin, &evidence, &policy, &mut |_| Ok(())).unwrap();
        let part = ReviewPart {
            task_object: &first_pin,
            task: &first_task,
            report: &first_report,
        };
        let partial = aggregate_parts(&group, &group_object, &[part], &policy, false, &mut |_| {
            Ok(())
        })
        .unwrap();
        assert_eq!(partial["group_coverage_complete"], false);
        assert_eq!(partial["finite_scope_qualified_with_holds"], false);
        let mut second_task = first_task.clone();
        second_task["clocks"][0] = group["clocks"][1].clone();
        let second_pin = Object::from_bytes(&serde_json::to_vec(&second_task).unwrap());
        let mut second_evidence = evidence;
        second_evidence["observations"][0]["sample_id"] = json!("two");
        second_evidence["observations"][0]["frame"] = json!(13);
        let second_report = build_subreport(
            &second_task,
            &second_pin,
            &second_evidence,
            &policy,
            &mut |_| Ok(()),
        )
        .unwrap();
        let parts = [
            ReviewPart {
                task_object: &second_pin,
                task: &second_task,
                report: &second_report,
            },
            ReviewPart {
                task_object: &first_pin,
                task: &first_task,
                report: &first_report,
            },
        ];
        let complete =
            aggregate_parts(
                &group,
                &group_object,
                &parts,
                &policy,
                true,
                &mut |_| Ok(()),
            )
            .unwrap();
        assert_eq!(complete["group_coverage_complete"], true);
        assert_eq!(complete["observations"][0]["sample_id"], "one");
        assert_eq!(complete["observations"][1]["sample_id"], "two");
        assert_eq!(complete["all_scope_qualification_pending"], true);
        let duplicate = [
            ReviewPart {
                task_object: &first_pin,
                task: &first_task,
                report: &first_report,
            },
            ReviewPart {
                task_object: &first_pin,
                task: &first_task,
                report: &first_report,
            },
        ];
        assert!(
            aggregate_parts(
                &group,
                &group_object,
                &duplicate,
                &policy,
                false,
                &mut |_| Ok(())
            )
            .is_err()
        );
    }
}
