//! Read-only binding of indexed, bounded existing-capture review records.
//!
//! The original task is never rewritten or assigned a new identity. This view
//! supplies only fields derived from its pinned index, group and source notes;
//! the ordinary proof, byte, observation, receipt and aggregation gates still run.
use super::*;
use anyhow::Context;

#[cfg(test)]
mod tests;

pub(super) struct Binding {
    pub task: Value,
    pub features: Value,
}

const ROLES: [&str; 4] = [
    "programme_native",
    "programme_true390",
    "Review_native",
    "Review_true390",
];
const FOOTER_FACTS: [&str; 6] = [
    "code_readable_Review_native",
    "code_readable_Review_true390",
    "code_wholly_footer_Review_native",
    "code_wholly_footer_Review_true390",
    "code_matches_declared_events_Review_native",
    "code_matches_declared_events_Review_true390",
];

fn same_pin(a: &Value, b: &Value) -> Result<()> {
    ensure!(pin(a)? == pin(b)?, "indexed source pin differs");
    Ok(())
}

fn member(values: &[Value], object: &Object) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut count = 0;
    for value in values {
        let declared = pin(value)?;
        ensure!(seen.insert(key(&declared)), "duplicate indexed descriptor");
        count += usize::from(declared == *object);
    }
    ensure!(count == 1, "task/group is outside its frozen index");
    Ok(())
}

fn active(target: &Value, frame: u64) -> Result<bool> {
    let span = array(target, "own_ES_span")?;
    ensure!(span.len() == 2, "invalid indexed target span");
    let start = span[0].as_u64().context("invalid indexed span start")?;
    let end = span[1].as_u64().context("invalid indexed span end")?;
    ensure!(start < end, "empty indexed target span");
    Ok((start..end).contains(&frame))
}

fn policy_contract(policy: &Policy, protocol: &Value) -> Result<()> {
    ensure!(
        policy.media_inputs
            == BTreeMap::from([
                ("programme.mp4".to_owned(), "actual_programme".to_owned()),
                ("Review.mp4".to_owned(), "actual_Review".to_owned()),
            ]),
        "indexed programme and Review source checks are both required"
    );
    ensure!(
        integer(protocol, "maximum_clocks_per_assignment")? == 3
            && policy.observations.max_clocks == 3,
        "indexed original clock bound differs"
    );
    let expected: BTreeSet<_> = ROLES.into_iter().map(str::to_owned).collect();
    ensure!(
        unique_strings(array(protocol, "role_names")?)? == expected
            && policy
                .role_dimensions
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
                == expected,
        "indexed four-role contract differs"
    );
    for flag in [
        "verify_all_frozen_descriptors_before_any_view",
        "verify_each_exact_original_file_SHA_bytes_RGB8_header_and_dimensions",
        "view_each_role_at_original_detail",
        "no_montage_resize_crop_composite_or_reencoding",
        "all_four_actual_role_views_required",
        "assess_own_ASS_and_current_footer_codes_against_exact_active_events",
        "record_all_active_corrected_occurrences_and_no_out_of_span_targets",
        "compare_original_and_selected_raw_references_before_requested_feature_judgments",
        "preserve_original_per_role_false_null_missing_readability_fine_colour_source_and_opaque_motion_holds",
        "retain_exact_task_group_source_programme_Review_and_view_descriptors",
        "no_cross_episode_or_language_visual_outcomes",
        "no_global_human_continuous_motion_or_audio_acceptance",
        "only_metadata_may_return_to_root",
        "no_registry_Drive_or_card_edits",
        "no_dispatch_authorized_by_preparation",
    ] {
        ensure!(
            boolean(protocol, flag)?,
            "indexed original protocol differs"
        );
    }
    ensure!(
        unique_strings(
            &policy
                .observations
                .footer_fields
                .iter()
                .map(|s| json!(s))
                .collect::<Vec<_>>()
        )? == FOOTER_FACTS.into_iter().map(str::to_owned).collect()
            && policy.observations.footer_events_field.as_deref()
                == Some("current_footer_Review_ID_events"),
        "indexed footer checks may not be omitted"
    );
    ensure!(
        policy.observations.targets.len() == 1,
        "indexed target policy required"
    );
    let rule = &policy.observations.targets[0];
    ensure!(
        rule.source_field == "indexed_source_targets"
            && rule.task_field == "indexed_active_targets"
            && rule.span_field == "own_ES_span"
            && rule.observation_field == "corrected_source_observations"
            && rule.source_exact_fields.is_empty()
            && rule.exact_fields == ["exact_requested_fix_lines_es"]
            && rule.picture_fields.iter().cloned().collect::<BTreeSet<_>>()
                == ["original_picture", "selected_picture"]
                    .into_iter()
                    .map(str::to_owned)
                    .collect()
            && rule
                .appearance_fields
                .iter()
                .any(|s| s == "selected_correction_appearance_visible"),
        "indexed source correction checks differ"
    );
    Ok(())
}

