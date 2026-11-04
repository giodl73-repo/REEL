//! Frozen review predicates. These checks neither observe images nor grant approval.
//!
//! Policy names describe fields in retained task/observation metadata. They are
//! caller-admitted data, not evidence that a historical protocol has been ported.
//! The admission entry reopens existing protected bytes before any reservation.
use anyhow::{Result, anyhow, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use tig_core::Object;

pub mod admission;
mod reports;
pub use reports::{ReviewPart, aggregate_parts, build_subreport, validate_subreport};
pub mod receipts;
mod retained;
pub use retained::{
    RetainedSnapshot, SnapshotView, validate_png_header, validate_retired_snapshot,
};

/// External review evidence contains fractional clocks. Its identity is the hash
/// of original bytes, not Tig's integer-only canonical JSON representation.
fn parse_json(bytes: &[u8]) -> Result<Value> {
    ensure!(
        bytes.len() <= 8 * 1024 * 1024,
        "review JSON exceeds byte bound"
    );
    crate::approval_attestation::ensure_no_duplicate_keys(bytes)?;
    let value: Value = serde_json::from_slice(bytes)?;
    fn bounded(value: &Value, depth: usize, nodes: &mut usize) -> Result<()> {
        *nodes += 1;
        ensure!(
            depth <= 64 && *nodes <= 200_000,
            "review JSON exceeds structural bound"
        );
        match value {
            Value::Array(items) => {
                for item in items {
                    bounded(item, depth + 1, nodes)?
                }
            }
            Value::Object(items) => {
                for item in items.values() {
                    bounded(item, depth + 1, nodes)?
                }
            }
            _ => {}
        }
        Ok(())
    }
    bounded(&value, 0, &mut 0)?;
    Ok(value)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationPolicy {
    pub scope: String,
    pub report_schema: String,
    pub max_clocks: usize,
    /// Nullable booleans required on every observation.
    pub footer_fields: Vec<String>,
    /// Sample event field; a qualified clock with events requires true footer facts.
    pub footer_events_field: Option<String>,
    pub targets: Vec<TargetPolicy>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetPolicy {
    pub task_field: String,
    /// Source feature-note collection from which this task selection was derived.
    pub source_field: String,
    /// Exact-field fallback JSON pointers when the source stores text indirectly.
    #[serde(default)]
    pub source_exact_fields: BTreeMap<String, String>,
    pub observation_field: String,
    pub span_field: String,
    pub picture_fields: Vec<String>,
    pub exact_fields: Vec<String>,
    pub appearance_fields: Vec<String>,
}

/// Preserves observation outcomes and extra notes. No finite or global success is
/// inferred from this value, and fresh-view assertions remain reviewer assertions.
#[derive(Debug)]
pub struct ValidatedObservations {
    pub observations: Vec<Value>,
    pub major_obstruction_found: bool,
    pub finite_scope_qualified_with_holds: bool,
}

fn at<'a>(value: &'a Value, key: &str) -> Result<&'a Value> {
    value
        .get(key)
        .ok_or_else(|| anyhow!("missing review field: {key}"))
}
fn optional<'a>(value: &'a Value, key: &str) -> &'a Value {
    value.get(key).unwrap_or(&Value::Null)
}
fn array<'a>(value: &'a Value, key: &str) -> Result<&'a Vec<Value>> {
    at(value, key)?
        .as_array()
        .ok_or_else(|| anyhow!("review field is not an array: {key}"))
}
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    at(value, key)?
        .as_str()
        .ok_or_else(|| anyhow!("review field is not text: {key}"))
}
fn boolean(value: &Value, key: &str) -> Result<bool> {
    at(value, key)?
        .as_bool()
        .ok_or_else(|| anyhow!("review field is not boolean: {key}"))
}
fn integer(value: &Value, key: &str) -> Result<u64> {
    at(value, key)?
        .as_u64()
        .ok_or_else(|| anyhow!("review field is not an unsigned integer: {key}"))
}
fn nullable_boolean(value: &Value, key: &str) -> Result<()> {
    let fact = at(value, key)?;
    ensure!(
        fact.is_null() || fact.is_boolean(),
        "review fact must be boolean or null: {key}"
    );
    Ok(())
}
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
        Value::Number(v) => v.as_f64().is_some_and(|v| v != 0.0),
    }
}
fn names(value: &Value) -> Result<BTreeSet<String>> {
    Ok(value
        .as_object()
        .ok_or_else(|| anyhow!("expected role map"))?
        .keys()
        .cloned()
        .collect())
}
fn pin(value: &Value) -> Result<Object> {
    Ok(serde_json::from_value(
        json!({"sha256":at(value,"sha256")?,"bytes":at(value,"bytes")?}),
    )?)
}
fn viewed(value: &Value) -> Result<()> {
    ensure!(
        boolean(value, "privately_viewed_detail_original")?,
        "missing private-view assertion"
    );
    ensure!(
        !text(value, "detail")?.trim().is_empty(),
        "empty private observation"
    );
    Ok(())
}
fn unique_strings(values: &[Value]) -> Result<BTreeSet<String>> {
    let mut seen = BTreeSet::new();
    for value in values {
        let name = value.as_str().ok_or_else(|| anyhow!("role is not text"))?;
        ensure!(
            !name.is_empty() && seen.insert(name.to_owned()),
            "empty or duplicate role"
        );
    }
    Ok(seen)
}

