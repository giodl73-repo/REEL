//! Explicit age-aware candidate selection. Age never grants final consent.
use super::*;
use git_asset_store::{
    clock::{Enrollment as ClockEnrollment, LocalClock},
    final_retirement::provider::{self, Clock, TimeBounds},
    ledger::{CapacityLedger, Checkpoint as ClockCheckpoint},
    source_claims::age,
};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Profile {
    schema: String,
    pub selection: Select,
    pub age: Age,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NativeClock {
    root: PathBuf,
    minimum: ClockCheckpoint,
    enrollment: ClockEnrollment,
}
impl NativeClock {
    pub(super) fn validate(&self, expected: &Digest) -> Result<()> {
        if !self.root.is_absolute()
            || self.minimum.identity() != &self.enrollment.journal
            || self.enrollment.id().map_err(|_| "invalid_clock")? != *expected
        {
            return Err("invalid_clock");
        }
        Ok(())
    }
}
impl Clock for NativeClock {
    fn now(&self, id: &Digest) -> provider::Result<u64> {
        Ok(self.bounds(id)?.latest)
    }
    fn bounds(&self, id: &Digest) -> provider::Result<TimeBounds> {
        let journal =
            CapacityLedger::open(&self.root, &self.minimum).map_err(|_| provider::Error::Clock)?;
        LocalClock::new(journal, self.enrollment.clone())
            .map_err(|_| provider::Error::Clock)?
            .bounds(id)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Age {
    enrollment: age::Enrollment,
    registry: Source,
    clock: NativeClock,
    required_age_millis: u64,
    #[serde(default)]
    limits: age::Limits,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        self.age
            .enrollment
            .id()
            .map_err(|_| "invalid_age_enrollment")?;
        self.age.limits.validate().map_err(|_| "budget")?;
        let a = &self.age;
        let p = &self.selection;
        if self.schema != "tig.native-retirement-aged-profile.design-1"
            || !a.registry.root.is_absolute()
            || a.registry.minimum.identity() != &a.enrollment.registry
            || p.metadata.minimum.identity() != &a.enrollment.metadata
            || !a.enrollment.observers.iter().any(|o| {
                &o.namespace == p.namespace.minimum.identity() && o.owners.contains(&p.owner)
            })
            || !a.clock.root.is_absolute()
            || a.clock.minimum.identity() != &a.clock.enrollment.journal
            || a.clock.enrollment.id().map_err(|_| "invalid_clock")? != a.enrollment.clock
            || a.required_age_millis > 9_007_199_254_740_991
        {
            return Err("invalid_profile");
        }
        Ok(())
    }
}
impl Age {
    pub fn assess(
        &self,
        p: &Select,
        namespace: &mut Namespace,
        nodes: &mut MetadataReader,
    ) -> Result<age::Assessment> {
        let mut registry = Namespace::open(&self.registry.root, &self.registry.minimum)
            .map_err(|_| "age_registry_unavailable")?;
        let request = age::Request {
            namespace: namespace.identity().clone(),
            source: p.source.clone(),
            predecessor_revision: p.predecessor_revision,
        };
        age::Inspection {
            enrollment: &self.enrollment,
            registry: &mut registry,
            minimum: &self.registry.minimum,
            metadata: p.metadata.minimum.identity().clone(),
            nodes,
            limits: &self.limits,
        }
        .assess(
            &request,
            namespace,
            &p.namespace.minimum,
            &self.clock,
            self.required_age_millis,
        )
        .map_err(|e| match e {
            age::Error::Budget => "budget",
            age::Error::Clock => "clock_held",
            age::Error::Authority => "wrong_age_authority",
            _ => "age_evidence_held",
        })
    }
}
