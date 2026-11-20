//! Read-only, immutable recovery planning for cumulative retained-owner history.
//! No plan grants a guard or changes the frozen input budget or worker runtime.
use super::{CapacityPreview, Target};
use crate::{NativeProfile, admission, error};
use reel_workflow::{ErrorKind, Packet, Result, ReturnedResult};
use serde::{Deserialize, Serialize};
use tig_core::Object;

const MAX_PLAN_BYTES: u64 = 64 * 1024;
const MAX_OWNER_OBJECTS: usize = 4096;
const MAX_OWNER_BYTES: u64 = 128 * 1024 * 1024 * 1024;

/// A finite proposed aggregation allowance, separate from per-job projection.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema: String,
    pub objects: usize,
    pub logical_bytes: u64,
}
impl Policy {
    fn validate(&self) -> Result<()> {
        if self.schema != "reel.workflow-retained-owner-policy.v1" {
            return Err(error(ErrorKind::Invalid, "retained-capacity-policy-schema"));
        }
        if self.objects == 0
            || self.objects > MAX_OWNER_OBJECTS
            || self.logical_bytes == 0
            || self.logical_bytes > MAX_OWNER_BYTES
        {
            return Err(error(ErrorKind::Budget, "retained-capacity-policy-bound"));
        }
        Ok(())
    }
}

pub(crate) struct Observation {
    pub preview: CapacityPreview,
    pub state: Object,
}