impl ObservationPolicy {
    fn validate(&self) -> Result<()> {
        ensure!(
            !self.scope.is_empty()
                && !self.report_schema.is_empty()
                && (1..=64).contains(&self.max_clocks),
            "invalid review policy bounds"
        );
        ensure!(
            self.targets.len() <= 16 && self.footer_fields.len() <= 32,
            "review policy exceeds bounds"
        );
        let mut names = BTreeSet::new();
        for field in &self.footer_fields {
            ensure!(
                !field.is_empty() && names.insert(field),
                "empty or duplicate footer field"
            );
        }
        ensure!(
            self.footer_events_field.is_some() == !self.footer_fields.is_empty(),
            "footer event/field policy must be paired"
        );
        let mut kinds = BTreeSet::new();
        for target in &self.targets {
            ensure!(kinds.insert(&target.task_field), "duplicate target policy");
            ensure!(
                !target.picture_fields.is_empty() && !target.appearance_fields.is_empty(),
                "target checks may not be empty"
            );
            for field in [
                &target.task_field,
                &target.observation_field,
                &target.span_field,
            ]
            .into_iter()
            .chain(target.picture_fields.iter())
            .chain(target.exact_fields.iter())
            .chain(target.appearance_fields.iter())
            {
                ensure!(
                    !field.is_empty() && field.len() <= 256,
                    "invalid target field"
                );
            }
        }
        Ok(())
    }
}