pub(super) fn bind(
    original_task: &Value,
    task_pin: &Object,
    method: &Object,
    method_bytes: &[u8],
    policy: &Policy,
    index_pin: &Object,
    cache: &mut ReadCache,
) -> Result<Binding> {
    let protocol = parse_json(method_bytes)?;
    policy_contract(policy, &protocol)?;
    let index = metadata(cache, index_pin)?;
    ensure!(
        boolean(&index, "no_dispatch")? && !boolean(&index, "human_approval")?,
        "indexed preparation state differs"
    );
    same_pin(at(&index, "protocol")?, &json!(method))?;
    let entries = array(&index, "tasks")?;
    let groups = array(&index, "original_groups")?;
    ensure!(
        entries.len() <= 4096
            && groups.len() <= 4096
            && integer(at(&index, "summary")?, "bounded_tasks")? == entries.len() as u64
            && integer(at(&index, "summary")?, "original_groups")? == groups.len() as u64,
        "indexed source counts differ"
    );
    member(entries, task_pin)?;
    let group_pin = pin(at(original_task, "original_frozen_group")?)?;
    member(groups, &group_pin)?;
    // These are bounded metadata/opaque controls, never executable instructions.
    bytes(cache, &pin(at(&index, "controls")?)?, META_MAX)?;
    metadata(cache, &pin(at(&index, "own_native_capture_parent")?)?)?;
    let group = metadata(cache, &group_pin)?;
    let notes_pin = pin(at(original_task, "exact_own_correction_reference_notes")?)?;
    same_pin(at(&index, "notes")?, &json!(notes_pin))?;
    let notes = metadata(cache, &notes_pin)?;
    let correspondence_pin = pin(at(original_task, "source_correspondence")?)?;
    same_pin(
        at(&group, "source_correspondence")?,
        &json!(correspondence_pin),
    )?;
    let correspondence = metadata(cache, &correspondence_pin)?;
    let original_source_pin = pin(at(&notes, "original_source_correspondence")?)?;
    same_pin(
        at(&correspondence, "original_source_correspondence")?,
        &json!(original_source_pin),
    )?;
    let original_source = metadata(cache, &original_source_pin)?;
    metadata(cache, &pin(at(&notes, "current_card_payload")?)?)?;
    let chain_pin = pin(at(original_task, "own_source_clock_state_chain")?)?;
    same_pin(
        at(&correspondence, "own_source_clock_state_chain")?,
        &json!(chain_pin),
    )?;
    metadata(cache, &chain_pin)?;
    for field in ["episode", "language"] {
        let expected = at(original_task, field)?;
        for context in [
            &index["summary"],
            &group,
            &notes,
            &correspondence,
            &original_source,
        ] {
            ensure!(
                context.get(field) == Some(expected),
                "indexed episode/language differs"
            );
        }
    }
    ensure!(
        policy.observations.scope
            == format!(
                "{}/{}",
                text(original_task, "episode")?,
                text(original_task, "language")?
            ),
        "indexed scope differs from its original episode/language"
    );
    for field in ["actual_programme", "actual_Review", "current_ASS"] {
        same_pin(at(original_task, field)?, at(&correspondence, field)?)?;
    }
    same_pin(
        at(original_task, "actual_programme")?,
        at(&original_source, "actual_programme")?,
    )?;
    ensure!(
        boolean(original_task, "private_pixel_observations_pending")?
            && boolean(
                original_task,
                "no_other_episode_language_or_visual_result_inheritance"
            )?
            && boolean(&group, "private_image_QA_pending")?
            && boolean(&notes, "actual_pixel_QA_pending")?
            && !boolean(original_task, "human_approval")?
            && !boolean(&group, "human_approval")?
            && !boolean(&original_source, "human_approval")?,
        "indexed pending/approval state differs"
    );
    ensure!(
        at(original_task, "source_holds")? == at(&notes, "source_holds")?
            && !boolean(at(original_task, "source_holds")?, "human_approval")?,
        "indexed original holds differ"
    );
    ensure!(
        at(original_task, "role_geometry")? == &json!(policy.role_dimensions),
        "indexed original role geometry differs"
    );
    let clocks = array(original_task, "clocks")?;
    let all = array(&group, "clocks")?;
    let part = integer(original_task, "part")?;
    ensure!(
        part > 0 && !all.is_empty() && all.len() <= 4096,
        "invalid indexed part"
    );
    let start = usize::try_from(part - 1)?
        .checked_mul(3)
        .context("indexed part overflow")?;
    ensure!(start < all.len(), "indexed part is outside group");
    let end = (start + 3).min(all.len());
    ensure!(
        clocks.as_slice() == &all[start..end]
            && original_task.get("group_number") == group.get("group_number")
            && integer(&group, "future_fresh_pixel_worker_max_clocks")? == 3,
        "indexed task is not its exact original bounded part"
    );
    let wrappers = array(&notes, "targets")?;
    let occurrences = wrappers
        .iter()
        .map(|w| at(w, "exact_own_source_occurrence").cloned())
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        occurrences == *array(&original_source, "changed_source_occurrences")?,
        "indexed notes dropped or changed original occurrences"
    );
    let by_sample = at(
        original_task,
        "active_corrected_source_occurrences_by_sample",
    )?;
    let sample_ids = clocks
        .iter()
        .map(|c| text(at(c, "sample")?, "sample_id").map(str::to_owned))
        .collect::<Result<BTreeSet<_>>>()?;
    ensure!(
        names(by_sample)? == sample_ids,
        "indexed target sample set differs"
    );
    for clock in clocks {
        let sample = at(clock, "sample")?;
        let frame = integer(sample, "frame")?;
        let selected = occurrences
            .iter()
            .filter_map(|occurrence| match active(occurrence, frame) {
                Ok(true) => Some(Ok(occurrence.clone())),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            at(by_sample, text(sample, "sample_id")?)? == &json!(selected),
            "indexed active correction selection differs"
        );
    }
    let mut targets = Vec::new();
    let mut selected = Vec::new();
    let mut raw = BTreeMap::new();
    let mut auxiliary = BTreeMap::new();
    for (wrapper, occurrence) in wrappers.iter().zip(occurrences) {
        ensure!(
            boolean(wrapper, "no_prior_visual_results_inherited")?,
            "indexed target inherits a visual outcome"
        );
        let mut target = occurrence;
        let lines = array(wrapper, "exact_requested_fix_lines_es")?;
        ensure!(
            !lines.is_empty()
                && lines.len() <= 100
                && lines.iter().all(|v| v
                    .as_str()
                    .is_some_and(|s| !s.trim().is_empty() && s.len() <= 2000)),
            "invalid original fix lines"
        );
        target["exact_requested_fix_lines_es"] = json!(lines);
        target["original_source_note"] = wrapper.clone();
        let relevant = clocks
            .iter()
            .map(|c| integer(at(c, "sample")?, "frame").and_then(|f| active(&target, f)))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .any(|a| a);
        if relevant {
            for field in ["selected_picture", "original_picture"] {
                let object = pin(at(&target, field)?)?;
                raw.insert(key(&object), object);
            }
            if let Some(value) = target
                .get("raw_identity_only_producer_reference")
                .filter(|v| !v.is_null())
            {
                let object = pin(value)?;
                // This is an opaque identity-only producer metadata record,
                // not another picture or an inherited visual outcome.
                metadata(cache, &object)?;
                auxiliary.insert(key(&object), object);
            }
            selected.push(target.clone());
        }
        targets.push(target);
    }
    let mut task = original_task.clone();
    task["scope"] = json!(policy.observations.scope);
    task["protocol_method"] = json!(method);
    task["source_group_all_clocks_remain_immutable"] = json!(true);
    task["all_scope_qualification_pending"] = json!(true);
    task["technical_AAC_r1_not_final_playback_copy"] = json!(false);
    task["original_group_number"] = group["group_number"].clone();
    task["original_clock_indices"] = json!((start..end).collect::<Vec<_>>());
    task["clock_count"] = json!(clocks.len());
    task["image_occurrences"] = json!(clocks.len() * ROLES.len());
    task["roles"] = json!(ROLES);
    task["raw_feature_source"] = json!(notes_pin);
    task["raw_references"] = json!(raw.into_values().collect::<Vec<_>>());
    task["indexed_active_targets"] = json!(selected);
    task["required_original_holds"] = original_task["source_holds"].clone();
    task["indexed_source_context"] = json!({"index":index_pin,"method":method,"notes":notes_pin,"source_correspondence":correspondence_pin,"original_source_correspondence":original_source_pin,"own_source_clock_state_chain":chain_pin,"identity_only_metadata":auxiliary.into_values().collect::<Vec<_>>(),"original_task_identity_preserved":true});
    Ok(Binding {
        task,
        features: json!({"current_ASS":original_task["current_ASS"],"indexed_source_targets":targets}),
    })
}