/// Private control data. A digest pin authenticates its exact bytes, not approval.
/// Rechecking compares the original request and current native state atomically
/// under source/target fences. Application remains a separate, unimplemented API.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    pub request: Object,
    pub state: Object,
    pub observed: CapacityPreview,
    pub policy: Policy,
    pub evidence_reserve_bytes: u64,
    pub required_objects: usize,
    pub required_logical_bytes: u64,
    pub required_changed_paths: usize,
}
impl Plan {
    fn build(request: Object, observation: Observation, policy: Policy) -> Result<Self> {
        policy.validate()?;
        let p = &observation.preview;
        if p.original_retention_work_exists || p.original_retention_work_complete.is_some() {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "retained-capacity-reconcile-original-work",
            ));
        }
        if p.resource_bound_ok {
            return Err(error(
                ErrorKind::Invalid,
                "retained-capacity-recovery-unnecessary",
            ));
        }
        let overflow = || error(ErrorKind::Budget, "retained-capacity-reserve-overflow");
        // Future registration must retain the immutable recovery control itself.
        // These are conservative reservations, not an assertion of staged work.
        let required_objects = p.combined_objects.checked_add(1).ok_or_else(overflow)?;
        let required_logical_bytes = p
            .combined_logical_bytes
            .checked_add(MAX_PLAN_BYTES)
            .ok_or_else(overflow)?;
        let required_changed_paths = p.additional_paths.checked_add(1).ok_or_else(overflow)?;
        if required_objects > policy.objects || required_logical_bytes > policy.logical_bytes {
            return Err(error(
                ErrorKind::Budget,
                "retained-capacity-policy-insufficient",
            ));
        }
        if !p.changed_path_bound_ok || required_changed_paths > p.changed_path_limit {
            return Err(error(
                ErrorKind::Budget,
                "retained-capacity-changed-path-bound",
            ));
        }
        Ok(Self {
            schema: "reel.workflow-retained-capacity-plan.v1".into(),
            request,
            state: observation.state,
            observed: observation.preview,
            policy,
            evidence_reserve_bytes: MAX_PLAN_BYTES,
            required_objects,
            required_logical_bytes,
            required_changed_paths,
        })
    }
    pub fn canonical_bytes(&self) -> Result<Vec<u8>> {
        let raw = admission::encode(self)?;
        if raw.len() as u64 > MAX_PLAN_BYTES {
            return Err(error(ErrorKind::Budget, "retained-capacity-plan-bytes"));
        }
        Ok(raw)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn prepare(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    target: &Target,
    policy: Policy,
) -> Result<Plan> {
    policy.validate()?;
    super::check(packet, source, receipt, result)?;
    let request = Object::from_bytes(&admission::encode(&(
        "reel.workflow-retained-capacity-request.v1",
        packet,
        source,
        profile,
        receipt,
        result,
        target,
    ))?);
    let observation = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
        .retained_capacity_observation(receipt, result, target)?;
    Plan::build(request, observation, policy)
}

/// Read-only stale-plan check. It never stages or resumes native work.
#[allow(clippy::too_many_arguments)]
pub fn recheck(
    plan: &Plan,
    expected_sha256: &str,
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    target: &Target,
) -> Result<()> {
    check_pin(plan, expected_sha256)?;
    let current = prepare(
        packet,
        source,
        profile,
        receipt,
        result,
        target,
        plan.policy.clone(),
    )?;
    compare(plan, &current)
}

fn check_pin(plan: &Plan, expected_sha256: &str) -> Result<()> {
    if reel_workflow::digest(&plan.canonical_bytes()?) != expected_sha256 {
        return Err(error(ErrorKind::Integrity, "retained-capacity-plan-pin"));
    }
    Ok(())
}

fn compare(plan: &Plan, current: &Plan) -> Result<()> {
    // Compare all fields, including reservations, schema and exact native state.
    if plan.canonical_bytes()? != current.canonical_bytes()? {
        return Err(error(ErrorKind::Stale, "retained-capacity-plan-stale"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn policy() -> Policy {
        Policy {
            schema: "reel.workflow-retained-owner-policy.v1".into(),
            objects: 1024,
            logical_bytes: 1024 * 1024 * 1024,
        }
    }
    fn observation() -> Observation {
        Observation {
            state: Object::from_bytes(b"exact original checkpoint, actors, candidate"),
            preview: CapacityPreview {
                schema: "reel.workflow-retained-capacity-preview.v1".into(),
                source_objects: 75,
                retained_objects: 473,
                combined_objects: 525,
                combined_logical_bytes: 270429684,
                object_limit: 512,
                logical_byte_limit: 1073741824,
                additional_paths: 52,
                changed_path_limit: 128,
                resource_bound_ok: false,
                changed_path_bound_ok: true,
                original_retention_work_exists: false,
                original_retention_work_complete: None,
            },
        }
    }
    fn build(o: Observation, p: Policy) -> Result<Plan> {
        Plan::build(Object::from_bytes(b"original request"), o, p)
    }
    #[test]
    fn recovery_reserves_its_own_evidence_without_widening_frozen_job_limits() {
        let p = build(observation(), policy()).unwrap();
        assert_eq!((p.required_objects, p.required_changed_paths), (526, 53));
        assert_eq!(p.required_logical_bytes, 270429684 + 65536);
        assert_eq!(p.observed.object_limit, 512);
        assert!(!p.observed.resource_bound_ok);
        let restored: Plan = serde_json::from_slice(&p.canonical_bytes().unwrap()).unwrap();
        compare(&p, &restored).unwrap();
    }
    #[test]
    fn policy_cannot_hide_its_evidence_or_bypass_phase_bounds() {
        let mut p = policy();
        p.objects = 525;
        assert_eq!(
            build(observation(), p).unwrap_err().code,
            "retained-capacity-policy-insufficient"
        );
        let mut p = policy();
        p.logical_bytes = 270429684;
        assert_eq!(build(observation(), p).unwrap_err().kind, ErrorKind::Budget);
        let mut o = observation();
        o.preview.additional_paths = 128;
        assert_eq!(
            build(o, policy()).unwrap_err().code,
            "retained-capacity-changed-path-bound"
        );
        let mut o = observation();
        o.preview.combined_logical_bytes = u64::MAX;
        assert_eq!(
            build(o, policy()).unwrap_err().code,
            "retained-capacity-reserve-overflow"
        );
    }
    #[test]
    fn existing_or_uncertain_work_requires_original_reconciliation() {
        for complete in [None, Some(false), Some(true)] {
            let mut o = observation();
            o.preview.original_retention_work_exists = true;
            o.preview.original_retention_work_complete = complete;
            assert_eq!(
                build(o, policy()).unwrap_err().kind,
                ErrorKind::NeedsRecovery
            );
        }
        let mut o = observation();
        o.preview.resource_bound_ok = true;
        assert_eq!(
            build(o, policy()).unwrap_err().code,
            "retained-capacity-recovery-unnecessary"
        );
    }
    #[test]
    fn stale_checkpoint_foreign_request_or_edited_plan_never_passes_recheck() {
        let original = build(observation(), policy()).unwrap();
        let pin = reel_workflow::digest(&original.canonical_bytes().unwrap());
        check_pin(&original, &pin).unwrap();
        for which in 0..6 {
            let mut edited = original.clone();
            match which {
                0 => edited.state = Object::from_bytes(b"advanced native state"),
                1 => edited.request = Object::from_bytes(b"different runtime or activation"),
                2 => edited.required_objects -= 1,
                3 => edited.policy.objects = 2048,
                4 => edited.schema.push('x'),
                _ => edited.observed.retained_objects -= 1,
            }
            assert_eq!(
                compare(&edited, &original).unwrap_err().kind,
                ErrorKind::Stale
            );
            assert_eq!(
                check_pin(&edited, &pin).unwrap_err().kind,
                ErrorKind::Integrity
            );
        }
    }
    #[test]
    fn policy_has_fixed_outer_bounds_and_strict_schema() {
        for (objects, logical_bytes) in [(0, 1), (4097, 1), (1, 0), (1, u64::MAX)] {
            let p = Policy {
                objects,
                logical_bytes,
                ..policy()
            };
            assert_eq!(
                build(observation(), p).unwrap_err().code,
                "retained-capacity-policy-bound"
            );
        }
        let mut p = policy();
        p.schema.push('x');
        assert_eq!(
            build(observation(), p).unwrap_err().kind,
            ErrorKind::Invalid
        );
        assert!(serde_json::from_str::<Policy>(r#"{"schema":"reel.workflow-retained-owner-policy.v1","objects":1024,"logical_bytes":1073741824,"unbounded":true}"#).is_err());
    }
}