/// Validate a bounded finite observation against immutable task metadata.
/// `verify` must reopen each exact raw/reference image through the admitted byte
/// source; metadata booleans such as `fresh_hash_bytes_verified` never replace it.
/// Receipt ownership, PNG headers, task/method/proof pins and exclusive saves are
/// separate prerequisites; callers must not treat this predicate layer as admission.
pub fn validate_observations(
    task: &Value,
    evidence: &Value,
    policy: &ObservationPolicy,
    verify: &mut impl FnMut(&Value) -> Result<()>,
) -> Result<ValidatedObservations> {
    policy.validate()?;
    ensure!(
        text(task, "scope")? == policy.scope,
        "review scope differs from policy"
    );
    let roles = unique_strings(array(task, "roles")?)?;
    ensure!(!roles.is_empty() && roles.len() <= 16, "invalid role count");
    let clocks = array(task, "clocks")?;
    ensure!(
        !clocks.is_empty() && clocks.len() <= policy.max_clocks,
        "clock budget exceeded"
    );
    ensure!(
        integer(task, "clock_count")? == clocks.len() as u64,
        "task clock count differs"
    );
    ensure!(
        integer(task, "image_occurrences")? == (clocks.len() * roles.len()) as u64,
        "task image count differs"
    );
    ensure!(
        boolean(evidence, "fresh_observations_not_inherited")?,
        "inherited observations are not fresh"
    );
    let qualified = boolean(evidence, "finite_scope_qualified_with_holds")?;
    ensure!(
        !boolean(evidence, "human_approval")?,
        "technical review cannot invent human approval"
    );
    ensure!(
        truthy(at(evidence, "holds_preserved")?),
        "holds must be preserved"
    );
    let raw = array(evidence, "raw_source_observations")?;
    ensure!(
        raw.len() <= 128
            && integer(evidence, "raw_reference_view_occurrences")? == raw.len() as u64,
        "raw observation count differs"
    );
    let mut raw_seen = BTreeSet::new();
    for observation in raw {
        viewed(observation)?;
        ensure!(
            boolean(observation, "fresh_hash_bytes_verified")?,
            "missing raw-byte assertion"
        );
        let asset = at(observation, "asset")?;
        let object = pin(asset)?;
        verify(asset)?;
        raw_seen.insert((object.sha256.as_str().to_owned(), object.bytes));
    }
    let supplied = array(evidence, "observations")?;
    ensure!(supplied.len() == clocks.len(), "observation count differs");
    let mut by_id = BTreeMap::new();
    for observation in supplied {
        ensure!(
            by_id
                .insert(text(observation, "sample_id")?, observation)
                .is_none(),
            "duplicate observed sample"
        );
    }
    let mut task_ids = BTreeSet::new();
    let mut observed = Vec::new();
    let mut major = false;
    for clock in clocks {
        let sample = at(clock, "sample")?;
        let id = text(sample, "sample_id")?;
        ensure!(task_ids.insert(id), "duplicate task sample");
        let source = by_id
            .get(id)
            .ok_or_else(|| anyhow!("missing expected sample"))?;
        let mut observation = (*source).clone();
        let frame = integer(sample, "frame")?;
        ensure!(
            integer(&observation, "frame")? == frame,
            "observation frame differs"
        );
        let facts = at(&observation, "fresh_role_observations")?.clone();
        ensure!(names(&facts)? == roles, "observed roles differ");
        let source_views = at(clock, "views")?;
        ensure!(names(source_views)? == roles, "task roles differ");
        let mut views = source_views.clone();
        for role in &roles {
            viewed(at(&facts, role)?)?;
            let view = at(source_views, role)?;
            pin(view)?;
            verify(view)?;
            views[role]["privately_viewed_detail_original"] = json!(true);
        }
        major |= boolean(&observation, "major_obstruction_observed")?;
        boolean(&observation, "minor_advisory")?;
        for field in &policy.footer_fields {
            nullable_boolean(&observation, field)?;
        }
        if let Some(field) = &policy.footer_events_field {
            let events = array(sample, field)?;
            if qualified && !events.is_empty() {
                ensure!(
                    policy
                        .footer_fields
                        .iter()
                        .all(|f| observation.get(f) == Some(&Value::Bool(true))),
                    "qualified footer facts must be true"
                );
            }
            observation["current_ASS_active_events"] = json!(events);
        }
        for target_policy in &policy.targets {
            let targets = array(task, &target_policy.task_field)?;
            ensure!(targets.len() <= 128, "target budget exceeded");
            for target in targets {
                let span = array(target, &target_policy.span_field)?;
                ensure!(span.len() == 2, "invalid target span");
                let start = span[0]
                    .as_u64()
                    .ok_or_else(|| anyhow!("invalid span start"))?;
                let end = span[1]
                    .as_u64()
                    .ok_or_else(|| anyhow!("invalid span end"))?;
                ensure!(start < end, "empty or reversed target span");
                if !(start..end).contains(&frame) {
                    continue;
                }
                let code = text(target, "code")?;
                let matches = array(&observation, &target_policy.observation_field)?
                    .iter()
                    .filter(|o| o.get("code").and_then(Value::as_str) == Some(code))
                    .collect::<Vec<_>>();
                ensure!(
                    matches.len() == 1,
                    "missing or duplicate active target observation"
                );
                let target_observation = matches[0];
                for field in &target_policy.exact_fields {
                    ensure!(
                        at(target_observation, field)? == at(target, field)?,
                        "declared target text differs"
                    );
                }
                for field in &target_policy.picture_fields {
                    let expected = pin(at(target, field)?)?;
                    ensure!(
                        pin(at(target_observation, field)?)? == expected,
                        "target picture differs"
                    );
                    ensure!(
                        raw_seen.contains(&(expected.sha256.as_str().to_owned(), expected.bytes)),
                        "required target source was not observed"
                    );
                }
                let per_role = at(target_observation, "per_role")?;
                ensure!(names(per_role)? == roles, "target observation roles differ");
                for role in &roles {
                    for field in &target_policy.appearance_fields {
                        nullable_boolean(at(per_role, role)?, field)?;
                    }
                }
            }
        }
        let map = observation
            .as_object_mut()
            .ok_or_else(|| anyhow!("observation is not an object"))?;
        map.remove("fresh_role_observations");
        map.insert("source_reasons".into(), at(sample, "reasons")?.clone());
        map.insert("views".into(), views);
        map.insert("per_role_fresh_observations".into(), facts);
        observed.push(observation);
    }
    if major && qualified {
        bail!("major obstruction cannot be finitely qualified");
    }
    Ok(ValidatedObservations {
        observations: observed,
        major_obstruction_found: major,
        finite_scope_qualified_with_holds: qualified,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_json_preserves_fractional_clocks_but_rejects_duplicate_fields() {
        assert_eq!(
            parse_json(br#"{"seconds":0.125,"nested":{"time":2.5}}"#).unwrap()["seconds"],
            json!(0.125)
        );
        assert!(parse_json(br#"{"nested":{"time":1,"time":2}}"#).is_err());
    }
    pub(super) fn fixture() -> (Value, Value, ObservationPolicy) {
        let image = json!({"sha256":"a".repeat(64),"bytes":8});
        let task = json!({"scope":"fixture","roles":["large","small"],"clock_count":1,"image_occurrences":2,"clocks":[{"sample":{"sample_id":"one","frame":12,"reasons":["boundary"],"footer":[{"code":"example"}]},"views":{"large":image,"small":image}}],"corrections":[]});
        let facts = json!({"privately_viewed_detail_original":true,"detail":"Fresh local view with a legible code"});
        let evidence = json!({"fresh_observations_not_inherited":true,"finite_scope_qualified_with_holds":false,"human_approval":false,"holds_preserved":["global qualification pending"],"raw_source_observations":[],"raw_reference_view_occurrences":0,"observations":[{"sample_id":"one","frame":12,"major_obstruction_observed":false,"minor_advisory":true,"legible":null,"fresh_role_observations":{"large":facts,"small":facts},"extra_note":"preserve me"}]});
        let policy = ObservationPolicy {
            scope: "fixture".into(),
            report_schema: "fixture.review-subreport.v1".into(),
            max_clocks: 3,
            footer_fields: vec!["legible".into()],
            footer_events_field: Some("footer".into()),
            targets: vec![],
        };
        (task, evidence, policy)
    }
    fn validate(
        task: &Value,
        evidence: &Value,
        policy: &ObservationPolicy,
    ) -> Result<ValidatedObservations> {
        validate_observations(task, evidence, policy, &mut |_| Ok(()))
    }
    #[test]
    fn preserves_false_null_notes_holds_and_source_events() {
        let (task, evidence, policy) = fixture();
        let result = validate(&task, &evidence, &policy).unwrap();
        assert!(!result.finite_scope_qualified_with_holds);
        assert!(result.observations[0]["legible"].is_null());
        assert_eq!(result.observations[0]["extra_note"], "preserve me");
        assert_eq!(
            result.observations[0]["source_reasons"],
            json!(["boundary"])
        );
        assert_eq!(
            result.observations[0]["current_ASS_active_events"],
            task["clocks"][0]["sample"]["footer"]
        );
    }
    #[test]
    fn qualified_footer_requires_true_and_major_prevents_qualification() {
        let (task, mut evidence, policy) = fixture();
        evidence["finite_scope_qualified_with_holds"] = json!(true);
        assert!(validate(&task, &evidence, &policy).is_err());
        evidence["observations"][0]["legible"] = json!(true);
        assert!(validate(&task, &evidence, &policy).is_ok());
        evidence["observations"][0]["major_obstruction_observed"] = json!(true);
        assert!(validate(&task, &evidence, &policy).is_err());
        evidence["finite_scope_qualified_with_holds"] = json!(false);
        assert!(
            validate(&task, &evidence, &policy)
                .unwrap()
                .major_obstruction_found
        );
    }
    #[test]
    fn rejects_duplicates_missing_nulls_and_false_as_number() {
        let (task, evidence, policy) = fixture();
        let mut bad = evidence.clone();
        bad["observations"]
            .as_array_mut()
            .unwrap()
            .push(evidence["observations"][0].clone());
        assert!(validate(&task, &bad, &policy).is_err());
        let mut bad = evidence.clone();
        bad["observations"][0]
            .as_object_mut()
            .unwrap()
            .remove("legible");
        assert!(validate(&task, &bad, &policy).is_err());
        let mut bad = evidence;
        bad["observations"][0]["major_obstruction_observed"] = json!(0);
        assert!(validate(&task, &bad, &policy).is_err());
    }
    #[test]
    fn verifies_every_view_and_propagates_integrity_failure() {
        let (task, evidence, policy) = fixture();
        let mut calls = 0;
        validate_observations(&task, &evidence, &policy, &mut |_| {
            calls += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(calls, 2);
        assert!(
            validate_observations(&task, &evidence, &policy, &mut |_| bail!(
                "corrupt retained bytes"
            ))
            .is_err()
        );
    }
    #[test]
    fn requires_exact_roles_frames_freshness_and_holds() {
        let (task, evidence, policy) = fixture();
        for (field, value) in [
            ("human_approval", json!(true)),
            ("fresh_observations_not_inherited", json!(false)),
            ("holds_preserved", json!([])),
            ("raw_reference_view_occurrences", json!(1)),
        ] {
            let mut bad = evidence.clone();
            bad[field] = value;
            assert!(validate(&task, &bad, &policy).is_err());
        }
        let mut bad = evidence.clone();
        bad["observations"][0]["frame"] = json!(13);
        assert!(validate(&task, &bad, &policy).is_err());
        let mut bad = evidence;
        bad["observations"][0]["fresh_role_observations"]
            .as_object_mut()
            .unwrap()
            .remove("small");
        assert!(validate(&task, &bad, &policy).is_err());
    }
    #[test]
    fn active_target_requires_exact_fix_sources_and_nullable_role_facts() {
        let (mut task, mut evidence, mut policy) = fixture();
        let picture = json!({"sha256":"b".repeat(64),"bytes":10});
        task["corrections"] = json!([{"code":"target","span":[12,13],"picture":picture,"fix_lines":["Exact original language"]}]);
        policy.targets.push(TargetPolicy {
            task_field: "corrections".into(),
            source_field: "corrections".into(),
            source_exact_fields: BTreeMap::new(),
            observation_field: "corrected".into(),
            span_field: "span".into(),
            picture_fields: vec!["picture".into()],
            exact_fields: vec!["fix_lines".into()],
            appearance_fields: vec!["visible".into()],
        });
        evidence["observations"][0]["corrected"] = json!([{"code":"target","picture":picture,"fix_lines":["Exact original language"],"per_role":{"large":{"visible":false},"small":{"visible":null}}}]);
        assert!(validate(&task, &evidence, &policy).is_err());
        evidence["raw_source_observations"] = json!([{"asset":picture,"privately_viewed_detail_original":true,"fresh_hash_bytes_verified":true,"detail":"Fresh original source observation"}]);
        evidence["raw_reference_view_occurrences"] = json!(1);
        assert!(validate(&task, &evidence, &policy).is_ok());
        evidence["observations"][0]["corrected"][0]["fix_lines"] = json!(["Paraphrased"]);
        assert!(validate(&task, &evidence, &policy).is_err());
    }
}
