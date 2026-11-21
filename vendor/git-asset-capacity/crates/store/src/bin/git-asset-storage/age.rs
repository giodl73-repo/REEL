//! Explicit private-root age evidence. No payload or final-deletion commands.
use super::{Result, document, raw, report};
use git_asset_core::Digest;
use git_asset_store::{
    clock::{Enrollment as ClockEnrollment, LocalClock},
    final_retirement::provider::{self, Clock, TimeBounds},
    ledger::{CapacityLedger, Checkpoint as ClockCheckpoint},
    metadata::{Checkpoint as MetadataCheckpoint, MetadataReader, MetadataStore},
    source_claims::{Checkpoint, admission::Namespace, age, subset},
};
use serde::Deserialize;
use serde_json::json;
use std::path::{Path, PathBuf};

const SCHEMA: &str = "tig.age-profile.design-1";
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    root: PathBuf,
    minimum: Checkpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    root: PathBuf,
    minimum: MetadataCheckpoint,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeClock {
    root: PathBuf,
    minimum: ClockCheckpoint,
    enrollment: ClockEnrollment,
}
impl Clock for NativeClock {
    fn now(&self, id: &Digest) -> provider::Result<u64> {
        Ok(self.bounds(id)?.latest)
    }
    fn bounds(&self, id: &Digest) -> provider::Result<TimeBounds> {
        // Delayed opening preserves time-free acknowledged retries and unknown
        // assessments. No wall-clock fallback when the enrolled clock is absent.
        let journal =
            CapacityLedger::open(&self.root, &self.minimum).map_err(|_| provider::Error::Clock)?;
        LocalClock::new(journal, self.enrollment.clone())
            .map_err(|_| provider::Error::Clock)?
            .bounds(id)
    }
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Permissions {
    initialize: bool,
    capture: bool,
    inherit: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    schema: String,
    enrollment: age::Enrollment,
    registry: Source,
    observers: Vec<Source>,
    metadata: Metadata,
    clock: Option<NativeClock>,
    required_age_millis: Option<u64>,
    subset: Option<subset::Enrollment>,
    #[serde(default)]
    permissions: Permissions,
    #[serde(default)]
    limits: age::Limits,
}
fn code(e: age::Error) -> &'static str {
    match e {
        age::Error::Invalid => "invalid_request",
        age::Error::Authority => "wrong_authority",
        age::Error::Missing => "missing_evidence",
        age::Error::Recovery => "needs_recovery",
        age::Error::Stale => "stale_basis",
        age::Error::Budget => "budget",
        age::Error::Clock => "clock_held",
        age::Error::Admission(_) => "source_held",
        age::Error::Metadata(_) => "metadata_held",
        age::Error::Subset(_) => "lineage_held",
        _ => "operation_failed",
    }
}
impl Profile {
    fn load(path: &Path, action: &str) -> Result<Self> {
        let p: Self = document(path)?;
        // Default-disabled mutation and static admission precede any root IO.
        if match action {
            "initialize" => !p.permissions.initialize,
            "capture" => !p.permissions.capture,
            "inherit" => !p.permissions.inherit,
            _ => false,
        } {
            return Err("execution_disabled");
        }
        p.enrollment.id().map_err(code)?;
        p.limits.validate().map_err(code)?;
        if p.schema != SCHEMA
            || !p.registry.root.is_absolute()
            || !p.metadata.root.is_absolute()
            || p.registry.minimum.identity() != &p.enrollment.registry
            || p.metadata.minimum.identity() != &p.enrollment.metadata
            || p.observers.len() != p.enrollment.observers.len()
            || p.observers
                .iter()
                .zip(&p.enrollment.observers)
                .any(|(s, e)| !s.root.is_absolute() || s.minimum.identity() != &e.namespace)
        {
            return Err("invalid_profile");
        }
        if let Some(c) = &p.clock {
            if !c.root.is_absolute()
                || c.minimum.identity() != &c.enrollment.journal
                || c.enrollment.id().map_err(|_| "invalid_clock")? != p.enrollment.clock
            {
                return Err("invalid_clock");
            }
        } else if matches!(action, "capture" | "assess") {
            return Err("clock_required");
        }
        if action == "assess" && p.required_age_millis.is_none() {
            return Err("age_policy_required");
        }
        if action == "inherit" {
            p.subset
                .as_ref()
                .ok_or("lineage_policy_required")?
                .id()
                .map_err(|_| "invalid_lineage_policy")?;
        }
        Ok(p)
    }
}
fn bounded(value: impl serde::Serialize) -> Result<Vec<u8>> {
    let raw = report(serde_json::to_value(value).map_err(|_| "response_budget")?)?;
    if raw.len() > 65536 {
        return Err("response_budget");
    }
    Ok(raw)
}
pub(super) fn run(args: &[String]) -> Result<Vec<u8>> {
    if args == ["describe"] {
        return bounded(
            json!({"schema":"tig.age-interface.design-1","profile_schema":SCHEMA,
        "operations":["initialize PROFILE","capture PROFILE REQUEST","status PROFILE REQUEST","assess PROFILE REQUEST","inherit PROFILE REQUEST PLAN"],
        "max_document_bytes":65536,"permissions_default_enabled":false,"initializes_stores":false,
        "status_samples_clock":false,"assessment_may_append_clock_observation":true,"authorizes_claim_release":false,"authorizes_removal":false}),
        );
    }
    let action = args.first().map(String::as_str).ok_or("usage")?;
    let count = match action {
        "initialize" => 2,
        "capture" | "status" | "assess" => 3,
        "inherit" => 4,
        _ => return Err("usage"),
    };
    if args.len() != count {
        return Err("usage");
    }
    let request: Option<age::Request> = if count >= 3 {
        Some(document(Path::new(&args[2]))?)
    } else {
        None
    };
    if let Some(r) = &request {
        r.id().map_err(code)?;
    }
    let plan = if count == 4 {
        Some(
            subset::Plan::from_json(&raw(Path::new(&args[3]))?)
                .map_err(|_| "invalid_lineage_plan")?,
        )
    } else {
        None
    };
    let p = Profile::load(Path::new(&args[1]), action)?;
    let observer = if let Some(r) = &request {
        Some(
            p.observers
                .iter()
                .find(|s| s.minimum.identity() == &r.namespace)
                .ok_or("unknown_namespace")?,
        )
    } else {
        None
    };
    let mut registry = Namespace::open(&p.registry.root, &p.registry.minimum)
        .map_err(|_| "registry_unavailable")?;
    if action == "initialize" {
        let mut metadata = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
            .map_err(|_| "metadata_unavailable")?;
        let checkpoint = age::Capture {
            enrollment: &p.enrollment,
            registry: &mut registry,
            minimum: &p.registry.minimum,
            metadata: &mut metadata,
            limits: &p.limits,
        }
        .initialize()
        .map_err(code)?;
        return bounded(
            json!({"schema":"tig.age-initialization.design-1","registry_checkpoint":checkpoint,"metadata_checkpoint":metadata.checkpoint(),"authorizes_removal":false}),
        );
    }
    let source = observer.ok_or("unknown_namespace")?;
    let mut namespace =
        Namespace::open(&source.root, &source.minimum).map_err(|_| "source_unavailable")?;
    let request = request.as_ref().ok_or("invalid_request")?;
    if matches!(action, "status" | "assess") {
        let mut nodes = MetadataReader::open(&p.metadata.root, &p.metadata.minimum)
            .map_err(|_| "metadata_unavailable")?;
        let mut api = age::Inspection {
            enrollment: &p.enrollment,
            registry: &mut registry,
            minimum: &p.registry.minimum,
            metadata: p.enrollment.metadata.clone(),
            nodes: &mut nodes,
            limits: &p.limits,
        };
        return if action == "status" {
            bounded(
                api.status(request, &mut namespace, &source.minimum)
                    .map_err(code)?,
            )
        } else {
            bounded(
                api.assess(
                    request,
                    &mut namespace,
                    &source.minimum,
                    p.clock.as_ref().ok_or("clock_required")?,
                    p.required_age_millis.ok_or("age_policy_required")?,
                )
                .map_err(code)?,
            )
        };
    }
    let mut metadata = MetadataStore::open(&p.metadata.root, &p.metadata.minimum)
        .map_err(|_| "metadata_unavailable")?;
    let mut api = age::Capture {
        enrollment: &p.enrollment,
        registry: &mut registry,
        minimum: &p.registry.minimum,
        metadata: &mut metadata,
        limits: &p.limits,
    };
    let status = if action == "capture" {
        api.capture(
            request,
            &mut namespace,
            &source.minimum,
            p.clock.as_ref().ok_or("clock_required")?,
        )
    } else {
        api.inherit(
            request,
            &mut namespace,
            &source.minimum,
            p.subset.as_ref().ok_or("lineage_policy_required")?,
            plan.as_ref().ok_or("invalid_lineage_plan")?,
        )
    }
    .map_err(code)?;
    bounded(
        json!({"schema":"tig.age-mutation-result.design-1","status":status,"metadata_checkpoint":metadata.checkpoint(),"authorizes_removal":false}),
    )
}
