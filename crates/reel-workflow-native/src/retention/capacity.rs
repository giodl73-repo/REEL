//! Read-only, immutable recovery planning for cumulative retained-owner history.
//! No plan grants a guard or changes the frozen input budget or worker runtime.
use super::{CapacityPreview, Target};
use crate::{NativeProfile, admission, error};
use reel_workflow::{ErrorKind, Packet, Result, ReturnedResult};
use serde::{Deserialize, Serialize};
use tig_core::Object;

pub(crate) const MAX_PLAN_BYTES: u64 = 64 * 1024;
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
    pub(crate) fn validate(&self) -> Result<()> {
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

/// A close operation may consume an already-retained recovery policy without
/// rewriting the original job's binding or lifecycle runtime.
pub(crate) struct ClosePolicy<'a> {
    pub policy: &'a Policy,
    pub request: Object,
    pub recovered: Option<&'a Plan>,
}

/// Private control data. A digest pin authenticates its exact bytes, not approval.
/// Rechecking compares the original request and current native state atomically
/// under source/target fences. Native execution stays with the original runtime.
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
    pub(crate) fn check_recovered_request(&self, request: &Object) -> Result<()> {
        if self.schema != "reel.workflow-retained-capacity-plan.v1" || &self.request != request {
            return Err(error(
                ErrorKind::Conflict,
                "retained-recovery-original-policy-binding",
            ));
        }
        self.check_observation(Observation {
            preview: self.observed.clone(),
            state: self.state.clone(),
        })
    }
    pub(crate) fn check_owner_request(&self, request: &Object, policy: &Policy) -> Result<()> {
        if !self.is_owner_admission() || &self.request != request || &self.policy != policy {
            return Err(error(
                ErrorKind::Conflict,
                "retained-owner-original-policy-binding",
            ));
        }
        self.check_observation(Observation {
            preview: self.observed.clone(),
            state: self.state.clone(),
        })
    }
    pub(crate) fn is_owner_admission(&self) -> bool {
        self.schema == "reel.workflow-retained-owner-plan.v1"
    }
    pub(crate) fn check_observation(&self, observation: Observation) -> Result<()> {
        compare(
            self,
            &Self::build_mode(
                self.request.clone(),
                observation,
                self.policy.clone(),
                self.is_owner_admission(),
            )?,
        )
    }
    fn build(request: Object, observation: Observation, policy: Policy) -> Result<Self> {
        Self::build_mode(request, observation, policy, false)
    }
    pub(crate) fn build_owner(
        request: Object,
        observation: Observation,
        policy: Policy,
    ) -> Result<Self> {
        Self::build_mode(request, observation, policy, true)
    }
    fn build_mode(
        request: Object,
        observation: Observation,
        policy: Policy,
        owner: bool,
    ) -> Result<Self> {
        policy.validate()?;
        let p = &observation.preview;
        if p.schema != "reel.workflow-retained-capacity-preview.v1"
            || p.changed_path_limit != tig_store::source_claims::protection::MAX_CHANGES
        {
            return Err(error(
                ErrorKind::Invalid,
                "retained-capacity-observation-contract",
            ));
        }
        if p.original_retention_work_exists || p.original_retention_work_complete.is_some() {
            return Err(error(
                ErrorKind::NeedsRecovery,
                "retained-capacity-reconcile-original-work",
            ));
        }
        if p.resource_bound_ok && !owner {
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
        // Owner admissions also keep an operation-specific named evidence link,
        // allowing exact policy recovery without scanning unrelated history.
        let required_changed_paths = p
            .additional_paths
            .checked_add(if owner { 2 } else { 1 })
            .ok_or_else(overflow)?;
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
            schema: if owner {
                "reel.workflow-retained-owner-plan.v1"
            } else {
                "reel.workflow-retained-capacity-plan.v1"
            }
            .into(),
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
    let request = request(packet, source, profile, receipt, result, target)?;
    let observation = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
        .retained_capacity_observation(receipt, result, target)?;
    Plan::build(request, observation, policy)
}

pub(crate) fn request(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    target: &Target,
) -> Result<Object> {
    Ok(Object::from_bytes(&admission::encode(&(
        "reel.workflow-retained-capacity-request.v1",
        packet,
        source,
        profile,
        receipt,
        result,
        target,
    ))?))
}

/// New-job policy admission. The caller must pin the policy in its handoff
/// binding before execution. Replays read the original named policy evidence;
/// they never create a fresh plan after native work has been staged.
#[allow(clippy::too_many_arguments)]
pub fn stage_for_owner_policy(
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    target: &Target,
    policy: &Policy,
) -> Result<super::Acknowledgement> {
    policy.validate()?;
    super::check(packet, source, receipt, result)?;
    let request = request(packet, source, profile, receipt, result, target)?;
    let _annex = crate::hold_tool(&source.annex)?;
    let guard = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
        .owner_policy_guard(receipt, result, target, &request, policy)?;
    drop(super::outputs(packet, source, result)?);
    drop(crate::producer::check_native(
        packet,
        source,
        result,
        guard.input_permit(),
    )?);
    guard.stage_owner_policy(receipt, result, target, request, policy)
}

/// Historical owner-policy check while the supplied retained guard holds the
/// owner fence. Does not require a still-active source actor or stage any work.
#[allow(clippy::too_many_arguments)]
pub(crate) fn verify_owner_policy(
    retained: &super::RetainedGuard,
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    acknowledgement: &super::Acknowledgement,
    policy: &Policy,
) -> Result<()> {
    retained._prepared.verify_owner_policy(
        receipt,
        acknowledgement,
        &request(
            packet,
            source,
            profile,
            receipt,
            result,
            &acknowledgement.target,
        )?,
        policy,
    )
}

/// Stage the SAME original retention operation with an explicit finite owner
/// policy. Reopen native output/producer bytes first, retaining original claims.
/// Does not execute the work, acknowledge custody, update the journal or retire
/// scratch. The frozen original runtime can resume the staged work normally.
#[allow(clippy::too_many_arguments)]
pub fn stage(
    plan: &Plan,
    expected_sha256: &str,
    packet: &Packet,
    source: &NativeProfile,
    profile: &admission::Profile,
    receipt: &admission::Receipt,
    result: &ReturnedResult,
    target: &Target,
) -> Result<super::Acknowledgement> {
    check_pin(plan, expected_sha256)?;
    plan.policy.validate()?;
    // Validate schema/reservations even on an idempotent original-work replay.
    let reconstructed = Plan::build_mode(
        plan.request.clone(),
        Observation {
            preview: plan.observed.clone(),
            state: plan.state.clone(),
        },
        plan.policy.clone(),
        plan.is_owner_admission(),
    )?;
    compare(plan, &reconstructed)?;
    super::check(packet, source, receipt, result)?;
    if plan.request != request(packet, source, profile, receipt, result, target)? {
        return Err(error(
            ErrorKind::Conflict,
            "retained-capacity-original-request",
        ));
    }
    let _annex = crate::hold_tool(&source.annex)?;
    let guard = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
        .capacity_recovery_guard(receipt, target)?;
    drop(super::outputs(packet, source, result)?);
    drop(crate::producer::check_native(
        packet,
        source,
        result,
        guard.input_permit(),
    )?);
    guard.stage_capacity_recovery(receipt, result, target, plan)
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
    plan.policy.validate()?;
    super::check(packet, source, receipt, result)?;
    let observation = admission::Prepared::open(packet, source, profile, &receipt.attempt_id)?
        .retained_capacity_observation(receipt, result, target)?;
    let current = Plan::build_mode(
        request(packet, source, profile, receipt, result, target)?,
        observation,
        plan.policy.clone(),
        plan.is_owner_admission(),
    )?;
    compare(plan, &current)
}

pub(crate) fn check_pin(plan: &Plan, expected_sha256: &str) -> Result<()> {
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
    #[test]
    fn owner_admission_enforces_its_own_limit_even_when_job_projection_fits() {
        let mut o = observation();
        o.preview.combined_objects = 40;
        o.preview.resource_bound_ok = true;
        let owner = Plan::build_owner(Object::from_bytes(b"request"), o, policy()).unwrap();
        assert!(owner.is_owner_admission());
        assert_eq!(owner.required_objects, 41);
        assert_eq!(owner.required_changed_paths, 54);
        owner
            .check_owner_request(&owner.request, &owner.policy)
            .unwrap();
        let mut insufficient = owner.policy.clone();
        insufficient.objects = 40;
        assert_eq!(
            Plan::build_owner(
                owner.request.clone(),
                Observation {
                    preview: owner.observed.clone(),
                    state: owner.state.clone()
                },
                insufficient
            )
            .unwrap_err()
            .kind,
            ErrorKind::Budget
        );
        let mut edited = owner.policy.clone();
        edited.objects = 2048;
        assert_eq!(
            owner
                .check_owner_request(&owner.request, &edited)
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        assert_eq!(
            owner
                .check_owner_request(&Object::from_bytes(b"foreign request"), &owner.policy)
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
    }
    #[test]
    fn named_owner_evidence_keeps_the_existing_changed_path_limit() {
        let mut o = observation();
        o.preview.additional_paths = 126;
        assert_eq!(
            Plan::build_owner(Object::from_bytes(b"request"), o, policy())
                .unwrap()
                .required_changed_paths,
            128
        );
        let mut o = observation();
        o.preview.additional_paths = 127;
        assert_eq!(
            Plan::build_owner(Object::from_bytes(b"request"), o, policy())
                .unwrap_err()
                .code,
            "retained-capacity-changed-path-bound"
        );
        let recovery = build(observation(), policy()).unwrap();
        assert_eq!(
            recovery
                .check_owner_request(&recovery.request, &recovery.policy)
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
    }
    #[test]
    fn retained_recovery_policy_is_request_and_mode_bound_for_legacy_close() {
        let recovered = build(observation(), policy()).unwrap();
        recovered
            .check_recovered_request(&recovered.request)
            .unwrap();
        assert_eq!(
            recovered
                .check_recovered_request(&Object::from_bytes(b"another job"))
                .unwrap_err()
                .code,
            "retained-recovery-original-policy-binding"
        );
        let owner = Plan::build_owner(recovered.request.clone(), observation(), policy()).unwrap();
        assert_eq!(
            owner
                .check_recovered_request(&owner.request)
                .unwrap_err()
                .kind,
            ErrorKind::Conflict
        );
        let mut edited = recovered.clone();
        edited.required_objects -= 1;
        assert_eq!(
            edited
                .check_recovered_request(&edited.request)
                .unwrap_err()
                .kind,
            ErrorKind::Stale
        );
    }
}
